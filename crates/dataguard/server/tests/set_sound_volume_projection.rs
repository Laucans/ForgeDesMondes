//! soundbox.setSoundVolume over PostgreSQL (`DATABASE_URL`): a sound's own
//! volume applied on `Sound/<id>` reaches its row of the `sound` table, and
//! nothing else of it nor of another sound; one rejected by
//! `sound-volume-range` writes nothing. Each test works in a scratch database
//! of its own, migrated as the server migrates it, dropped when it ends.

use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use dataguard_server::{Engine, Planted, planted, router};
use harness_dataguard::command::{CommandResult, State};
use serde_json::{Value, json};
use tokio_postgres::{Client, NoTls};
use tower::ServiceExt;

const MIGRATIONS: [&str; 2] = [
    include_str!("../../../data/migrations/0002_campaign.sql"),
    include_str!("../../../data/migrations/0007_sound.sql"),
];

const VALOMBRE: &str = "c1000000-0000-4000-8000-000000000001";

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

/// A database of its own, migrated, holding Valombre.
struct Scratch {
    name: String,
    url: String,
    client: Client,
}

impl Scratch {
    async fn new(name: &str) -> Scratch {
        let name = format!("set_sound_volume_test_{name}_{}", std::process::id());
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
                "INSERT INTO campaign (id, name, kind) VALUES ('{VALOMBRE}', 'Les Cendres de Valombre', 'campagne');"
            ))
            .await
            .expect("Valombre");
        Scratch { name, url, client }
    }

    /// The engine as the server plants it over this database, and its wire
    /// protocol.
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

    /// The `sound` row `id` names: its name, file, tags and volume.
    async fn sound(&self, id: &str) -> Value {
        let row = self
            .client
            .query_one(
                "SELECT jsonb_build_object('name', name, 'file', file_path, 'tags', to_jsonb(tags),
                    'volume', volume)::text FROM sound WHERE id = $1::text::uuid",
                &[&id],
            )
            .await
            .expect("the sound's row");
        serde_json::from_str(row.get::<_, &str>(0)).expect("JSON")
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

/// `POST /commands` of `data_capability` by the DM: the answer, and what the
/// applier made of it.
async fn command(
    engine: &Engine,
    app: &Router,
    data_capability: &str,
    mut body: Value,
) -> (Value, CommandResult) {
    body["dataCapability"] = json!(data_capability);
    body["caller"] = json!({ "by": "dm", "from": "LiveSoundbox", "roles": ["role:approver"] });
    let request = Request::post("/commands")
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .expect("a request");
    let response = app.clone().oneshot(request).await.expect("an answer");
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("a body");
    let answer: Value = serde_json::from_slice(&bytes).expect("JSON");
    assert_eq!(status, StatusCode::ACCEPTED, "{answer}");
    let mut settled = engine.run_once().await.expect("the applier ran");
    assert_eq!(settled.len(), 1, "{settled:?}");
    (answer, settled.remove(0))
}

/// The sound `name` at 80 %, added by soundbox.addSound and applied: its id.
async fn add(engine: &Engine, app: &Router, name: &str) -> String {
    let (answer, result) = command(
        engine,
        app,
        "soundbox.addSound@1",
        json!({ "payload": {
            "campaign": VALOMBRE,
            "name": name,
            "tags": ["bois"],
            "volume": 80,
            "file": "sons/porte.ogg",
        } }),
    )
    .await;
    assert_eq!(result.status, State::Applied, "{result:?}");
    answer["partition"]
        .as_str()
        .and_then(|p| p.strip_prefix("Sound/"))
        .expect("a Sound partition")
        .to_owned()
}

/// The slider of the sound `id` moved to `volume` percent: what the applier
/// made of it.
async fn slide(engine: &Engine, app: &Router, id: &str, volume: Value) -> CommandResult {
    command(
        engine,
        app,
        "soundbox.setSoundVolume@1",
        json!({ "target": { "aggregate": "Sound", "id": id }, "payload": { "volume": volume } }),
    )
    .await
    .1
}

/// A sound as `add` leaves it in the table.
fn added(name: &str) -> Value {
    json!({ "name": name, "file": "sons/porte.ogg", "tags": ["bois"], "volume": 0.8 })
}

#[tokio::test]
async fn a_sound_at_80_set_to_40_is_stored_0_4_and_nothing_else_changes() {
    let scratch = Scratch::new("applied").await;
    let (engine, app) = scratch.serve().await;
    let door = add(&engine, &app, "Porte").await;
    let crowd = add(&engine, &app, "Foule").await;
    let result = slide(&engine, &app, &door, json!(40)).await;
    assert_eq!(result.status, State::Applied, "{result:?}");
    assert_eq!(
        scratch.sound(&door).await,
        json!({ "name": "Porte", "file": "sons/porte.ogg", "tags": ["bois"], "volume": 0.4 })
    );
    assert_eq!(scratch.sound(&crowd).await, added("Foule"));
    scratch.drop().await;
}

#[tokio::test]
async fn a_volume_of_101_is_rejected_by_sound_volume_range_and_reaches_no_table() {
    let scratch = Scratch::new("rejected").await;
    let (engine, app) = scratch.serve().await;
    let door = add(&engine, &app, "Porte").await;
    let result = slide(&engine, &app, &door, json!(101)).await;
    assert_eq!(result.status, State::Rejected, "{result:?}");
    assert_eq!(result.violations, ["sound-volume-range"]);
    assert_eq!(scratch.sound(&door).await, added("Porte"));
    scratch.drop().await;
}
