//! live.showOnPlayerView over PostgreSQL (`DATABASE_URL`), through the wire
//! protocol: a send is prepared against the live session, the instance and
//! the media the tables hold, and what the engine applies on
//! `Diffusion/<session>` reaches the session's `diffusion` row — its base
//! and its framing, the window as the table holds it. Each test works in a
//! scratch database of its own, migrated by the Data layer, dropped when it
//! ends.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use dataguard_server::{Engine, Planted, planted, router};
use harness_dataguard::command::{CommandResult, State};
use serde_json::{Value, json};
use tokio_postgres::error::SqlState;
use tokio_postgres::{Client, NoTls};
use tower::ServiceExt;

const VALOMBRE: &str = "c1000000-0000-4000-8000-000000000001";
/// Valombre's battlemap.
const CARTE: &str = "d1000000-0000-4000-8000-000000000001";
/// Valombre's illustration of 800 × 1200.
const PORTRAIT: &str = "d1000000-0000-4000-8000-000000000002";
/// Valombre's other illustration.
const BLASON: &str = "d1000000-0000-4000-8000-000000000003";
/// Valombre's session 1, closed.
const SEANCE_1: &str = "51000000-0000-4000-8000-000000000001";
/// Valombre's session 2, live.
const SEANCE: &str = "51000000-0000-4000-8000-000000000002";
/// The tavern's battlemap instance, of session 2.
const INSTANCE: &str = "b1000000-0000-4000-8000-000000000002";
/// The road's battlemap instance, of session 1.
const AUTRE: &str = "b1000000-0000-4000-8000-000000000001";

fn url() -> String {
    std::env::var("DATABASE_URL").expect("DATABASE_URL names the harness's PostgreSQL")
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

fn migrations() -> PathBuf {
    Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../data/migrations"
    ))
    .to_path_buf()
}

async fn connect(url: &str) -> Client {
    let (client, connection) = tokio_postgres::connect(url, NoTls)
        .await
        .expect("PostgreSQL");
    tokio::spawn(connection);
    client
}

/// A database of its own, migrated by the Data layer: Valombre, its closed
/// session 1 and its live session 2, a battlemap with an instance in a
/// scene of each, and two illustrations.
struct Scratch {
    name: String,
    url: String,
    client: Client,
}

impl Scratch {
    async fn new(name: &str) -> Scratch {
        let name = format!("show_on_player_view_test_{name}_{}", std::process::id());
        let admin = connect(&url()).await;
        for sql in [
            format!("DROP DATABASE IF EXISTS {name} WITH (FORCE)"),
            format!("CREATE DATABASE {name}"),
        ] {
            admin.batch_execute(&sql).await.expect("a scratch database");
        }
        let url = naming(&name);
        let mut client = connect(&url).await;
        data::migrate(&mut client, &migrations())
            .await
            .expect("migrated by the Data layer");
        client
            .batch_execute(&format!(
                "INSERT INTO campaign (id, name, kind) VALUES ('{VALOMBRE}', 'Les Cendres de Valombre', 'campagne'); \
                 INSERT INTO media (id, campaign_id, kind, title, file_path, width_px, height_px) VALUES \
                 ('{CARTE}', '{VALOMBRE}', 'battlemap', 'Taverne', 'carte.png', 2048, 1536), \
                 ('{PORTRAIT}', '{VALOMBRE}', 'illustration', 'Morlox', 'morlox.png', 800, 1200), \
                 ('{BLASON}', '{VALOMBRE}', 'illustration', 'Blason', 'blason.png', 512, 512); \
                 INSERT INTO battlemap_grid (media_id, hex_size_px, offset_x, offset_y, orientation, color, opacity) \
                 VALUES ('{CARTE}', 64, 0, 0, 'pointe', '#202020', 0.4); \
                 INSERT INTO session (id, campaign_id, position, title, status, microphone_on) VALUES \
                 ('{SEANCE_1}', '{VALOMBRE}', 1, 'La Route', 'closed', false), \
                 ('{SEANCE}', '{VALOMBRE}', 2, 'Le Sanglier', 'live', false); \
                 INSERT INTO scene (id, session_id, position, title) VALUES \
                 ('5c000000-0000-4000-8000-000000000001', '{SEANCE_1}', 1, 'La route'), \
                 ('5c000000-0000-4000-8000-000000000002', '{SEANCE}', 1, 'Arrivée'); \
                 INSERT INTO battlemap_instance (id, battlemap_media_id, session_id, scene_id) VALUES \
                 ('{AUTRE}', '{CARTE}', '{SEANCE_1}', '5c000000-0000-4000-8000-000000000001'), \
                 ('{INSTANCE}', '{CARTE}', '{SEANCE}', '5c000000-0000-4000-8000-000000000002')"
            ))
            .await
            .expect("Valombre, its sessions, its media and its instances");
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

    /// Session `session`'s diffusion row, as JSON text of its columns:
    /// `kind media instance window zoom panX panY`, a null as `-`.
    async fn diffusion(&self, session: &str) -> Vec<String> {
        self.client
            .query(
                "SELECT concat_ws(' ', base_kind, coalesce(base_media_id::text, '-'), \
                 coalesce(base_instance_id::text, '-'), coalesce(window_media_id::text, '-'), \
                 view_zoom, view_pan_x, view_pan_y) FROM diffusion WHERE session_id = $1::text::uuid",
                &[&session],
            )
            .await
            .expect("the diffusion rows")
            .iter()
            .map(|row| row.get(0))
            .collect()
    }

    /// Drops the database, again while an autovacuum worker the writes woke
    /// finishes on it.
    async fn drop(self) {
        drop(self.client);
        let admin = connect(&url()).await;
        let sql = format!("DROP DATABASE {} WITH (FORCE)", self.name);
        for _ in 0..20 {
            match admin.batch_execute(&sql).await {
                Ok(()) => return,
                Err(e) if e.code() == Some(&SqlState::INSUFFICIENT_PRIVILEGE) => {
                    tokio::time::sleep(std::time::Duration::from_millis(250)).await;
                }
                Err(e) => panic!("the scratch database dropped: {e:?}"),
            }
        }
        panic!("the scratch database {} is still in use", self.name);
    }
}

/// `POST /commands` of live.showOnPlayerView by the DM, approver of their
/// own campaign's critical fields: the status and the answer.
async fn post(app: &Router, payload: Value) -> (StatusCode, Value) {
    let body = json!({
        "dataCapability": "live.showOnPlayerView@1",
        "target": { "aggregate": "Diffusion" },
        "payload": payload,
        "caller": { "by": "dm", "from": "Battlemap", "roles": ["role:approver"] },
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

/// Sends `payload`: the answer, and what the applier made of it.
async fn sent(engine: &Engine, app: &Router, payload: Value) -> (Value, CommandResult) {
    let (status, answer) = post(app, payload).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{answer}");
    let mut settled = engine.run_once().await.expect("the applier ran");
    assert_eq!(settled.len(), 1, "{settled:?}");
    (answer, settled.remove(0))
}

#[tokio::test]
async fn the_first_send_makes_the_live_session_s_row_and_a_battlemap_then_replaces_its_base() {
    let s = Scratch::new("sends").await;
    let (engine, app) = s.serve().await;
    let (answer, result) = sent(
        &engine,
        &app,
        json!({ "campaign": VALOMBRE, "kind": "illustration", "media": PORTRAIT }),
    )
    .await;
    assert_eq!(result.status, State::Applied, "{result:?}");
    assert_eq!(
        answer["partition"],
        format!("Diffusion/{SEANCE}"),
        "{answer}"
    );
    assert_eq!(
        s.diffusion(SEANCE).await,
        [format!("illustration {PORTRAIT} - - 1.000 0.00 0.00")]
    );
    // As the Battlemap sends it: no campaign, the instance's is the one.
    let (_, result) = sent(
        &engine,
        &app,
        json!({ "kind": "battlemap", "instance": INSTANCE, "zoom": 1.5, "panX": -120, "panY": 40 }),
    )
    .await;
    assert_eq!(result.status, State::Applied, "{result:?}");
    assert_eq!(
        s.diffusion(SEANCE).await,
        [format!("battlemap - {INSTANCE} - 1.500 -120.00 40.00")]
    );
    s.drop().await;
}

#[tokio::test]
async fn an_instance_of_another_session_is_rejected_under_base_coherent_and_the_row_stays() {
    let s = Scratch::new("other_session").await;
    let (engine, app) = s.serve().await;
    sent(
        &engine,
        &app,
        json!({ "campaign": VALOMBRE, "kind": "illustration", "media": PORTRAIT }),
    )
    .await;
    let before = s.diffusion(SEANCE).await;
    let (answer, result) = sent(
        &engine,
        &app,
        json!({ "kind": "battlemap", "instance": AUTRE, "zoom": 2, "panX": 0, "panY": 0 }),
    )
    .await;
    assert_eq!(result.status, State::Rejected, "{result:?}");
    assert_eq!(result.violations, ["base-coherent"], "{result:?}");
    assert!(
        answer["warnings"].as_array().is_some_and(|w| w
            .iter()
            .any(|w| w == show_on_player_view::NOT_OF_THE_SESSION)),
        "{answer}"
    );
    assert_eq!(s.diffusion(SEANCE).await, before);
    assert!(s.diffusion(SEANCE_1).await.is_empty());
    s.drop().await;
}

#[tokio::test]
async fn a_battlemap_media_sent_as_an_illustration_is_rejected_under_base_coherent() {
    let s = Scratch::new("not_illustration").await;
    let (engine, app) = s.serve().await;
    let (_, result) = sent(
        &engine,
        &app,
        json!({ "campaign": VALOMBRE, "kind": "illustration", "media": CARTE }),
    )
    .await;
    assert_eq!(result.status, State::Rejected, "{result:?}");
    assert_eq!(result.violations, ["base-coherent"], "{result:?}");
    assert!(s.diffusion(SEANCE).await.is_empty());
    s.drop().await;
}

#[tokio::test]
async fn a_send_keeps_the_window_the_table_holds() {
    let s = Scratch::new("window").await;
    s.client
        .batch_execute(&format!(
            "INSERT INTO diffusion (session_id, base_kind, window_media_id, window_x, window_y, \
             window_width, view_zoom, view_pan_x, view_pan_y) \
             VALUES ('{SEANCE}', 'cover', '{BLASON}', 0.1, 0.2, 0.3, 1, 0, 0)"
        ))
        .await
        .expect("a window open over the cover");
    let (engine, app) = s.serve().await;
    let (_, result) = sent(
        &engine,
        &app,
        json!({ "campaign": VALOMBRE, "kind": "illustration", "media": PORTRAIT, "zoom": 1.25 }),
    )
    .await;
    assert_eq!(result.status, State::Applied, "{result:?}");
    assert_eq!(
        s.diffusion(SEANCE).await,
        [format!(
            "illustration {PORTRAIT} - {BLASON} 1.250 0.00 0.00"
        )]
    );
    let window: Vec<String> = s
        .client
        .query(
            "SELECT concat_ws(' ', window_x, window_y, window_width) FROM diffusion",
            &[],
        )
        .await
        .expect("the window")
        .iter()
        .map(|row| row.get(0))
        .collect();
    assert_eq!(window, ["0.1000 0.2000 0.3000"]);
    s.drop().await;
}

#[tokio::test]
async fn an_illustration_sent_with_no_campaign_shows_in_its_campaign_s_live_session() {
    let s = Scratch::new("media_campaign").await;
    let (engine, app) = s.serve().await;
    let (_, result) = sent(
        &engine,
        &app,
        json!({ "kind": "illustration", "media": BLASON }),
    )
    .await;
    assert_eq!(result.status, State::Applied, "{result:?}");
    assert_eq!(
        s.diffusion(SEANCE).await,
        [format!("illustration {BLASON} - - 1.000 0.00 0.00")]
    );
    s.drop().await;
}

#[tokio::test]
async fn an_instance_the_tables_do_not_hold_is_unknown_before_it_is_queued() {
    let s = Scratch::new("unknown").await;
    let (engine, app) = s.serve().await;
    let (status, answer) = post(
        &app,
        json!({ "kind": "battlemap", "instance": "b1000000-0000-4000-8000-0000000000ff" }),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{answer}");
    assert_eq!(answer, json!({ "error": "instance n'existe pas" }));
    assert!(engine.run_once().await.expect("the applier ran").is_empty());
    s.drop().await;
}

#[tokio::test]
async fn a_campaign_with_no_live_session_is_refused_before_it_is_queued() {
    let s = Scratch::new("no_live").await;
    s.client
        .batch_execute(&format!(
            "UPDATE session SET status = 'closed' WHERE id = '{SEANCE}'"
        ))
        .await
        .expect("session 2 closed");
    let (engine, app) = s.serve().await;
    let (status, answer) = post(
        &app,
        json!({ "campaign": VALOMBRE, "kind": "illustration", "media": PORTRAIT }),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{answer}");
    assert!(engine.run_once().await.expect("the applier ran").is_empty());
    assert!(s.diffusion(SEANCE).await.is_empty());
    s.drop().await;
}
