//! What the fake promises beyond the contract: it is deterministic, it sends
//! only text, and its journal is the one a Capability lists.

use ports_llm::fake::{FAKE_LATENCY_MS, FakeLlm};
use ports_llm::{
    ApiType, Binary, GenerateRequest, Llm, LlmError, Models, Provider, ProviderConfig, RequestKind,
    TextKind,
};
use serde_json::json;

const OPENCODE_URL: &str = "http://192.168.1.40:4096";

fn models() -> Models {
    Models {
        generation: "qwen2.5-32b-instruct".into(),
        summary: "qwen2.5-14b-instruct".into(),
    }
}

fn api(url: &str, api_type: Option<ApiType>) -> ProviderConfig {
    ProviderConfig::Api {
        url: url.into(),
        api_type,
        key: None,
        models: models(),
    }
}

fn home_server() -> FakeLlm {
    FakeLlm::new().with_endpoint(OPENCODE_URL, json!({"object": "list", "data": []}))
}

#[tokio::test]
async fn the_cloud_url_connects_with_a_latency_and_both_models() {
    let report = FakeLlm::new()
        .test_connection(&api("https://api.anthropic.com/v1", None))
        .await;
    assert!(report.ok);
    assert_eq!(
        report.provider.and_then(Provider::api_type),
        Some(ApiType::Cloud)
    );
    assert_eq!(report.latency_ms, FAKE_LATENCY_MS);
    assert!(
        report.message.contains("qwen2.5-32b-instruct"),
        "{}",
        report.message
    );
    assert!(
        report.message.contains("qwen2.5-14b-instruct"),
        "{}",
        report.message
    );
}

#[tokio::test]
async fn the_opencode_url_connects_once_its_endpoint_answers_the_list() {
    let report = home_server()
        .test_connection(&api(OPENCODE_URL, None))
        .await;
    assert!(report.ok);
    assert_eq!(report.provider, Some(Provider::OpencodeServer));
    assert!(
        report
            .message
            .contains("endpoint opencode du serveur maison"),
        "{}",
        report.message
    );
}

#[tokio::test]
async fn an_unrecognised_url_fails_with_the_concepts_message() {
    for url in [OPENCODE_URL, "https://inconnu.example.org/pas-une-api", ""] {
        let report = FakeLlm::new().test_connection(&api(url, None)).await;
        assert!(!report.ok, "{url:?}");
        assert!(
            report.message.contains("type d'API non reconnu"),
            "{}",
            report.message
        );
        assert_eq!(report.provider, None);
    }
}

#[tokio::test]
async fn each_binary_connects() {
    for (binary, provider, label) in [
        (
            Binary::Opencode,
            Provider::OpencodeBinary,
            "binaire opencode",
        ),
        (
            Binary::ClaudeCode,
            Provider::ClaudeCodeBinary,
            "binaire Claude Code",
        ),
    ] {
        let config = ProviderConfig::Binary {
            binary,
            models: models(),
        };
        let report = FakeLlm::new().test_connection(&config).await;
        assert!(report.ok);
        assert_eq!(report.provider, Some(provider));
        assert!(report.message.contains(label), "{}", report.message);
    }
}

#[tokio::test]
async fn the_same_request_gets_the_same_answer() {
    let fake = FakeLlm::new();
    let config = api("https://api.anthropic.com", None);
    let request = GenerateRequest::new(TextKind::Notes, "Les notes");
    let first = fake.generate(&config, &request).await;
    let second = fake.generate(&config, &request).await;
    assert_eq!(first, second);
    let answer = first.unwrap();
    assert_eq!(answer.model, "qwen2.5-32b-instruct");
    assert!(answer.text.contains("9 caractères"), "{}", answer.text);
}

#[tokio::test]
async fn the_summary_model_answers_the_journal() {
    let answer = FakeLlm::new()
        .generate(
            &api("https://api.anthropic.com", None),
            &GenerateRequest::new(TextKind::Journal, "00:01 …"),
        )
        .await
        .unwrap();
    assert_eq!(answer.model, "qwen2.5-14b-instruct");
}

#[tokio::test]
async fn a_saved_type_spares_the_endpoint_for_generation_but_not_for_the_test() {
    let fake = FakeLlm::new();
    let config = api(OPENCODE_URL, Some(ApiType::Opencode));
    let request = GenerateRequest::new(TextKind::Prompt, "bonjour");
    assert!(fake.generate(&config, &request).await.is_ok());
    assert!(!fake.test_connection(&config).await.ok);
}

#[tokio::test]
async fn generation_through_an_unrecognised_url_fails_and_journals_nothing() {
    let fake = FakeLlm::new();
    let request = GenerateRequest::new(TextKind::Prompt, "bonjour");
    assert_eq!(
        fake.generate(&api(OPENCODE_URL, None), &request).await,
        Err(LlmError::UnrecognisedApiType)
    );
    assert!(fake.journal().list().is_empty());
}

#[tokio::test]
async fn the_journal_lists_the_requests_in_order_with_kind_and_text() {
    let fake = FakeLlm::new();
    let config = api("https://api.anthropic.com", None);
    fake.test_connection(&config).await;
    for (kind, text) in [
        (TextKind::Brief, "un brief"),
        (TextKind::Notes, "des notes"),
    ] {
        fake.generate(&config, &GenerateRequest::new(kind, text))
            .await
            .unwrap();
    }
    let listed: Vec<_> = fake
        .journal()
        .list()
        .into_iter()
        .map(|record| (record.sequence, record.kind, record.text))
        .collect();
    assert_eq!(
        listed,
        [
            (
                1,
                RequestKind::ConnectionTest,
                ports_llm::CONNECTION_TEST_PROMPT.to_owned()
            ),
            (2, RequestKind::Brief, "un brief".to_owned()),
            (3, RequestKind::Notes, "des notes".to_owned()),
        ]
    );
}

#[tokio::test]
async fn the_journal_a_capability_holds_is_the_one_the_port_writes() {
    let fake = FakeLlm::new();
    let listing = fake.journal().clone();
    let config = ProviderConfig::Binary {
        binary: Binary::ClaudeCode,
        models: models(),
    };
    fake.generate(&config, &GenerateRequest::new(TextKind::Prompt, "bonjour"))
        .await
        .unwrap();
    assert_eq!(listing.list().len(), 1);
    let json = serde_json::to_value(listing.list()).unwrap();
    assert_eq!(
        json,
        json!([{"sequence": 1, "kind": "prompt", "provider": "claude-code-binary", "text": "bonjour"}])
    );
}
