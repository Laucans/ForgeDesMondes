//! `prepare`: the configuration on screen tested through the llm port's
//! fake, and only the outcome kept.

use std::collections::BTreeMap;
use std::sync::Mutex;

use ports_llm::fake::{FAKE_LATENCY_MS, FakeLlm};
use ports_llm::{
    ConnectionReport, GenerateAnswer, GenerateRequest, Llm, LlmError, ProviderConfig,
    RequestJournal, UNRECOGNISED_API_TYPE,
};
use serde_json::{Value, json};
use test_connection::{NO_PROVIDER, Prepared, names, prepare};

const HOME: &str = "http://192.168.1.40:4096";
const NOW: &str = "2026-10-17T10:00:00Z";
const LAST_TEST: [&str; 4] = [
    "lastTestAt",
    "lastTestLatencyMs",
    "lastTestMessage",
    "lastTestOk",
];

/// The fake whose home server answers its model list at `HOME`.
fn llm() -> FakeLlm {
    FakeLlm::new().with_endpoint(HOME, json!({ "object": "list", "data": [] }))
}

async fn prepared<L: Llm>(llm: &L, wire: Value) -> Prepared {
    let Value::Object(wire) = wire else {
        panic!("a payload is an object");
    };
    let wire: BTreeMap<String, Value> = wire.into_iter().collect();
    prepare(llm, &wire, NOW).await
}

fn api(url: &str, key: &str) -> Value {
    json!({
        "mode": "api",
        "url": url,
        "generationModel": "qwen2.5-32b-instruct",
        "summaryModel": "qwen2.5-14b-instruct",
        "key": key
    })
}

/// Whether `prepared` is a success on `row`, with the fake's latency.
fn succeeded_on(prepared: &Prepared, row: &str) {
    assert_eq!(prepared.target_id, row, "{prepared:?}");
    let payload = &prepared.payload;
    assert_eq!(
        payload.keys().map(String::as_str).collect::<Vec<_>>(),
        LAST_TEST
    );
    assert_eq!(payload["lastTestOk"], true);
    assert_eq!(payload["lastTestAt"], NOW);
    assert_eq!(payload["lastTestLatencyMs"], FAKE_LATENCY_MS);
    assert!(
        payload["lastTestMessage"]
            .as_str()
            .is_some_and(|m| m.starts_with("Connexion réussie")),
        "{payload:?}"
    );
}

/// Whether `prepared` is a refusal on no provider, saying the type is not
/// recognised.
fn refused(prepared: &Prepared) {
    assert_eq!(prepared.target_id, NO_PROVIDER, "{prepared:?}");
    assert_eq!(prepared.payload["lastTestOk"], false);
    assert!(
        prepared.payload["lastTestMessage"]
            .as_str()
            .is_some_and(|m| m.contains(UNRECOGNISED_API_TYPE)),
        "{prepared:?}"
    );
}

#[tokio::test]
async fn the_cloud_url_is_tested_on_the_cloud_row() {
    let key = "sk-ant-api03-secret";
    let prepared = prepared(&llm(), api("https://api.anthropic.com/v1", key)).await;
    succeeded_on(&prepared, "a1000000-0000-4000-8000-000000000001");
    assert!(!format!("{prepared:?}").contains(key));
}

#[tokio::test]
async fn the_home_server_s_url_is_tested_on_the_opencode_row() {
    succeeded_on(
        &prepared(&llm(), api(HOME, "")).await,
        "a1000000-0000-4000-8000-000000000002",
    );
}

#[tokio::test]
async fn each_binary_is_tested_on_its_row() {
    for (binary, row) in [
        ("opencode", "a1000000-0000-4000-8000-000000000003"),
        ("claude", "a1000000-0000-4000-8000-000000000004"),
        ("claude-code", "a1000000-0000-4000-8000-000000000004"),
    ] {
        for mode in ["binary", "binaire"] {
            let wire = json!({ "mode": mode, "binary": binary, "url": HOME });
            succeeded_on(&prepared(&llm(), wire).await, row);
        }
    }
}

#[tokio::test]
async fn an_unrecognised_url_is_refused_on_no_provider_with_no_latency() {
    let prepared = prepared(&llm(), api("https://inconnu.example.org/pas-une-api", "")).await;
    refused(&prepared);
    assert_eq!(prepared.payload["lastTestLatencyMs"], Value::Null);
}

#[tokio::test]
async fn what_cannot_be_tested_is_refused_and_sends_nothing() {
    let llm = llm();
    for wire in [
        api("", ""),
        json!({ "url": HOME }),
        json!({ "mode": "binary", "binary": "ollama" }),
        json!({ "mode": "binary" }),
    ] {
        let prepared = prepared(&llm, wire).await;
        refused(&prepared);
        assert_eq!(prepared.payload["lastTestLatencyMs"], Value::Null);
    }
    assert!(llm.journal().list().is_empty());
}

/// A port that reached no provider and says why in its own words; it keeps
/// the configurations it was asked to test.
#[derive(Default)]
struct Unreachable {
    journal: RequestJournal,
    asked: Mutex<Vec<ProviderConfig>>,
}

impl Llm for Unreachable {
    async fn test_connection(&self, config: &ProviderConfig) -> ConnectionReport {
        self.asked
            .lock()
            .expect("not poisoned")
            .push(config.clone());
        ConnectionReport {
            ok: false,
            provider: None,
            latency_ms: 5000,
            message: "délai dépassé".to_owned(),
        }
    }

    async fn generate(
        &self,
        _: &ProviderConfig,
        _: &GenerateRequest,
    ) -> Result<GenerateAnswer, LlmError> {
        Err(LlmError::Failed("not asked".to_owned()))
    }

    fn journal(&self) -> &RequestJournal {
        &self.journal
    }
}

#[tokio::test]
async fn a_test_that_reached_no_provider_says_the_type_is_not_recognised_and_why() {
    let prepared = prepared(&Unreachable::default(), api(HOME, "")).await;
    refused(&prepared);
    assert_eq!(
        prepared.payload["lastTestMessage"],
        format!("{UNRECOGNISED_API_TYPE} : délai dépassé")
    );
    assert_eq!(prepared.payload["lastTestLatencyMs"], 5000);
}

#[test]
fn the_id_is_named_with_or_without_its_version() {
    assert!(names("ai.testConnection"));
    assert!(names("ai.testConnection@1"));
    assert!(!names("ai.saveSettings@1"));
    assert!(!names("ai.testConnectionAgain"));
}

#[tokio::test]
async fn the_key_on_screen_is_the_port_s_for_the_test_and_no_key_is_none() {
    let llm = Unreachable::default();
    prepared(&llm, api(HOME, "sk-test-only")).await;
    prepared(&llm, api(HOME, "")).await;
    let keys: Vec<Option<String>> = llm
        .asked
        .lock()
        .expect("not poisoned")
        .iter()
        .map(|config| match config {
            ProviderConfig::Api { key, .. } => key.as_ref().map(|k| k.expose().to_owned()),
            ProviderConfig::Binary { .. } => panic!("an api test"),
        })
        .collect();
    assert_eq!(keys, [Some("sk-test-only".to_owned()), None]);
}
