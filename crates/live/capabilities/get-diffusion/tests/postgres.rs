//! live.getDiffusion over PostgreSQL (`DATABASE_URL`): the real SQL of the
//! Capability, run in a READ ONLY transaction on a scratch database migrated
//! as the server migrates it, and answered by `answer`. The live session's
//! `diffusion` row is the base, the floating window with its derived height
//! and the framing; a session with no row shows the campaign's cover; the
//! same values answer view 'dm' and view 'player'. Each test works in a
//! database of its own, dropped when it ends.

use std::collections::BTreeMap;
use std::path::Path;

use get_diffusion::{SQL, answer};
use harness_dataguard::tokio_postgres::types::Type;
use harness_dataguard::tokio_postgres::{self, Client, NoTls};
use serde_json::{Value, json};

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

async fn connect(url: &str) -> Client {
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
    client: Client,
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
        let migrations = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../data/migrations");
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
        Scratch { name, client }
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

    /// The rows of [`SQL`] in a READ ONLY transaction, as the Data layer's
    /// read-only connection answers them: the columns of the types it reads,
    /// as JSON.
    async fn rows(&mut self, campaign: &str) -> Value {
        let tx = self
            .client
            .build_transaction()
            .read_only(true)
            .start()
            .await
            .expect("a read only transaction");
        let rows = tx.query(SQL, &[&campaign]).await.expect("the SQL runs");
        tx.rollback().await.expect("rolled back");
        let rows = rows.iter().map(|row| {
            let mut object = serde_json::Map::new();
            for (i, column) in row.columns().iter().enumerate() {
                let value = match *column.type_() {
                    Type::TEXT => json!(row.get::<_, Option<String>>(i)),
                    Type::FLOAT8 => json!(row.get::<_, Option<f64>>(i)),
                    Type::BOOL => json!(row.get::<_, Option<bool>>(i)),
                    ref other => panic!(
                        "{} is a {other}, which the Data layer cannot read",
                        column.name()
                    ),
                };
                object.insert(column.name().to_string(), value);
            }
            Value::Object(object)
        });
        Value::Array(rows.collect())
    }

    async fn read(&mut self, campaign: &str, view: &str) -> Value {
        let variables = BTreeMap::from([
            ("campaign".to_string(), json!(campaign)),
            ("view".to_string(), json!(view)),
        ]);
        answer(&self.rows(campaign).await, &variables)
    }

    /// Dropped with a retry: an autovacuum worker still on the database
    /// makes the first attempts fail.
    async fn drop(self) {
        drop(self.client);
        let admin = connect(&url()).await;
        let sql = format!("DROP DATABASE IF EXISTS {} WITH (FORCE)", self.name);
        let mut last = None;
        for _ in 0..20 {
            match admin.batch_execute(&sql).await {
                Ok(()) => return,
                Err(e) => last = Some(e),
            }
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        }
        panic!("the scratch database dropped: {last:?}");
    }
}

#[tokio::test]
async fn a_session_with_no_diffusion_row_shows_the_cover_and_no_window() {
    let mut db = Scratch::new("no_row").await;
    let got = db.read(VALOMBRE, "player").await;
    assert_eq!(got["base"], json!({ "kind": "cover", "id": PORTRAIT }));
    assert_eq!(got["window"], Value::Null);
    assert_eq!(got["view"], Value::Null);
    db.drop().await;
}

#[tokio::test]
async fn a_campaign_with_no_live_session_shows_a_bare_cover() {
    let mut db = Scratch::new("no_live").await;
    let got = db.read(BRUMES, "player").await;
    assert_eq!(
        got,
        json!({ "base": { "kind": "cover", "id": "" }, "window": null, "view": null })
    );
    db.drop().await;
}

#[tokio::test]
async fn the_window_comes_with_its_height_derived_from_the_illustration() {
    let mut db = Scratch::new("window").await;
    db.diffusion(&format!(
        "'battlemap', NULL, '{INSTANCE}', '{PORTRAIT}', 0.1, 0.2, 0.4, 1.5, -40, 12.5"
    ))
    .await;
    let got = db.read(VALOMBRE, "dm").await;
    assert_eq!(got["base"], json!({ "kind": "battlemap", "id": INSTANCE }));
    // 800 × 1200: h = 0.4 × 1200 / 800 = 0.6, the 2/3 ratio.
    assert_eq!(
        got["window"],
        json!({ "media": PORTRAIT, "x": 0.1, "y": 0.2, "w": 0.4, "h": 0.6 })
    );
    assert_eq!(
        got["view"],
        json!({ "zoom": 1.5, "panX": -40.0, "panY": 12.5 })
    );
    db.drop().await;
}

#[tokio::test]
async fn view_dm_and_view_player_read_the_same_values() {
    let mut db = Scratch::new("views").await;
    db.diffusion(&format!(
        "'illustration', '{PORTRAIT}', NULL, '{PORTRAIT}', 0.25, 0.5, 0.3, 2, 10, -3.5"
    ))
    .await;
    let dm = db.read(VALOMBRE, "dm").await;
    let player = db.read(VALOMBRE, "player").await;
    assert_eq!(dm, player);
    assert_eq!(
        dm["base"],
        json!({ "kind": "illustration", "id": PORTRAIT })
    );
    assert_eq!(dm["window"]["h"], json!(0.45));
    db.drop().await;
}

#[tokio::test]
async fn a_base_whose_instance_was_deleted_shows_the_cover() {
    let mut db = Scratch::new("nullified").await;
    db.diffusion("'battlemap', NULL, NULL, NULL, NULL, NULL, NULL, 1, 0, 0")
        .await;
    let got = db.read(VALOMBRE, "player").await;
    assert_eq!(got["base"], json!({ "kind": "cover", "id": PORTRAIT }));
    assert_eq!(got["window"], Value::Null);
    db.drop().await;
}

#[tokio::test]
async fn with_no_campaign_the_live_session_is_read() {
    let mut db = Scratch::new("any_campaign").await;
    db.diffusion(&format!(
        "'battlemap', NULL, '{INSTANCE}', NULL, NULL, NULL, NULL, 1, 0, 0"
    ))
    .await;
    let got = db.read("", "player").await;
    assert_eq!(got["base"], json!({ "kind": "battlemap", "id": INSTANCE }));
    db.drop().await;
}
