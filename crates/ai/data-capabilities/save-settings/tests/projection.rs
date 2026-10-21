//! ai.saveSettings over PostgreSQL (`DATABASE_URL`): what the DM saves is
//! prepared, applied on `AiSettings/1` and projected into `ai_provider` and
//! `ai_settings` in the same transaction, and a start projects it again.
//! Each test works in a scratch database of its own, migrated as the server
//! migrates it, dropped when it ends.

use std::sync::Arc;

use harness_dataguard::aggregate::Aggregate;
use harness_dataguard::api::Submission;
use harness_dataguard::command::State;
use harness_dataguard::engine::Engine;
use harness_dataguard::guard::Caller;
use harness_dataguard::manifest::DataCapability;
use harness_dataguard::memory::MemoryNotifier;
use harness_dataguard::plugins::{Plugins, Shared};
use harness_dataguard::postgres::{PgStore, SystemClock};
use harness_dataguard::tokio_postgres::{self, Client, NoTls};
use ports_llm::SharedLlm;
use ports_llm::fake::FakeLlm;
use serde_json::{Value, json};

const HOME: &str = "http://192.168.1.40:4096";
const CLOUD: &str = "a1000000-0000-4000-8000-000000000001";
const OPENCODE: &str = "a1000000-0000-4000-8000-000000000002";
const OPENCODE_BINARY: &str = "a1000000-0000-4000-8000-000000000003";
const CLAUDE_CODE: &str = "a1000000-0000-4000-8000-000000000004";

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

/// The units the server plants for a save: its target aggregate's and its
/// own.
fn plugins() -> Plugins {
    [aggregate_ai_settings::plugin(), save_settings::plugin()]
        .into_iter()
        .collect()
}

/// A database of its own, migrated, and the server's engine over it.
struct Scratch {
    name: String,
    url: String,
    client: Client,
}

impl Scratch {
    async fn new(name: &str) -> Self {
        let name = format!("save_settings_test_{name}_{}", std::process::id());
        let admin = connect(&url()).await;
        for sql in [
            format!("DROP DATABASE IF EXISTS {name} WITH (FORCE)"),
            format!("CREATE DATABASE {name}"),
        ] {
            admin.batch_execute(&sql).await.expect("a scratch database");
        }
        let url = naming(&name);
        let client = connect(&url).await;
        let migration = platform_conformance::repository_root()
            .join("crates/data/migrations/0001_ai_settings.sql");
        let migration = std::fs::read_to_string(migration).expect("the migration");
        client.batch_execute(&migration).await.expect("migrated");
        Self { name, url, client }
    }

    /// The engine as the server plants it, the llm port's fake shared,
    /// whose home server answers its model list.
    async fn serve(&self) -> Engine {
        let plugins = plugins();
        let store = PgStore::open(connect(&self.url).await)
            .await
            .expect("the store")
            .with_projections(&plugins)
            .await
            .expect("warmed");
        let llm = FakeLlm::new().with_endpoint(HOME, json!({ "object": "list", "data": [] }));
        let mut shared = Shared::default();
        shared.insert(Arc::new(SharedLlm::new(llm)));
        let engine = Engine::new(
            Arc::new(store),
            Arc::new(MemoryNotifier::default()),
            Arc::new(SystemClock),
        )
        .with_aggregate(Aggregate::parse(aggregate_ai_settings::MANIFEST).expect("contract I"))
        .with_data_capability(
            DataCapability::parse(include_str!("../data-capability.json")).expect("contract F"),
        )
        .with_plugins(plugins)
        .with_shared(shared);
        engine.load().await.expect("an empty queue");
        engine
    }

    async fn active(&self) -> String {
        self.client
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
            .client
            .query_one(
                "SELECT row_to_json(p)::text FROM ai_provider p WHERE id = ($1::text)::uuid",
                &[&id],
            )
            .await
            .expect("the provider's row")
            .get(0);
        serde_json::from_str(&text).expect("JSON")
    }

    async fn drop(self) {
        drop(self.client);
        connect(&url())
            .await
            .batch_execute(&format!("DROP DATABASE {} WITH (FORCE)", self.name))
            .await
            .expect("the scratch database dropped");
    }
}

/// Saves `payload` as the DM and settles it: it is applied.
async fn save(engine: &Engine, payload: Value) {
    assert_eq!(settle(engine, payload).await, State::Applied);
}

/// Saves `payload` as the DM, settles it, and tells how it ended.
async fn settle(engine: &Engine, payload: Value) -> State {
    engine
        .submit(Submission {
            data_capability: "ai.saveSettings@1".to_string(),
            payload: payload
                .as_object()
                .expect("an object")
                .clone()
                .into_iter()
                .collect(),
            target_id: None,
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
    let applied = engine.run_once().await.expect("the applier ran");
    assert_eq!(applied.len(), 1, "{applied:?}");
    applied[0].status
}

#[tokio::test]
async fn a_save_reaches_ai_provider_and_ai_settings() {
    let db = Scratch::new("reaches").await;
    let engine = db.serve().await;
    assert_eq!(db.active().await, CLOUD, "the seed's");

    save(
        &engine,
        json!({
            "mode": "api",
            "url": HOME,
            "generationModel": "qwen3-32b",
            "summaryModel": "qwen3-14b",
            "key": "",
        }),
    )
    .await;
    assert_eq!(db.active().await, OPENCODE);
    let opencode = db.provider(OPENCODE).await;
    assert_eq!(opencode["url"], HOME);
    assert_eq!(opencode["detected_api_type"], "opencode");
    assert_eq!(opencode["generation_model"], "qwen3-32b");
    assert_eq!(opencode["summary_model"], "qwen3-14b");
    assert_eq!(opencode["api_key"], Value::Null);

    save(
        &engine,
        json!({
            "mode": "binary",
            "binary": "claude",
            "generationModel": "claude-sonnet-5-5",
            "summaryModel": "claude-haiku-4-5-20251001",
        }),
    )
    .await;
    assert_eq!(db.active().await, CLAUDE_CODE);
    let claude = db.provider(CLAUDE_CODE).await;
    assert_eq!(claude["mode"], "binaire");
    assert_eq!(claude["binary"], "claude-code");
    assert_eq!(claude["url"], Value::Null);
    // The provider saved before keeps its row.
    assert_eq!(db.provider(OPENCODE).await["generation_model"], "qwen3-32b");

    save(
        &engine,
        json!({
            "mode": "api",
            "url": "https://api.anthropic.com/v1",
            "generationModel": "claude-sonnet-5-5",
            "summaryModel": "claude-haiku-4-5-20251001",
            "key": "sk-ant-du-dm",
        }),
    )
    .await;
    assert_eq!(db.active().await, CLOUD);
    let cloud = db.provider(CLOUD).await;
    assert_eq!(cloud["api_key"], "sk-ant-du-dm");
    assert_eq!(cloud["detected_api_type"], "cloud");
    drop(engine);
    db.drop().await;
}

#[tokio::test]
async fn a_save_missing_a_model_is_rejected_and_the_tables_keep_theirs() {
    let db = Scratch::new("models").await;
    let engine = db.serve().await;
    let before = db.provider(OPENCODE).await;
    let state = settle(
        &engine,
        json!({ "mode": "api", "url": HOME, "summaryModel": "qwen3-14b", "key": "" }),
    )
    .await;
    assert_eq!(state, State::Rejected);
    assert_eq!(db.active().await, CLOUD, "the seed's");
    assert_eq!(db.provider(OPENCODE).await, before);
    drop(engine);
    db.drop().await;
}

#[tokio::test]
async fn a_start_projects_the_saved_settings_again() {
    let db = Scratch::new("repair").await;
    let engine = db.serve().await;
    save(
        &engine,
        json!({
            "mode": "binary",
            "binary": "opencode",
            "generationModel": "qwen3-32b",
            "summaryModel": "qwen3-14b",
        }),
    )
    .await;
    drop(engine);
    // The tables changed behind the engine.
    db.client
        .batch_execute(&format!(
            "UPDATE ai_settings SET active_provider_id = '{CLOUD}';
             UPDATE ai_provider SET generation_model = 'perdu' WHERE id = '{OPENCODE_BINARY}'"
        ))
        .await
        .expect("the tables changed behind the engine");

    let engine = db.serve().await;
    assert_eq!(db.active().await, OPENCODE_BINARY);
    assert_eq!(
        db.provider(OPENCODE_BINARY).await["generation_model"],
        "qwen3-32b"
    );
    drop(engine);
    db.drop().await;
}
