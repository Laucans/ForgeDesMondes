//! soundbox.addSound over PostgreSQL (`DATABASE_URL`): an addition of a
//! sound to a campaign the tables hold — one the fixtures wrote, which no
//! command wrote before — reaches the `sound` table, its tags normalised and
//! its volume a fraction of 1; one the DataGuard rejects reaches nothing, and
//! one of a campaign the tables do not hold is refused before it is queued.
//! A sound the table lost is added again at the next start. Each test works
//! in a scratch database of its own, migrated as the server migrates it,
//! dropped when it ends.

use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use dataguard_server::{Engine, Planted, planted, router};
use harness_dataguard::command::State;
use serde_json::{Value, json};
use tokio_postgres::{Client, NoTls};
use tower::ServiceExt;

const MIGRATIONS: [&str; 2] = [
    include_str!("../../../data/migrations/0002_campaign.sql"),
    include_str!("../../../data/migrations/0007_sound.sql"),
];

const VALOMBRE: &str = "c1000000-0000-4000-8000-000000000001";
const NOWHERE: &str = "c1000000-0000-4000-8000-0000000000ff";

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

/// A database of its own, migrated, holding Valombre as the fixtures write
/// it.
struct Scratch {
    name: String,
    url: String,
    client: Client,
}

impl Scratch {
    async fn new(name: &str) -> Scratch {
        let name = format!("add_sound_test_{name}_{}", std::process::id());
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
        client
            .batch_execute(&format!(
                "INSERT INTO campaign (id, name, kind) VALUES ('{VALOMBRE}', 'Les Cendres de Valombre', 'campagne')"
            ))
            .await
            .expect("Valombre");
        Scratch { name, url, client }
    }

    /// The engine as the server plants it over this database — which
    /// repairs what the tables lost — and its wire protocol.
    async fn serve(&self) -> (Arc<Engine>, Router) {
        let Planted { engine, rows } = planted(&self.url)
            .await
            .expect("the engine over PostgreSQL");
        let engine = Arc::new(engine);
        let app = router(
            engine.clone(),
            rows,
            Arc::new(ports_llm::fake::FakeLlm::new()),
        );
        (engine, app)
    }

    /// The `sound` row of `id` — its campaign, name, file, tags and volume —
    /// when the table holds it.
    async fn sound(&self, id: &str) -> Option<Value> {
        self.client
            .query_opt(
                "SELECT jsonb_build_object('campaign', campaign_id, 'name', name,
                     'file', file_path, 'tags', to_jsonb(tags), 'volume', volume::text)::text
                 FROM sound WHERE id = ($1::text)::uuid",
                &[&id],
            )
            .await
            .expect("the sound table")
            .map(|row| serde_json::from_str(row.get::<_, &str>(0)).expect("JSON"))
    }

    async fn sounds(&self) -> i64 {
        self.client
            .query_one("SELECT count(*) FROM sound", &[])
            .await
            .expect("the sound table")
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

/// `POST /commands` of soundbox.addSound by the DM's library: the status and
/// the answer.
async fn add(app: &Router, payload: Value) -> (StatusCode, Value) {
    let body = json!({
        "dataCapability": "soundbox.addSound@1",
        "payload": payload,
        "caller": { "by": "dm", "from": "SoundLibrary", "roles": ["role:approver"] },
    });
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

/// Adds `payload` and runs the applier once: the sound's id, and its status.
async fn added(engine: &Engine, app: &Router, payload: Value) -> (String, State) {
    let (status, answer) = add(app, payload).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{answer}");
    let sound = answer["partition"]
        .as_str()
        .and_then(|p| p.strip_prefix("Sound/"))
        .unwrap_or_else(|| panic!("a Sound partition: {answer}"))
        .to_owned();
    let mut settled = engine.run_once().await.expect("the applier ran");
    assert_eq!(settled.len(), 1, "{settled:?}");
    (sound, settled.remove(0).status)
}

fn crowd(volume: Value) -> Value {
    json!({
        "campaign": VALOMBRE,
        "name": "Bruit de foule",
        "tags": ["Taverne", "taverne "],
        "volume": volume,
        "file": "son.ogg",
    })
}

#[tokio::test]
async fn a_sound_added_to_a_campaign_of_the_fixtures_reaches_the_sound_table() {
    let scratch = Scratch::new("reaches").await;
    let (engine, app) = scratch.serve().await;
    let (sound, status) = added(&engine, &app, crowd(json!(55))).await;
    assert_eq!(status, State::Applied);
    assert_eq!(
        scratch.sound(&sound).await,
        Some(json!({
            "campaign": VALOMBRE,
            "name": "Bruit de foule",
            "file": "son.ogg",
            "tags": ["taverne"],
            "volume": "0.55",
        }))
    );
    scratch.drop().await;
}

#[tokio::test]
async fn a_sound_rejected_by_sound_volume_range_reaches_no_table() {
    let scratch = Scratch::new("rejected").await;
    let (engine, app) = scratch.serve().await;
    let (sound, status) = added(&engine, &app, crowd(json!(120))).await;
    assert_eq!(status, State::Rejected);
    assert_eq!(scratch.sound(&sound).await, None);
    assert_eq!(scratch.sounds().await, 0);
    scratch.drop().await;
}

#[tokio::test]
async fn a_sound_of_a_campaign_the_tables_do_not_hold_is_refused_404() {
    let scratch = Scratch::new("nowhere").await;
    let (engine, app) = scratch.serve().await;
    let mut payload = crowd(json!(55));
    payload["campaign"] = json!(NOWHERE);
    let (status, answer) = add(&app, payload).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{answer}");
    assert!(engine.run_once().await.expect("the applier ran").is_empty());
    assert_eq!(scratch.sounds().await, 0);
    scratch.drop().await;
}

#[tokio::test]
async fn a_sound_the_table_lost_is_added_again_at_the_next_start() {
    let scratch = Scratch::new("repair").await;
    let (engine, app) = scratch.serve().await;
    let (sound, status) = added(&engine, &app, crowd(json!(80))).await;
    assert_eq!(status, State::Applied);
    drop((engine, app));
    scratch
        .client
        .execute("DELETE FROM sound WHERE id = ($1::text)::uuid", &[&sound])
        .await
        .expect("the row lost");
    assert_eq!(scratch.sound(&sound).await, None);
    let _restarted = scratch.serve().await;
    let row = scratch.sound(&sound).await.expect("the sound added again");
    assert_eq!(row["volume"], json!("0.80"));
    assert_eq!(row["tags"], json!(["taverne"]));
    scratch.drop().await;
}
