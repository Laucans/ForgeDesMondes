//! The Capability on the llm port's fake: an empty journal, both modes in
//! order, and an answer that carries text and nothing else.

use list_provider_requests::list_provider_requests;
use ports_llm::fake::FakeLlm;
use ports_llm::{Binary, GenerateRequest, Llm, Models, Provider, ProviderConfig, Secret, TextKind};
use serde_json::{Value, json};

const CLOUD_URL: &str = "https://api.anthropic.com/v1";
const KEY: &str = "sk-ant-never-listed";

fn models() -> Models {
    Models {
        generation: "claude-sonnet-5-5".into(),
        summary: "claude-haiku-4-5-20251001".into(),
    }
}

fn api() -> ProviderConfig {
    ProviderConfig::Api {
        url: CLOUD_URL.into(),
        api_type: None,
        key: Some(Secret::new(KEY)),
        models: models(),
    }
}

fn binary() -> ProviderConfig {
    ProviderConfig::Binary {
        binary: Binary::ClaudeCode,
        models: models(),
    }
}

fn answer(llm: &FakeLlm) -> Value {
    serde_json::to_value(list_provider_requests(llm)).expect("the answer serializes")
}

#[test]
fn no_request_sent_lists_an_empty_array() {
    assert_eq!(answer(&FakeLlm::new()), json!({ "requests": [] }));
}

#[tokio::test]
async fn a_test_in_api_mode_then_one_in_binary_mode_are_listed_in_order() {
    let llm = FakeLlm::new();
    assert!(llm.test_connection(&api()).await.ok);
    assert!(llm.test_connection(&binary()).await.ok);

    let requests = answer(&llm)["requests"].as_array().cloned().unwrap();
    assert_eq!(requests.len(), 2);
    assert_eq!(
        (
            requests[0]["sequence"].clone(),
            requests[1]["sequence"].clone()
        ),
        (json!(1), json!(2))
    );
    assert_eq!(requests[0]["kind"], "connection-test");
    assert_eq!(requests[1]["kind"], "connection-test");
    assert_eq!(requests[0]["provider"], "cloud");
    assert_eq!(requests[1]["provider"], "claude-code-binary");
    assert_eq!(
        requests[0]["text"],
        llm.journal().list()[0].text,
        "the text listed is the text journalled"
    );
}

#[tokio::test]
async fn a_generation_is_listed_with_its_kind_and_its_text() {
    let llm = FakeLlm::new();
    let request = GenerateRequest::new(TextKind::Brief, "Elowen entre dans la taverne.");
    llm.generate(&binary(), &request).await.expect("generates");

    let listed = answer(&llm);
    assert_eq!(listed["requests"][0]["kind"], "brief");
    assert_eq!(
        listed["requests"][0]["text"],
        "Elowen entre dans la taverne."
    );
    assert_eq!(
        listed["requests"][0]["provider"],
        serde_json::to_value(Provider::ClaudeCodeBinary).unwrap()
    );
}

#[tokio::test]
async fn every_request_is_text_and_carries_no_key_audio_or_binary_field() {
    let llm = FakeLlm::new();
    llm.test_connection(&api()).await;
    llm.generate(&api(), &GenerateRequest::new(TextKind::Notes, "des notes"))
        .await
        .expect("generates");

    let listed = answer(&llm);
    assert!(!listed.to_string().contains(KEY), "the key was listed");
    let requests = listed["requests"].as_array().unwrap();
    assert_eq!(requests.len(), 2);
    for request in requests {
        assert_eq!(request["modalities"], json!(["text"]));
        let mut fields: Vec<&str> = request
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        fields.sort_unstable();
        assert_eq!(
            fields,
            ["kind", "modalities", "provider", "sequence", "text"]
        );
    }
}
