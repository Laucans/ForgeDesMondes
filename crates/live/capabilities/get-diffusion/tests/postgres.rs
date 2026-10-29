//! live.getDiffusion over PostgreSQL (`DATABASE_URL`): the real SQL of the
//! Capability, run in a READ ONLY transaction on a scratch database migrated
//! as the server migrates it, and answered by `answer`. The live session's
//! `diffusion` row is the base, the floating window with its derived height
//! and the framing; a session with no row shows the campaign's cover; the
//! same values answer view 'dm' and view 'player'. Each test works in a
//! `ScratchDb` of its own, dropped even when an assertion fails.

use std::collections::BTreeMap;
use std::path::Path;

use get_diffusion::{SQL, answer};
use harness_dataguard::tokio_postgres::types::Type;
use harness_dataguard::tokio_postgres::{self, Client};
use platform_scratch_db::ScratchDb;
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

/// A database of its own: Valombre (its cover the portrait), its closed
/// session 1 and its live session 2 with a battlemap instance; Brumes, with
/// no session.
struct Scratch {
    client: Client,
    /// Last field: the connection closes before the database is dropped.
    _db: ScratchDb,
}

impl Scratch {
    async fn new(name: &str) -> Scratch {
        let db = ScratchDb::create(&format!("get_diffusion_{name}"))
            .await
            .expect("a scratch database");
        let mut client = db.connect().await.expect("PostgreSQL");
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
        Scratch { client, _db: db }
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
        self.rows_bound(&campaign).await
    }

    /// The rows of [`SQL`] with `$1` bound to `campaign`: a text, or NULL as
    /// the engine binds a variable the request leaves out.
    async fn rows_bound(&mut self, campaign: &(dyn tokio_postgres::types::ToSql + Sync)) -> Value {
        let tx = self
            .client
            .build_transaction()
            .read_only(true)
            .start()
            .await
            .expect("a read only transaction");
        let rows = tx.query(SQL, &[campaign]).await.expect("the SQL runs");
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

    /// A read whose request names no campaign: `$1` is NULL.
    async fn read_without_campaign(&mut self, view: &str) -> Value {
        let variables = BTreeMap::from([("view".to_string(), json!(view))]);
        let rows = self.rows_bound(&Option::<String>::None).await;
        answer(&rows, &variables)
    }

    async fn read(&mut self, campaign: &str, view: &str) -> Value {
        let variables = BTreeMap::from([
            ("campaign".to_string(), json!(campaign)),
            ("view".to_string(), json!(view)),
        ]);
        answer(&self.rows(campaign).await, &variables)
    }
}

#[tokio::test]
async fn a_session_with_no_diffusion_row_shows_the_cover_and_no_window() {
    let mut db = Scratch::new("no_row").await;
    let got = db.read(VALOMBRE, "player").await;
    assert_eq!(got["base"], json!({ "kind": "cover", "id": PORTRAIT }));
    assert_eq!(got["window"], Value::Null);
    assert_eq!(got["view"], Value::Null);
}

#[tokio::test]
async fn a_campaign_with_no_live_session_shows_a_bare_cover() {
    let mut db = Scratch::new("no_live").await;
    let got = db.read(BRUMES, "player").await;
    assert_eq!(
        got,
        json!({ "base": { "kind": "cover", "id": "" }, "window": null, "view": null })
    );
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
}

#[tokio::test]
async fn a_base_whose_instance_was_deleted_shows_the_cover() {
    let mut db = Scratch::new("nullified").await;
    db.diffusion("'battlemap', NULL, NULL, NULL, NULL, NULL, NULL, 1, 0, 0")
        .await;
    let got = db.read(VALOMBRE, "player").await;
    assert_eq!(got["base"], json!({ "kind": "cover", "id": PORTRAIT }));
    assert_eq!(got["window"], Value::Null);
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
}

// AC1: a battlemap on the diffusion row, read with no variables, answers the
// battlemap and the same view as with campaign '' (R113).
#[tokio::test]
async fn with_no_campaign_variable_the_live_session_is_read_as_with_an_empty_one() {
    let mut db = Scratch::new("absent_campaign").await;
    db.diffusion(&format!(
        "'battlemap', NULL, '{INSTANCE}', NULL, NULL, NULL, NULL, 1.5, -40, 12.5"
    ))
    .await;
    let absent = db.read_without_campaign("dm").await;
    assert_eq!(
        absent["base"],
        json!({ "kind": "battlemap", "id": INSTANCE })
    );
    assert_eq!(db.read("", "dm").await, absent);
}

// AC2: the illustration the DM sent as a floating window, read with no
// campaign and view player, is answered (R24).
#[tokio::test]
async fn with_no_campaign_variable_the_floating_window_is_answered() {
    let mut db = Scratch::new("absent_window").await;
    db.diffusion(&format!(
        "'illustration', '{PORTRAIT}', NULL, '{PORTRAIT}', 0.1, 0.2, 0.4, 1, 0, 0"
    ))
    .await;
    let got = db.read_without_campaign("player").await;
    assert_eq!(
        got["window"],
        json!({ "media": PORTRAIT, "x": 0.1, "y": 0.2, "w": 0.4, "h": 0.6 })
    );
    assert_eq!(
        got["base"],
        json!({ "kind": "illustration", "id": PORTRAIT })
    );
}

// AC3: no live session, read with no variables, answers the bare cover.
#[tokio::test]
async fn with_no_campaign_variable_and_no_live_session_the_cover_is_bare() {
    let mut db = Scratch::new("absent_no_live").await;
    db.client
        .batch_execute(&format!(
            "UPDATE session SET status = 'closed' WHERE id = '{SEANCE}'"
        ))
        .await
        .expect("the session closed");
    assert_eq!(
        db.read_without_campaign("player").await,
        json!({ "base": { "kind": "cover", "id": "" }, "window": null, "view": null })
    );
}

// Two campaigns live, `$1` NULL: the session started last is read, whichever
// campaign it belongs to.
#[tokio::test]
async fn with_no_campaign_variable_and_two_live_sessions_the_last_started_is_read() {
    let mut db = Scratch::new("absent_two_live").await;
    let brumes_cover = "d1000000-0000-4000-8000-000000000003";
    db.client
        .batch_execute(&format!(
            "INSERT INTO media (id, campaign_id, kind, title, file_path, width_px, height_px) VALUES \
             ('{brumes_cover}', '{BRUMES}', 'illustration', 'Brume', 'brume.png', 800, 1200); \
             UPDATE campaign SET cover_media_id = '{brumes_cover}' WHERE id = '{BRUMES}'; \
             INSERT INTO session (id, campaign_id, position, title, status, microphone_on) VALUES \
             ('51000000-0000-4000-8000-000000000003', '{BRUMES}', 1, 'Le Gué', 'live', false)"
        ))
        .await
        .expect("a second live session");
    let valombre =
        json!({ "base": { "kind": "cover", "id": PORTRAIT }, "window": null, "view": null });
    let brumes =
        json!({ "base": { "kind": "cover", "id": brumes_cover }, "window": null, "view": null });
    for (valombre_at, brumes_at, expected) in [
        ("2026-01-02", "2026-01-01", &valombre),
        ("2026-01-01", "2026-01-02", &brumes),
    ] {
        db.client
            .batch_execute(&format!(
                "UPDATE session SET started_at = '{valombre_at}' WHERE id = '{SEANCE}'; \
                 UPDATE session SET started_at = '{brumes_at}' WHERE id = '51000000-0000-4000-8000-000000000003'"
            ))
            .await
            .expect("the start times");
        assert_eq!(&db.read_without_campaign("player").await, expected);
    }
}
