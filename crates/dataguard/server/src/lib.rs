//! The product's DataGuard server: the engine of `harness-dataguard` bound
//! to this product's aggregates and DataCapabilities, over its PostgreSQL,
//! speaking the wire protocol the shell client and the mock already speak.
//!
//! What a binary serves it with: [`plant`] builds the engine, [`router`]
//! answers the wire protocol through it, [`applier`] runs the single
//! applier. This crate's own binary (`main.rs`) and the product's one server
//! (`crates/server`) both mount them.
//!
//! `DATABASE_URL` names the database — or is `memory`, and the server runs
//! on the engine's in-memory store, which forgets everything at exit: for
//! a mock-up, a demo, a smoke test. The invariants a product executes
//! beyond `aggregate.json`'s rules, and the AI that proposes
//! reconciliations, are wired in `bound()` — the two places this file
//! expects the product to grow.
//!
//! A read (`POST /capabilities/<id>`) is answered by the Capability of the
//! product that [`Reads`] serves, over the connection of its own that
//! [`router_reading`] is given; [`router`] serves none.
//!
//! Over a database, the engine's writes of the AI settings and of the
//! campaign's elements reach the product's tables through
//! [`projection::ProjectingStore`]; a save of the AI settings
//! (`ai.saveSettings`) and a connection test (`ai.testConnection`) are
//! prepared before they are submitted, with the `llm` port [`router`] is
//! given. A save of an element (`campaign.saveEntity`) is prepared against
//! the element's row as the queue will leave it, which [`Rows`] reads, and
//! its version is settled when it is applied ([`versioning::Versioning`]). A
//! restoration of an element (`campaign.restoreEntityVersion`) is prepared
//! so too, against the versions the element has, which [`Rows`] keeps. An
//! export of a campaign (`campaign.exportCampaign`) is prepared against what
//! the tables hold of it, which [`Rows`] reads: its archive is answered with
//! the command's result once its stamp is applied. A new scene
//! (`session.addScene`) is prepared at the rank after its session's last,
//! against what the engine holds of the session and — over a database — what
//! its tables hold ([`Rows::refresh_session`]): the places it links are
//! judged against the same.

use std::sync::Arc;

use axum::extract::{FromRef, Path, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use harness_dataguard::Error;
use harness_dataguard::aggregate::Aggregate;
use harness_dataguard::api::{HoldRequest, ReadResponse, ReviewDecision, Submission};
use harness_dataguard::command::Entry;
pub use harness_dataguard::engine::Engine;
use harness_dataguard::guard::Invariants;
use harness_dataguard::manifest::DataCapability;
use harness_dataguard::memory::{MemoryNotifier, MemoryStore};
use harness_dataguard::postgres::{PgStore, SystemClock};
use harness_dataguard::registry::Registry;
use harness_dataguard::store::Store;
use ports_llm::Llm;

mod ai_settings;
mod aliases;
mod battlemap_instance;
mod campaign;
mod diffusion;
mod entity;
mod exports;
mod history;
mod media;
mod note_import;
mod player;
mod projection;
mod reads;
mod scenes;
mod session;
mod sound;
mod sound_playback;
mod suggestion;
mod versioning;

pub use reads::Reads;
pub use versioning::Rows;

/// The `DATABASE_URL` that means no database: the in-memory store.
pub const MEMORY: &str = "memory";

mod registry {
    include!(concat!(env!("OUT_DIR"), "/registry.rs"));
}

type Shared = Arc<Engine>;

/// What the wire protocol is served with: the engine and its rows, the
/// `llm` port a save of the AI settings detects its API type through, and the
/// reads.
struct Served<L> {
    engine: Shared,
    rows: Rows,
    llm: Arc<L>,
    reads: Arc<Reads>,
    exports: Arc<exports::Exports>,
}

impl<L> Clone for Served<L> {
    fn clone(&self) -> Self {
        Self {
            engine: self.engine.clone(),
            rows: self.rows.clone(),
            llm: self.llm.clone(),
            reads: self.reads.clone(),
            exports: self.exports.clone(),
        }
    }
}

impl<L> FromRef<Served<L>> for Arc<exports::Exports> {
    fn from_ref(served: &Served<L>) -> Self {
        served.exports.clone()
    }
}

impl<L> FromRef<Served<L>> for Arc<Reads> {
    fn from_ref(served: &Served<L>) -> Self {
        served.reads.clone()
    }
}

impl<L> FromRef<Served<L>> for Shared {
    fn from_ref(served: &Served<L>) -> Self {
        served.engine.clone()
    }
}

/// Why the engine could not be planted.
#[derive(Debug, thiserror::Error)]
pub enum PlantError {
    /// The database could not be reached.
    #[error("database: {0}")]
    Database(#[from] tokio_postgres::Error),
    /// A manifest the checkout holds does not parse.
    #[error("manifest: {0}")]
    Manifest(#[from] serde_json::Error),
    /// The engine refused its store or its queue.
    #[error("engine: {0}")]
    Engine(#[from] Error),
}

/// The engine over this product's manifests and its database.
///
/// # Errors
///
/// [`PlantError`] when the database cannot be reached, a manifest does not
/// parse, or the engine cannot load its queue.
pub async fn plant(url: &str) -> Result<Engine, PlantError> {
    Ok(planted(url).await?.engine)
}

/// What [`planted`] answers: the engine, and the rows it holds, which the
/// wire protocol prepares a save of an element against.
pub struct Planted {
    /// The engine.
    pub engine: Engine,
    /// Its rows.
    pub rows: Rows,
}

/// [`plant`], with the engine's rows.
///
/// # Errors
///
/// As [`plant`].
pub async fn planted(url: &str) -> Result<Planted, PlantError> {
    let aliases = Arc::new(aliases::Aliases::default());
    let history = Arc::new(history::History::default());
    let scenes = Arc::new(scenes::Scenes::default());
    let mut tables = None;
    let store: Arc<dyn Store> = if url == MEMORY {
        Arc::new(MemoryStore::default())
    } else {
        let (client, connection) = tokio_postgres::connect(url, tokio_postgres::NoTls).await?;
        tokio::spawn(async move {
            if let Err(e) = connection.await {
                eprintln!("postgres: {e}");
            }
        });
        let store = projection::ProjectingStore::new(PgStore::open(client).await?, aliases.clone())
            .keeping(history.clone());
        store.repair().await;
        store.repair_tests().await;
        store.elements().await;
        store.versions(None).await;
        store.sessions(&scenes).await;
        let store = Arc::new(store);
        tables = Some(store.clone());
        store
    };
    // Over memory, the engine's rows are the tables: they keep the terms and
    // the versions.
    let kept = (url == MEMORY).then(|| aliases.clone());
    let versions = (url == MEMORY).then(|| history.clone());
    let store: Arc<dyn Store> = Arc::new(
        versioning::Versioning::new(store, kept)
            .keeping(versions)
            .following(scenes.clone()),
    );
    let engine = bound(
        Engine::new(
            store.clone(),
            Arc::new(MemoryNotifier::default()),
            Arc::new(SystemClock),
        ),
        aliases,
        scenes.clone(),
    )?;
    engine.load().await?;
    Ok(Planted {
        engine,
        rows: Rows::keeping(store, history)
            .reading(tables)
            .following(scenes),
    })
}

/// `engine` bound to what the checkout holds: the registry, every
/// aggregate and DataCapability, and the invariants the product executes —
/// `alias-unique-in-campaign` reading `aliases`, `scene-links-same-campaign`
/// reading `scenes`.
fn bound(
    engine: Engine,
    aliases: Arc<aliases::Aliases>,
    scenes: Arc<scenes::Scenes>,
) -> Result<Engine, PlantError> {
    let mut engine = engine.with_registry(Registry::parse(registry::REGISTRY)?);
    for text in registry::AGGREGATES {
        engine = engine.with_aggregate(Aggregate::parse(text)?);
    }
    for text in registry::DATA_CAPABILITIES {
        engine = engine.with_data_capability(DataCapability::parse(text)?);
    }
    engine = engine.with_invariants(Arc::new(Product {
        entity: entity::Entity::new(aliases),
        session: session::Session::new(scenes),
    }));
    // engine = engine.with_proposer(Arc::new(my_product::Proposer));
    Ok(engine)
}

/// The invariants of every aggregate the product executes: the engine holds
/// one judge, and each aggregate's own judges its rows alone.
struct Product {
    entity: entity::Entity,
    session: session::Session,
}

impl Invariants for Product {
    fn violated(
        &self,
        aggregate: &str,
        state: &std::collections::BTreeMap<String, serde_json::Value>,
        entry: &Entry,
    ) -> Vec<String> {
        let judges: [&dyn Invariants; 12] = [
            &ai_settings::AiSettings,
            &campaign::Campaign,
            &player::Player,
            &self.entity,
            &media::Media,
            &note_import::NoteImport,
            &self.session,
            &sound::Sound,
            &battlemap_instance::BattlemapInstance,
            &diffusion::Diffusion,
            &sound_playback::SoundPlayback,
            &suggestion::Suggestion,
        ];
        judges
            .iter()
            .flat_map(|judge| judge.violated(aggregate, state, entry))
            .collect()
    }
}

/// The shell's wire protocol, answered by `engine`, whose rows are `rows`;
/// `llm` is the port a save of the AI settings reaches (a generic: [`Llm`] is
/// not dyn-compatible). No Capability reads a row: see [`router_reading`].
pub fn router<L: Llm + 'static>(engine: Arc<Engine>, rows: Rows, llm: Arc<L>) -> Router {
    router_reading(engine, rows, llm, Arc::new(Reads::none()))
}

/// [`router`], its reads answered through `reads`.
pub fn router_reading<L: Llm + 'static>(
    engine: Arc<Engine>,
    rows: Rows,
    llm: Arc<L>,
    reads: Arc<Reads>,
) -> Router {
    Router::new()
        .route("/capabilities/{id}", post(capability))
        .route("/commands", post(submit::<L>))
        .route("/commands/{id}", get(status))
        .route("/commands/{id}/confirm", post(confirm))
        .route("/commands/{id}/cancel", post(cancel))
        .route("/commands/{id}/requeue", post(requeue))
        .route("/holds", post(hold))
        .route("/holds/{id}/renew", post(renew))
        .route("/holds/{id}/release", post(release))
        .route("/reviews/{id}", post(decide))
        .route("/dataVersion", get(version))
        .with_state(Served {
            engine,
            rows,
            llm,
            reads,
            exports: Arc::default(),
        })
}

/// The single applier: every second, each partition's head. It runs until
/// the handle is aborted.
pub fn applier(engine: Arc<Engine>) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            if let Err(e) = engine.run_once().await {
                eprintln!("applier: {e}");
            }
        }
    })
}

fn status_of(error: &Error) -> axum::http::StatusCode {
    use axum::http::StatusCode;
    match error {
        Error::NotFound(_) | Error::UnknownDataCapability(_) | Error::UnregisteredQuery(_) => {
            StatusCode::NOT_FOUND
        }
        Error::Forbidden(_) => StatusCode::FORBIDDEN,
        Error::NotNow(_) => StatusCode::CONFLICT,
        Error::Store(_) => StatusCode::SERVICE_UNAVAILABLE,
    }
}

type Reply<T> =
    Result<(axum::http::StatusCode, Json<T>), (axum::http::StatusCode, Json<serde_json::Value>)>;

fn reply<T>(status: axum::http::StatusCode, result: Result<T, Error>) -> Reply<T> {
    result.map(|value| (status, Json(value))).map_err(|e| {
        (
            status_of(&e),
            Json(serde_json::json!({ "error": e.to_string() })),
        )
    })
}

/// A command enters the queue; a save of the AI settings is first made the
/// state it writes (`save_settings::prepare`), and its refusal, when its URL
/// is not recognised, is told among the warnings. A connection test is first
/// run through the port, and only its outcome is submitted
/// (`test_connection::prepare`): the configuration it tested never is. A
/// save of an element is made the state and the version it writes on the
/// element's partition (`save_entity::prepare`), against the row the
/// commands queued there will leave; a new element is given its id, and a
/// save the tables could never hold is refused, 422, before it is queued. A
/// restoration is made so the state and the version it writes
/// (`restore_entity_version::prepare`), against the element's row and its
/// versions: one naming a version the element does not have — or any of an
/// element the engine does not hold — is refused, 404, and one naming no
/// element, no version or a change there is not, 422. An export
/// (`export_campaign::prepare`) is made the stamp it writes on its campaign's
/// partition, and its archive is built from what the tables hold of the
/// campaign now and kept until the command is applied ([`status`]): one of a
/// campaign the tables do not hold is refused, 404, and one they cannot make
/// an archive of, 422 — neither is queued, so nothing changes. A new scene
/// (`add_scene::prepare`) is given its id and the rank after its session's
/// last, what the tables hold of the session read first: one of a session
/// neither the engine nor the tables hold is refused, 404, and one the
/// tables could never hold, 422 — neither is queued.
///
/// The element a command targets is its `targetId`, or the `id` of its
/// `target`, as the Micro-UIs and the rules' tests name it.
async fn submit<L: Llm>(
    State(served): State<Served<L>>,
    Json(raw): Json<serde_json::Value>,
) -> Reply<harness_dataguard::api::Submitted> {
    let target = raw
        .get("target")
        .and_then(|target| target.get("id"))
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned);
    let mut body: Submission = match serde_json::from_value(raw) {
        Ok(body) => body,
        Err(e) => {
            return Err((
                axum::http::StatusCode::UNPROCESSABLE_ENTITY,
                Json(serde_json::json!({ "error": e.to_string() })),
            ));
        }
    };
    if body.target_id.is_none() {
        body.target_id = target;
    }
    let mut refusal = None;
    let mut archive = None;
    // To the millisecond: two tests in a second are told apart by the read of
    // the last one (`ai.getSettings`).
    let now = || {
        let elapsed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default();
        let stamp = save_settings::timestamp(elapsed.as_secs());
        format!(
            "{}.{:03}Z",
            stamp.trim_end_matches('Z'),
            elapsed.subsec_millis()
        )
    };
    if save_settings::names(&body.data_capability) {
        let prepared = save_settings::prepare(served.llm.as_ref(), &body.payload, &now()).await;
        body.payload = prepared.payload;
        body.target_id = Some(save_settings::TARGET_ID.to_string());
        refusal = prepared.refusal;
    } else if test_connection::names(&body.data_capability) {
        let prepared = test_connection::prepare(served.llm.as_ref(), &body.payload, &now()).await;
        body.payload = prepared.payload;
        body.target_id = Some(prepared.target_id);
    } else if save_entity::names(&body.data_capability) {
        let unrandom = |_| Error::Store("no randomness for an id".to_string());
        let named = save_entity::element_id(&body.payload).or_else(|| body.target_id.clone());
        let ids = match named {
            Some(id) => fresh_uuid().map(|version| (id, version)),
            None => fresh_uuid().and_then(|id| fresh_uuid().map(|version| (id, version))),
        };
        let (id, version) = match ids.map_err(unrandom) {
            Ok(ids) => ids,
            Err(e) => return reply(axum::http::StatusCode::ACCEPTED, Err(e)),
        };
        let base = match served.rows.projected(&format!("Entity/{id}")).await {
            Ok(base) => base,
            Err(e) => return reply(axum::http::StatusCode::ACCEPTED, Err(e)),
        };
        let now = now();
        let fresh = save_entity::Fresh {
            entity: &id,
            version: &version,
            now: &now,
        };
        match save_entity::prepare(&body.payload, &base, fresh) {
            Ok(prepared) => {
                body.payload = prepared.payload;
                body.target_id = Some(prepared.target_id);
            }
            // What the tables could never hold is not queued.
            Err(refusal) => {
                return Err((
                    axum::http::StatusCode::UNPROCESSABLE_ENTITY,
                    Json(serde_json::json!({ "error": refusal.to_string() })),
                ));
            }
        }
    } else if restore_entity_version::names(&body.data_capability) {
        let id = body
            .target_id
            .clone()
            .or_else(|| restore_entity_version::element_id(&body.payload))
            .unwrap_or_default();
        let version = match fresh_uuid() {
            Ok(version) => version,
            Err(_) => {
                let e = Error::Store("no randomness for an id".to_string());
                return reply(axum::http::StatusCode::ACCEPTED, Err(e));
            }
        };
        let base = match served.rows.projected(&format!("Entity/{id}")).await {
            Ok(base) => base,
            Err(e) => return reply(axum::http::StatusCode::ACCEPTED, Err(e)),
        };
        let now = now();
        let fresh = restore_entity_version::Fresh {
            version: &version,
            now: &now,
        };
        let history = served.rows.versions(&id);
        match restore_entity_version::prepare(&body.payload, &base, &history, &id, fresh) {
            Ok(prepared) => {
                body.payload = prepared.payload;
                body.target_id = Some(id);
            }
            Err(refusal) => {
                let status = if refusal == restore_entity_version::Refusal::UnknownVersion {
                    axum::http::StatusCode::NOT_FOUND
                } else {
                    axum::http::StatusCode::UNPROCESSABLE_ENTITY
                };
                return Err((
                    status,
                    Json(serde_json::json!({ "error": refusal.to_string() })),
                ));
            }
        }
    } else if export_campaign::names(&body.data_capability) {
        let id = body
            .target_id
            .clone()
            .or_else(|| export_campaign::campaign_id(&body.payload))
            .unwrap_or_default();
        let tables = match served.rows.archived(&id).await {
            Ok(Some(tables)) => tables,
            Ok(None) => {
                return Err((
                    axum::http::StatusCode::NOT_FOUND,
                    Json(
                        serde_json::json!({ "error": export_campaign::Refusal::NoCampaign.to_string() }),
                    ),
                ));
            }
            Err(e) => return reply(axum::http::StatusCode::ACCEPTED, Err(e)),
        };
        match export_campaign::prepare(&tables, &now()) {
            Ok(prepared) => {
                body.payload = prepared.payload;
                body.target_id = Some(prepared.target_id);
                archive = Some(prepared.archive);
            }
            Err(refusal) => {
                return Err((
                    axum::http::StatusCode::UNPROCESSABLE_ENTITY,
                    Json(serde_json::json!({ "error": refusal.to_string() })),
                ));
            }
        }
    } else if add_scene::names(&body.data_capability) {
        let refused = |status, refusal: add_scene::Refusal| {
            Err((
                status,
                Json(serde_json::json!({ "error": refusal.to_string() })),
            ))
        };
        let Some(session) = add_scene::session_of(&body.payload) else {
            let unnamed = add_scene::Refusal::NoSession;
            return refused(axum::http::StatusCode::UNPROCESSABLE_ENTITY, unnamed);
        };
        let scene = match fresh_uuid() {
            Ok(scene) => scene,
            Err(_) => {
                let e = Error::Store("no randomness for an id".to_string());
                return reply(axum::http::StatusCode::ACCEPTED, Err(e));
            }
        };
        if let Err(e) = served.rows.refresh_session(&session).await {
            return reply(axum::http::StatusCode::ACCEPTED, Err(e));
        }
        if !served.rows.knows_session(&session) {
            let unknown = add_scene::Refusal::UnknownSession;
            return refused(axum::http::StatusCode::NOT_FOUND, unknown);
        }
        let fresh = add_scene::Fresh {
            scene: &scene,
            position: served.rows.next_scene_position(&session, &scene),
        };
        match add_scene::prepare(&body.payload, fresh) {
            Ok(prepared) => {
                body.payload = prepared.payload;
                body.target_id = Some(prepared.target_id);
            }
            Err(refusal) => return refused(axum::http::StatusCode::UNPROCESSABLE_ENTITY, refusal),
        }
    }
    let submitted = served.engine.submit(body).await.map(|mut submitted| {
        submitted.warnings.extend(refusal.map(str::to_string));
        if let Some(archive) = archive {
            served.exports.keep(&submitted.command_id, archive);
        }
        submitted
    });
    reply(axum::http::StatusCode::ACCEPTED, submitted)
}

/// A random (version 4) uuid: a new element's id, a version's.
fn fresh_uuid() -> Result<String, getrandom::Error> {
    let mut bytes = [0_u8; 16];
    getrandom::fill(&mut bytes)?;
    // The version (4) and the variant (10), on the bits cleared for them.
    bytes[6] = 0x40 + (bytes[6] & 0x0f);
    bytes[8] = 0x80 + (bytes[8] & 0x3f);
    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    Ok(format!(
        "{}-{}-{}-{}-{}",
        &hex[..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..]
    ))
}

/// A command's result; an export's, once applied, with its archive —
/// `result.archive.file`, the file's name, and `result.archive.content`, the
/// document —
/// which the shell hands to the DM as a download.
async fn status(
    State(engine): State<Shared>,
    State(exports): State<Arc<exports::Exports>>,
    Path(id): Path<String>,
) -> Reply<serde_json::Value> {
    reply(
        axum::http::StatusCode::OK,
        engine.status(&id).await.map(|r| {
            let mut answer = serde_json::json!({ "result": r });
            let applied = answer["result"]["status"] == "applied";
            if let Some(archive) = exports.of(&id).filter(|_| applied) {
                answer["result"]["archive"] =
                    serde_json::json!({ "file": archive.file, "content": archive.content });
            }
            answer
        }),
    )
}

#[derive(serde::Deserialize)]
struct By {
    by: String,
}

async fn confirm(
    State(engine): State<Shared>,
    Path(id): Path<String>,
    Json(who): Json<By>,
) -> Reply<serde_json::Value> {
    reply(
        axum::http::StatusCode::OK,
        engine
            .confirm(&id, &who.by)
            .await
            .map(|r| serde_json::json!({ "result": r })),
    )
}

async fn cancel(
    State(engine): State<Shared>,
    Path(id): Path<String>,
    Json(who): Json<By>,
) -> Reply<serde_json::Value> {
    reply(
        axum::http::StatusCode::OK,
        engine
            .cancel(&id, &who.by)
            .await
            .map(|r| serde_json::json!({ "result": r })),
    )
}

async fn requeue(
    State(engine): State<Shared>,
    Path(id): Path<String>,
    Json(who): Json<By>,
) -> Reply<harness_dataguard::api::Submitted> {
    reply(
        axum::http::StatusCode::ACCEPTED,
        engine.requeue(&id, &who.by).await,
    )
}

async fn hold(
    State(engine): State<Shared>,
    Json(body): Json<HoldRequest>,
) -> Reply<harness_dataguard::queue::Hold> {
    let ttl = body
        .ttl
        .as_deref()
        .and_then(harness_dataguard::aggregate::duration_seconds);
    reply(
        axum::http::StatusCode::CREATED,
        engine.hold(&body.partition, &body.by, ttl).await,
    )
}

async fn renew(
    State(engine): State<Shared>,
    Path(id): Path<String>,
) -> Reply<harness_dataguard::queue::Hold> {
    reply(axum::http::StatusCode::OK, engine.renew(&id).await)
}

async fn release(State(engine): State<Shared>, Path(id): Path<String>) -> Reply<serde_json::Value> {
    reply(
        axum::http::StatusCode::OK,
        engine.release(&id).await.map(|()| serde_json::json!({})),
    )
}

async fn decide(
    State(engine): State<Shared>,
    Path(id): Path<String>,
    Json(body): Json<ReviewDecision>,
) -> Reply<serde_json::Value> {
    reply(
        axum::http::StatusCode::OK,
        engine
            .decide(&id, &body.by, &body.roles, body.approved)
            .await
            .map(|r| serde_json::json!({ "result": r })),
    )
}

/// What `POST /capabilities/<id>` carries: the hash of the persisted query the
/// Capability reads through — the registry's own when the caller names the
/// Capability alone, as a Micro-UI does — and its variables.
#[derive(serde::Deserialize, Default)]
struct Read {
    #[serde(default)]
    sha256: Option<String>,
    #[serde(default)]
    variables: serde_json::Value,
}

/// The hash `scripts/queries.sh` registered for `capability`.
fn registered(capability: &str) -> Option<&'static str> {
    static REGISTRY: std::sync::OnceLock<Registry> = std::sync::OnceLock::new();
    REGISTRY
        .get_or_init(|| Registry::parse(registry::REGISTRY).unwrap_or_default())
        .queries
        .iter()
        .find(|q| q.capability == capability)
        .map(|q| q.sha256.as_str())
}

/// A read through a persisted query: served only when the registry holds
/// the Capability's query under this hash (contract C). What the query
/// answers is the Capability's, over `reads`.
async fn capability(
    State(engine): State<Shared>,
    State(reads): State<Arc<Reads>>,
    Path(id): Path<String>,
    Json(body): Json<Read>,
) -> Reply<ReadResponse> {
    let sha256 = body
        .sha256
        .as_deref()
        .or_else(|| registered(&id))
        .unwrap_or_default();
    let answer = match engine.served(&id, sha256) {
        Ok(()) => match (
            engine.data_version().await,
            reads.run(&id, &body.variables).await,
        ) {
            (Ok(as_of), Ok(data)) => Ok(ReadResponse { as_of, data }),
            (Err(e), _) | (_, Err(e)) => Err(e),
        },
        Err(e) => Err(e),
    };
    reply(axum::http::StatusCode::OK, answer)
}

async fn version(State(engine): State<Shared>) -> Reply<serde_json::Value> {
    reply(
        axum::http::StatusCode::OK,
        engine
            .data_version()
            .await
            .map(|v| serde_json::json!({ "dataVersion": v })),
    )
}

#[cfg(test)]
mod tests {
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use harness_dataguard::command::State;
    use tower::ServiceExt;

    use super::*;

    /// What `router` answers to one request, over the in-memory engine.
    async fn answer(request: Request<Body>) -> (StatusCode, serde_json::Value) {
        let Planted { engine, rows } = planted(MEMORY).await.expect("the in-memory engine");
        let response = router(
            Arc::new(engine),
            rows,
            Arc::new(ports_llm::fake::FakeLlm::new()),
        )
        .oneshot(request)
        .await
        .expect("an answer");
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("a body");
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null),
        )
    }

    #[tokio::test]
    async fn the_memory_url_plants_the_in_memory_engine() {
        let engine = plant(MEMORY).await.expect("no database is reached");
        assert_eq!(engine.data_version().await.expect("a version"), 0);
    }

    #[tokio::test]
    async fn the_router_answers_the_data_version() {
        let request = Request::get("/dataVersion").body(Body::empty()).unwrap();
        let (status, body) = answer(request).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body, serde_json::json!({ "dataVersion": 0 }));
    }

    #[tokio::test]
    async fn an_unknown_data_capability_is_refused_typed() {
        let request = Request::post("/commands")
            .header("content-type", "application/json")
            .body(Body::from(r#"{"dataCapability":"x.y"}"#))
            .unwrap();
        let (status, body) = answer(request).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(
            body,
            serde_json::json!({ "error": "no DataCapability named x.y" })
        );
    }

    /// The home server's URL in the fixtures, and the fake that answers its
    /// model list there.
    const HOME: &str = "http://192.168.1.40:4096";

    /// An engine as `plant()` binds it over a memory store the test reads,
    /// served by `router` with the llm port's fake.
    async fn saving() -> (Arc<Engine>, Arc<MemoryStore>, Router) {
        served_by(
            ports_llm::fake::FakeLlm::new()
                .with_endpoint(HOME, serde_json::json!({ "object": "list", "data": [] })),
        )
        .await
    }

    /// The same, served with `llm`: a clone shares its journal.
    async fn served_by(llm: ports_llm::fake::FakeLlm) -> (Arc<Engine>, Arc<MemoryStore>, Router) {
        let store = Arc::new(MemoryStore::default());
        let engine = bound(
            Engine::new(
                store.clone(),
                Arc::new(MemoryNotifier::default()),
                Arc::new(harness_dataguard::memory::FixedClock::at(1000)),
            ),
            Arc::default(),
            Arc::default(),
        )
        .expect("the product's manifests parse");
        engine.load().await.expect("an empty queue");
        let engine = Arc::new(engine);
        let app = router(engine.clone(), Rows::new(store.clone()), Arc::new(llm));
        (engine, store, app)
    }

    /// `POST /commands` of ai.saveSettings by the DM, approver of their own
    /// settings, as the rules' tests name it (`@1`).
    async fn save(
        app: &Router,
        payload: serde_json::Value,
        key: Option<&str>,
    ) -> (StatusCode, serde_json::Value, String) {
        let mut body = serde_json::json!({
            "dataCapability": "ai.saveSettings@1",
            "payload": payload,
            "caller": { "by": "dm", "from": "AiSettings", "roles": ["role:approver"] },
        });
        if let Some(key) = key {
            body["idempotencyKey"] = serde_json::json!(key);
        }
        let request = Request::post("/commands")
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap();
        let response = app.clone().oneshot(request).await.expect("an answer");
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("a body");
        let text = String::from_utf8_lossy(&bytes).into_owned();
        (
            status,
            serde_json::from_str(&text).unwrap_or_default(),
            text,
        )
    }

    /// What `GET /commands/<id>` answers, as text.
    async fn read_status(app: &Router, id: &str) -> String {
        let request = Request::get(format!("/commands/{id}"))
            .body(Body::empty())
            .unwrap();
        let response = app.clone().oneshot(request).await.expect("an answer");
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("a body");
        String::from_utf8_lossy(&bytes).into_owned()
    }

    fn api(url: &str, key: &str) -> serde_json::Value {
        serde_json::json!({
            "mode": "api",
            "url": url,
            "generationModel": "qwen2.5-32b-instruct",
            "summaryModel": "qwen2.5-14b-instruct",
            "key": key,
        })
    }

    /// Saves `payload` and answers what the applier made of it.
    async fn saved(
        engine: &Engine,
        app: &Router,
        payload: serde_json::Value,
    ) -> harness_dataguard::command::CommandResult {
        let (status, body, _) = save(app, payload, None).await;
        assert_eq!(status, StatusCode::ACCEPTED, "{body}");
        let mut applied = engine.run_once().await.expect("the applier ran");
        assert_eq!(applied.len(), 1, "{applied:?}");
        applied.remove(0)
    }

    const OPENCODE: &str = "a1000000-0000-4000-8000-000000000002";

    #[tokio::test]
    async fn saving_the_home_server_s_url_applies_the_opencode_provider_and_makes_it_active() {
        let (engine, store, app) = saving().await;
        let result = saved(&engine, &app, api(HOME, "")).await;
        assert_eq!(result.status, State::Applied, "{result:?}");
        let row = store.row_now(projection::AI_SETTINGS);
        assert_eq!(row["aiProviderId"], OPENCODE);
        assert_eq!(row["activeProviderId"], OPENCODE);
        assert_eq!(row["url"], HOME);
        assert_eq!(row["detectedApiType"], "opencode");
        assert_eq!(row["generationModel"], "qwen2.5-32b-instruct");
        assert_eq!(row["summaryModel"], "qwen2.5-14b-instruct");
        assert_eq!(row["apiKey"], serde_json::Value::Null);
    }

    #[tokio::test]
    async fn saving_binary_claude_stores_claude_code_with_no_url_and_makes_it_active() {
        let (engine, store, app) = saving().await;
        saved(&engine, &app, api(HOME, "")).await;
        let result = saved(
            &engine,
            &app,
            serde_json::json!({
                "mode": "binary",
                "binary": "claude",
                "generationModel": "claude-sonnet-5-5",
                "summaryModel": "claude-haiku-4-5-20251001",
            }),
        )
        .await;
        assert_eq!(result.status, State::Applied, "{result:?}");
        let row = store.row_now(projection::AI_SETTINGS);
        assert_eq!(row["mode"], "binaire");
        assert_eq!(row["binary"], "claude-code");
        assert_eq!(
            row["activeProviderId"],
            "a1000000-0000-4000-8000-000000000004"
        );
        // Nothing of the api provider saved before is left on the row.
        for field in ["url", "detectedApiType", "apiKey"] {
            assert_eq!(row[field], serde_json::Value::Null, "{field}");
        }
    }

    #[tokio::test]
    async fn an_unrecognised_url_is_rejected_saying_why_and_nothing_changes() {
        let (engine, store, app) = saving().await;
        saved(&engine, &app, api(HOME, "")).await;
        let before = store.row_now(projection::AI_SETTINGS);
        let (_, body, _) = save(
            &app,
            api("https://inconnu.example.org/pas-une-api", ""),
            None,
        )
        .await;
        assert!(
            body["warnings"]
                .as_array()
                .is_some_and(|w| w.iter().any(|w| w == "type d'API non reconnu")),
            "{body}"
        );
        let mut settled = engine.run_once().await.expect("the applier ran");
        let result = settled.remove(0);
        assert_eq!(result.status, State::Rejected, "{result:?}");
        assert_eq!(result.violations, ["api-type-recognized"]);
        assert_eq!(store.row_now(projection::AI_SETTINGS), before);
    }

    #[tokio::test]
    async fn an_api_type_the_caller_gives_is_not_the_one_stored() {
        let (engine, store, app) = saving().await;
        let mut payload = api(HOME, "");
        payload["apiType"] = serde_json::json!("cloud");
        payload["detectedApiType"] = serde_json::json!("cloud");
        let result = saved(&engine, &app, payload).await;
        assert_eq!(result.status, State::Applied, "{result:?}");
        let row = store.row_now(projection::AI_SETTINGS);
        assert_eq!(row["detectedApiType"], "opencode");
        assert!(!row.contains_key("apiType"));
    }

    #[tokio::test]
    async fn the_same_idempotency_key_twice_is_one_save() {
        let (engine, _, app) = saving().await;
        let (_, first, _) = save(&app, api(HOME, ""), Some("save-1")).await;
        let (_, second, _) = save(&app, api(HOME, ""), Some("save-1")).await;
        assert_eq!(second["replayed"], true, "{second}");
        assert_eq!(second["commandId"], first["commandId"]);
        let applied = engine.run_once().await.expect("the applier ran");
        assert_eq!(applied.len(), 1, "{applied:?}");
        assert!(engine.run_once().await.expect("the applier ran").is_empty());
    }

    #[tokio::test]
    async fn the_api_key_is_never_echoed_by_the_submission_or_the_result() {
        let (engine, _, app) = saving().await;
        let key = "sk-ant-api03-très-secret";
        let (_, body, text) = save(
            &app,
            serde_json::json!({
                "mode": "api",
                "url": "https://api.anthropic.com/v1",
                "generationModel": "claude-sonnet-5-5",
                "summaryModel": "claude-haiku-4-5-20251001",
                "key": key,
            }),
            None,
        )
        .await;
        assert!(!text.contains(key), "{text}");
        let applied = engine.run_once().await.expect("the applier ran");
        let result = serde_json::to_string(&applied).expect("a result serialises");
        assert!(!result.contains(key), "{result}");
        let status = read_status(&app, body["commandId"].as_str().expect("an id")).await;
        assert!(status.contains("applied"), "{status}");
        assert!(!status.contains(key), "{status}");
    }

    #[tokio::test]
    async fn each_kind_of_provider_is_saved_on_its_seeded_row_under_its_label() {
        let seed = include_str!("../../../data/migrations/0001_ai_settings.sql");
        let llm = ports_llm::fake::FakeLlm::new()
            .with_endpoint(HOME, serde_json::json!({ "object": "list", "data": [] }));
        for wire in [
            api("https://api.anthropic.com/v1", ""),
            api(HOME, ""),
            serde_json::json!({ "mode": "binary", "binary": "opencode" }),
            serde_json::json!({ "mode": "binary", "binary": "claude" }),
        ] {
            let wire = wire
                .as_object()
                .expect("an object")
                .clone()
                .into_iter()
                .collect();
            let payload = save_settings::prepare(&llm, &wire, "2026-10-17T10:00:00Z")
                .await
                .payload;
            let id = payload["aiProviderId"].as_str().expect("a row");
            let label = payload["label"].as_str().expect("a label");
            assert!(
                seed.contains(&format!(
                    "('{id}', '{}', '{label}'",
                    payload["mode"].as_str().unwrap()
                )),
                "{id} {label} is not seeded so"
            );
        }
    }

    /// `POST /commands` of ai.testConnection by the DM, and what the applier
    /// made of it.
    async fn tested(
        engine: &Engine,
        app: &Router,
        payload: serde_json::Value,
    ) -> (String, harness_dataguard::command::CommandResult) {
        let body = serde_json::json!({
            "dataCapability": "ai.testConnection@1",
            "payload": payload,
            "caller": { "by": "dm", "from": "AiSettings", "roles": [] },
        });
        let request = Request::post("/commands")
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap();
        let response = app.clone().oneshot(request).await.expect("an answer");
        assert_eq!(response.status(), StatusCode::ACCEPTED);
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("a body");
        let mut applied = engine.run_once().await.expect("the applier ran");
        assert_eq!(applied.len(), 1, "{applied:?}");
        (
            String::from_utf8_lossy(&bytes).into_owned(),
            applied.remove(0),
        )
    }

    const CLOUD: &str = "a1000000-0000-4000-8000-000000000001";
    const CLAUDE_CODE: &str = "a1000000-0000-4000-8000-000000000004";

    fn partition(id: &str) -> String {
        format!("AiSettings/{id}")
    }

    #[tokio::test]
    async fn a_test_of_each_provider_writes_its_last_test_on_its_row() {
        let seed = include_str!("../../../data/migrations/0001_ai_settings.sql");
        let (engine, store, app) = saving().await;
        for (payload, row) in [
            (api("https://api.anthropic.com/v1", "sk-ant-secret"), CLOUD),
            (api(HOME, ""), OPENCODE),
            (
                serde_json::json!({ "mode": "binary", "binary": "opencode" }),
                "a1000000-0000-4000-8000-000000000003",
            ),
            (
                serde_json::json!({ "mode": "binary", "binary": "claude" }),
                CLAUDE_CODE,
            ),
        ] {
            // The row tested is one the migration seeds.
            assert!(seed.contains(&format!("('{row}'")), "{row} is not seeded");
            let (_, result) = tested(&engine, &app, payload).await;
            assert_eq!(result.status, State::Applied, "{result:?}");
            let now = store.row_now(&partition(row));
            assert_eq!(now["lastTestOk"], true, "{row}: {now:?}");
            assert_eq!(now["lastTestLatencyMs"], ports_llm::fake::FAKE_LATENCY_MS);
            assert!(
                now["lastTestAt"]
                    .as_str()
                    .is_some_and(|at| at.ends_with('Z')),
                "{now:?}"
            );
            assert!(
                now["lastTestMessage"]
                    .as_str()
                    .is_some_and(|m| m.starts_with("Connexion réussie")),
                "{now:?}"
            );
        }
    }

    #[tokio::test]
    async fn an_unrecognised_url_is_refused_on_no_provider_s_partition_saying_why() {
        let (engine, store, app) = saving().await;
        let (_, result) = tested(
            &engine,
            &app,
            api("https://inconnu.example.org/pas-une-api", ""),
        )
        .await;
        assert_eq!(result.status, State::Applied, "{result:?}");
        let now = store.row_now(&partition(test_connection::NO_PROVIDER));
        assert_eq!(now["lastTestOk"], false, "{now:?}");
        assert!(
            now["lastTestMessage"]
                .as_str()
                .is_some_and(|m| m.contains("type d'API non reconnu")),
            "{now:?}"
        );
    }

    #[tokio::test]
    async fn a_test_saves_nothing_of_what_it_tested_and_never_echoes_the_key() {
        let (engine, store, app) = saving().await;
        saved(&engine, &app, api(HOME, "")).await;
        let before = store.row_now(projection::AI_SETTINGS);
        let key = "sk-ant-api03-très-secret";
        let mut payload = api("https://api.anthropic.com/v1", key);
        // What the screen would never send is not saved either.
        payload["detectedApiType"] = serde_json::json!("cloud");
        payload["activeProviderId"] = serde_json::json!(CLOUD);
        let (submitted, result) = tested(&engine, &app, payload).await;
        assert_eq!(result.status, State::Applied, "{result:?}");
        assert_eq!(store.row_now(projection::AI_SETTINGS), before);
        let now = store.row_now(&partition(CLOUD));
        let fields: Vec<&str> = now.keys().map(String::as_str).collect();
        assert_eq!(
            fields,
            [
                "lastTestAt",
                "lastTestLatencyMs",
                "lastTestMessage",
                "lastTestOk"
            ]
        );
        let body: serde_json::Value = serde_json::from_str(&submitted).expect("JSON");
        let status = read_status(&app, body["commandId"].as_str().expect("an id")).await;
        for text in [submitted, status, format!("{now:?}"), format!("{result:?}")] {
            assert!(!text.contains(key), "{text}");
        }
    }

    #[tokio::test]
    async fn what_a_test_sends_the_provider_is_its_text_prompt_alone() {
        let llm = ports_llm::fake::FakeLlm::new();
        let (engine, _, app) = served_by(llm.clone()).await;
        let key = "sk-ant-api03-très-secret";
        tested(&engine, &app, api("https://api.anthropic.com/v1", key)).await;
        tested(
            &engine,
            &app,
            serde_json::json!({ "mode": "binary", "binary": "claude" }),
        )
        .await;
        let sent = llm.journal().list();
        assert_eq!(sent.len(), 2, "{sent:?}");
        for request in &sent {
            assert_eq!(request.kind, ports_llm::RequestKind::ConnectionTest);
            assert_eq!(request.text, ports_llm::CONNECTION_TEST_PROMPT);
        }
        assert_eq!(
            (sent[0].provider, sent[1].provider),
            (
                ports_llm::Provider::Cloud,
                ports_llm::Provider::ClaudeCodeBinary
            )
        );
    }

    /// `POST /capabilities/<id>` with `body`.
    async fn read(id: &str, body: serde_json::Value) -> (StatusCode, serde_json::Value) {
        let request = Request::post(format!("/capabilities/{id}"))
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap();
        answer(request).await
    }

    #[test]
    fn a_fresh_uuid_is_a_random_version_4_uuid() {
        let drawn: Vec<String> = (0..256)
            .map(|_| fresh_uuid().expect("randomness"))
            .collect();
        for id in &drawn {
            let chars: Vec<char> = id.chars().collect();
            assert_eq!(chars.len(), 36, "{id}");
            for (i, c) in chars.iter().enumerate() {
                match i {
                    8 | 13 | 18 | 23 => assert_eq!(*c, '-', "{id}"),
                    14 => assert_eq!(*c, '4', "{id}"),
                    19 => assert!("89ab".contains(*c), "{id}"),
                    _ => assert!(c.is_ascii_hexdigit() && !c.is_ascii_uppercase(), "{id}"),
                }
            }
        }
        let mut distinct = drawn.clone();
        distinct.sort_unstable();
        distinct.dedup();
        assert_eq!(distinct.len(), drawn.len());
    }

    #[test]
    fn a_capability_is_registered_under_the_hash_of_its_query() {
        let hash = registered("ai.getSettings").expect("ai.getSettings is registered");
        assert_eq!(hash.len(), 64);
        assert!(hash.chars().all(|c| c.is_ascii_hexdigit()), "{hash}");
        assert_eq!(registered("nothing.registered"), None);
    }

    #[tokio::test]
    async fn a_read_naming_the_registered_hash_or_none_is_served() {
        let hash = registered("ai.getSettings").expect("registered");
        for body in [
            serde_json::json!({ "sha256": hash, "variables": {} }),
            serde_json::json!({ "variables": {} }),
        ] {
            let (status, answered) = read("ai.getSettings", body).await;
            assert_eq!(status, StatusCode::OK, "{answered}");
            assert!(answered["data"]["providers"].is_object(), "{answered}");
            assert!(answered["asOf"].is_u64(), "{answered}");
        }
    }

    #[tokio::test]
    async fn every_campaign_capability_the_product_serves_has_a_persisted_query() {
        let variables =
            serde_json::json!({ "variables": { "campaign": "c", "id": "e", "entity": "e" } });
        for id in [
            "campaign.listCampaigns",
            "campaign.getCampaign",
            "campaign.listPlayers",
            "campaign.listCharacters",
            "campaign.listPlaces",
            "campaign.listEvents",
            "campaign.getEntity",
            "campaign.listEntityVersions",
        ] {
            assert!(registered(id).is_some(), "{id} has no persisted query");
            let (status, answered) = read(id, variables.clone()).await;
            assert_eq!(status, StatusCode::OK, "{id}: {answered}");
        }
    }

    #[tokio::test]
    async fn a_read_under_another_hash_or_of_no_registered_query_is_404() {
        for (id, body) in [
            (
                "ai.getSettings",
                serde_json::json!({ "sha256": "0".repeat(64) }),
            ),
            ("nothing.registered", serde_json::json!({})),
            (
                "nothing.registered",
                serde_json::json!({ "sha256": "0".repeat(64) }),
            ),
        ] {
            let (status, answered) = read(id, body).await;
            assert_eq!(status, StatusCode::NOT_FOUND, "{id}: {answered}");
        }
    }

    #[test]
    fn each_refusal_has_its_status() {
        let cases = [
            (Error::NotFound(String::new()), StatusCode::NOT_FOUND),
            (
                Error::UnknownDataCapability(String::new()),
                StatusCode::NOT_FOUND,
            ),
            (
                Error::UnregisteredQuery(String::new()),
                StatusCode::NOT_FOUND,
            ),
            (Error::Forbidden(String::new()), StatusCode::FORBIDDEN),
            (Error::NotNow(String::new()), StatusCode::CONFLICT),
            (Error::Store(String::new()), StatusCode::SERVICE_UNAVAILABLE),
        ];
        for (error, status) in cases {
            assert_eq!(status_of(&error), status, "{error:?}");
        }
    }
}
