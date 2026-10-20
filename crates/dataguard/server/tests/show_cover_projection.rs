//! live.showCover over PostgreSQL (`DATABASE_URL`), through the wire
//! protocol: a return to the cover is prepared against the campaign's live
//! session the tables hold, and what the engine applies on
//! `Diffusion/<session>` reaches the session's `diffusion` row — its base
//! the cover alone, its window and its framing as the table holds them. Each
//! test works in a scratch database of its own, migrated by the Data layer,
//! dropped when it ends.

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
/// Valombre's battlemap.
const CARTE: &str = "d1000000-0000-4000-8000-000000000001";
/// Valombre's illustration of 800 × 1200.
const PORTRAIT: &str = "d1000000-0000-4000-8000-000000000002";
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
/// scene of session 2, an illustration; Brumes, with no session.
struct Scratch {
    name: String,
    url: String,
    client: Client,
}

impl Scratch {
    async fn new(name: &str) -> Scratch {
        let name = format!("show_cover_test_{name}_{}", std::process::id());
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
                 ('{PORTRAIT}', '{VALOMBRE}', 'illustration', 'Morlox', 'morlox.png', 800, 1200); \
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

    /// Session 2 shows the tavern's instance, framed, with the portrait open
    /// in the floating window — as a send to the player view and an opened
    /// window leave its row — written a minute ago.
    async fn showing_a_battlemap_with_a_window(&self) {
        self.client
            .batch_execute(&format!(
                "INSERT INTO diffusion (session_id, base_kind, base_instance_id, window_media_id, \
                 window_x, window_y, window_width, view_zoom, view_pan_x, view_pan_y, updated_at) \
                 VALUES ('{SEANCE}', 'battlemap', '{INSTANCE}', '{PORTRAIT}', 0.1, 0.2, 0.3, 1.5, -120, 40, \
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

/// `POST /commands` of live.showCover by the DM, approver of their own
/// campaign's critical fields: the status and the answer.
async fn post(app: &Router, payload: Value) -> (StatusCode, Value) {
    let body = json!({
        "dataCapability": "live.showCover@1",
        "target": { "aggregate": "Diffusion" },
        "payload": payload,
        "caller": { "by": "dm", "from": "LiveSessionBar", "roles": ["role:approver"] },
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

/// Brings Valombre's player view back to its cover: the partition it was
/// queued on, once the applier applied it.
async fn covered(engine: &Engine, app: &Router) -> Value {
    let (status, answer) = post(app, json!({ "campaign": VALOMBRE })).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{answer}");
    let settled = engine.run_once().await.expect("the applier ran");
    assert!(
        settled.len() == 1 && settled[0].status == State::Applied,
        "{settled:?}"
    );
    answer["partition"].clone()
}

#[tokio::test]
async fn a_session_showing_a_battlemap_goes_back_to_the_cover_its_window_and_framing_kept() {
    let scratch = Scratch::new("battlemap").await;
    scratch.showing_a_battlemap_with_a_window().await;
    let (engine, app) = scratch.serve().await;
    let partition = covered(&engine, &app).await;
    assert_eq!(partition, format!("Diffusion/{SEANCE}"));
    assert_eq!(
        scratch.diffusion(SEANCE).await,
        [format!(
            "cover - - {PORTRAIT} 0.1000 0.2000 0.3000 1.500 -120.00 40.00"
        )]
    );
    scratch.drop().await;
}

#[tokio::test]
async fn a_session_with_no_diffusion_row_gets_one_showing_the_cover() {
    let scratch = Scratch::new("first").await;
    let (engine, app) = scratch.serve().await;
    covered(&engine, &app).await;
    assert_eq!(
        scratch.diffusion(SEANCE).await,
        ["cover - - - - - - 1.000 0.00 0.00"]
    );
    // single-diffusion-source: one row, the live session's.
    assert_eq!(scratch.diffusions().await, 1);
    assert!(scratch.diffusion(SEANCE_1).await.is_empty());
    scratch.drop().await;
}

#[tokio::test]
async fn a_campaign_with_no_live_session_or_none_the_tables_hold_is_refused_and_nothing_changes() {
    let scratch = Scratch::new("refused").await;
    let (engine, app) = scratch.serve().await;
    for campaign in [BRUMES, "c1000000-0000-4000-8000-0000000000ff"] {
        let (status, answer) = post(&app, json!({ "campaign": campaign })).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{answer}");
        assert_eq!(answer["error"], "aucune séance n'est en cours");
    }
    assert!(engine.run_once().await.expect("the applier ran").is_empty());
    assert_eq!(scratch.diffusions().await, 0);
    scratch.drop().await;
}

#[tokio::test]
async fn a_restart_never_moves_back_a_row_the_table_holds_a_later_write_of() {
    let scratch = Scratch::new("restart").await;
    let (engine, app) = scratch.serve().await;
    covered(&engine, &app).await;
    scratch
        .client
        .batch_execute(&format!(
            "UPDATE diffusion SET base_kind = 'illustration', base_media_id = '{PORTRAIT}', \
             updated_at = now() + interval '1 minute' WHERE session_id = '{SEANCE}'"
        ))
        .await
        .expect("a later write of session 2's row");
    // A start projects every diffusion row the engine holds again.
    drop(scratch.serve().await);
    assert_eq!(
        scratch.diffusion(SEANCE).await,
        [format!("illustration {PORTRAIT} - - - - - 1.000 0.00 0.00")]
    );
    drop(engine);
    scratch.drop().await;
}
