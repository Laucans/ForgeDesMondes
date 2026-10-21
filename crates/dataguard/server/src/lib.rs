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
//! a mock-up, a demo, a smoke test.
//!
//! This is the legacy server, strangled unit by unit. Every unit's plugin
//! comes from the registry (`dataguard_registry`, which
//! `scripts/register.sh` writes): the aggregates' judges — the invariants
//! beyond `aggregate.json`'s rules — and each DataCapability's `Prepare`
//! once it has one, with what the server shares with them
//! ([`planted_with`]). Every DataCapability is prepared by its own crate's
//! `Prepare`, and every aggregate projects its own writes: what is left here
//! is the wire protocol and the reads, until the installed server replaces
//! it.
//!
//! A read (`POST /capabilities/<id>`) is answered by the Capability of the
//! product that [`Reads`] serves — this server's own reads, then the
//! registry's — over the connection of its own that [`router_reading`] is
//! given; [`router`] serves none.
//!
//! Over a database, the engine's writes reach the product's tables in the
//! apply transaction: each aggregate's `ProjectPg`, which the store runs.

use std::sync::Arc;

use axum::extract::{FromRef, Path, State};
use axum::routing::{get, post};
use axum::{Json, Router};
pub use harness_dataguard as engine;
use harness_dataguard::Error;
use harness_dataguard::aggregate::Aggregate;
use harness_dataguard::api::{HoldRequest, ReadResponse, ReviewDecision, Submission};
pub use harness_dataguard::engine::Engine;
use harness_dataguard::manifest::DataCapability;
use harness_dataguard::memory::{MemoryNotifier, MemoryStore};
use harness_dataguard::plugins::{Plugins, RefusalKind};
use harness_dataguard::postgres::{PgStore, ReadOnly, SystemClock};
use harness_dataguard::registry::Registry;
use harness_dataguard::store::{RandomIds, Store};
use ports_llm::Llm;
use ports_speech_to_text::SpeechToText;
use ports_speech_to_text::fake::FakeSpeechToText;

mod reads;
mod versioning;

pub use reads::Reads;
pub use versioning::Rows;

/// The `DATABASE_URL` that means no database: the in-memory store.
pub const MEMORY: &str = "memory";

mod registry {
    include!(concat!(env!("OUT_DIR"), "/registry.rs"));
}

type Shared = Arc<Engine>;

/// What the wire protocol is served with: the engine and its rows, and the
/// reads.
struct Served<L, S> {
    engine: Shared,
    rows: Rows,
    ports: std::marker::PhantomData<fn() -> (L, S)>,
    reads: Arc<Reads>,
}

impl<L, S> Clone for Served<L, S> {
    fn clone(&self) -> Self {
        Self {
            engine: self.engine.clone(),
            rows: self.rows.clone(),
            ports: std::marker::PhantomData,
            reads: self.reads.clone(),
        }
    }
}

impl<L, S> FromRef<Served<L, S>> for Arc<Reads> {
    fn from_ref(served: &Served<L, S>) -> Self {
        served.reads.clone()
    }
}

impl<L, S> FromRef<Served<L, S>> for Shared {
    fn from_ref(served: &Served<L, S>) -> Self {
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

/// [`plant`], with the engine's rows, the units' ports on their fakes
/// ([`fakes`]): this crate's binary and the tests'.
///
/// # Errors
///
/// As [`plant`].
pub async fn planted(url: &str) -> Result<Planted, PlantError> {
    planted_with(url, fakes()).await
}

/// What the units' `Prepare`s need, every port on its fake: the `llm` port
/// a save of the AI settings, a connection test and an analysis of notes
/// reach, and the `speech-to-text` port a dictation is transcribed by, which
/// hears nothing.
fn fakes() -> harness_dataguard::plugins::Shared {
    let mut shared = harness_dataguard::plugins::Shared::default();
    shared.insert(Arc::new(ports_llm::SharedLlm::new(
        ports_llm::fake::FakeLlm::new(),
    )));
    shared.insert(Arc::new(ports_speech_to_text::SharedSpeechToText::new(
        FakeSpeechToText::new(String::new()),
    )));
    shared
}

/// [`planted`], the units reading what `shared` holds: the ports a
/// DataCapability's `Prepare` declares it `needs`.
///
/// # Errors
///
/// As [`plant`]; and the engine refuses units that do not fit together — a
/// need nothing shares (`Error::Misconfigured`).
pub async fn planted_with(
    url: &str,
    shared: harness_dataguard::plugins::Shared,
) -> Result<Planted, PlantError> {
    let plugins = plugins();
    // The Prepares' read-only SQL over the tables (`Context::rows`): none
    // over memory.
    let mut reads: Option<Arc<dyn harness_dataguard::reads::Rows>> = None;
    let store: Arc<dyn Store> = if url == MEMORY {
        Arc::new(MemoryStore::default())
    } else {
        let (client, connection) = tokio_postgres::connect(url, tokio_postgres::NoTls).await?;
        tokio::spawn(async move {
            if let Err(e) = connection.await {
                eprintln!("postgres: {e}");
            }
        });
        let (reading, connection) = tokio_postgres::connect(url, tokio_postgres::NoTls).await?;
        tokio::spawn(async move {
            if let Err(e) = connection.await {
                eprintln!("postgres: {e}");
            }
        });
        reads = Some(Arc::new(ReadOnly::new(reading)));
        // The aggregates' projections in the apply transaction.
        Arc::new(
            PgStore::open(client)
                .await?
                .with_projections(&plugins)
                .await?,
        )
    };
    // A new row's id, and a version's, is a random uuid, as the tables key
    // their rows.
    let mut engine = Engine::new(
        store.clone(),
        Arc::new(MemoryNotifier::default()),
        Arc::new(SystemClock),
    )
    .with_ids(Arc::new(RandomIds));
    if let Some(reads) = reads {
        engine = engine.with_rows(reads);
    }
    let engine = bound(engine, plugins, shared)?;
    engine.load().await?;
    Ok(Planted {
        engine,
        rows: Rows::new(store),
    })
}

/// Every unit's plugin, as the registry gathers them.
fn plugins() -> Plugins {
    dataguard_registry::AGGREGATES
        .iter()
        .chain(dataguard_registry::DATA_CAPABILITIES)
        .chain(dataguard_registry::CAPABILITIES)
        .filter_map(|unit| unit.plugin.map(|plugin| plugin()))
        .collect()
}

/// `engine` bound to what the checkout holds: the registry, every
/// aggregate and DataCapability, every unit's plugin — the aggregates'
/// judges among them — and what `shared` holds for them.
fn bound(
    engine: Engine,
    plugins: Plugins,
    shared: harness_dataguard::plugins::Shared,
) -> Result<Engine, PlantError> {
    let mut engine = engine.with_registry(Registry::parse(registry::REGISTRY)?);
    for text in registry::AGGREGATES {
        engine = engine.with_aggregate(Aggregate::parse(text)?);
    }
    for text in registry::DATA_CAPABILITIES {
        engine = engine.with_data_capability(DataCapability::parse(text)?);
    }
    // engine = engine.with_proposer(Arc::new(my_product::Proposer));
    Ok(engine.with_plugins(plugins).with_shared(shared))
}

/// The shell's wire protocol, answered by `engine`, whose rows are `rows`.
/// No Capability reads a row: see [`router_reading`].
///
/// `llm` is no longer read: every unit reaches its ports through what
/// [`planted_with`] shares. The parameter stays until the composition
/// moves to the installed server.
pub fn router<L: Llm + 'static>(engine: Arc<Engine>, rows: Rows, llm: Arc<L>) -> Router {
    router_reading(
        engine,
        rows,
        llm,
        Arc::new(FakeSpeechToText::new(String::new())),
        Arc::new(Reads::none()),
    )
}

/// [`router`], its reads answered through `reads`. Neither `llm` nor
/// `speech` is read any more: see [`router`].
pub fn router_reading<L: Llm + 'static, S: SpeechToText + 'static>(
    engine: Arc<Engine>,
    rows: Rows,
    llm: Arc<L>,
    speech: Arc<S>,
    reads: Arc<Reads>,
) -> Router {
    drop((llm, speech));
    Router::new()
        .route("/capabilities/{id}", post(capability))
        .route("/commands", post(submit::<L, S>))
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
            ports: std::marker::PhantomData,
            reads,
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
        Error::Misconfigured(_) => StatusCode::INTERNAL_SERVER_ERROR,
        Error::Refused(refusal) => match refusal.kind {
            RefusalKind::NotFound => StatusCode::NOT_FOUND,
            RefusalKind::Unprocessable => StatusCode::UNPROCESSABLE_ENTITY,
        },
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

/// A command enters the queue: the engine's alone, its DataCapability's
/// `Prepare` included.
///
/// The element a command targets is its `targetId`, or the `id` of its
/// `target`, as the Micro-UIs and the rules' tests name it.
async fn submit<L: Llm, S: SpeechToText>(
    State(served): State<Served<L, S>>,
    Json(raw): Json<serde_json::Value>,
) -> Reply<harness_dataguard::api::Submitted> {
    let body: Submission = match serde_json::from_value(raw) {
        Ok(body) => body,
        Err(e) => {
            return Err((
                axum::http::StatusCode::UNPROCESSABLE_ENTITY,
                Json(serde_json::json!({ "error": e.to_string() })),
            ));
        }
    };
    reply(
        axum::http::StatusCode::ACCEPTED,
        served.engine.submit(body).await,
    )
}

/// A command's result; once applied, with what its `Prepare` attached for
/// its author — an export's archive, `result.attachment.file` and
/// `result.attachment.content`, which the shell hands to the DM as a
/// download.
async fn status(State(engine): State<Shared>, Path(id): Path<String>) -> Reply<serde_json::Value> {
    reply(
        axum::http::StatusCode::OK,
        engine
            .status(&id)
            .await
            .map(|r| serde_json::json!({ "result": r })),
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

    /// `POST /capabilities/<id>` with `body`.
    async fn read(id: &str, body: serde_json::Value) -> (StatusCode, serde_json::Value) {
        let request = Request::post(format!("/capabilities/{id}"))
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap();
        answer(request).await
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
