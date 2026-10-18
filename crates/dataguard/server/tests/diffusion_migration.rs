//! The Diffusion aggregate (`crates/dataguard/aggregates/Diffusion/aggregate.json`)
//! on its table, as `0012_diffusion.sql` lays it, against the harness's
//! PostgreSQL (`DATABASE_URL`): every field the aggregate declares is a
//! column of `diffusion`, every relation it declares is a foreign key that
//! deletes and updates as it says, the table holds one row per session
//! (single-diffusion-source), stores no window height (window-proportional)
//! and no background outside the three kinds (base-coherent). What the rest
//! of base-coherent and window-fields-together say of a row, the engine
//! judges (`tests/diffusion_invariants.rs`): the table does not judge them
//! again. Each test works in a scratch database of its own, migrated by the
//! Data layer, dropped when it ends.

use std::path::{Path, PathBuf};

use serde_json::Value;
use tokio_postgres::error::SqlState;
use tokio_postgres::{Client, NoTls};

const AGGREGATE: &str = include_str!("../../aggregates/Diffusion/aggregate.json");

const VALOMBRE: &str = "c1000000-0000-4000-8000-000000000001";
/// Valombre's battlemap.
const CARTE: &str = "d1000000-0000-4000-8000-000000000001";
/// Valombre's illustration of 800 × 1200.
const PORTRAIT: &str = "d1000000-0000-4000-8000-000000000002";
/// Valombre's other illustration.
const BLASON: &str = "d1000000-0000-4000-8000-000000000003";
/// Valombre's session 1.
const SEANCE: &str = "51000000-0000-4000-8000-000000000001";
/// Valombre's session 2.
const SEANCE_2: &str = "51000000-0000-4000-8000-000000000002";
const ARRIVEE: &str = "5c000000-0000-4000-8000-000000000001";
/// The tavern's battlemap instance, of session 1.
const INSTANCE: &str = "b1000000-0000-4000-8000-000000000001";

fn url() -> String {
    std::env::var("DATABASE_URL").expect("DATABASE_URL names the harness's PostgreSQL")
}

fn migrations() -> PathBuf {
    Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../data/migrations"
    ))
    .to_path_buf()
}

async fn connect(config: &tokio_postgres::Config) -> Client {
    let (client, connection) = config.connect(NoTls).await.expect("PostgreSQL");
    tokio::spawn(connection);
    client
}

fn aggregate() -> Value {
    serde_json::from_str(AGGREGATE).expect("contract I")
}

/// A field's column: `baseMediaId` is `base_media_id`.
fn column(field: &str) -> String {
    field.chars().fold(String::new(), |mut out, c| {
        if c.is_ascii_uppercase() {
            out.push('_');
        }
        out.push(c.to_ascii_lowercase());
        out
    })
}

/// A database of its own, migrated by the Data layer: Valombre, its two
/// sessions, a battlemap with its instance in session 1's first scene, and
/// two illustrations.
struct Scratch {
    client: Client,
    name: String,
}

impl Scratch {
    async fn new(name: &str) -> Scratch {
        let name = format!("diffusion_test_{name}_{}", std::process::id());
        let config: tokio_postgres::Config = url().parse().expect("a PostgreSQL URL");
        let admin = connect(&config).await;
        for sql in [
            format!("DROP DATABASE IF EXISTS {name} WITH (FORCE)"),
            format!("CREATE DATABASE {name}"),
        ] {
            admin.batch_execute(&sql).await.expect("a scratch database");
        }
        let mut client = connect(config.clone().dbname(&name)).await;
        data::migrate(&mut client, &migrations())
            .await
            .expect("migrated by the Data layer");
        let s = Scratch { client, name };
        s.accepted(&format!(
            "INSERT INTO campaign (id, name, kind) VALUES ('{VALOMBRE}', 'Les Cendres de Valombre', 'campagne'); \
             INSERT INTO media (id, campaign_id, kind, title, file_path, width_px, height_px) VALUES \
             ('{CARTE}', '{VALOMBRE}', 'battlemap', 'Taverne', 'carte.png', 2048, 1536), \
             ('{PORTRAIT}', '{VALOMBRE}', 'illustration', 'Morlox', 'morlox.png', 800, 1200), \
             ('{BLASON}', '{VALOMBRE}', 'illustration', 'Blason', 'blason.png', 512, 512); \
             INSERT INTO battlemap_grid (media_id, hex_size_px, offset_x, offset_y, orientation, color, opacity) \
             VALUES ('{CARTE}', 64, 0, 0, 'pointe', '#202020', 0.4); \
             INSERT INTO session (id, campaign_id, position, title, status, microphone_on) VALUES \
             ('{SEANCE}', '{VALOMBRE}', 1, 'Le Sanglier', 'live', false), \
             ('{SEANCE_2}', '{VALOMBRE}', 2, 'La Forêt', 'prep', false); \
             INSERT INTO scene (id, session_id, position, title) VALUES ('{ARRIVEE}', '{SEANCE}', 1, 'Arrivée'); \
             INSERT INTO battlemap_instance (id, battlemap_media_id, session_id, scene_id) \
             VALUES ('{INSTANCE}', '{CARTE}', '{SEANCE}', '{ARRIVEE}')"
        ))
        .await;
        s
    }

    async fn accepted(&self, sql: &str) {
        self.client
            .batch_execute(sql)
            .await
            .unwrap_or_else(|e| panic!("{sql}: {e:?}"));
    }

    /// The code and the constraint the database refuses `sql` under.
    async fn refused_by(&self, sql: &str) -> (SqlState, Option<String>) {
        let error = self.client.batch_execute(sql).await.expect_err(sql);
        let db = error.as_db_error().expect("a refusal of the database");
        (db.code().clone(), db.constraint().map(str::to_string))
    }

    /// One text column of the rows `sql` answers, a null as `-`.
    async fn texts(&self, sql: &str) -> Vec<String> {
        self.client
            .query(sql, &[])
            .await
            .unwrap_or_else(|e| panic!("{sql}: {e:?}"))
            .iter()
            .map(|row| {
                row.get::<_, Option<String>>(0)
                    .unwrap_or_else(|| "-".to_string())
            })
            .collect()
    }

    /// Session 1's player view: `base` its background (SQL:
    /// `kind, media, instance`), `window` its window (SQL: `media, x, y,
    /// width`).
    async fn shows(&self, session: &str, base: &str, window: &str) {
        self.accepted(&format!(
            "INSERT INTO diffusion (session_id, base_kind, base_media_id, base_instance_id, \
             window_media_id, window_x, window_y, window_width, view_zoom, view_pan_x, view_pan_y) \
             VALUES ('{session}', {base}, {window}, 1, 0, 0)"
        ))
        .await;
    }

    /// What the diffusion rows point to, a row a line, sorted by session.
    async fn diffusions(&self) -> Vec<String> {
        self.texts(
            "SELECT session_id::text || ' ' || base_kind \
             || ' ' || coalesce(base_media_id::text, '-') \
             || ' ' || coalesce(base_instance_id::text, '-') \
             || ' ' || coalesce(window_media_id::text, '-') FROM diffusion ORDER BY session_id",
        )
        .await
    }

    /// Drops the database. An autovacuum worker the writes woke may still be
    /// on it, which `FORCE` cannot end for a role that is not a superuser:
    /// the drop is tried again while the worker finishes.
    async fn drop(self) {
        drop(self.client);
        let config: tokio_postgres::Config = url().parse().expect("a PostgreSQL URL");
        let admin = connect(&config).await;
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
async fn every_field_the_aggregate_declares_is_a_column_of_the_diffusion_table() {
    let s = Scratch::new("fields").await;
    let mut declared: Vec<String> = aggregate()["fields"]
        .as_object()
        .expect("fields")
        .keys()
        .map(|field| column(field))
        .collect();
    declared.sort();
    let columns = s
        .texts(
            "SELECT attname::text FROM pg_attribute WHERE attrelid = 'diffusion'::regclass \
             AND attnum > 0 AND NOT attisdropped ORDER BY 1",
        )
        .await;
    assert_eq!(columns, declared);
    s.drop().await;
}

#[tokio::test]
async fn each_relation_the_aggregate_declares_is_a_foreign_key_that_acts_as_it_says() {
    let s = Scratch::new("relations").await;
    // As `pg_constraint` codes the actions.
    let code = |action: &str| match action {
        "cascade" | "propagate" => "c",
        "nullify" => "n",
        "restrict" => "r",
        other => panic!("no foreign key action for {other}"),
    };
    let mut declared: Vec<String> = aggregate()["relations"]
        .as_array()
        .expect("relations")
        .iter()
        .map(|relation| {
            let from = relation["from"].as_str().expect("from");
            let field = from
                .strip_prefix("Diffusion.")
                .expect("a field of Diffusion");
            format!(
                "{} {} {}",
                column(field),
                code(relation["onDelete"].as_str().expect("onDelete")),
                code(relation["onUpdate"].as_str().expect("onUpdate")),
            )
        })
        .collect();
    declared.sort();
    let keys = s
        .texts(
            "SELECT a.attname || ' ' || c.confdeltype::text || ' ' || c.confupdtype::text \
             FROM pg_constraint c \
             JOIN pg_attribute a ON a.attrelid = c.conrelid AND a.attnum = c.conkey[1] \
             WHERE c.conrelid = 'diffusion'::regclass AND c.contype = 'f' ORDER BY 1",
        )
        .await;
    assert_eq!(keys, declared);
    s.drop().await;
}

#[tokio::test]
async fn a_deleted_media_or_instance_leaves_the_view_without_it_and_a_deleted_session_its_row() {
    let s = Scratch::new("deletes").await;
    s.shows(
        SEANCE,
        &format!("'illustration', '{PORTRAIT}', NULL"),
        &format!("'{BLASON}', 0.1, 0.2, 0.3"),
    )
    .await;
    s.shows(
        SEANCE_2,
        &format!("'battlemap', NULL, '{INSTANCE}'"),
        &format!("'{PORTRAIT}', 0.5, 0.5, 0.2"),
    )
    .await;
    // Relation Diffusion.baseMediaId and Diffusion.windowMediaId: onDelete
    // nullify.
    s.accepted(&format!("DELETE FROM media WHERE id = '{PORTRAIT}'"))
        .await;
    assert_eq!(
        s.diffusions().await,
        [
            format!("{SEANCE} illustration - - {BLASON}"),
            format!("{SEANCE_2} battlemap - {INSTANCE} -"),
        ]
    );
    // Relation Diffusion.baseInstanceId: onDelete nullify.
    s.accepted(&format!(
        "DELETE FROM battlemap_instance WHERE id = '{INSTANCE}'"
    ))
    .await;
    assert_eq!(
        s.diffusions().await[1],
        format!("{SEANCE_2} battlemap - - -")
    );
    // Relation Diffusion.sessionId: onDelete cascade.
    s.accepted(&format!("DELETE FROM session WHERE id = '{SEANCE}'"))
        .await;
    assert_eq!(
        s.diffusions().await,
        [format!("{SEANCE_2} battlemap - - -")]
    );
    s.drop().await;
}

#[tokio::test]
async fn a_new_id_reaches_the_view_that_shows_it() {
    let s = Scratch::new("updates").await;
    s.shows(
        SEANCE,
        &format!("'battlemap', NULL, '{INSTANCE}'"),
        &format!("'{PORTRAIT}', 0.1, 0.2, 0.3"),
    )
    .await;
    s.shows(
        SEANCE_2,
        &format!("'illustration', '{BLASON}', NULL"),
        "NULL, NULL, NULL, NULL",
    )
    .await;
    // The same id under another first digit.
    let new = |id: &str| format!("f{}", &id[1..]);
    for (table, id) in [
        ("session", SEANCE),
        ("battlemap_instance", INSTANCE),
        ("media", PORTRAIT),
        ("media", BLASON),
    ] {
        s.accepted(&format!(
            "UPDATE {table} SET id = '{}' WHERE id = '{id}'",
            new(id)
        ))
        .await;
    }
    assert_eq!(
        s.diffusions().await,
        [
            format!("{SEANCE_2} illustration {} - -", new(BLASON)),
            format!(
                "{} battlemap - {} {}",
                new(SEANCE),
                new(INSTANCE),
                new(PORTRAIT)
            ),
        ]
    );
    s.drop().await;
}

#[tokio::test]
async fn single_diffusion_source_the_table_holds_one_row_per_session() {
    let s = Scratch::new("single_source").await;
    // The row is keyed by its session alone.
    assert_eq!(
        s.texts(
            "SELECT a.attname::text FROM pg_constraint c \
             JOIN pg_attribute a ON a.attrelid = c.conrelid AND a.attnum = ANY (c.conkey) \
             WHERE c.conrelid = 'diffusion'::regclass AND c.contype = 'p'",
        )
        .await,
        ["session_id"]
    );
    s.shows(SEANCE, "'cover', NULL, NULL", "NULL, NULL, NULL, NULL")
        .await;
    let (code, constraint) = s
        .refused_by(&format!(
            "INSERT INTO diffusion (session_id, base_kind, view_zoom, view_pan_x, view_pan_y) \
             VALUES ('{SEANCE}', 'illustration', 1, 0, 0)"
        ))
        .await;
    assert_eq!(code, SqlState::UNIQUE_VIOLATION);
    assert_eq!(constraint.as_deref(), Some("diffusion_pkey"));
    // The projected view and the DM's tab change the one row, and read it.
    s.accepted(&format!(
        "UPDATE diffusion SET view_zoom = 1.5, view_pan_x = -120, view_pan_y = 40 \
         WHERE session_id = '{SEANCE}'"
    ))
    .await;
    assert_eq!(
        s.texts(&format!(
            "SELECT view_zoom || ' ' || view_pan_x || ' ' || view_pan_y FROM diffusion \
             WHERE session_id = '{SEANCE}'"
        ))
        .await,
        ["1.500 -120.00 40.00"]
    );
    s.drop().await;
}

#[tokio::test]
async fn window_proportional_the_height_is_not_stored_but_follows_from_the_media() {
    let s = Scratch::new("proportional").await;
    assert_eq!(
        s.texts(
            "SELECT attname::text FROM pg_attribute WHERE attrelid = 'diffusion'::regclass \
             AND attnum > 0 AND NOT attisdropped AND attname LIKE '%height%'",
        )
        .await,
        Vec::<String>::new()
    );
    s.shows(
        SEANCE,
        "'cover', NULL, NULL",
        &format!("'{PORTRAIT}', 0.1, 0.2, 0.3"),
    )
    .await;
    // window_width × height_px / width_px: 0.3 × 1200 / 800, a width of 2/3
    // of the height.
    assert_eq!(
        s.texts(
            "SELECT round(d.window_width * m.height_px / m.width_px, 4)::text \
             || ' ' || round(d.window_width / (d.window_width * m.height_px / m.width_px), 4) \
             FROM diffusion d JOIN media m ON m.id = d.window_media_id",
        )
        .await,
        ["0.4500 0.6667"]
    );
    s.drop().await;
}

#[tokio::test]
async fn base_coherent_the_table_refuses_a_background_outside_the_three_kinds() {
    let s = Scratch::new("base_kind").await;
    for kind in ["cover", "illustration", "battlemap"] {
        s.accepted(&format!(
            "BEGIN; INSERT INTO diffusion (session_id, base_kind, view_zoom, view_pan_x, view_pan_y) \
             VALUES ('{SEANCE}', '{kind}', 1, 0, 0); ROLLBACK"
        ))
        .await;
    }
    let (code, constraint) = s
        .refused_by(&format!(
            "INSERT INTO diffusion (session_id, base_kind, view_zoom, view_pan_x, view_pan_y) \
             VALUES ('{SEANCE}', 'video', 1, 0, 0)"
        ))
        .await;
    assert_eq!(code, SqlState::CHECK_VIOLATION);
    assert_eq!(constraint.as_deref(), Some("diffusion_base_kind"));
    s.drop().await;
}
