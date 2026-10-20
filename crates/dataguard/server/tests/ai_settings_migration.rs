//! The AiSettings migration (`crates/data/migrations/0001_ai_settings.sql`)
//! against the harness's PostgreSQL (`DATABASE_URL`): the tables judge the
//! aggregate's invariants again, beside the DataGuard. Each test works in a
//! scratch database of its own, dropped when it ends.

use serde_json::Value;
use tokio_postgres::error::SqlState;
use tokio_postgres::{Client, NoTls};

const MIGRATION: &str = include_str!("../../../data/migrations/0001_ai_settings.sql");

const CLOUD: &str = "a1000000-0000-4000-8000-000000000001";

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
        let name = format!("ai_settings_test_{name}_{}", std::process::id());
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

    /// Both tables, every row, as text: what a replay must leave as it was.
    async fn snapshot(&self) -> Vec<String> {
        self.client
            .query(
                "SELECT row_to_json(p)::text FROM ai_provider p \
                 UNION ALL SELECT row_to_json(s)::text FROM ai_settings s ORDER BY 1",
                &[],
            )
            .await
            .expect("the tables read")
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

/// An `ai_provider` insert with these columns overridden.
fn provider(mode: &str, url: &str, kind: &str, binary: &str) -> String {
    format!(
        "INSERT INTO ai_provider (mode, label, url, detected_api_type, \"binary\", \
         generation_model, summary_model) VALUES ({mode}, 'x', {url}, {kind}, {binary}, 'g', 's')"
    )
}

#[tokio::test]
async fn the_migration_replays_as_a_no_op() {
    let mut s = Scratch::new("replay").await;
    let first = s.snapshot().await;
    assert_eq!(first.len(), 5, "four providers and the settings row");
    s.client
        .batch_execute(MIGRATION)
        .await
        .expect("replayed as psql replays it");
    assert_eq!(s.snapshot().await, first);
    // The server's applier after the rules' runtime's psql, which records
    // nothing: it runs the file again, and nothing moves.
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../data/migrations");
    let applied = data::migrate(&mut s.client, std::path::Path::new(dir))
        .await
        .expect("replayed by the Data layer");
    // Every file of the folder is new to the applier; this one among them.
    assert!(
        applied.iter().any(|file| file == "0001_ai_settings.sql"),
        "{applied:?}"
    );
    assert_eq!(s.snapshot().await, first);
    s.drop().await;
}

#[tokio::test]
async fn single_settings_row_is_seeded_on_the_anthropic_cloud_provider() {
    let s = Scratch::new("seed").await;
    let rows = s
        .client
        .query(
            "SELECT s.id, s.active_provider_id::text, p.mode, p.detected_api_type, p.url, \
             p.generation_model, p.summary_model, p.api_key IS NULL \
             FROM ai_settings s JOIN ai_provider p ON p.id = s.active_provider_id",
            &[],
        )
        .await
        .expect("the settings read");
    assert_eq!(rows.len(), 1);
    let row = &rows[0];
    assert_eq!(row.get::<_, i16>(0), 1);
    assert_eq!(row.get::<_, String>(1), CLOUD);
    assert_eq!(
        (2..=6).map(|i| row.get::<_, String>(i)).collect::<Vec<_>>(),
        [
            "api",
            "cloud",
            "https://api.anthropic.com/v1",
            "claude-sonnet-5-5",
            "claude-haiku-4-5-20251001"
        ]
    );
    assert!(row.get::<_, bool>(7), "no key is seeded");
    let kinds: Vec<String> = s
        .client
        .query(
            "SELECT mode || ' ' || coalesce(detected_api_type, \"binary\") FROM ai_provider ORDER BY 1",
            &[],
        )
        .await
        .expect("the providers read")
        .iter()
        .map(|row| row.get(0))
        .collect();
    assert_eq!(
        kinds,
        [
            "api cloud",
            "api opencode",
            "binaire claude-code",
            "binaire opencode"
        ]
    );
    s.drop().await;
}

#[tokio::test]
async fn mode_fields_is_kept_by_the_table() {
    let s = Scratch::new("mode_fields").await;
    let refused = [
        // api without url, without type, or with a binary.
        provider("'api'", "NULL", "'cloud'", "NULL"),
        provider("'api'", "''", "'cloud'", "NULL"),
        provider("'api'", "'https://api.anthropic.com/v1'", "NULL", "NULL"),
        provider(
            "'api'",
            "'https://api.anthropic.com/v1'",
            "'cloud'",
            "'opencode'",
        ),
        // binaire with another binary, none, or a url.
        provider("'binaire'", "NULL", "NULL", "'ollama'"),
        provider("'binaire'", "NULL", "NULL", "NULL"),
        provider(
            "'binaire'",
            "'http://192.168.1.40:4096'",
            "NULL",
            "'opencode'",
        ),
        // neither mode.
        provider("'binary'", "NULL", "NULL", "'opencode'"),
    ];
    for sql in refused {
        assert_eq!(
            s.refused(&sql).await,
            (
                SqlState::CHECK_VIOLATION,
                "ai_provider_mode_fields".to_string()
            ),
            "{sql}"
        );
    }
    s.drop().await;
}

#[tokio::test]
async fn api_type_recognized_is_kept_by_the_table_with_the_concept_s_types() {
    let s = Scratch::new("api_type").await;
    let concept: Value = serde_json::from_str(include_str!(
        "../../../../concepts/ProviderApiType/v1/concept.json"
    ))
    .expect("ProviderApiType@1's concept.json");
    for kind in concept["output"]["type"]["enum"].as_array().expect("types") {
        let kind = kind.as_str().expect("a type");
        s.client
            .batch_execute(&provider(
                "'api'",
                "'http://h'",
                &format!("'{kind}'"),
                "NULL",
            ))
            .await
            .unwrap_or_else(|e| panic!("{kind} is a type of the concept: {e}"));
    }
    assert_eq!(
        s.refused(&provider("'api'", "'http://h'", "'openai'", "NULL"))
            .await,
        (
            SqlState::CHECK_VIOLATION,
            "ai_provider_api_type_recognized".to_string()
        )
    );
    s.drop().await;
}

#[tokio::test]
async fn single_settings_row_is_kept_by_the_table() {
    let s = Scratch::new("single_row").await;
    let second = format!("INSERT INTO ai_settings (id, active_provider_id) VALUES (2, '{CLOUD}')");
    assert_eq!(
        s.refused(&second).await,
        (
            SqlState::CHECK_VIOLATION,
            "ai_settings_single_row".to_string()
        )
    );
    let again = format!("INSERT INTO ai_settings (id, active_provider_id) VALUES (1, '{CLOUD}')");
    assert_eq!(
        s.refused(&again).await,
        (SqlState::UNIQUE_VIOLATION, "ai_settings_pkey".to_string())
    );
    let nowhere = "UPDATE ai_settings SET active_provider_id = gen_random_uuid()";
    assert_eq!(
        s.refused(nowhere).await,
        (
            SqlState::FOREIGN_KEY_VIOLATION,
            "ai_settings_active_provider_id_fkey".to_string()
        )
    );
    // onDelete restrict: the active provider stays.
    let delete = format!("DELETE FROM ai_provider WHERE id = '{CLOUD}'");
    assert_eq!(
        s.refused(&delete).await,
        (
            SqlState::FOREIGN_KEY_VIOLATION,
            "ai_settings_active_provider_id_fkey".to_string()
        )
    );
    // onUpdate propagate: a new id follows.
    s.client
        .batch_execute(&format!(
            "UPDATE ai_provider SET id = 'b2000000-0000-4000-8000-000000000001' WHERE id = '{CLOUD}'"
        ))
        .await
        .expect("the id propagates");
    let active: String = s
        .client
        .query_one("SELECT active_provider_id::text FROM ai_settings", &[])
        .await
        .expect("the settings read")
        .get(0);
    assert_eq!(active, "b2000000-0000-4000-8000-000000000001");
    s.drop().await;
}
