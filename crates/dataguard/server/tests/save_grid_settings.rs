//! media.saveGridSettings over PostgreSQL (`DATABASE_URL`), through the wire
//! protocol as the GridSettings panel sends it: a save of a battlemap the
//! tables hold — one no command wrote before — is judged against its media's
//! row as the tables hold it, and its six settings reach `battlemap_grid`,
//! the feet per hex still 5 (`hex-is-five-feet`). A save on an illustration
//! is rejected under `grid-only-for-battlemap`, a hex size of 0 or an opacity
//! of 1.5 under `grid-values-range`, and a payload carrying another field is
//! refused before it is queued; two saves at once both queue and both apply.
//! Each test works in a scratch database of its own, migrated as the server
//! migrates it, dropped when it ends.

use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use dataguard_server::{Engine, Planted, planted, router};
use harness_dataguard::command::{CommandResult, State};
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
/// The tavern's battlemap, uploaded with its default grid.
const CARTE: &str = "d1000000-0000-4000-8000-00000000000a";
/// Another battlemap, whose grid no save of the tavern's touches.
const ECUEIL: &str = "d1000000-0000-4000-8000-00000000000b";
/// An illustration: no grid.
const PORTRAIT: &str = "d1000000-0000-4000-8000-00000000000c";
/// No media at all.
const NOWHERE: &str = "d1000000-0000-4000-8000-0000000000ff";

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

/// A database of its own, migrated, holding Valombre's two battlemaps with
/// their grids and its illustration, as an upload leaves them.
struct Scratch {
    name: String,
    url: String,
    client: Client,
}

impl Scratch {
    async fn new(name: &str) -> Scratch {
        let name = format!("save_grid_settings_test_{name}_{}", std::process::id());
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
                "INSERT INTO campaign (id, name, kind) VALUES ('{VALOMBRE}', 'Les Cendres de Valombre', 'campagne');
                 INSERT INTO media (id, campaign_id, kind, title, file_path, width_px, height_px) VALUES
                     ('{CARTE}', '{VALOMBRE}', 'battlemap', 'La Taverne', 'media/taverne.png', 900, 600),
                     ('{ECUEIL}', '{VALOMBRE}', 'battlemap', 'L''Écueil', 'media/ecueil.png', 1200, 800),
                     ('{PORTRAIT}', '{VALOMBRE}', 'illustration', 'Elowen', 'media/elowen.png', 400, 400);
                 INSERT INTO battlemap_grid (media_id, hex_size_px, offset_x, offset_y, orientation, color, opacity) VALUES
                     ('{CARTE}', 64, 0, 0, 'pointe', 'accent', 0.4),
                     ('{ECUEIL}', 48.5, 3, 4, 'plat', 'danger', 1);"
            ))
            .await
            .expect("Valombre and its media");
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

    /// Every grid the table holds, each row as JSON text, by media.
    async fn grids(&self) -> Vec<String> {
        self.client
            .query(
                "SELECT to_jsonb(g)::text FROM battlemap_grid g ORDER BY media_id",
                &[],
            )
            .await
            .expect("the grids")
            .iter()
            .map(|row| row.get(0))
            .collect()
    }

    /// `media`'s grid as the table holds it: its six settings and its feet
    /// per hex, as JSON.
    async fn grid(&self, media: &str) -> Option<Value> {
        self.client
            .query_opt(
                "SELECT jsonb_build_object('hexSizePx', hex_size_px::float8, 'offsetX', offset_x::float8,
                     'offsetY', offset_y::float8, 'orientation', orientation, 'color', color,
                     'opacity', opacity::float8, 'feetPerHex', feet_per_hex)::text
                 FROM battlemap_grid WHERE media_id = ($1::text)::uuid",
                &[&media],
            )
            .await
            .expect("a grid")
            .map(|row| serde_json::from_str(row.get::<_, &str>(0)).expect("JSON"))
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

/// `body` posted to `/commands` as the GridSettings panel sends it, and the
/// answer.
async fn post(app: &Router, target: &str, payload: Value) -> (StatusCode, Value) {
    send(
        app,
        json!({
            "dataCapability": "media.saveGridSettings@1",
            "target": { "aggregate": "Media", "id": target },
            "payload": payload,
        }),
    )
    .await
}

/// `body` posted to `/commands` as the GridSettings panel, and the answer.
async fn send(app: &Router, mut body: Value) -> (StatusCode, Value) {
    body["caller"] = json!({ "by": "dm", "from": "GridSettings", "roles": [] });
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

/// A save of `target`'s grid, queued then run by the applier: what it made
/// of it.
async fn save(engine: &Engine, app: &Router, target: &str, payload: Value) -> CommandResult {
    let (status, answer) = post(app, target, payload).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{answer}");
    let mut settled = engine.run_once().await.expect("the applier ran");
    assert_eq!(settled.len(), 1, "{settled:?}");
    settled.remove(0)
}

/// The save of the acceptance criteria: hex size 48, offsets 30/25, flat,
/// a colour, opacity 0.2.
fn adjusted() -> Value {
    json!({ "size": 48, "ox": 30, "oy": 25, "orient": "flat", "color": "#c8a24b", "opacity": 0.2 })
}

fn rejected_by(result: &CommandResult, id: &str) {
    assert_eq!(result.status, State::Rejected, "{result:?}");
    assert_eq!(result.violations, [id], "{result:?}");
}

#[tokio::test]
async fn hex_is_five_feet_a_save_holds_exactly_its_values_and_a_hex_is_still_five_feet() {
    let scratch = Scratch::new("saved").await;
    let (engine, app) = scratch.serve().await;
    let ecueil = scratch.grid(ECUEIL).await;

    // The id in whatever case the request used: one battlemap, one row.
    let result = save(&engine, &app, &CARTE.to_uppercase(), adjusted()).await;
    assert_eq!(result.status, State::Applied, "{result:?}");
    assert_eq!(
        scratch.grid(CARTE).await,
        Some(json!({
            "hexSizePx": 48.0,
            "offsetX": 30.0,
            "offsetY": 25.0,
            "orientation": "plat",
            "color": "#c8a24b",
            "opacity": 0.2,
            "feetPerHex": 5,
        }))
    );
    assert_eq!(
        scratch.grid(ECUEIL).await,
        ecueil,
        "another battlemap's grid"
    );
    assert_eq!(scratch.grid(PORTRAIT).await, None, "an illustration's grid");
    scratch.drop().await;
}

#[tokio::test]
async fn grid_only_for_battlemap_rejects_a_save_on_an_illustration_or_on_no_media() {
    let scratch = Scratch::new("illustration").await;
    let (engine, app) = scratch.serve().await;
    let before = scratch.grids().await;

    for target in [PORTRAIT, NOWHERE] {
        let result = save(&engine, &app, target, adjusted()).await;
        rejected_by(&result, "grid-only-for-battlemap");
    }
    assert_eq!(scratch.grids().await, before);
    scratch.drop().await;
}

#[tokio::test]
async fn grid_values_range_rejects_a_hex_size_of_zero_or_an_opacity_of_one_and_a_half() {
    let scratch = Scratch::new("range").await;
    let (engine, app) = scratch.serve().await;
    let before = scratch.grids().await;

    for (field, value) in [("size", json!(0)), ("opacity", json!(1.5))] {
        let mut payload = adjusted();
        payload[field] = value;
        let result = save(&engine, &app, CARTE, payload).await;
        rejected_by(&result, "grid-values-range");
    }
    assert_eq!(scratch.grids().await, before);
    scratch.drop().await;
}

#[tokio::test]
async fn a_payload_carrying_the_feet_per_hex_or_another_field_is_refused_and_not_queued() {
    let scratch = Scratch::new("payload").await;
    let (engine, app) = scratch.serve().await;
    let before = scratch.grids().await;

    for (field, value) in [
        ("feetPerHex", json!(10)),
        ("feet_per_hex", json!(10)),
        ("title", json!("Une autre carte")),
    ] {
        let mut payload = adjusted();
        payload[field] = value;
        let (status, answer) = post(&app, CARTE, payload).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{answer}");
        assert!(
            answer["error"].as_str().is_some_and(|e| e.contains(field)),
            "{answer}"
        );
    }
    assert!(engine.run_once().await.expect("the applier ran").is_empty());
    assert_eq!(scratch.grids().await, before);
    scratch.drop().await;
}

#[tokio::test]
async fn a_save_naming_its_battlemap_by_the_payload_s_id_is_applied_and_one_naming_none_is_refused()
{
    let scratch = Scratch::new("target").await;
    let (engine, app) = scratch.serve().await;

    let mut payload = adjusted();
    let (status, answer) = send(
        &app,
        json!({ "dataCapability": "media.saveGridSettings@1", "payload": payload }),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{answer}");
    assert!(engine.run_once().await.expect("the applier ran").is_empty());

    payload["id"] = json!(CARTE);
    let (status, answer) = send(
        &app,
        json!({ "dataCapability": "media.saveGridSettings@1", "payload": payload }),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{answer}");
    assert_eq!(answer["partition"], format!("Media/{CARTE}"), "{answer}");
    let result = engine.run_once().await.expect("the applier ran").remove(0);
    assert_eq!(result.status, State::Applied, "{result:?}");
    let grid = scratch.grid(CARTE).await.expect("the grid");
    assert_eq!(grid["hexSizePx"], 48.0);
    scratch.drop().await;
}

#[tokio::test]
async fn two_saves_at_once_both_queue_on_the_battlemap_and_neither_is_rejected() {
    let scratch = Scratch::new("concurrent").await;
    let (engine, app) = scratch.serve().await;

    let mut later = adjusted();
    later["size"] = json!(52);
    later["orient"] = json!("pointy");
    let (first, second) = tokio::join!(
        post(&app, CARTE, adjusted()),
        post(&app, CARTE, later.clone()),
    );
    for (status, answer) in [&first, &second] {
        assert_eq!(*status, StatusCode::ACCEPTED, "{answer}");
        assert_eq!(answer["partition"], format!("Media/{CARTE}"), "{answer}");
    }
    let mut settled = Vec::new();
    while settled.len() < 2 {
        let applied = engine.run_once().await.expect("the applier ran");
        assert!(!applied.is_empty(), "both saves settle: {settled:?}");
        settled.extend(applied);
    }
    for result in &settled {
        assert_eq!(result.status, State::Applied, "{result:?}");
    }
    // The later of the two is the grid kept.
    let last = settled.last().expect("a save");
    let kept = if last.command_id == first.1["commandId"] {
        adjusted()
    } else {
        later
    };
    let grid = scratch.grid(CARTE).await.expect("the grid");
    assert_eq!(grid["hexSizePx"], json!(kept["size"].as_f64()));
    assert_eq!(grid["feetPerHex"], 5);
    scratch.drop().await;
}

#[tokio::test]
async fn a_saved_grid_the_table_lost_is_written_again_at_the_next_start() {
    let scratch = Scratch::new("repair").await;
    {
        let (engine, app) = scratch.serve().await;
        let result = save(&engine, &app, CARTE, adjusted()).await;
        assert_eq!(result.status, State::Applied, "{result:?}");
    }
    scratch
        .client
        .batch_execute(&format!(
            "UPDATE battlemap_grid SET hex_size_px = 64, orientation = 'pointe' WHERE media_id = '{CARTE}'"
        ))
        .await
        .expect("the grid lost");

    let _restarted = scratch.serve().await;
    let grid = scratch.grid(CARTE).await.expect("the grid");
    assert_eq!(grid["hexSizePx"], 48.0);
    assert_eq!(grid["orientation"], "plat");
    scratch.drop().await;
}
