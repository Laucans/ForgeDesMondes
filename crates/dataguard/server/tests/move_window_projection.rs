//! live.moveWindow over PostgreSQL (`DATABASE_URL`), through the wire
//! protocol: a move is prepared against the campaign's live session, and
//! what the engine applies on `Diffusion/<session>` reaches the session's
//! `diffusion` row — its x and y set, its width, its media, its base and its
//! framing as they were. Each test works in a scratch database of its own,
//! migrated by the Data layer, dropped when it ends.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use dataguard_server::{Engine, Planted, planted, router};
use harness_dataguard::command::State;
use serde_json::{Value, json};
use tokio_postgres::error::SqlState;
use tokio_postgres::{Client, NoTls};
use tower::ServiceExt;

const VALOMBRE: &str = "c1000000-0000-4000-8000-000000000001";
/// A campaign with no live session.
const BRUMES: &str = "c1000000-0000-4000-8000-000000000002";
/// Valombre's battlemap: a media, but no illustration.
const CARTE: &str = "d1000000-0000-4000-8000-000000000001";
/// Valombre's uploaded illustration of 800 × 1200.
const PORTRAIT: &str = "d1000000-0000-4000-8000-000000000002";
/// An illustration of Brumes.
const BRUMES_PORTRAIT: &str = "d1000000-0000-4000-8000-000000000003";
/// Valombre's session 1, closed.
const SEANCE_1: &str = "51000000-0000-4000-8000-000000000001";
/// Valombre's session 2, live.
const SEANCE: &str = "51000000-0000-4000-8000-000000000002";
/// The tavern's battlemap instance, of session 2.
const INSTANCE: &str = "b1000000-0000-4000-8000-000000000002";

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
/// scene of session 2, an illustration of 800 × 1200; Brumes, with no
/// session and an illustration of its own.
struct Scratch {
    name: String,
    url: String,
    client: Client,
}

impl Scratch {
    async fn new(name: &str) -> Scratch {
        let name = format!("move_window_test_{name}_{}", std::process::id());
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
                "INSERT INTO campaign (id, name, kind) VALUES \
                 ('{VALOMBRE}', 'Les Cendres de Valombre', 'campagne'), \
                 ('{BRUMES}', 'Les Brumes', 'oneshot'); \
                 INSERT INTO media (id, campaign_id, kind, title, file_path, width_px, height_px) VALUES \
                 ('{CARTE}', '{VALOMBRE}', 'battlemap', 'Taverne', 'carte.png', 2048, 1536), \
                 ('{PORTRAIT}', '{VALOMBRE}', 'illustration', 'Morlox', 'morlox.png', 800, 1200), \
                 ('{BRUMES_PORTRAIT}', '{BRUMES}', 'illustration', 'Brume', 'brume.png', 800, 1200); \
                 INSERT INTO battlemap_grid (media_id, hex_size_px, offset_x, offset_y, orientation, color, opacity) \
                 VALUES ('{CARTE}', 64, 0, 0, 'pointe', '#202020', 0.4); \
                 INSERT INTO session (id, campaign_id, position, title, status, microphone_on) VALUES \
                 ('{SEANCE_1}', '{VALOMBRE}', 1, 'La Route', 'closed', false), \
                 ('{SEANCE}', '{VALOMBRE}', 2, 'Le Sanglier', 'live', false); \
                 INSERT INTO scene (id, session_id, position, title) VALUES \
                 ('5c000000-0000-4000-8000-000000000002', '{SEANCE}', 1, 'Arrivée'); \
                 INSERT INTO battlemap_instance (id, battlemap_media_id, session_id, scene_id) VALUES \
                 ('{INSTANCE}', '{CARTE}', '{SEANCE}', '5c000000-0000-4000-8000-000000000002')"
            ))
            .await
            .expect("Valombre, its sessions, its media and its instance");
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

    /// Session 2 shows the tavern's instance, framed, no window open — as a
    /// send to the player view leaves its row — written a minute ago.
    async fn showing_a_battlemap(&self) {
        self.client
            .batch_execute(&format!(
                "INSERT INTO diffusion (session_id, base_kind, base_instance_id, \
                 view_zoom, view_pan_x, view_pan_y, updated_at) \
                 VALUES ('{SEANCE}', 'battlemap', '{INSTANCE}', 1.5, -120, 40, \
                 now() - interval '1 minute')"
            ))
            .await
            .expect("session 2's diffusion row");
    }

    /// Session `session`'s diffusion row, as text of its columns:
    /// `kind media instance window x y width zoom panX panY`, a null as `-`.
    async fn diffusion(&self, session: &str) -> Vec<String> {
        self.client
            .query(
                "SELECT concat_ws(' ', base_kind, coalesce(base_media_id::text, '-'), \
                 coalesce(base_instance_id::text, '-'), coalesce(window_media_id::text, '-'), \
                 coalesce(window_x::text, '-'), coalesce(window_y::text, '-'), \
                 coalesce(window_width::text, '-'), view_zoom, view_pan_x, view_pan_y) \
                 FROM diffusion WHERE session_id = $1::text::uuid",
                &[&session],
            )
            .await
            .expect("the diffusion rows")
            .iter()
            .map(|row| row.get(0))
            .collect()
    }

    /// Every diffusion row the table holds.
    async fn diffusions(&self) -> i64 {
        self.client
            .query_one("SELECT count(*) FROM diffusion", &[])
            .await
            .expect("the diffusion rows")
            .get(0)
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

/// `POST /commands` of `data_capability` with `payload` by the DM from
/// their player-view tab, holding `roles`: the status and the answer.
async fn post(
    app: &Router,
    data_capability: &str,
    payload: Value,
    roles: &[&str],
) -> (StatusCode, Value) {
    let body = json!({
        "dataCapability": data_capability,
        "target": { "aggregate": "Diffusion" },
        "payload": payload,
        "caller": { "by": "dm", "from": "PlayerStage", "roles": roles },
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

/// What the applier made of the one command queued: its state and its
/// violations.
async fn settled(engine: &Engine) -> (State, Vec<String>) {
    let mut settled = engine.run_once().await.expect("the applier ran");
    assert_eq!(settled.len(), 1, "{settled:?}");
    let result = settled.remove(0);
    (result.status, result.violations)
}

/// Opens the portrait in the window of Valombre's player view, by the DM,
/// approver of their own table — as live.showWindowOnPlayerView leaves it.
async fn opened(engine: &Engine, app: &Router) {
    let (status, answer) = post(
        app,
        "live.showWindowOnPlayerView@1",
        json!({ "campaign": VALOMBRE, "media": PORTRAIT }),
        &["role:approver"],
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{answer}");
    assert_eq!(settled(engine).await.0, State::Applied);
}

/// Drags Valombre's window to `x`, `y`, by the DM, who holds no role: the
/// answer, and the state the applier left the command in with its
/// violations.
async fn moved(engine: &Engine, app: &Router, x: f64, y: f64) -> (Value, State, Vec<String>) {
    let (status, answer) = post(
        app,
        "live.moveWindow@1",
        json!({ "campaign": VALOMBRE, "x": x, "y": y }),
        &[],
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{answer}");
    let (state, violations) = settled(engine).await;
    (answer, state, violations)
}

#[tokio::test]
async fn a_drag_of_the_open_window_reaches_the_live_session_s_row_its_width_kept() {
    let scratch = Scratch::new("drag").await;
    let (engine, app) = scratch.serve().await;
    opened(&engine, &app).await;
    let (answer, state, _) = moved(&engine, &app, 0.12, 0.08).await;
    assert_eq!(state, State::Applied, "{answer}");
    assert_eq!(answer["partition"], format!("Diffusion/{SEANCE}"));
    // window_x and window_y moved; the width, the media, the base and the
    // framing as they were.
    assert_eq!(
        scratch.diffusion(SEANCE).await,
        [format!(
            "cover - - {PORTRAIT} 0.1200 0.0800 0.3000 1.000 0.00 0.00"
        )]
    );
    // single-diffusion-source: one row, the live session's.
    assert_eq!(scratch.diffusions().await, 1);
    assert!(scratch.diffusion(SEANCE_1).await.is_empty());
    scratch.drop().await;
}

#[tokio::test]
async fn drags_in_a_row_each_reach_the_row_the_last_one_kept() {
    let scratch = Scratch::new("drags").await;
    let (engine, app) = scratch.serve().await;
    opened(&engine, &app).await;
    for (x, y) in [(0.1, 0.1), (0.2, 0.15), (0.35, 0.4)] {
        let (answer, state, _) = moved(&engine, &app, x, y).await;
        assert_eq!(state, State::Applied, "{answer}");
    }
    assert_eq!(
        scratch.diffusion(SEANCE).await,
        [format!(
            "cover - - {PORTRAIT} 0.3500 0.4000 0.3000 1.000 0.00 0.00"
        )]
    );
    scratch.drop().await;
}

#[tokio::test]
async fn a_drag_off_the_screen_reaches_the_row_at_the_screen_s_edge() {
    let scratch = Scratch::new("edge").await;
    let (engine, app) = scratch.serve().await;
    opened(&engine, &app).await;
    let (answer, state, _) = moved(&engine, &app, 120.0, 80.0).await;
    assert_eq!(state, State::Applied, "{answer}");
    // numeric(6,4) holds the edge, where it could not hold 120.
    assert_eq!(
        scratch.diffusion(SEANCE).await,
        [format!(
            "cover - - {PORTRAIT} 1.0000 1.0000 0.3000 1.000 0.00 0.00"
        )]
    );
    scratch.drop().await;
}

#[tokio::test]
async fn a_window_that_is_not_open_is_rejected_under_window_fields_together_and_the_row_stays() {
    let scratch = Scratch::new("closed").await;
    scratch.showing_a_battlemap().await;
    let before = scratch.diffusion(SEANCE).await;
    let (engine, app) = scratch.serve().await;
    let (answer, state, violations) = moved(&engine, &app, 0.12, 0.08).await;
    assert_eq!(state, State::Rejected, "{answer}");
    assert!(
        violations.contains(&"window-fields-together".to_string()),
        "{violations:?}"
    );
    assert_eq!(scratch.diffusion(SEANCE).await, before);
    scratch.drop().await;
}

#[tokio::test]
async fn a_campaign_with_no_live_session_is_refused_and_nothing_changes() {
    let scratch = Scratch::new("refused").await;
    let (engine, app) = scratch.serve().await;
    let (status, answer) = post(
        &app,
        "live.moveWindow@1",
        json!({ "campaign": BRUMES, "x": 0.12, "y": 0.08 }),
        &[],
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{answer}");
    assert_eq!(answer["error"], "aucune séance n'est en cours");
    assert!(engine.run_once().await.expect("the applier ran").is_empty());
    assert_eq!(scratch.diffusions().await, 0);
    scratch.drop().await;
}
