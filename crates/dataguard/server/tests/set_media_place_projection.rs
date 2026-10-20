//! media.setMediaPlace over PostgreSQL (`DATABASE_URL`): a change of place
//! of a media the tables hold — one the fixtures wrote, which no command
//! wrote before — is judged against the media and the place as the tables
//! hold them, and writes `media.place_id` and no other column of any table;
//! a place that is no lieu of the media's campaign is refused naming
//! `media-place-is-lieu`, and changes nothing. Each test works in a scratch
//! database of its own, migrated as the server migrates it, dropped when it
//! ends.

use std::collections::BTreeMap;
use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use dataguard_server::{Engine, Planted, planted, router};
use harness_dataguard::api::Submission;
use harness_dataguard::command::State;
use harness_dataguard::guard::Caller;
use harness_dataguard::manifest::DataCapability;
use serde_json::{Value, json};
use tokio_postgres::{Client, NoTls};
use tower::ServiceExt;

const MIGRATIONS: [&str; 4] = [
    include_str!("../../../data/migrations/0002_campaign.sql"),
    include_str!("../../../data/migrations/0003_player.sql"),
    include_str!("../../../data/migrations/0004_entity.sql"),
    include_str!("../../../data/migrations/0005_media.sql"),
];

const VALOMBRE: &str = "c1000000-0000-4000-8000-000000000001";
const BRUME: &str = "c1000000-0000-4000-8000-000000000002";
const TAVERNE: &str = "e1000000-0000-4000-8000-000000000002";
const MORLOX: &str = "e1000000-0000-4000-8000-000000000003";
const PHARE: &str = "e1000000-0000-4000-8000-000000000004";
const MOULIN: &str = "e1000000-0000-4000-8000-000000000005";
const CARTE: &str = "a1000000-0000-4000-8000-000000000001";
const SALLE: &str = "a1000000-0000-4000-8000-000000000002";

/// A media renamed, test-only: another write of a media's partition, as
/// `media.saveGridSettings` is.
const RENAME: &str = r#"{"dataCapability":"test.renameMedia","owner":"dataguard","version":1,"effect":"update","target":{"aggregate":"Media","id":"$id"},"touches":["Media.title"],"mode":"overwrite","callableBy":["*"]}"#;

fn url() -> String {
    std::env::var("DATABASE_URL").expect("DATABASE_URL names the harness's PostgreSQL")
}

async fn connect(url: &str) -> Client {
    let (client, connection) = tokio_postgres::connect(url, NoTls)
        .await
        .expect("PostgreSQL");
    tokio::spawn(connection);
    client
}

/// `DATABASE_URL` naming the database `name` instead.
fn naming(name: &str) -> String {
    let url = url();
    let (address, query) = url.split_once('?').unwrap_or((&url, ""));
    let (server, _) = address.rsplit_once('/').expect("a URL naming its database");
    let query = if query.is_empty() {
        String::new()
    } else {
        format!("?{query}")
    };
    format!("{server}/{name}{query}")
}

/// A database of its own, migrated, holding — as the fixtures write them —
/// Valombre with the Taverne and the Moulin (places) and Morlox (an NPC),
/// Brume with its lighthouse (a place), Valombre's battlemap of the ford
/// with its grid and no place, and its illustration of the tavern's hall at
/// the Taverne. Each element is at version 1.
struct Scratch {
    name: String,
    url: String,
    client: Client,
}

impl Scratch {
    async fn new(name: &str) -> Scratch {
        let name = format!("set_media_place_test_{name}_{}", std::process::id());
        let admin = connect(&url()).await;
        for sql in [
            format!("DROP DATABASE IF EXISTS {name} WITH (FORCE)"),
            format!("CREATE DATABASE {name}"),
        ] {
            admin.batch_execute(&sql).await.expect("a scratch database");
        }
        let url = naming(&name);
        let client = connect(&url).await;
        for migration in MIGRATIONS {
            client.batch_execute(migration).await.expect("migrated");
        }
        let versions: String = [TAVERNE, MORLOX, PHARE, MOULIN]
            .iter()
            .map(|element| {
                format!(
                    "INSERT INTO entity_version (entity_id, version_number, source, snapshot, diff) \
                     VALUES ('{element}', 1, 'dm', '{{}}', '[]');"
                )
            })
            .collect();
        client
            .batch_execute(&format!(
                "INSERT INTO campaign (id, name, kind) VALUES
                     ('{VALOMBRE}', 'Les Cendres de Valombre', 'campagne'),
                     ('{BRUME}', 'La Brume', 'oneshot');
                 INSERT INTO entity (id, campaign_id, type, name) VALUES
                     ('{TAVERNE}', '{VALOMBRE}', 'lieu', 'La Taverne du Sanglier'),
                     ('{MORLOX}', '{VALOMBRE}', 'pnj', 'Morlox'),
                     ('{PHARE}', '{BRUME}', 'lieu', 'Le Phare'),
                     ('{MOULIN}', '{VALOMBRE}', 'lieu', 'Le Moulin');
                 {versions}
                 INSERT INTO media (id, campaign_id, kind, title, file_path, width_px, height_px, place_id) VALUES
                     ('{CARTE}', '{VALOMBRE}', 'battlemap', 'Le Gué', 'media/gue.png', 960, 640, NULL),
                     ('{SALLE}', '{VALOMBRE}', 'illustration', 'La salle', 'media/salle.png', 800, 600, '{TAVERNE}');
                 INSERT INTO battlemap_grid (media_id, hex_size_px, offset_x, offset_y, orientation, color, opacity)
                     VALUES ('{CARTE}', 30, 12, 8, 'pointe', '#202020', 0.4);"
            ))
            .await
            .expect("Valombre, Brume and their media");
        Scratch { name, url, client }
    }

    /// The engine as the server plants it over this database, and its wire
    /// protocol.
    async fn serve(&self) -> (Arc<Engine>, Router) {
        self.serve_with(&[]).await
    }

    /// The same with the test-only DataCapabilities `manifests`.
    async fn serve_with(&self, manifests: &[&str]) -> (Arc<Engine>, Router) {
        let Planted { mut engine, rows } = planted(&self.url)
            .await
            .expect("the engine over PostgreSQL");
        for manifest in manifests {
            engine =
                engine.with_data_capability(DataCapability::parse(manifest).expect("contract F"));
        }
        let engine = Arc::new(engine);
        let app = router(
            engine.clone(),
            rows,
            Arc::new(ports_llm::fake::FakeLlm::new()),
        );
        (engine, app)
    }

    /// Every row of every table of the product, as JSON, by table: the
    /// DataGuard's own (`dataguard_*`) aside — its queue and its journal
    /// record every command, whatever it writes.
    async fn tables(&self) -> BTreeMap<String, Vec<Value>> {
        let names = self
            .client
            .query(
                "SELECT tablename::text FROM pg_tables
                 WHERE schemaname = 'public' AND tablename NOT LIKE 'dataguard%'
                 ORDER BY tablename",
                &[],
            )
            .await
            .expect("the tables");
        let mut tables = BTreeMap::new();
        for name in names {
            let name: String = name.get(0);
            let rows = self
                .client
                .query(
                    &format!("SELECT to_jsonb(t)::text FROM {name} t ORDER BY to_jsonb(t)::text"),
                    &[],
                )
                .await
                .expect("a table's rows")
                .iter()
                .map(|row| serde_json::from_str(row.get::<_, &str>(0)).expect("JSON"))
                .collect();
            tables.insert(name, rows);
        }
        tables
    }

    /// `media`'s place as its row holds it: `None` for none.
    async fn place_of(&self, media: &str) -> Option<String> {
        self.client
            .query_one(
                "SELECT place_id::text FROM media WHERE id = ($1::text)::uuid",
                &[&media],
            )
            .await
            .expect("the media")
            .get(0)
    }

    async fn drop(self) {
        drop(self.client);
        connect(&url())
            .await
            .batch_execute(&format!("DROP DATABASE {} WITH (FORCE)", self.name))
            .await
            .expect("the scratch database dropped");
    }
}

/// `tables` with `media`'s place made `place`: what a change of place
/// leaves, and nothing else.
fn placed(
    mut tables: BTreeMap<String, Vec<Value>>,
    media: &str,
    place: Option<&str>,
) -> BTreeMap<String, Vec<Value>> {
    for row in tables.get_mut("media").expect("the media table") {
        if row["id"] == json!(media) {
            row["place_id"] = place.map_or(Value::Null, Value::from);
        }
    }
    sorted(tables)
}

/// `tables`, each one's rows in the same order whatever changed in them.
fn sorted(mut tables: BTreeMap<String, Vec<Value>>) -> BTreeMap<String, Vec<Value>> {
    for rows in tables.values_mut() {
        rows.sort_by_key(ToString::to_string);
    }
    tables
}

/// `POST /commands` of media.setMediaPlace on `media`, to `place`, as the
/// MediaLibrary card calls it: the answer.
async fn place(
    app: &Router,
    media: &str,
    place: Option<&str>,
    key: Option<&str>,
) -> (StatusCode, Value) {
    let mut body = json!({
        "dataCapability": "media.setMediaPlace@1",
        "targetId": media,
        "payload": { "place": place },
        "caller": { "by": "dm", "from": "media.MediaLibrary", "roles": [] },
    });
    if let Some(key) = key {
        body["idempotencyKey"] = json!(key);
    }
    let request = Request::post("/commands")
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .expect("a request");
    let response = app.clone().oneshot(request).await.expect("an answer");
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("a body");
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

/// Places `media` at `place` and applies it.
async fn applied(engine: &Engine, app: &Router, media: &str, to: Option<&str>) {
    let (status, answer) = place(app, media, to, None).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{answer}");
    let result = engine.run_once().await.expect("the applier ran").remove(0);
    assert_eq!(result.status, State::Applied, "{result:?}");
}

#[tokio::test]
async fn a_lieu_of_the_campaign_reaches_place_id_and_no_other_column() {
    let scratch = Scratch::new("attach").await;
    let (engine, app) = scratch.serve().await;
    let before = scratch.tables().await;

    applied(&engine, &app, CARTE, Some(TAVERNE)).await;
    assert_eq!(scratch.place_of(CARTE).await.as_deref(), Some(TAVERNE));
    assert_eq!(
        sorted(scratch.tables().await),
        placed(before.clone(), CARTE, Some(TAVERNE))
    );

    // Changed for another lieu of the campaign, in another case.
    applied(
        &engine,
        &app,
        &CARTE.to_uppercase(),
        Some(&MOULIN.to_uppercase()),
    )
    .await;
    assert_eq!(scratch.place_of(CARTE).await.as_deref(), Some(MOULIN));
    assert_eq!(
        sorted(scratch.tables().await),
        placed(before, CARTE, Some(MOULIN))
    );
    scratch.drop().await;
}

#[tokio::test]
async fn a_null_place_empties_place_id_and_the_media_stays_valid() {
    let scratch = Scratch::new("detach").await;
    let (engine, app) = scratch.serve().await;
    let before = scratch.tables().await;
    applied(&engine, &app, SALLE, None).await;
    assert_eq!(scratch.place_of(SALLE).await, None);
    assert_eq!(sorted(scratch.tables().await), placed(before, SALLE, None));
    scratch.drop().await;
}

#[tokio::test]
async fn a_place_that_is_no_lieu_of_the_campaign_is_refused_and_changes_nothing() {
    let scratch = Scratch::new("refuse").await;
    let (engine, app) = scratch.serve().await;
    let before = sorted(scratch.tables().await);
    for (to, why) in [(MORLOX, "an NPC"), (PHARE, "a lieu of another campaign")] {
        let (status, answer) = place(&app, SALLE, Some(to), None).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{why}: {answer}");
        let error = answer["error"].as_str().expect("an error");
        assert!(error.starts_with("media-place-is-lieu"), "{why}: {error}");
        assert!(
            engine.run_once().await.expect("the applier ran").is_empty(),
            "{why}: nothing was queued"
        );
    }
    let (status, answer) = place(
        &app,
        "a1000000-0000-4000-8000-0000000000ff",
        Some(TAVERNE),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{answer}");
    assert_eq!(sorted(scratch.tables().await), before);
    scratch.drop().await;
}

#[tokio::test]
async fn a_command_replayed_with_its_idempotency_key_writes_the_place_once() {
    let scratch = Scratch::new("replay").await;
    let (engine, app) = scratch.serve().await;
    let before = scratch.tables().await;
    let (status, first) = place(&app, CARTE, Some(TAVERNE), Some("attach-1")).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{first}");
    let result = engine.run_once().await.expect("the applier ran").remove(0);
    assert_eq!(result.status, State::Applied, "{result:?}");
    let (status, again) = place(&app, CARTE, Some(TAVERNE), Some("attach-1")).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{again}");
    assert_eq!(again["replayed"], json!(true), "{again}");
    assert_eq!(again["commandId"], first["commandId"]);
    assert!(engine.run_once().await.expect("the applier ran").is_empty());
    assert_eq!(
        sorted(scratch.tables().await),
        placed(before, CARTE, Some(TAVERNE))
    );
    scratch.drop().await;
}

#[tokio::test]
async fn another_write_of_the_media_never_writes_its_place_again() {
    let scratch = Scratch::new("other_write").await;
    let (engine, app) = scratch.serve_with(&[RENAME]).await;
    applied(&engine, &app, CARTE, Some(TAVERNE)).await;
    // The place leaves the media behind the engine's back, as `ON DELETE SET
    // NULL` does when the place is removed: the engine's row keeps it.
    scratch
        .client
        .batch_execute(&format!(
            "UPDATE media SET place_id = NULL WHERE id = '{CARTE}'"
        ))
        .await
        .expect("the place nullified");
    engine
        .submit(Submission {
            data_capability: "test.renameMedia".to_string(),
            payload: [("title".to_string(), json!("Le Gué des Corbeaux"))].into(),
            target_id: Some(CARTE.to_string()),
            based_on: None,
            idempotency_key: None,
            caller: Caller {
                by: "dm".to_string(),
                from: "test".to_string(),
                permissions: Vec::new(),
                roles: vec!["role:approver".to_string()],
            },
        })
        .await
        .expect("queued");
    let result = engine.run_once().await.expect("the applier ran").remove(0);
    assert_eq!(result.status, State::Applied, "{result:?}");
    assert_eq!(scratch.place_of(CARTE).await, None);
    scratch.drop().await;
}

#[tokio::test]
async fn a_lieu_still_queued_is_a_place_as_the_queue_will_leave_it() {
    let scratch = Scratch::new("queued").await;
    let (engine, app) = scratch.serve().await;

    // The Clairière is saved, and not applied: the tables do not hold it.
    let request = Request::post("/commands")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "dataCapability": "campaign.saveEntity@1",
                "payload": {
                    "campaign": VALOMBRE,
                    "entity": { "kind": "lieu", "name": "La Clairière" },
                },
                "caller": { "by": "dm", "from": "campaign.EntityEditor", "roles": ["role:approver"] },
            })
            .to_string(),
        ))
        .expect("a request");
    let response = app.clone().oneshot(request).await.expect("an answer");
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("a body");
    let saved: Value = serde_json::from_slice(&bytes).expect("JSON");
    let clairiere = saved["partition"]
        .as_str()
        .and_then(|partition| partition.strip_prefix("Entity/"))
        .expect("an Entity partition")
        .to_owned();

    let (status, answer) = place(&app, CARTE, Some(&clairiere), None).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{answer}");

    let settled = engine.run_once().await.expect("the applier ran");
    assert_eq!(settled.len(), 2, "{settled:?}");
    assert!(
        settled.iter().all(|result| result.status == State::Applied),
        "{settled:?}"
    );
    assert_eq!(
        scratch.place_of(CARTE).await.as_deref(),
        Some(clairiere.as_str())
    );
    scratch.drop().await;
}
