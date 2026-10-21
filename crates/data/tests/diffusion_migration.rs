//! The Diffusion aggregate (`crates/live/aggregates/Diffusion/aggregate.json`)
//! on its table, as `0012_diffusion.sql` lays it, against the harness's
//! PostgreSQL (`DATABASE_URL`): every field the aggregate declares is a
//! column of `diffusion`, every relation it declares is a foreign key that
//! deletes and updates as it says, the table holds one row per session
//! (single-diffusion-source), stores no window height (window-proportional)
//! and no background outside the three kinds (base-coherent). What else
//! base-coherent, window-fields-together and window-proportional say of a
//! row, the DataGuard judges: rows the table holds, read back, are judged
//! here by the engine as the server binds the aggregate — the table does not judge them
//! again. Each test works in a scratch database of its own, migrated by the
//! Data layer, dropped when it ends.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use harness_dataguard::aggregate::Aggregate;
use harness_dataguard::api::Submission;
use harness_dataguard::command::State;
use harness_dataguard::engine::Engine;
use harness_dataguard::guard::Caller;
use harness_dataguard::manifest::DataCapability;
use harness_dataguard::memory::{MemoryNotifier, MemoryStore};
use harness_dataguard::postgres::SystemClock;
use serde_json::{Value, json};
use tokio_postgres::error::SqlState;
use tokio_postgres::{Client, NoTls};

const AGGREGATE: &str = include_str!("../../live/aggregates/Diffusion/aggregate.json");

/// A session's player view written whole, under the session its payload
/// names: test-only, no DataCapability of the product writes a diffusion yet.
const SHOW: &str = r#"{"dataCapability":"test.showOnPlayerView","owner":"dataguard","version":1,"effect":"upsert","target":{"aggregate":"Diffusion","id":"$sessionId"},"mode":"overwrite","callableBy":["*"]}"#;

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
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/migrations")).to_path_buf()
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

/// A column's field: `base_media_id` is `baseMediaId`.
fn field(column: &str) -> String {
    let mut words = column.split('_');
    let first = words.next().unwrap_or_default().to_string();
    words.fold(first, |mut out, word| {
        let mut chars = word.chars();
        if let Some(c) = chars.next() {
            out.push(c.to_ascii_uppercase());
            out.extend(chars);
        }
        out
    })
}

/// The invariants the DataGuard, binding the aggregate as the server does —
/// its manifest and its plugin — finds `row` breaks when a fresh engine is
/// asked to write it whole: none when it applies.
async fn judged(row: Value) -> Vec<String> {
    let engine = Engine::new(
        Arc::new(MemoryStore::default()),
        Arc::new(MemoryNotifier::default()),
        Arc::new(SystemClock),
    )
    .with_aggregate(Aggregate::parse(AGGREGATE).expect("contract I"))
    .with_plugins(std::iter::once(aggregate_diffusion::plugin()).collect());
    engine.load().await.expect("the in-memory engine");
    let engine = engine.with_data_capability(DataCapability::parse(SHOW).expect("contract F"));
    let Value::Object(payload) = row else {
        panic!("a row is an object");
    };
    engine
        .submit(Submission {
            data_capability: "test.showOnPlayerView".to_string(),
            payload: payload.into_iter().collect(),
            target_id: None,
            based_on: None,
            idempotency_key: None,
            caller: Caller {
                by: "dm".to_string(),
                from: "live.player-stage".to_string(),
                permissions: Vec::new(),
                // The critical fields' review is not what these tests judge.
                roles: vec!["role:approver".to_string()],
            },
        })
        .await
        .expect("queued");
    let mut applied = engine.run_once().await.expect("the applier ran");
    assert_eq!(applied.len(), 1, "{applied:?}");
    let result = applied.remove(0);
    let expected = if result.violations.is_empty() {
        State::Applied
    } else {
        State::Rejected
    };
    assert_eq!(result.status, expected, "{result:?}");
    result.violations
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

    /// The session's row as the table holds it, each column under its
    /// field.
    async fn row(&self, session: &str) -> Value {
        let text = self
            .texts(&format!(
                "SELECT row_to_json(d)::text FROM diffusion d WHERE session_id = '{session}'"
            ))
            .await;
        let [text] = text.as_slice() else {
            panic!("one row for {session}: {text:?}");
        };
        let Value::Object(columns) = serde_json::from_str(text).expect("a JSON row") else {
            panic!("a row is an object");
        };
        Value::Object(
            columns
                .into_iter()
                .map(|(column, value)| (field(&column), value))
                .collect(),
        )
    }

    /// What the DataGuard finds the session's row, read back, breaks.
    async fn judged(&self, session: &str) -> Vec<String> {
        judged(self.row(session).await).await
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
async fn window_proportional_the_table_stores_no_height_and_the_guard_judges_the_width() {
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
    let row = s.row(SEANCE).await;
    assert_eq!(judged(row.clone()).await, Vec::<String>::new());
    // A height beside the row's width is a second truth of its proportions.
    let mut with_height = row;
    with_height["windowHeight"] = json!(0.45);
    assert_eq!(judged(with_height).await, ["window-proportional"]);
    // The table holds a width no height can follow; the guard refuses it.
    for width in ["0", "-0.2"] {
        s.accepted(&format!(
            "UPDATE diffusion SET window_width = {width} WHERE session_id = '{SEANCE}'"
        ))
        .await;
        assert_eq!(s.judged(SEANCE).await, ["window-proportional"], "{width}");
    }
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

#[tokio::test]
async fn base_coherent_judges_the_rows_the_table_holds() {
    let s = Scratch::new("base_coherent").await;
    for (base, violations) in [
        // A cover has neither a media nor an instance, an illustration its
        // media alone, a battlemap its instance alone.
        ("'cover', NULL, NULL".to_string(), &[][..]),
        (format!("'illustration', '{PORTRAIT}', NULL"), &[]),
        (format!("'battlemap', NULL, '{INSTANCE}'"), &[]),
        // What the table holds and is no background.
        (format!("'cover', '{PORTRAIT}', NULL"), &["base-coherent"]),
        (format!("'cover', NULL, '{INSTANCE}'"), &["base-coherent"]),
        ("'illustration', NULL, NULL".to_string(), &["base-coherent"]),
        (
            format!("'illustration', '{PORTRAIT}', '{INSTANCE}'"),
            &["base-coherent"],
        ),
        ("'battlemap', NULL, NULL".to_string(), &["base-coherent"]),
        (format!("'battlemap', '{CARTE}', NULL"), &["base-coherent"]),
    ] {
        s.shows(SEANCE, &base, "NULL, NULL, NULL, NULL").await;
        assert_eq!(s.judged(SEANCE).await, violations, "{base}");
        s.accepted("DELETE FROM diffusion").await;
    }
    s.drop().await;
}

#[tokio::test]
async fn base_coherent_flags_the_row_a_deleted_media_or_instance_leaves() {
    let s = Scratch::new("base_nullified").await;
    s.shows(
        SEANCE,
        &format!("'illustration', '{PORTRAIT}', NULL"),
        "NULL, NULL, NULL, NULL",
    )
    .await;
    s.shows(
        SEANCE_2,
        &format!("'battlemap', NULL, '{INSTANCE}'"),
        "NULL, NULL, NULL, NULL",
    )
    .await;
    assert_eq!(s.judged(SEANCE).await, Vec::<String>::new());
    assert_eq!(s.judged(SEANCE_2).await, Vec::<String>::new());
    // Relations Diffusion.baseMediaId and Diffusion.baseInstanceId: onDelete
    // nullify, which leaves a background that is not its kind's.
    s.accepted(&format!(
        "DELETE FROM media WHERE id = '{PORTRAIT}'; \
         DELETE FROM battlemap_instance WHERE id = '{INSTANCE}'"
    ))
    .await;
    assert_eq!(s.judged(SEANCE).await, ["base-coherent"]);
    assert_eq!(s.judged(SEANCE_2).await, ["base-coherent"]);
    s.drop().await;
}

#[tokio::test]
async fn window_fields_together_judges_the_rows_the_table_holds() {
    let s = Scratch::new("window_together").await;
    for (window, violations) in [
        // An open window has its position and width; a closed one none.
        (format!("'{BLASON}', 0.1, 0.2, 0.3"), &[][..]),
        ("NULL, NULL, NULL, NULL".to_string(), &[]),
        // What the table holds and is half a window.
        (
            format!("'{BLASON}', 0.1, NULL, 0.3"),
            &["window-fields-together"],
        ),
        (
            format!("'{BLASON}', NULL, NULL, NULL"),
            &["window-fields-together"],
        ),
        (
            "NULL, 0.1, 0.2, 0.3".to_string(),
            &["window-fields-together"],
        ),
    ] {
        s.shows(SEANCE, "'cover', NULL, NULL", &window).await;
        assert_eq!(s.judged(SEANCE).await, violations, "{window}");
        s.accepted("DELETE FROM diffusion").await;
    }
    s.drop().await;
}

#[tokio::test]
async fn window_fields_together_flags_the_row_a_deleted_window_media_leaves() {
    let s = Scratch::new("window_nullified").await;
    s.shows(
        SEANCE,
        "'cover', NULL, NULL",
        &format!("'{PORTRAIT}', 0.1, 0.2, 0.3"),
    )
    .await;
    assert_eq!(s.judged(SEANCE).await, Vec::<String>::new());
    // Relation Diffusion.windowMediaId: onDelete nullify, which leaves the
    // window's position and width without its media.
    s.accepted(&format!("DELETE FROM media WHERE id = '{PORTRAIT}'"))
        .await;
    assert_eq!(s.judged(SEANCE).await, ["window-fields-together"]);
    s.drop().await;
}
