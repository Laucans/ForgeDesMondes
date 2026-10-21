//! `ai.testConnection` as the engine runs it: its `Prepare` ([`plugin`])
//! over the in-memory store, beside its target aggregate's plugin, the
//! `llm` port shared as the server shares it.

use std::collections::BTreeMap;
use std::sync::Arc;

use harness_dataguard::Error;
use harness_dataguard::aggregate::Aggregate;
use harness_dataguard::api::{Submission, Submitted};
use harness_dataguard::command::{CommandResult, State};
use harness_dataguard::engine::Engine;
use harness_dataguard::guard::Caller;
use harness_dataguard::manifest::DataCapability;
use harness_dataguard::memory::{FixedClock, MemoryNotifier, MemoryStore};
use harness_dataguard::plugins::Shared;
use ports_llm::fake::{FAKE_LATENCY_MS, FakeLlm};
use ports_llm::{CONNECTION_TEST_PROMPT, Provider, RequestKind, SharedLlm};
use serde_json::{Value, json};
use test_connection::{NO_PROVIDER, plugin};

const HOME: &str = "http://192.168.1.40:4096";
const CLOUD: &str = "a1000000-0000-4000-8000-000000000001";
const OPENCODE: &str = "a1000000-0000-4000-8000-000000000002";
const OPENCODE_BINARY: &str = "a1000000-0000-4000-8000-000000000003";
const CLAUDE_CODE: &str = "a1000000-0000-4000-8000-000000000004";

/// Now, as the engine's clock tells it: a millisecond into a second.
const NOW_MILLIS: u64 = 1_792_231_200_123;

fn llm() -> FakeLlm {
    FakeLlm::new().with_endpoint(HOME, json!({ "object": "list", "data": [] }))
}

fn object(value: Value) -> BTreeMap<String, Value> {
    value
        .as_object()
        .expect("an object")
        .clone()
        .into_iter()
        .collect()
}

/// The settings the home server's save leaves on `AiSettings/1`.
fn home_saved() -> BTreeMap<String, Value> {
    object(json!({
        "aiProviderId": OPENCODE,
        "activeProviderId": OPENCODE,
        "mode": "api",
        "label": "Endpoint opencode du serveur maison",
        "url": HOME,
        "detectedApiType": "opencode",
        "apiKey": null,
        "binary": null,
        "generationModel": "qwen2.5-32b-instruct",
        "summaryModel": "qwen2.5-14b-instruct",
        "updatedAt": "2026-10-17T10:00:00.000Z",
    }))
}

/// The engine over `store`, which the test reads, `llm` shared; a clone
/// of the fake shares its journal.
async fn testing(store: MemoryStore, llm: FakeLlm) -> (Engine, Arc<MemoryStore>, Arc<FixedClock>) {
    let store = Arc::new(store);
    let clock = Arc::new(FixedClock::at_millis(NOW_MILLIS));
    let mut shared = Shared::default();
    shared.insert(Arc::new(SharedLlm::new(llm)));
    let engine = Engine::new(
        store.clone(),
        Arc::new(MemoryNotifier::default()),
        clock.clone(),
    )
    .with_aggregate(Aggregate::parse(aggregate_ai_settings::MANIFEST).expect("contract I"))
    .with_data_capability(
        DataCapability::parse(include_str!("../data-capability.json")).expect("contract F"),
    )
    .with_plugins(
        [aggregate_ai_settings::plugin(), plugin()]
            .into_iter()
            .collect(),
    )
    .with_shared(shared);
    engine.load().await.expect("an empty queue");
    (engine, store, clock)
}

/// ai.testConnection by the DM, as the Micro-UI sends it, and what the
/// applier made of it.
async fn tested(engine: &Engine, payload: Value) -> (Submitted, CommandResult) {
    let submitted = engine
        .submit(Submission {
            data_capability: "ai.testConnection@1".to_string(),
            payload: object(payload),
            target_id: None,
            based_on: None,
            idempotency_key: None,
            caller: Caller {
                by: "dm".to_string(),
                from: "AiSettings".to_string(),
                permissions: Vec::new(),
                roles: Vec::new(),
            },
        })
        .await
        .expect("queued");
    let mut applied = engine.run_once().await.expect("the applier ran");
    assert_eq!(applied.len(), 1, "{applied:?}");
    (submitted, applied.remove(0))
}

fn api(url: &str, key: &str) -> Value {
    json!({
        "mode": "api",
        "url": url,
        "generationModel": "qwen2.5-32b-instruct",
        "summaryModel": "qwen2.5-14b-instruct",
        "key": key,
    })
}

fn partition(id: &str) -> String {
    format!("AiSettings/{id}")
}

#[tokio::test]
async fn a_test_of_each_provider_writes_its_last_test_on_its_row() {
    let seed =
        platform_conformance::repository_root().join("crates/data/migrations/0001_ai_settings.sql");
    let seed = std::fs::read_to_string(seed).expect("the migration");
    let (engine, store, _) = testing(MemoryStore::default(), llm()).await;
    for (payload, row) in [
        (api("https://api.anthropic.com/v1", "sk-ant-secret"), CLOUD),
        (api(HOME, ""), OPENCODE),
        (
            json!({ "mode": "binary", "binary": "opencode" }),
            OPENCODE_BINARY,
        ),
        (json!({ "mode": "binary", "binary": "claude" }), CLAUDE_CODE),
    ] {
        // The row tested is one the migration seeds.
        assert!(seed.contains(&format!("('{row}'")), "{row} is not seeded");
        let (submitted, result) = tested(&engine, payload).await;
        assert_eq!(submitted.partition, partition(row));
        assert_eq!(result.status, State::Applied, "{result:?}");
        let now = store.row_now(&partition(row));
        assert_eq!(now["lastTestOk"], true, "{row}: {now:?}");
        assert_eq!(now["lastTestLatencyMs"], FAKE_LATENCY_MS);
        assert_eq!(now["lastTestAt"], "2026-10-17T10:00:00.123Z", "{now:?}");
        assert!(
            now["lastTestMessage"]
                .as_str()
                .is_some_and(|m| m.starts_with("Connexion réussie")),
            "{now:?}"
        );
    }
}

#[tokio::test]
async fn two_tests_in_a_second_are_stamped_apart() {
    let (engine, store, clock) = testing(MemoryStore::default(), llm()).await;
    tested(&engine, api(HOME, "")).await;
    clock.set_millis(NOW_MILLIS + 500);
    tested(&engine, api(HOME, "")).await;
    assert_eq!(
        store.row_now(&partition(OPENCODE))["lastTestAt"],
        "2026-10-17T10:00:00.623Z"
    );
}

#[tokio::test]
async fn an_unrecognised_url_is_refused_on_no_provider_s_partition_saying_why() {
    let (engine, store, _) = testing(MemoryStore::default(), llm()).await;
    let (_, result) = tested(&engine, api("https://inconnu.example.org/pas-une-api", "")).await;
    assert_eq!(result.status, State::Applied, "{result:?}");
    let now = store.row_now(&partition(NO_PROVIDER));
    assert_eq!(now["lastTestOk"], false, "{now:?}");
    assert!(
        now["lastTestMessage"]
            .as_str()
            .is_some_and(|m| m.contains("type d'API non reconnu")),
        "{now:?}"
    );
}

#[tokio::test]
async fn a_test_saves_nothing_of_what_it_tested_and_never_echoes_the_key() {
    let store = MemoryStore::default().with_row("AiSettings/1", home_saved());
    let (engine, store, _) = testing(store, llm()).await;
    let key = "sk-ant-api03-très-secret";
    let mut payload = api("https://api.anthropic.com/v1", key);
    // What the screen would never send is not saved either, and is said.
    payload["detectedApiType"] = json!("cloud");
    payload["activeProviderId"] = json!(CLOUD);
    let (submitted, result) = tested(&engine, payload).await;
    assert_eq!(
        submitted.warnings,
        ["payload key(s) activeProviderId, detectedApiType not declared by ai.testConnection"]
    );
    assert_eq!(result.status, State::Applied, "{result:?}");
    assert_eq!(store.row_now("AiSettings/1"), home_saved());
    let now = store.row_now(&partition(CLOUD));
    let fields: Vec<&str> = now.keys().map(String::as_str).collect();
    assert_eq!(
        fields,
        [
            "lastTestAt",
            "lastTestLatencyMs",
            "lastTestMessage",
            "lastTestOk"
        ]
    );
    let status = engine
        .status(&submitted.command_id)
        .await
        .expect("a status");
    for text in [
        serde_json::to_string(&submitted).expect("a submission serialises"),
        serde_json::to_string(&status).expect("a status serialises"),
        format!("{now:?}"),
        format!("{result:?}"),
    ] {
        assert!(!text.contains(key), "{text}");
    }
}

#[tokio::test]
async fn what_a_test_sends_the_provider_is_its_text_prompt_alone() {
    let llm = FakeLlm::new();
    let (engine, _, _) = testing(MemoryStore::default(), llm.clone()).await;
    let key = "sk-ant-api03-très-secret";
    tested(&engine, api("https://api.anthropic.com/v1", key)).await;
    tested(&engine, json!({ "mode": "binary", "binary": "claude" })).await;
    let sent = ports_llm::Llm::journal(&llm).list();
    assert_eq!(sent.len(), 2, "{sent:?}");
    for request in &sent {
        assert_eq!(request.kind, RequestKind::ConnectionTest);
        assert_eq!(request.text, CONNECTION_TEST_PROMPT);
    }
    assert_eq!(
        (sent[0].provider, sent[1].provider),
        (Provider::Cloud, Provider::ClaudeCodeBinary)
    );
}

#[tokio::test]
async fn a_server_that_shares_no_llm_port_does_not_load() {
    let engine = Engine::new(
        Arc::new(MemoryStore::default()),
        Arc::new(MemoryNotifier::default()),
        Arc::new(FixedClock::at(1000)),
    )
    .with_aggregate(Aggregate::parse(aggregate_ai_settings::MANIFEST).expect("contract I"))
    .with_data_capability(
        DataCapability::parse(include_str!("../data-capability.json")).expect("contract F"),
    )
    .with_plugins(std::iter::once(plugin()).collect());
    let refused = engine.load().await;
    assert!(
        matches!(&refused, Err(Error::Misconfigured(why)) if why.contains("SharedLlm")),
        "{refused:?}"
    );
}
