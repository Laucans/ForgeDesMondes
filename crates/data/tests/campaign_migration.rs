//! The Campaign migration (`crates/data/migrations/0002_campaign.sql`)
//! against the harness's PostgreSQL (`DATABASE_URL`): the table judges the
//! aggregate's invariants again, beside the DataGuard, and is what
//! `data/model.json` says it is. Each test works in a scratch database of its
//! own, dropped when it ends.

use serde_json::Value;
use tokio_postgres::error::SqlState;
use tokio_postgres::{Client, NoTls};

const MIGRATION: &str = include_str!("../migrations/0002_campaign.sql");

fn url() -> String {
    std::env::var("DATABASE_URL").expect("DATABASE_URL names the harness's PostgreSQL")
}

async fn connect(config: &tokio_postgres::Config) -> Client {
    let (client, connection) = config.connect(NoTls).await.expect("PostgreSQL");
    tokio::spawn(connection);
    client
}

/// A database of its own, migrated once.
struct Scratch {
    client: Client,
    name: String,
}

impl Scratch {
    async fn new(name: &str) -> Scratch {
        let name = format!("campaign_test_{name}_{}", std::process::id());
        let config: tokio_postgres::Config = url().parse().expect("a PostgreSQL URL");
        let admin = connect(&config).await;
        for sql in [
            format!("DROP DATABASE IF EXISTS {name} WITH (FORCE)"),
            format!("CREATE DATABASE {name}"),
        ] {
            admin.batch_execute(&sql).await.expect("a scratch database");
        }
        let client = connect(config.clone().dbname(&name)).await;
        client.batch_execute(MIGRATION).await.expect("migrated");
        Scratch { client, name }
    }

    /// The SQLSTATE and the constraint `sql` is refused with.
    async fn refused(&self, sql: &str) -> (SqlState, String) {
        let error = self
            .client
            .batch_execute(sql)
            .await
            .expect_err("PostgreSQL refuses it");
        let db = error.as_db_error().expect("a refusal of the database");
        (
            db.code().clone(),
            db.constraint().unwrap_or_default().to_string(),
        )
    }

    /// Every row, as text: what a replay must leave as it was.
    async fn snapshot(&self) -> Vec<String> {
        self.client
            .query(
                "SELECT row_to_json(c)::text FROM campaign c ORDER BY 1",
                &[],
            )
            .await
            .expect("the table reads")
            .iter()
            .map(|row| row.get(0))
            .collect()
    }

    async fn drop(self) {
        drop(self.client);
        let config: tokio_postgres::Config = url().parse().expect("a PostgreSQL URL");
        connect(&config)
            .await
            .batch_execute(&format!("DROP DATABASE {} WITH (FORCE)", self.name))
            .await
            .expect("the scratch database dropped");
    }
}

/// A `campaign` insert of the required columns, these overridden.
fn campaign(kind: &str, volume: &str) -> String {
    format!(
        "INSERT INTO campaign (name, kind, sound_master_volume) \
         VALUES ('Les Cendres de Valombre', {kind}, {volume})"
    )
}

/// `campaign` as `data/model.json` describes it: each field's name, type and
/// nullability, in the table's order.
fn modelled() -> Vec<(String, String, bool)> {
    let model: Value = serde_json::from_str(include_str!("../../../data/model.json"))
        .expect("data/model.json is JSON");
    let table = model["tables"]
        .as_array()
        .expect("tables")
        .iter()
        .find(|table| table["name"] == "campaign")
        .expect("data/model.json describes campaign");
    table["fields"]
        .as_array()
        .expect("fields")
        .iter()
        .map(|field| {
            (
                field["name"].as_str().expect("a name").to_string(),
                field["type"].as_str().expect("a type").to_string(),
                field["nullable"].as_bool().expect("a nullability"),
            )
        })
        .collect()
}

#[tokio::test]
async fn the_migration_replays_as_a_no_op() {
    let mut s = Scratch::new("replay").await;
    s.client
        .batch_execute(&campaign("'oneshot'", "0.8"))
        .await
        .expect("a oneshot");
    let first = s.snapshot().await;
    assert_eq!(first.len(), 1);
    s.client
        .batch_execute(MIGRATION)
        .await
        .expect("replayed as psql replays it");
    assert_eq!(s.snapshot().await, first);
    // The server's applier after the rules' runtime's psql, which records
    // nothing: it runs every file again, and nothing moves.
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/migrations");
    let applied = data::migrate(&mut s.client, std::path::Path::new(dir))
        .await
        .expect("replayed by the Data layer");
    assert!(
        applied.iter().any(|file| file == "0002_campaign.sql"),
        "{applied:?}"
    );
    assert_eq!(s.snapshot().await, first);
    s.drop().await;
}

#[tokio::test]
async fn the_table_is_what_data_model_json_describes() {
    let s = Scratch::new("model").await;
    let real: Vec<(String, String, bool)> = s
        .client
        .query(
            "SELECT a.attname, format_type(a.atttypid, a.atttypmod), NOT a.attnotnull \
             FROM pg_attribute a WHERE a.attrelid = 'campaign'::regclass \
             AND a.attnum > 0 AND NOT a.attisdropped ORDER BY a.attnum",
            &[],
        )
        .await
        .expect("the columns read")
        .iter()
        .map(|row| {
            let ty: String = row.get(1);
            // As the model writes it, pg_dump's own alias.
            let ty = ty.replace("timestamp with time zone", "timestamptz");
            (row.get(0), ty, row.get(2))
        })
        .collect();
    assert_eq!(real, modelled());
    // No foreign key yet: cover_media_id's comes with media, and
    // origin_campaign_id never has one.
    let keys: i64 = s
        .client
        .query_one(
            "SELECT count(*) FROM pg_constraint \
             WHERE conrelid = 'campaign'::regclass AND contype = 'f'",
            &[],
        )
        .await
        .expect("the constraints read")
        .get(0);
    assert_eq!(keys, 0);
    s.drop().await;
}

#[tokio::test]
async fn master_volume_range_is_kept_by_the_table() {
    let s = Scratch::new("volume").await;
    for volume in ["0", "0.5", "1"] {
        s.client
            .batch_execute(&campaign("'campagne'", volume))
            .await
            .unwrap_or_else(|e| panic!("{volume} is in range: {e}"));
    }
    for volume in ["1.2", "-0.1", "1.01"] {
        assert_eq!(
            s.refused(&campaign("'campagne'", volume)).await,
            (
                SqlState::CHECK_VIOLATION,
                "campaign_master_volume_range".to_string()
            ),
            "{volume}"
        );
    }
    assert_eq!(
        s.refused(&campaign("'campagne'", "NULL")).await.0,
        SqlState::NOT_NULL_VIOLATION
    );
    // Full volume until the DM moves it.
    let volume: String = s
        .client
        .query_one(
            "INSERT INTO campaign (name, kind) VALUES ('x', 'campagne') \
             RETURNING sound_master_volume::text",
            &[],
        )
        .await
        .expect("the default volume")
        .get(0);
    assert_eq!(volume, "1.00");
    s.drop().await;
}

#[tokio::test]
async fn oneshot_single_session_has_a_kind_to_judge_by() {
    let s = Scratch::new("kind").await;
    for kind in ["'campagne'", "'oneshot'"] {
        s.client
            .batch_execute(&campaign(kind, "1"))
            .await
            .unwrap_or_else(|e| panic!("{kind} is a kind: {e}"));
    }
    for kind in ["'campaign'", "'one-shot'", "''"] {
        assert_eq!(
            s.refused(&campaign(kind, "1")).await,
            (SqlState::CHECK_VIOLATION, "campaign_kind".to_string()),
            "{kind}"
        );
    }
    assert_eq!(
        s.refused(&campaign("NULL", "1")).await.0,
        SqlState::NOT_NULL_VIOLATION
    );
    s.drop().await;
}

#[tokio::test]
async fn import_creates_copy_needs_no_original_on_this_machine() {
    let s = Scratch::new("import").await;
    // An archive's origin that this machine never had, and a cover whose
    // media table does not exist yet: both stored as they are.
    let row = s
        .client
        .query_one(
            "INSERT INTO campaign (name, kind, origin_campaign_id, imported_at, cover_media_id) \
             VALUES ('Copie', 'campagne', gen_random_uuid(), now(), gen_random_uuid()) \
             RETURNING id <> origin_campaign_id",
            &[],
        )
        .await
        .expect("a copy whose original is elsewhere");
    assert!(row.get::<_, bool>(0), "the copy has an id of its own");
    s.drop().await;
}
