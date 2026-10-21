//! The aggregate's projection over PostgreSQL (`DATABASE_URL`): what the
//! engine applies reaches `ai_provider` and `ai_settings` in the apply
//! transaction, what the tables refuse is rejected, and a start writes again
//! what the tables lost. Each test works in a scratch database of its own,
//! migrated as the server migrates it, dropped when it ends.
//!
//! The commands are test-only DataCapabilities writing the aggregate's state
//! as given, under the ids the invariants know: no `Prepare` runs here.

use std::sync::Arc;

use aggregate_ai_settings::{MANIFEST, NO_PROVIDER, SETTINGS_PARTITION, plugin};
use harness_dataguard::aggregate::Aggregate;
use harness_dataguard::api::Submission;
use harness_dataguard::command::{CommandResult, State};
use harness_dataguard::engine::Engine;
use harness_dataguard::guard::Caller;
use harness_dataguard::manifest::DataCapability;
use harness_dataguard::memory::{FixedClock, MemoryNotifier};
use harness_dataguard::plugins::Plugins;
use harness_dataguard::postgres::PgStore;
use harness_dataguard::tokio_postgres::{self, Client, NoTls};
use serde_json::{Value, json};

/// The settings row written whole, as ai.saveSettings writes it.
const SAVE_SETTINGS: &str = r#"{"dataCapability":"ai.saveSettings","owner":"dataguard","version":1,"effect":"upsert","target":{"aggregate":"AiSettings"},"mode":"overwrite","callableBy":["*"]}"#;
/// A connection test's outcome, under the id `test-does-not-save` knows.
const TEST_CONNECTION: &str = r#"{"dataCapability":"ai.testConnection","owner":"dataguard","version":1,"effect":"update","target":{"aggregate":"AiSettings"},"mode":"overwrite","callableBy":["*"]}"#;

const CLOUD: &str = "a1000000-0000-4000-8000-000000000001";
const HOME: &str = "a1000000-0000-4000-8000-000000000002";
const OPENCODE_BINARY: &str = "a1000000-0000-4000-8000-000000000003";

fn url() -> String {
    std::env::var("DATABASE_URL").expect("DATABASE_URL names the harness's PostgreSQL")
}

async fn connect(url: &str) -> Client {
    let (client, connection) = tokio_postgres::connect(url, NoTls)
        .await
        .expect("PostgreSQL");
    tokio::spawn(connection);
    client
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

/// A database of its own, and the tables over it.
struct Scratch {
    name: String,
    url: String,
    tables: Client,
}

impl Scratch {
    /// A scratch database, migrated (`0001_ai_settings.sql`) or not.
    async fn new(name: &str, migrated: bool) -> Self {
        let name = format!("ai_settings_projection_{name}_{}", std::process::id());
        let admin = connect(&url()).await;
        for sql in [
            format!("DROP DATABASE IF EXISTS {name} WITH (FORCE)"),
            format!("CREATE DATABASE {name}"),
        ] {
            admin.batch_execute(&sql).await.expect("a scratch database");
        }
        let url = naming(&name);
        let tables = connect(&url).await;
        if migrated {
            let migration = platform_conformance::repository_root()
                .join("crates/data/migrations/0001_ai_settings.sql");
            let migration = std::fs::read_to_string(migration).expect("the migration");
            tables.batch_execute(&migration).await.expect("migrated");
        }
        Self { name, url, tables }
    }

    /// The store over the database, the aggregate's projection warmed.
    async fn store(&self) -> Result<PgStore, harness_dataguard::Error> {
        let plugins: Plugins = std::iter::once(plugin()).collect();
        PgStore::open(connect(&self.url).await)
            .await
            .expect("the store")
            .with_projections(&plugins)
            .await
    }

    /// The engine as the server plants it: the aggregate, its plugin, and
    /// the test-only DataCapabilities.
    async fn engine(&self) -> Engine {
        self.telling(Arc::default()).await
    }

    /// The same, its messages sent through `notifier`.
    async fn telling(&self, notifier: Arc<MemoryNotifier>) -> Engine {
        let store = self.store().await.expect("warmed");
        let mut engine = Engine::new(Arc::new(store), notifier, Arc::new(FixedClock::at(1000)))
            .with_aggregate(Aggregate::parse(MANIFEST).expect("contract I"))
            .with_plugins(std::iter::once(plugin()).collect());
        for manifest in [SAVE_SETTINGS, TEST_CONNECTION] {
            engine =
                engine.with_data_capability(DataCapability::parse(manifest).expect("contract F"));
        }
        engine.load().await.expect("an empty queue");
        engine
    }

    async fn active(&self) -> String {
        self.tables
            .query_one(
                "SELECT active_provider_id::text FROM ai_settings WHERE id = 1",
                &[],
            )
            .await
            .expect("the settings row")
            .get(0)
    }

    /// A provider's row as JSON.
    async fn provider(&self, id: &str) -> Value {
        let text: String = self
            .tables
            .query_one(
                "SELECT row_to_json(p)::text FROM ai_provider p WHERE id = ($1::text)::uuid",
                &[&id],
            )
            .await
            .expect("the provider's row")
            .get(0);
        serde_json::from_str(&text).expect("JSON")
    }

    /// Every column of both tables, as text, `last_test_*` aside or not.
    async fn snapshot(&self, last_test: bool) -> Vec<String> {
        let drop = if last_test {
            ""
        } else {
            " - 'last_test_ok' - 'last_test_at' - 'last_test_message' - 'last_test_latency_ms'"
        };
        let sql = format!(
            "SELECT (to_jsonb(p){drop})::text FROM ai_provider p \
             UNION ALL SELECT to_jsonb(s)::text FROM ai_settings s ORDER BY 1"
        );
        self.tables
            .query(&sql, &[])
            .await
            .expect("the tables")
            .iter()
            .map(|row| row.get(0))
            .collect()
    }

    async fn drop(self) {
        drop(self.tables);
        connect(&url())
            .await
            .batch_execute(&format!("DROP DATABASE {} WITH (FORCE)", self.name))
            .await
            .expect("the scratch database dropped");
    }
}

/// Submits `payload` on `AiSettings/<target>` and answers what the applier
/// made of it.
async fn written(
    engine: &Engine,
    data_capability: &str,
    target: &str,
    payload: Value,
) -> CommandResult {
    let Value::Object(payload) = payload else {
        panic!("a payload is an object");
    };
    engine
        .submit(Submission {
            data_capability: data_capability.to_string(),
            payload: payload.into_iter().collect(),
            target_id: Some(target.to_string()),
            based_on: None,
            idempotency_key: None,
            caller: Caller {
                by: "dm".to_string(),
                from: "AiSettings".to_string(),
                permissions: Vec::new(),
                roles: vec!["role:approver".to_string()],
            },
        })
        .await
        .expect("queued");
    let mut settled = engine.run_once().await.expect("the applier ran");
    assert_eq!(settled.len(), 1, "{settled:?}");
    settled.remove(0)
}

/// The settings row carrying `provider` whole and making it active.
fn saved(provider: &str, fields: Value) -> Value {
    let mut state = json!({
        "aiProviderId": provider,
        "activeProviderId": provider,
        "url": null,
        "detectedApiType": null,
        "apiKey": null,
        "binary": null,
        "updatedAt": "2026-10-17T10:00:00.000Z",
    });
    for (field, value) in fields.as_object().expect("an object") {
        state[field] = value.clone();
    }
    state
}

fn opencode_binary(generation: &str) -> Value {
    saved(
        OPENCODE_BINARY,
        json!({
            "mode": "binaire",
            "label": "Binaire opencode",
            "binary": "opencode",
            "generationModel": generation,
            "summaryModel": "qwen3-14b",
        }),
    )
}

fn outcome(ok: bool, at: &str, message: &str, latency: Value) -> Value {
    json!({
        "lastTestOk": ok,
        "lastTestAt": at,
        "lastTestMessage": message,
        "lastTestLatencyMs": latency,
    })
}

#[tokio::test]
async fn a_save_reaches_both_tables_and_a_start_writes_it_again() {
    let db = Scratch::new("save", true).await;
    let engine = db.engine().await;
    assert_eq!(db.active().await, CLOUD, "the seed's");

    let result = written(
        &engine,
        "ai.saveSettings@1",
        "1",
        opencode_binary("qwen3-32b"),
    )
    .await;
    assert_eq!(result.status, State::Applied, "{result:?}");
    assert_eq!(db.active().await, OPENCODE_BINARY);
    let row = db.provider(OPENCODE_BINARY).await;
    assert_eq!(row["generation_model"], "qwen3-32b");
    assert_eq!(row["mode"], "binaire");
    let updated: bool = db
        .tables
        .query_one(
            "SELECT updated_at = '2026-10-17T10:00:00Z' FROM ai_settings WHERE id = 1",
            &[],
        )
        .await
        .expect("the settings row")
        .get(0);
    assert!(updated, "updated_at is the save's");
    drop(engine);

    // The tables changed behind the engine: a start writes the save again.
    db.tables
        .batch_execute(&format!(
            "UPDATE ai_settings SET active_provider_id = '{CLOUD}';
             UPDATE ai_provider SET generation_model = 'perdu' WHERE id = '{OPENCODE_BINARY}'"
        ))
        .await
        .expect("the tables changed");
    let engine = db.engine().await;
    assert_eq!(db.active().await, OPENCODE_BINARY);
    assert_eq!(
        db.provider(OPENCODE_BINARY).await["generation_model"],
        "qwen3-32b"
    );
    drop(engine);
    db.drop().await;
}

#[tokio::test]
async fn a_connection_test_reaches_the_last_test_columns_of_its_row_and_nothing_else() {
    let db = Scratch::new("last_test", true).await;
    db.tables
        .batch_execute(&format!(
            "UPDATE ai_settings SET active_provider_id = '{HOME}' WHERE id = 1"
        ))
        .await
        .expect("the home server active");
    let engine = db.engine().await;
    let before = db.snapshot(false).await;

    // The home server tested first, then a test that reached no provider
    // lands on it, then a test applied late, older than both.
    for (target, ok, message, latency, at) in [
        (
            HOME,
            true,
            "Connexion réussie",
            json!(30),
            "2026-10-17T09:00:00.000Z",
        ),
        (
            CLOUD,
            true,
            "Connexion réussie",
            json!(12),
            "2026-10-17T10:00:00.000Z",
        ),
        (
            NO_PROVIDER,
            false,
            "type d'API non reconnu",
            Value::Null,
            "2026-10-17T10:00:00.000Z",
        ),
        (
            HOME,
            true,
            "Connexion réussie",
            json!(40),
            "2026-10-17T08:00:00.000Z",
        ),
    ] {
        let result = written(
            &engine,
            "ai.testConnection@1",
            target,
            outcome(ok, at, message, latency),
        )
        .await;
        assert_eq!(result.status, State::Applied, "{result:?}");
    }

    let tested = |id: &str| {
        format!(
            "SELECT last_test_ok, last_test_message, last_test_latency_ms, \
             last_test_at = '2026-10-17T10:00:00Z' FROM ai_provider WHERE id = '{id}'"
        )
    };
    let cloud = db
        .tables
        .query_one(&tested(CLOUD), &[])
        .await
        .expect("cloud");
    assert_eq!(cloud.get::<_, Option<bool>>(0), Some(true));
    assert_eq!(cloud.get::<_, Option<&str>>(1), Some("Connexion réussie"));
    assert_eq!(cloud.get::<_, Option<i32>>(2), Some(12));
    assert_eq!(cloud.get::<_, Option<bool>>(3), Some(true));
    // The test that reached no provider is the active one's.
    let home = db.tables.query_one(&tested(HOME), &[]).await.expect("home");
    assert_eq!(home.get::<_, Option<bool>>(0), Some(false));
    assert_eq!(
        home.get::<_, Option<&str>>(1),
        Some("type d'API non reconnu")
    );
    assert_eq!(home.get::<_, Option<i32>>(2), None);
    let untested = db.snapshot(true).await;
    assert_eq!(
        untested
            .iter()
            .filter(|row| row.contains("\"last_test_ok\": null"))
            .count(),
        2,
        "{untested:?}"
    );
    assert_eq!(db.snapshot(false).await, before);
    drop(engine);

    // A test the tables lost is written again at a start; the test that
    // reached no provider is left where it landed, over the home server's
    // own older one.
    let tests = db.snapshot(true).await;
    db.tables
        .batch_execute(&format!(
            "UPDATE ai_provider SET last_test_ok = NULL, last_test_at = NULL, \
             last_test_message = NULL, last_test_latency_ms = NULL WHERE id = '{CLOUD}'"
        ))
        .await
        .expect("the cloud's test lost");
    let engine = db.engine().await;
    assert_eq!(db.snapshot(true).await, tests);
    drop(engine);
    db.drop().await;
}

#[tokio::test]
async fn two_tests_in_a_second_are_told_apart_and_the_later_one_wins() {
    let db = Scratch::new("millis", true).await;
    let engine = db.engine().await;
    for (message, at) in [
        ("la seconde", "2026-10-17T10:00:00.900Z"),
        ("la première", "2026-10-17T10:00:00.100Z"),
    ] {
        let result = written(
            &engine,
            "ai.testConnection@1",
            CLOUD,
            outcome(true, at, message, json!(12)),
        )
        .await;
        assert_eq!(result.status, State::Applied, "{result:?}");
    }
    assert_eq!(db.provider(CLOUD).await["last_test_message"], "la seconde");
    drop(engine);
    db.drop().await;
}

#[tokio::test]
async fn a_write_the_tables_refuse_is_rejected_saying_where_never_what() {
    let db = Scratch::new("refused", true).await;
    let notifier = Arc::new(MemoryNotifier::default());
    let engine = db.telling(notifier.clone()).await;
    let before = db.snapshot(true).await;
    let key = "sk-ant-api03-très-secret";

    // No label: the judges let it through, `ai_provider` does not (NOT NULL).
    let unlabelled = saved(
        CLOUD,
        json!({
            "mode": "api",
            "url": "https://api.anthropic.com/v1",
            "detectedApiType": "cloud",
            "apiKey": key,
            "generationModel": "claude-sonnet-5-5",
            "summaryModel": "claude-haiku-4-5-20251001",
        }),
    );
    let result = written(&engine, "ai.saveSettings@1", "1", unlabelled).await;
    assert_eq!(result.status, State::Rejected, "{result:?}");
    let told = format!("{:?}", notifier.to("dm"));
    assert!(
        told.contains("AiSettings/1 not projected: 23502 label"),
        "{told}"
    );
    assert!(!told.contains(key), "{told}");
    assert!(!format!("{result:?}").contains(key), "{result:?}");

    // A stamp no timestamptz can hold.
    let result = written(
        &engine,
        "ai.testConnection@1",
        CLOUD,
        outcome(true, "pas une date", "Connexion réussie", json!(12)),
    )
    .await;
    assert_eq!(result.status, State::Rejected, "{result:?}");
    let told = format!("{:?}", notifier.to("dm"));
    assert!(
        told.contains(&format!("AiSettings/{CLOUD} not projected: 22007")),
        "{told}"
    );

    // Neither reached the tables, nor the engine's rows.
    assert_eq!(db.snapshot(true).await, before);
    let held: i64 = db
        .tables
        .query_one(
            "SELECT count(*) FROM dataguard_rows WHERE partition LIKE 'AiSettings/%'",
            &[],
        )
        .await
        .expect("the engine's rows")
        .get(0);
    assert_eq!(held, 0);
    drop(engine);
    db.drop().await;
}

#[tokio::test]
async fn a_failure_the_tables_do_not_judge_fails_the_step() {
    let db = Scratch::new("failed", true).await;
    let engine = db.engine().await;
    db.tables
        .batch_execute("DROP TABLE ai_settings")
        .await
        .expect("the settings table gone");
    engine
        .submit(Submission {
            data_capability: "ai.saveSettings@1".to_string(),
            payload: opencode_binary("qwen3-32b")
                .as_object()
                .expect("an object")
                .clone()
                .into_iter()
                .collect(),
            target_id: Some("1".to_string()),
            based_on: None,
            idempotency_key: None,
            caller: Caller {
                by: "dm".to_string(),
                from: "AiSettings".to_string(),
                permissions: Vec::new(),
                roles: vec!["role:approver".to_string()],
            },
        })
        .await
        .expect("queued");
    let failed = engine.run_once().await.expect_err("the step fails");
    assert!(failed.to_string().contains("42P01"), "{failed}");
    drop(engine);
    db.drop().await;
}

#[tokio::test]
async fn a_start_the_tables_refuse_does_not_start() {
    let db = Scratch::new("cold", true).await;
    let engine = db.engine().await;
    let result = written(
        &engine,
        "ai.saveSettings@1",
        "1",
        opencode_binary("qwen3-32b"),
    )
    .await;
    assert_eq!(result.status, State::Applied, "{result:?}");
    drop(engine);
    // The tables changed behind the engine, and now refuse what it holds:
    // the provider is written again, the settings row cannot be.
    db.tables
        .batch_execute(&format!(
            "UPDATE ai_settings SET active_provider_id = '{CLOUD}';
             UPDATE ai_provider SET generation_model = 'perdu' WHERE id = '{OPENCODE_BINARY}';
             ALTER TABLE ai_settings ADD CONSTRAINT no_binary
                 CHECK (active_provider_id <> '{OPENCODE_BINARY}') NOT VALID"
        ))
        .await
        .expect("a constraint the saved settings break");
    let refused = db.store().await.err().expect("the store does not start");
    assert!(refused.to_string().contains("23514 no_binary"), "{refused}");
    // Nothing of the repair was kept: the provider's row is rolled back too.
    assert_eq!(db.active().await, CLOUD);
    assert_eq!(
        db.provider(OPENCODE_BINARY).await["generation_model"],
        "perdu"
    );
    db.drop().await;
}

#[tokio::test]
async fn a_start_leaves_tables_not_migrated_yet_alone() {
    let db = Scratch::new("bare", false).await;
    assert!(db.store().await.is_ok(), "nothing to write");
    db.drop().await;
}

#[test]
fn the_settings_partition_is_the_one_row() {
    assert_eq!(SETTINGS_PARTITION, "AiSettings/1");
}
