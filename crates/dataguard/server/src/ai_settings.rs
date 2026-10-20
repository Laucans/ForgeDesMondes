//! The invariants of the `AiSettings` aggregate
//! (`crates/dataguard/aggregates/AiSettings/aggregate.json`), which its
//! rules write in words: the engine judges them here, on the row's state
//! after the command.
//!
//! One aggregate, two kinds of row, told apart by the partition's id as the
//! migration types it: a provider (`ai_provider`) is a uuid, anything else
//! claims to be the settings (`ai_settings`), whose one id is 1. The engine
//! sees one row at a time: that the active provider exists is judged by the
//! aggregate's `restrict` relation, which blocks its deletion, and on the
//! tables by the foreign key of the migration (`crates/data/migrations/`),
//! whose CHECKs judge the other invariants there again.
//!
//! `ai.saveSettings` writes the settings row whole, carrying the provider it
//! makes active (`aiProviderId` = `activeProviderId`) and that provider's
//! fields: the store projects it into both tables. Such a row is judged as
//! the provider it carries, too.

use std::collections::BTreeMap;
use std::sync::LazyLock;

use harness_dataguard::aggregate::Aggregate;
use harness_dataguard::command::{Effect, Entry};
use harness_dataguard::guard::Invariants;
use serde_json::Value;

const AGGREGATE: &str = "AiSettings";

/// The binaries a provider in « binaire » mode may name.
const BINARIES: [&str; 2] = ["opencode", "claude-code"];

/// What « Tester la connexion » may write.
const LAST_TEST: [&str; 4] = [
    "lastTestOk",
    "lastTestAt",
    "lastTestMessage",
    "lastTestLatencyMs",
];

/// The DataCapability behind « Tester la connexion ».
const TEST_CONNECTION: &str = "ai.testConnection";

/// The models every provider names, which `ai_provider` requires.
const MODELS: [&str; 2] = ["generationModel", "summaryModel"];

/// The fields only the settings row carries.
const SETTINGS: [&str; 3] = ["aiSettingsId", "activeProviderId", "updatedAt"];

/// The API types ProviderApiType@1 names, read from its concept.json.
static API_TYPES: LazyLock<Vec<String>> = LazyLock::new(|| {
    let concept: Value = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../concepts/ProviderApiType/v1/concept.json"
    )))
    .expect("ProviderApiType@1's concept.json is JSON");
    concept["output"]["type"]["enum"]
        .as_array()
        .map(|types| {
            types
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .expect("ProviderApiType@1 names its types in output.type.enum")
});

/// The fields the aggregate declares: the only ones a command may write.
static DECLARED: LazyLock<Vec<String>> = LazyLock::new(|| {
    Aggregate::parse(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../aggregates/AiSettings/aggregate.json"
    )))
    .expect("AiSettings' aggregate.json conforms to contract I")
    .fields
    .into_keys()
    .collect()
});

/// One invariant: its id, and whether it holds on the state the command
/// leaves.
type Check = (&'static str, fn(&BTreeMap<String, Value>, &Entry) -> bool);

const CHECKS: [Check; 5] = [
    ("mode-fields", mode_fields),
    ("api-type-recognized", api_type_recognized),
    ("test-does-not-save", test_does_not_save),
    ("single-settings-row", single_settings_row),
    ("text-only-payload", text_only_payload),
];

/// The invariants `plant()` binds the engine with.
pub struct AiSettings;

impl Invariants for AiSettings {
    fn violated(
        &self,
        aggregate: &str,
        state: &BTreeMap<String, Value>,
        entry: &Entry,
    ) -> Vec<String> {
        if aggregate != AGGREGATE {
            return Vec::new();
        }
        CHECKS
            .iter()
            .filter(|(_, holds)| !holds(state, entry))
            .map(|(id, _)| (*id).to_string())
            .collect()
    }
}

/// Whether the field holds a value: neither absent, `null` nor `""` — as
/// the migration's `coalesce(field, '') <> ''` judges it.
fn set(state: &BTreeMap<String, Value>, field: &str) -> bool {
    match state.get(field) {
        None | Some(Value::Null) => false,
        Some(Value::String(text)) => !text.is_empty(),
        Some(_) => true,
    }
}

/// Whether the row is the settings row, not a provider: its partition's id
/// is no uuid, whatever fields the command carries.
fn settings_row(entry: &Entry) -> bool {
    !uuid(entry.target_id())
}

/// Whether `id` is a uuid, as `ai_provider.id` is typed.
pub fn uuid(id: &str) -> bool {
    id.len() == 36
        && id.char_indices().all(|(at, c)| match at {
            8 | 13 | 18 | 23 => c == '-',
            _ => c.is_ascii_hexdigit(),
        })
}

fn text<'a>(state: &'a BTreeMap<String, Value>, field: &str) -> Option<&'a str> {
    state.get(field).and_then(Value::as_str)
}

/// Whether the settings row carries a provider, as `ai.saveSettings` writes
/// it: it names the provider's row — `null` when the save named no kind of
/// provider, which `single-settings-row` refuses.
fn carries_provider(state: &BTreeMap<String, Value>, entry: &Entry) -> bool {
    settings_row(entry) && state.contains_key("aiProviderId")
}

/// In « api » mode, url and detectedApiType are set and binary is empty; in
/// « binaire » mode, binary is opencode or claude-code and url is empty.
/// On the settings row carrying a provider, a missing type is a URL that was
/// not recognised: `api-type-recognized` refuses it, with its message.
/// A connection test is not judged here: it writes none of these fields
/// (`test-does-not-save`), on a provider's partition the engine may know by
/// its last tests alone.
fn mode_fields(state: &BTreeMap<String, Value>, entry: &Entry) -> bool {
    if state.is_empty()
        || testing(entry)
        || (settings_row(entry) && !carries_provider(state, entry))
    {
        return true;
    }
    match text(state, "mode") {
        Some("api") => {
            set(state, "url")
                && (set(state, "detectedApiType") || settings_row(entry))
                && !set(state, "binary")
        }
        Some("binaire") => {
            text(state, "binary").is_some_and(|binary| BINARIES.contains(&binary))
                && !set(state, "url")
        }
        _ => false,
    }
}

/// A detected API type is one ProviderApiType@1 names; the provider the
/// settings row carries in « api » mode has one.
fn api_type_recognized(state: &BTreeMap<String, Value>, entry: &Entry) -> bool {
    match state.get("detectedApiType") {
        None | Some(Value::Null) => {
            !(carries_provider(state, entry) && text(state, "mode") == Some("api"))
        }
        Some(Value::String(kind)) => API_TYPES.contains(kind),
        Some(_) => false,
    }
}

/// « Tester la connexion » writes the last_test_* fields, and nothing else.
fn test_does_not_save(_state: &BTreeMap<String, Value>, entry: &Entry) -> bool {
    !testing(entry)
        || entry
            .fields()
            .iter()
            .chain(entry.payload.keys())
            .all(|field| LAST_TEST.contains(&field.as_str()) || field == "aiProviderId")
}

/// Whether the command is « Tester la connexion ».
fn testing(entry: &Entry) -> bool {
    entry.data_capability.split('@').next() == Some(TEST_CONNECTION)
}

/// The settings row is the one of id 1, it is never deleted, it names a
/// provider, and it carries no provider's field but the active provider's;
/// a provider carries none of its fields.
fn single_settings_row(state: &BTreeMap<String, Value>, entry: &Entry) -> bool {
    if !settings_row(entry) {
        return !SETTINGS.iter().any(|field| state.contains_key(*field));
    }
    if entry.effect == Effect::Delete {
        return false;
    }
    let one = |id: &Value| id.as_i64() == Some(1) || id.as_str() == Some("1");
    entry.target_id() == "1"
        && state.get("aiSettingsId").is_none_or(one)
        && set(state, "activeProviderId")
        && (active_provider_carried(state)
            || !state
                .keys()
                .any(|field| DECLARED.contains(field) && !SETTINGS.contains(&field.as_str())))
}

/// Whether the provider the settings row carries is the one it makes active,
/// named by a uuid as `ai_provider.id` is typed, and whole enough to exist
/// there: both its models set, as `ai_provider` requires them.
fn active_provider_carried(state: &BTreeMap<String, Value>) -> bool {
    text(state, "aiProviderId").is_some_and(|id| {
        uuid(id) && state.get("activeProviderId").and_then(Value::as_str) == Some(id)
    }) && MODELS.iter().all(|field| set(state, field))
}

/// A command writes only fields the aggregate declares, each a text, a
/// number or a boolean: no audio, no blob, has anywhere to go.
fn text_only_payload(_state: &BTreeMap<String, Value>, entry: &Entry) -> bool {
    entry.payload.iter().all(|(field, value)| {
        DECLARED.contains(field) && !matches!(value, Value::Array(_) | Value::Object(_))
    })
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use harness_dataguard::api::Submission;
    use harness_dataguard::command::{CommandResult, State};
    use harness_dataguard::guard::Caller;
    use harness_dataguard::manifest::DataCapability;
    use harness_dataguard::memory::{FixedClock, MemoryNotifier, MemoryStore};
    use serde_json::json;

    use super::*;
    use crate::{Engine, bound};

    /// A provider written whole: what ai.saveSettings will do. Test-only:
    /// no DataCapability of the product is in this task.
    const WRITE_PROVIDER: &str = r#"{"dataCapability":"test.writeProvider","owner":"dataguard","version":1,"effect":"upsert","target":{"aggregate":"AiSettings","id":"$aiProviderId"},"mode":"overwrite","callableBy":["*"]}"#;
    /// The settings row written: what ai.saveSettings will do.
    const WRITE_SETTINGS: &str = r#"{"dataCapability":"test.writeSettings","owner":"dataguard","version":1,"effect":"upsert","target":{"aggregate":"AiSettings","id":"$aiSettingsId"},"mode":"overwrite","callableBy":["*"]}"#;
    /// The connection test, under the id the invariant knows it by.
    const TEST_CONNECTION_MANIFEST: &str = r#"{"dataCapability":"ai.testConnection","owner":"dataguard","version":1,"effect":"update","target":{"aggregate":"AiSettings","id":"$aiProviderId"},"mode":"overwrite","callableBy":["*"]}"#;
    /// A provider deleted.
    const DELETE_PROVIDER: &str = r#"{"dataCapability":"test.deleteProvider","owner":"dataguard","version":1,"effect":"delete","target":{"aggregate":"AiSettings","id":"$aiProviderId"},"mode":"overwrite","callableBy":["*"]}"#;
    /// The settings row deleted.
    const DELETE_SETTINGS: &str = r#"{"dataCapability":"test.deleteSettings","owner":"dataguard","version":1,"effect":"delete","target":{"aggregate":"AiSettings","id":"$aiSettingsId"},"mode":"overwrite","callableBy":["*"]}"#;

    const CLOUD: &str = "a1000000-0000-4000-8000-000000000001";

    /// The engine as `plant()` binds it, over a memory store, with the
    /// test-only DataCapabilities.
    async fn engine(store: MemoryStore) -> (Engine, Arc<MemoryStore>) {
        let store = Arc::new(store);
        let mut engine = bound(
            Engine::new(
                store.clone(),
                Arc::new(MemoryNotifier::default()),
                Arc::new(FixedClock::at(1000)),
            ),
            Arc::default(),
            Arc::default(),
        )
        .expect("the product's manifests parse");
        for manifest in [
            WRITE_PROVIDER,
            WRITE_SETTINGS,
            TEST_CONNECTION_MANIFEST,
            DELETE_PROVIDER,
            DELETE_SETTINGS,
        ] {
            engine =
                engine.with_data_capability(DataCapability::parse(manifest).expect("contract F"));
        }
        engine.load().await.expect("an empty queue");
        (engine, store)
    }

    /// What the single applier made of one command.
    async fn written(engine: &Engine, data_capability: &str, payload: Value) -> CommandResult {
        let Value::Object(payload) = payload else {
            panic!("a payload is an object");
        };
        engine
            .submit(Submission {
                data_capability: data_capability.to_string(),
                payload: payload.into_iter().collect(),
                target_id: None,
                based_on: None,
                idempotency_key: None,
                caller: Caller {
                    by: "dm".to_string(),
                    from: "ai.settings".to_string(),
                    permissions: Vec::new(),
                    // The critical fields' review is not what these tests judge.
                    roles: vec!["role:approver".to_string()],
                },
            })
            .await
            .expect("queued");
        let mut applied = engine.run_once().await.expect("the applier ran");
        assert_eq!(applied.len(), 1, "{applied:?}");
        applied.remove(0)
    }

    async fn provider(payload: Value) -> CommandResult {
        let (engine, _) = engine(MemoryStore::default()).await;
        written(&engine, "test.writeProvider", payload).await
    }

    fn rejected_by(result: &CommandResult, id: &str) {
        assert_eq!(result.status, State::Rejected, "{result:?}");
        assert_eq!(result.violations, [id], "{result:?}");
    }

    fn applied(result: &CommandResult) {
        assert_eq!(result.status, State::Applied, "{result:?}");
    }

    fn cloud() -> Value {
        json!({
            "aiProviderId": CLOUD,
            "mode": "api",
            "label": "API cloud",
            "url": "https://api.anthropic.com/v1",
            "detectedApiType": "cloud",
            "generationModel": "claude-sonnet-5-5",
            "summaryModel": "claude-haiku-4-5-20251001",
        })
    }

    fn binary(name: &str) -> Value {
        json!({
            "aiProviderId": "a1000000-0000-4000-8000-000000000004",
            "mode": "binaire",
            "label": "Binaire",
            "binary": name,
            "generationModel": "claude-sonnet-5-5",
            "summaryModel": "claude-haiku-4-5-20251001",
        })
    }

    fn with(mut base: Value, field: &str, value: Value) -> Value {
        base[field] = value;
        base
    }

    fn without(mut base: Value, field: &str) -> Value {
        base.as_object_mut().expect("an object").remove(field);
        base
    }

    #[tokio::test]
    async fn mode_fields_lets_a_whole_api_or_binary_provider_through() {
        applied(&provider(cloud()).await);
        applied(&provider(binary("opencode")).await);
        applied(&provider(binary("claude-code")).await);
    }

    #[tokio::test]
    async fn mode_fields_rejects_an_api_provider_without_url_or_type_or_with_a_binary() {
        for payload in [
            without(cloud(), "url"),
            with(cloud(), "url", json!("")),
            without(cloud(), "detectedApiType"),
            with(cloud(), "detectedApiType", Value::Null),
            with(cloud(), "binary", json!("opencode")),
        ] {
            rejected_by(&provider(payload).await, "mode-fields");
        }
    }

    #[tokio::test]
    async fn mode_fields_rejects_a_binary_provider_with_another_binary_or_a_url() {
        for payload in [
            binary("ollama"),
            binary(""),
            without(binary("opencode"), "binary"),
            with(binary("opencode"), "url", json!("http://192.168.1.40:4096")),
        ] {
            rejected_by(&provider(payload).await, "mode-fields");
        }
    }

    #[tokio::test]
    async fn mode_fields_rejects_a_mode_that_is_neither_api_nor_binaire() {
        rejected_by(
            &provider(with(cloud(), "mode", json!("binary"))).await,
            "mode-fields",
        );
        rejected_by(&provider(without(cloud(), "mode")).await, "mode-fields");
        // A provider with none of the mode's fields is no provider either.
        rejected_by(
            &provider(json!({ "aiProviderId": CLOUD, "lastTestOk": true })).await,
            "mode-fields",
        );
    }

    #[tokio::test]
    async fn mode_fields_judges_the_row_a_partial_write_leaves() {
        // The row is whole; the command blanks its url alone.
        let row = cloud().as_object().expect("an object").clone();
        let store = MemoryStore::default()
            .with_row(&format!("AiSettings/{CLOUD}"), row.into_iter().collect());
        let (engine, _) = engine(store).await;
        let result = written(
            &engine,
            "test.writeProvider",
            json!({ "aiProviderId": CLOUD, "url": "" }),
        )
        .await;
        rejected_by(&result, "mode-fields");
    }

    #[tokio::test]
    async fn mode_fields_judges_a_provider_that_carries_a_settings_field() {
        // The row's kind is its partition's, not its fields': updatedAt does
        // not make a provider the settings row.
        let result = provider(with(
            without(cloud(), "url"),
            "updatedAt",
            json!("2026-10-17T10:00:00Z"),
        ))
        .await;
        assert_eq!(result.status, State::Rejected, "{result:?}");
        assert_eq!(
            result.violations,
            ["mode-fields", "single-settings-row"],
            "{result:?}"
        );
    }

    #[tokio::test]
    async fn api_type_recognized_lets_every_type_of_the_concept_s_fixtures_through() {
        let fixtures: Value = serde_json::from_str(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../../concepts/ProviderApiType/v1/fixtures.json"
        )))
        .expect("ProviderApiType@1's fixtures");
        let recognised: Vec<&Value> = fixtures["cases"]
            .as_array()
            .expect("cases")
            .iter()
            .filter(|case| case["output"]["recognized"] == json!(true))
            .collect();
        assert!(!recognised.is_empty());
        for case in recognised {
            let payload = with(
                with(cloud(), "url", case["input"]["url"].clone()),
                "detectedApiType",
                case["output"]["type"].clone(),
            );
            applied(&provider(payload).await);
        }
    }

    #[tokio::test]
    async fn api_type_recognized_rejects_a_type_the_concept_does_not_name() {
        for kind in [json!("openai"), json!("Cloud"), json!(1)] {
            rejected_by(
                &provider(with(cloud(), "detectedApiType", kind)).await,
                "api-type-recognized",
            );
        }
        // As the migration's CHECK: "" is no type either.
        rejected_by(
            &provider(with(binary("opencode"), "detectedApiType", json!(""))).await,
            "api-type-recognized",
        );
    }

    #[tokio::test]
    async fn test_does_not_save_lets_the_last_test_fields_through_and_nothing_else_moves() {
        let row = cloud().as_object().expect("an object").clone();
        let partition = format!("AiSettings/{CLOUD}");
        let store = MemoryStore::default().with_row(&partition, row.clone().into_iter().collect());
        let (engine, store) = engine(store).await;
        let result = written(
            &engine,
            "ai.testConnection",
            json!({
                "aiProviderId": CLOUD,
                "lastTestOk": true,
                "lastTestAt": "2026-10-17T10:00:00Z",
                "lastTestMessage": "Connexion réussie",
                "lastTestLatencyMs": 840,
            }),
        )
        .await;
        applied(&result);
        let now = store.row_now(&partition);
        for field in [
            "mode",
            "url",
            "detectedApiType",
            "generationModel",
            "summaryModel",
        ] {
            assert_eq!(now.get(field), row.get(field), "{field} moved");
        }
        assert_eq!(now.get("lastTestOk"), Some(&json!(true)));
    }

    #[tokio::test]
    async fn mode_fields_leaves_a_connection_test_on_a_provider_known_by_its_tests_alone() {
        // The engine holds no row for the provider: ai.saveSettings writes
        // the settings row, the seed is the table's.
        let (engine, store) = engine(MemoryStore::default()).await;
        let result = written(
            &engine,
            "ai.testConnection",
            json!({
                "aiProviderId": CLOUD,
                "lastTestOk": false,
                "lastTestAt": "2026-10-17T10:00:00Z",
                "lastTestMessage": "délai dépassé",
                "lastTestLatencyMs": 0,
            }),
        )
        .await;
        applied(&result);
        let now = store.row_now(&format!("AiSettings/{CLOUD}"));
        assert_eq!(now.get("mode"), None);
        assert_eq!(now.get("lastTestOk"), Some(&json!(false)));
        // Any other command writing such a row is still judged.
        let result = written(
            &engine,
            "test.writeProvider",
            json!({ "aiProviderId": CLOUD, "lastTestOk": true }),
        )
        .await;
        rejected_by(&result, "mode-fields");
    }

    #[tokio::test]
    async fn test_does_not_save_rejects_a_connection_test_that_writes_the_url_models_or_mode() {
        for (field, value) in [
            ("url", json!("http://192.168.1.40:4096")),
            ("generationModel", json!("qwen2.5-32b-instruct")),
            ("mode", json!("api")),
        ] {
            let row = cloud().as_object().expect("an object").clone();
            let store = MemoryStore::default()
                .with_row(&format!("AiSettings/{CLOUD}"), row.into_iter().collect());
            let (engine, _) = engine(store).await;
            let payload = json!({ "aiProviderId": CLOUD, "lastTestOk": false, field: value });
            rejected_by(
                &written(&engine, "ai.testConnection", payload).await,
                "test-does-not-save",
            );
        }
    }

    #[tokio::test]
    async fn single_settings_row_lets_row_1_naming_a_provider_through() {
        // The id as a number or as the text a partition is named by.
        for id in [json!(1), json!("1")] {
            let (engine, _) = engine(MemoryStore::default()).await;
            let result = written(
                &engine,
                "test.writeSettings",
                json!({ "aiSettingsId": id, "activeProviderId": CLOUD }),
            )
            .await;
            applied(&result);
        }
    }

    #[tokio::test]
    async fn single_settings_row_rejects_a_second_row_or_a_row_naming_no_provider() {
        for payload in [
            json!({ "aiSettingsId": 2, "activeProviderId": CLOUD }),
            json!({ "aiSettingsId": "2", "activeProviderId": CLOUD }),
            json!({ "aiSettingsId": 1, "activeProviderId": null }),
            json!({ "aiSettingsId": 1, "activeProviderId": "" }),
            json!({ "aiSettingsId": 1 }),
            json!({ "aiSettingsId": 1, "updatedAt": "2026-10-17T10:00:00Z" }),
            // The settings row mixed with a provider's fields.
            json!({ "aiSettingsId": 1, "activeProviderId": CLOUD, "mode": "api" }),
        ] {
            let (engine, _) = engine(MemoryStore::default()).await;
            rejected_by(
                &written(&engine, "test.writeSettings", payload).await,
                "single-settings-row",
            );
        }
    }

    #[tokio::test]
    async fn single_settings_row_rejects_a_provider_carrying_a_settings_field() {
        for (field, value) in [
            ("updatedAt", json!("2026-10-17T10:00:00Z")),
            ("activeProviderId", json!(CLOUD)),
            ("aiSettingsId", json!(1)),
        ] {
            rejected_by(
                &provider(with(cloud(), field, value)).await,
                "single-settings-row",
            );
        }
    }

    #[tokio::test]
    async fn single_settings_row_rejects_deleting_the_settings_row() {
        let row = json!({ "aiSettingsId": 1, "activeProviderId": CLOUD });
        let row = row.as_object().expect("an object").clone();
        let store = MemoryStore::default().with_row("AiSettings/1", row.into_iter().collect());
        let (engine, store) = engine(store).await;
        let result = written(&engine, "test.deleteSettings", json!({ "aiSettingsId": 1 })).await;
        rejected_by(&result, "single-settings-row");
        assert!(!store.is_tombstoned("AiSettings/1"));
    }

    #[tokio::test]
    async fn the_active_provider_is_not_deleted() {
        let partition = format!("AiSettings/{CLOUD}");
        let row = cloud().as_object().expect("an object").clone();
        // The settings row points at the provider: the relation restricts.
        let store = MemoryStore::default()
            .with_row(&partition, row.into_iter().collect())
            .pointed_at(
                &partition,
                vec![("AiSettings.activeProviderId".to_string(), 1)],
            );
        let (engine, store) = engine(store).await;
        let result = written(
            &engine,
            "test.deleteProvider",
            json!({ "aiProviderId": CLOUD }),
        )
        .await;
        assert_eq!(result.status, State::Queued, "{result:?}");
        assert!(!store.is_tombstoned(&partition));
    }

    #[tokio::test]
    async fn a_provider_nothing_points_at_is_soft_deleted_with_a_tombstone() {
        let partition = format!("AiSettings/{CLOUD}");
        let row = cloud().as_object().expect("an object").clone();
        let store = MemoryStore::default().with_row(&partition, row.into_iter().collect());
        let (engine, store) = engine(store).await;
        let result = written(
            &engine,
            "test.deleteProvider",
            json!({ "aiProviderId": CLOUD }),
        )
        .await;
        applied(&result);
        assert!(store.is_tombstoned(&partition));
    }

    #[tokio::test]
    async fn text_only_payload_rejects_audio_or_anything_but_text() {
        for payload in [
            with(cloud(), "audio", json!("UklGRiQAAABXQVZF")),
            with(
                cloud(),
                "generationModel",
                json!({ "bytes": [82, 73, 70, 70] }),
            ),
            with(
                cloud(),
                "summaryModel",
                json!(["claude-haiku-4-5-20251001"]),
            ),
        ] {
            rejected_by(&provider(payload).await, "text-only-payload");
        }
    }

    #[test]
    fn text_only_payload_has_no_audio_field_to_store() {
        let model: Value = serde_json::from_str(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../../data/model.json"
        )))
        .expect("data/model.json");
        let tables: Vec<&Value> = model["tables"]
            .as_array()
            .expect("tables")
            .iter()
            .filter(|t| t["name"] == "ai_provider" || t["name"] == "ai_settings")
            .collect();
        assert_eq!(tables.len(), 2, "data/model.json holds both tables");
        let text_types = [
            "uuid",
            "text",
            "boolean",
            "timestamptz",
            "integer",
            "smallint",
        ];
        for field in tables
            .iter()
            .flat_map(|t| t["fields"].as_array().expect("fields"))
        {
            let name = field["name"].as_str().expect("a name");
            assert!(!name.contains("audio"), "{name}");
            assert!(
                text_types.contains(&field["type"].as_str().expect("a type")),
                "{field}"
            );
        }
        assert!(
            DECLARED
                .iter()
                .all(|field| !field.to_lowercase().contains("audio"))
        );
    }

    /// The settings row carrying `provider` as the active one: what
    /// ai.saveSettings writes.
    fn saved(provider: Value) -> Value {
        let id = provider["aiProviderId"].clone();
        with(
            with(provider, "aiSettingsId", json!(1)),
            "activeProviderId",
            id,
        )
    }

    async fn settings(payload: Value) -> CommandResult {
        let (engine, _) = engine(MemoryStore::default()).await;
        written(&engine, "test.writeSettings", payload).await
    }

    #[tokio::test]
    async fn the_settings_row_carrying_its_active_provider_whole_is_applied() {
        applied(&settings(saved(cloud())).await);
        applied(&settings(saved(binary("claude-code"))).await);
        applied(
            &settings(with(
                saved(cloud()),
                "updatedAt",
                json!("2026-10-17T10:00:00Z"),
            ))
            .await,
        );
    }

    #[tokio::test]
    async fn api_type_recognized_rejects_the_settings_row_s_api_provider_without_a_type() {
        // An unrecognised URL: its refusal is this invariant's alone.
        for kind in [Value::Null, json!("openai")] {
            rejected_by(
                &settings(saved(with(cloud(), "detectedApiType", kind))).await,
                "api-type-recognized",
            );
        }
        rejected_by(
            &settings(saved(without(cloud(), "detectedApiType"))).await,
            "api-type-recognized",
        );
    }

    #[tokio::test]
    async fn mode_fields_judges_the_provider_the_settings_row_carries() {
        for payload in [
            saved(without(cloud(), "url")),
            saved(with(cloud(), "binary", json!("opencode"))),
            saved(binary("ollama")),
            saved(with(
                binary("opencode"),
                "url",
                json!("http://192.168.1.40:4096"),
            )),
            saved(with(cloud(), "mode", json!("binary"))),
        ] {
            rejected_by(&settings(payload).await, "mode-fields");
        }
    }

    #[tokio::test]
    async fn mode_fields_rejects_a_save_that_names_no_kind_of_provider() {
        // What ai.saveSettings makes of `binary: ollama`: no provider's row.
        let payload = with(
            with(saved(binary("ollama")), "aiProviderId", Value::Null),
            "activeProviderId",
            Value::Null,
        );
        let result = settings(payload).await;
        assert_eq!(result.status, State::Rejected, "{result:?}");
        assert_eq!(
            result.violations,
            ["mode-fields", "single-settings-row"],
            "{result:?}"
        );
    }

    #[tokio::test]
    async fn single_settings_row_rejects_a_carried_provider_that_is_not_the_active_one() {
        let other = "a1000000-0000-4000-8000-000000000002";
        for payload in [
            with(saved(cloud()), "activeProviderId", json!(other)),
            saved(with(cloud(), "aiProviderId", json!("cloud"))),
        ] {
            rejected_by(&settings(payload).await, "single-settings-row");
        }
    }

    #[tokio::test]
    async fn single_settings_row_rejects_a_carried_provider_without_its_models() {
        // ai_provider requires both models: such a provider would not exist.
        for field in ["generationModel", "summaryModel"] {
            for payload in [
                saved(without(cloud(), field)),
                saved(with(cloud(), field, Value::Null)),
                saved(with(binary("opencode"), field, json!(""))),
            ] {
                rejected_by(&settings(payload).await, "single-settings-row");
            }
        }
    }

    #[tokio::test]
    async fn the_settings_row_s_provider_is_judged_on_the_row_the_save_leaves() {
        // Row 1 holds the cloud provider; a save leaving its type null is
        // refused, and the row stays as it was.
        let row = saved(cloud()).as_object().expect("an object").clone();
        let store =
            MemoryStore::default().with_row("AiSettings/1", row.clone().into_iter().collect());
        let (engine, store) = engine(store).await;
        let result = written(
            &engine,
            "test.writeSettings",
            json!({ "aiSettingsId": 1, "url": "https://inconnu.example.org", "detectedApiType": null }),
        )
        .await;
        rejected_by(&result, "api-type-recognized");
        assert_eq!(store.row_now("AiSettings/1").get("url"), row.get("url"));
    }

    #[test]
    fn the_api_types_are_the_concept_s() {
        assert_eq!(*API_TYPES, ["cloud", "opencode"]);
    }
}
