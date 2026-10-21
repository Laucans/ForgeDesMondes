//! `ai.saveSettings` as the engine runs it: its `Prepare` ([`plugin`]) over
//! the in-memory store, beside its target aggregate's plugin, the `llm`
//! port shared as the server shares it.

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
use ports_llm::SharedLlm;
use ports_llm::fake::FakeLlm;
use save_settings::{TARGET_ID, plugin, prepare};
use serde_json::{Value, json};

/// The home server's URL in the fixtures, and the fake that answers its
/// model list there.
const HOME: &str = "http://192.168.1.40:4096";
const OPENCODE: &str = "a1000000-0000-4000-8000-000000000002";
const CLAUDE_CODE: &str = "a1000000-0000-4000-8000-000000000004";
const SETTINGS: &str = "AiSettings/1";

/// Now, as the engine's clock tells it: a millisecond into a second.
const NOW_MILLIS: u64 = 1_792_231_200_123;

fn llm() -> FakeLlm {
    FakeLlm::new().with_endpoint(HOME, json!({ "object": "list", "data": [] }))
}

/// The engine over a memory store the test reads, `llm` shared.
async fn saving(llm: FakeLlm) -> (Engine, Arc<MemoryStore>) {
    let store = Arc::new(MemoryStore::default());
    let mut shared = Shared::default();
    shared.insert(Arc::new(SharedLlm::new(llm)));
    let engine = Engine::new(
        store.clone(),
        Arc::new(MemoryNotifier::default()),
        Arc::new(FixedClock::at_millis(NOW_MILLIS)),
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
    (engine, store)
}

fn object(value: Value) -> BTreeMap<String, Value> {
    value
        .as_object()
        .expect("an object")
        .clone()
        .into_iter()
        .collect()
}

/// ai.saveSettings by the DM, approver of their own settings, as the
/// Micro-UI sends it: no target, the payload the screen's.
async fn save(engine: &Engine, payload: Value, key: Option<&str>) -> Submitted {
    engine
        .submit(Submission {
            data_capability: "ai.saveSettings@1".to_string(),
            payload: object(payload),
            target_id: None,
            based_on: None,
            idempotency_key: key.map(str::to_string),
            caller: Caller {
                by: "dm".to_string(),
                from: "AiSettings".to_string(),
                permissions: Vec::new(),
                roles: vec!["role:approver".to_string()],
            },
        })
        .await
        .expect("queued")
}

/// Saves `payload` and answers what the applier made of it.
async fn saved(engine: &Engine, payload: Value) -> CommandResult {
    save(engine, payload, None).await;
    let mut applied = engine.run_once().await.expect("the applier ran");
    assert_eq!(applied.len(), 1, "{applied:?}");
    applied.remove(0)
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

#[tokio::test]
async fn saving_the_home_server_s_url_applies_the_opencode_provider_and_makes_it_active() {
    let (engine, store) = saving(llm()).await;
    let submitted = save(&engine, api(HOME, ""), None).await;
    assert_eq!(submitted.partition, SETTINGS);
    assert!(submitted.warnings.is_empty(), "{submitted:?}");
    let result = engine.run_once().await.expect("the applier ran");
    assert_eq!(result[0].status, State::Applied, "{result:?}");
    let row = store.row_now(SETTINGS);
    assert_eq!(row["aiProviderId"], OPENCODE);
    assert_eq!(row["activeProviderId"], OPENCODE);
    assert_eq!(row["url"], HOME);
    assert_eq!(row["detectedApiType"], "opencode");
    assert_eq!(row["generationModel"], "qwen2.5-32b-instruct");
    assert_eq!(row["summaryModel"], "qwen2.5-14b-instruct");
    assert_eq!(row["apiKey"], Value::Null);
}

#[tokio::test]
async fn a_save_is_stamped_to_the_millisecond() {
    let (engine, store) = saving(llm()).await;
    saved(&engine, api(HOME, "")).await;
    assert_eq!(
        store.row_now(SETTINGS)["updatedAt"],
        "2026-10-17T10:00:00.123Z"
    );
}

#[tokio::test]
async fn saving_binary_claude_stores_claude_code_with_no_url_and_makes_it_active() {
    let (engine, store) = saving(llm()).await;
    saved(&engine, api(HOME, "")).await;
    let result = saved(
        &engine,
        json!({
            "mode": "binary",
            "binary": "claude",
            "generationModel": "claude-sonnet-5-5",
            "summaryModel": "claude-haiku-4-5-20251001",
        }),
    )
    .await;
    assert_eq!(result.status, State::Applied, "{result:?}");
    let row = store.row_now(SETTINGS);
    assert_eq!(row["mode"], "binaire");
    assert_eq!(row["binary"], "claude-code");
    assert_eq!(row["activeProviderId"], CLAUDE_CODE);
    // Nothing of the api provider saved before is left on the row.
    for field in ["url", "detectedApiType", "apiKey"] {
        assert_eq!(row[field], Value::Null, "{field}");
    }
}

#[tokio::test]
async fn an_unrecognised_url_is_rejected_saying_why_and_nothing_changes() {
    let (engine, store) = saving(llm()).await;
    saved(&engine, api(HOME, "")).await;
    let before = store.row_now(SETTINGS);
    let submitted = save(
        &engine,
        api("https://inconnu.example.org/pas-une-api", ""),
        None,
    )
    .await;
    // Its own reason first, then the engine's.
    assert_eq!(
        submitted.warnings,
        [
            "type d'API non reconnu",
            "invariant(s) at risk: api-type-recognized"
        ]
    );
    let mut settled = engine.run_once().await.expect("the applier ran");
    let result = settled.remove(0);
    assert_eq!(result.status, State::Rejected, "{result:?}");
    assert_eq!(result.violations, ["api-type-recognized"]);
    assert_eq!(store.row_now(SETTINGS), before);
}

#[tokio::test]
async fn an_api_type_the_caller_gives_is_not_the_one_stored_and_the_caller_is_told() {
    let (engine, store) = saving(llm()).await;
    let mut payload = api(HOME, "");
    payload["apiType"] = json!("cloud");
    payload["detectedApiType"] = json!("cloud");
    let submitted = save(&engine, payload, None).await;
    // What the manifest does not declare is said: the engine reads the
    // payload sent, not the one prepared.
    assert_eq!(
        submitted.warnings,
        ["payload key(s) apiType, detectedApiType not declared by ai.saveSettings"]
    );
    let result = engine.run_once().await.expect("the applier ran");
    assert_eq!(result[0].status, State::Applied, "{result:?}");
    let row = store.row_now(SETTINGS);
    assert_eq!(row["detectedApiType"], "opencode");
    assert!(!row.contains_key("apiType"));
}

#[tokio::test]
async fn the_same_idempotency_key_twice_is_one_save() {
    let (engine, _) = saving(llm()).await;
    let first = save(&engine, api(HOME, ""), Some("save-1")).await;
    let second = save(&engine, api(HOME, ""), Some("save-1")).await;
    assert!(second.replayed, "{second:?}");
    assert_eq!(second.command_id, first.command_id);
    let applied = engine.run_once().await.expect("the applier ran");
    assert_eq!(applied.len(), 1, "{applied:?}");
    assert!(engine.run_once().await.expect("the applier ran").is_empty());
}

#[tokio::test]
async fn the_api_key_is_never_echoed_by_the_submission_or_the_result() {
    let (engine, _) = saving(llm()).await;
    let key = "sk-ant-api03-très-secret";
    let submitted = save(
        &engine,
        json!({
            "mode": "api",
            "url": "https://api.anthropic.com/v1",
            "generationModel": "claude-sonnet-5-5",
            "summaryModel": "claude-haiku-4-5-20251001",
            "key": key,
        }),
        None,
    )
    .await;
    let text = serde_json::to_string(&submitted).expect("a submission serialises");
    assert!(!text.contains(key), "{text}");
    let applied = engine.run_once().await.expect("the applier ran");
    let result = serde_json::to_string(&applied).expect("a result serialises");
    assert!(!result.contains(key), "{result}");
    let status = engine
        .status(&submitted.command_id)
        .await
        .expect("a status");
    assert_eq!(status.status, State::Applied);
    let status = serde_json::to_string(&status).expect("a status serialises");
    assert!(!status.contains(key), "{status}");
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

#[tokio::test]
async fn each_kind_of_provider_is_saved_on_its_seeded_row_under_its_label() {
    let seed =
        platform_conformance::repository_root().join("crates/data/migrations/0001_ai_settings.sql");
    let seed = std::fs::read_to_string(seed).expect("the migration");
    for wire in [
        api("https://api.anthropic.com/v1", ""),
        api(HOME, ""),
        json!({ "mode": "binary", "binary": "opencode" }),
        json!({ "mode": "binary", "binary": "claude" }),
    ] {
        let payload = prepare(&llm(), &object(wire), "2026-10-17T10:00:00.000Z")
            .await
            .payload;
        let id = payload["aiProviderId"].as_str().expect("a row");
        let label = payload["label"].as_str().expect("a label");
        let mode = payload["mode"].as_str().expect("a mode");
        assert!(
            seed.contains(&format!("('{id}', '{mode}', '{label}'")),
            "{id} {label} is not seeded so"
        );
    }
    assert_eq!(TARGET_ID, "1");
}
