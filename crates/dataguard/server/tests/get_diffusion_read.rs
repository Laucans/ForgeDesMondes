//! live.getDiffusion over PostgreSQL (`DATABASE_URL`), through the connection
//! `Reads` opens: the live session's `diffusion` row is answered as the base,
//! the floating window with its derived height and the framing; a session
//! with no row shows the campaign's cover; the same values answer view 'dm'
//! and view 'player'. Each test works in a scratch database of its own,
//! migrated by the Data layer, dropped when it ends.

use std::path::Path;

use dataguard_server::Reads;
use serde_json::{Value, json};
use tokio_postgres::NoTls;
use tokio_postgres::error::SqlState;

const VALOMBRE: &str = "c1000000-0000-4000-8000-000000000001";
/// A campaign with no live session.
const BRUMES: &str = "c1000000-0000-4000-8000-000000000002";
/// Valombre's illustration of 800 × 1200, also its cover.
const PORTRAIT: &str = "d1000000-0000-4000-8000-000000000002";
const CARTE: &str = "d1000000-0000-4000-8000-000000000001";
const SEANCE_1: &str = "51000000-0000-4000-8000-000000000001";
const SEANCE: &str = "51000000-0000-4000-8000-000000000002";
const INSTANCE: &str = "b1000000-0000-4000-8000-000000000002";

fn url() -> String {
    std::env::var("DATABASE_URL").expect("DATABASE_URL names the harness's PostgreSQL")
}

fn naming(name: &str) -> String {
    let url = url();
    let (address, query) = url.split_once('?').unwrap_or((&url, ""));
    let (server, _) = address.rsplit_once('/').expect("a URL naming its database");
    if query.is_empty() {
        format!("{server}/{name}")
    } else {
        format!("{server}/{name}?{query}")
    }
}

async fn connect(url: &str) -> tokio_postgres::Client {
    let (client, connection) = tokio_postgres::connect(url, NoTls)
        .await
        .expect("PostgreSQL");
    tokio::spawn(connection);
    client
}

/// A database of its own: Valombre (its cover the portrait), its closed
/// session 1 and its live session 2 with a battlemap instance; Brumes, with
/// no session.
struct Scratch {
    name: String,
    client: tokio_postgres::Client,
    reads: Reads,
}

impl Scratch {
    async fn new(name: &str) -> Scratch {
        let name = format!("get_diffusion_test_{name}_{}", std::process::id());
        let admin = connect(&url()).await;
        for sql in [
            format!("DROP DATABASE IF EXISTS {name} WITH (FORCE)"),
            format!("CREATE DATABASE {name}"),
        ] {
            admin.batch_execute(&sql).await.expect("a scratch database");
        }
        let mut client = connect(&naming(&name)).await;
        let migrations = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/migrations");
        data::migrate(&mut client, &migrations)
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
                 UPDATE campaign SET cover_media_id = '{PORTRAIT}' WHERE id = '{VALOMBRE}'; \
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
        let reads = Reads::open(&naming(&name)).await.expect("the reads");
        Scratch {
            name,
            client,
            reads,
        }
    }

    async fn diffusion(&self, row: &str) {
        self.client
            .batch_execute(&format!(
                "INSERT INTO diffusion (session_id, base_kind, base_media_id, base_instance_id, \
                 window_media_id, window_x, window_y, window_width, view_zoom, view_pan_x, view_pan_y) \
                 VALUES ('{SEANCE}', {row})"
            ))
            .await
            .expect("the diffusion row");
    }

    async fn read(&self, variables: Value) -> Value {
        self.reads
            .run("live.getDiffusion", &variables)
            .await
            .expect("an answer")
    }

    async fn drop(self) {
        drop(self.client);
        drop(self.reads);
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

#[tokio::test]
async fn a_session_with_no_diffusion_row_shows_the_campaigns_cover_and_no_window() {
    let db = Scratch::new("no_row").await;
    let got = db
        .read(json!({ "campaign": VALOMBRE, "view": "player" }))
        .await;
    assert_eq!(
        got,
        json!({ "base": { "kind": "cover", "id": PORTRAIT }, "window": null, "view": null })
    );
    db.drop().await;
}

#[tokio::test]
async fn a_campaign_with_no_live_session_shows_a_bare_cover() {
    let db = Scratch::new("no_live").await;
    for campaign in [BRUMES, "not-a-uuid"] {
        let got = db.read(json!({ "campaign": campaign })).await;
        assert_eq!(
            got,
            json!({ "base": { "kind": "cover", "id": "" }, "window": null, "view": null }),
            "{campaign}"
        );
    }
    db.drop().await;
}

#[tokio::test]
async fn a_window_comes_with_its_derived_height_and_the_framing() {
    let db = Scratch::new("window").await;
    db.diffusion(&format!(
        "'battlemap', NULL, '{INSTANCE}', '{PORTRAIT}', 0.1, 0.2, 0.3, 1.5, -120, 40"
    ))
    .await;
    let got = db.read(json!({ "campaign": VALOMBRE })).await;
    assert_eq!(got["base"], json!({ "kind": "battlemap", "id": INSTANCE }));
    // 800 × 1200: a window 0.3 wide is 0.45 high.
    assert_eq!(
        got["window"],
        json!({ "media": PORTRAIT, "x": 0.1, "y": 0.2, "w": 0.3, "h": 0.45 })
    );
    assert_eq!(
        got["view"],
        json!({ "zoom": 1.5, "panX": -120.0, "panY": 40.0 })
    );
    let ratio = got["window"]["w"].as_f64().unwrap() / got["window"]["h"].as_f64().unwrap();
    assert!((ratio - 2.0 / 3.0).abs() < 1e-9, "{ratio}");
    db.drop().await;
}

#[tokio::test]
async fn an_illustration_base_names_its_media_and_a_closed_window_is_null() {
    let db = Scratch::new("illustration").await;
    db.diffusion(&format!(
        "'illustration', '{PORTRAIT}', NULL, NULL, NULL, NULL, NULL, 1, 0, 0"
    ))
    .await;
    let got = db.read(json!({ "campaign": VALOMBRE })).await;
    assert_eq!(
        got["base"],
        json!({ "kind": "illustration", "id": PORTRAIT })
    );
    assert_eq!(got["window"], Value::Null);
    assert_eq!(
        got["view"],
        json!({ "zoom": 1.0, "panX": 0.0, "panY": 0.0 })
    );
    db.drop().await;
}

#[tokio::test]
async fn both_views_read_the_same_values_and_no_campaign_means_the_live_session() {
    let db = Scratch::new("views").await;
    db.diffusion(&format!(
        "'battlemap', NULL, '{INSTANCE}', '{PORTRAIT}', 0.25, 0.5, 0.6, 2, 10, -10"
    ))
    .await;
    let dm = db.read(json!({ "campaign": VALOMBRE, "view": "dm" })).await;
    let player = db
        .read(json!({ "campaign": VALOMBRE, "view": "player" }))
        .await;
    assert_eq!(dm, player);
    assert_eq!(dm["window"]["h"], json!(0.9));
    // R25 calls it with no campaign: the live session is read.
    assert_eq!(db.read(json!({ "view": "player" })).await, dm);
    db.drop().await;
}

#[tokio::test]
async fn a_background_whose_instance_was_deleted_shows_the_cover() {
    let db = Scratch::new("nullified").await;
    db.diffusion(&format!(
        "'battlemap', NULL, '{INSTANCE}', NULL, NULL, NULL, NULL, 1, 0, 0"
    ))
    .await;
    db.client
        .batch_execute(&format!(
            "DELETE FROM battlemap_instance WHERE id = '{INSTANCE}'"
        ))
        .await
        .expect("the instance is deleted, the row's reference nullified");
    let got = db.read(json!({ "campaign": VALOMBRE })).await;
    assert_eq!(got["base"], json!({ "kind": "cover", "id": PORTRAIT }));
    db.drop().await;
}

#[tokio::test]
async fn with_no_campaign_the_last_session_started_is_read_whatever_its_position() {
    let db = Scratch::new("started").await;
    // Brumes' only session is at position 1, below Valombre's 2, but started later.
    db.client
        .batch_execute(&format!(
            "UPDATE session SET started_at = now() - interval '2 hours' WHERE id = '{SEANCE}'; \
             INSERT INTO session (id, campaign_id, position, title, status, started_at, microphone_on) \
             VALUES ('51000000-0000-4000-8000-000000000003', '{BRUMES}', 1, 'Brume', 'live', now(), false); \
             INSERT INTO diffusion (session_id, base_kind, view_zoom, view_pan_x, view_pan_y) \
             VALUES ('51000000-0000-4000-8000-000000000003', 'cover', 3, 0, 0)"
        ))
        .await
        .expect("Brumes' live session");
    assert_eq!(db.read(json!({})).await["view"]["zoom"], json!(3.0));
    // Naming a campaign reads that campaign's own live session.
    assert_eq!(
        db.read(json!({ "campaign": VALOMBRE })).await["view"],
        Value::Null
    );
    db.drop().await;
}
