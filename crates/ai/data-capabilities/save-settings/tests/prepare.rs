//! `prepare`: the wire payload of a save made the state written on
//! `AiSettings/1`, with the llm port's fake.

use std::collections::BTreeMap;

use ports_llm::fake::FakeLlm;
use ports_llm::{Llm, UNRECOGNISED_API_TYPE};
use save_settings::{MISSING_MODELS, Prepared, names, prepare, timestamp};
use serde_json::{Value, json};

const HOME: &str = "http://192.168.1.40:4096";
const NOW: &str = "2026-10-17T10:00:00Z";

/// The fake whose home server answers its model list at `HOME`.
fn llm() -> FakeLlm {
    FakeLlm::new().with_endpoint(HOME, json!({ "object": "list", "data": [] }))
}

async fn prepared(llm: &FakeLlm, wire: Value) -> Prepared {
    let Value::Object(wire) = wire else {
        panic!("a payload is an object");
    };
    let wire: BTreeMap<String, Value> = wire.into_iter().collect();
    prepare(llm, &wire, NOW).await
}

fn api(url: &str) -> Value {
    json!({
        "mode": "api",
        "url": url,
        "generationModel": "qwen2.5-32b-instruct",
        "summaryModel": "qwen2.5-14b-instruct",
        "key": ""
    })
}

fn binary(name: &str) -> Value {
    json!({
        "mode": "binary",
        "binary": name,
        "generationModel": "claude-sonnet-5-5",
        "summaryModel": "claude-haiku-4-5-20251001"
    })
}

#[tokio::test]
async fn the_home_server_s_url_is_the_opencode_provider_made_active() {
    let llm = llm();
    let Prepared { payload, refusal } = prepared(&llm, api(HOME)).await;
    assert_eq!(refusal, None);
    let opencode = json!("a1000000-0000-4000-8000-000000000002");
    let expected: BTreeMap<String, Value> = [
        ("aiProviderId", opencode.clone()),
        ("activeProviderId", opencode),
        ("mode", json!("api")),
        ("label", json!("Endpoint opencode du serveur maison")),
        ("url", json!(HOME)),
        ("detectedApiType", json!("opencode")),
        ("apiKey", Value::Null),
        ("binary", Value::Null),
        ("generationModel", json!("qwen2.5-32b-instruct")),
        ("summaryModel", json!("qwen2.5-14b-instruct")),
        ("updatedAt", json!(NOW)),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v))
    .collect();
    assert_eq!(payload, expected);
}

#[tokio::test]
async fn the_cloud_host_is_told_by_its_url_alone_and_nothing_is_sent() {
    let llm = FakeLlm::new();
    let Prepared { payload, refusal } = prepared(
        &llm,
        json!({
            "mode": "api",
            "url": "https://api.anthropic.com/v1",
            "generationModel": "claude-sonnet-5-5",
            "summaryModel": "claude-haiku-4-5-20251001",
            "key": "sk-ant-secret"
        }),
    )
    .await;
    assert_eq!(refusal, None);
    assert_eq!(payload["detectedApiType"], "cloud");
    assert_eq!(
        payload["aiProviderId"],
        "a1000000-0000-4000-8000-000000000001"
    );
    assert_eq!(payload["label"], "API cloud");
    assert_eq!(payload["apiKey"], "sk-ant-secret");
    assert!(
        llm.journal().list().is_empty(),
        "no connection test for the cloud host"
    );
}

#[tokio::test]
async fn another_host_is_asked_through_the_port_without_the_key_in_its_journal() {
    let llm = llm();
    let mut wire = api(HOME);
    wire["key"] = json!("sk-home-secret");
    let Prepared { payload, .. } = prepared(&llm, wire).await;
    assert_eq!(payload["apiKey"], "sk-home-secret");
    let journal = llm.journal().list();
    assert_eq!(journal.len(), 1, "{journal:?}");
    assert!(!format!("{journal:?}").contains("sk-home-secret"));
}

#[tokio::test]
async fn an_unrecognised_url_has_no_type_and_says_why() {
    let llm = llm();
    let Prepared { payload, refusal } =
        prepared(&llm, api("https://inconnu.example.org/pas-une-api")).await;
    assert_eq!(refusal, Some(UNRECOGNISED_API_TYPE));
    assert_eq!(payload["detectedApiType"], Value::Null);
    assert_eq!(payload["url"], "https://inconnu.example.org/pas-une-api");
    // The home server's candidate: the invariants refuse it for its type alone.
    assert_eq!(payload["aiProviderId"], payload["activeProviderId"]);
    assert_eq!(
        payload["aiProviderId"],
        "a1000000-0000-4000-8000-000000000002"
    );
}

#[tokio::test]
async fn an_empty_url_is_not_asked_and_not_recognised() {
    let llm = llm();
    let Prepared { payload, refusal } = prepared(&llm, api("")).await;
    assert_eq!(refusal, Some(UNRECOGNISED_API_TYPE));
    assert_eq!(payload["detectedApiType"], Value::Null);
    assert!(llm.journal().list().is_empty());
}

#[tokio::test]
async fn a_type_or_a_row_the_caller_gives_is_dropped() {
    let llm = llm();
    let mut wire = api(HOME);
    for (field, value) in [
        ("apiType", json!("cloud")),
        ("detectedApiType", json!("cloud")),
        (
            "aiProviderId",
            json!("a1000000-0000-4000-8000-000000000001"),
        ),
        (
            "activeProviderId",
            json!("a1000000-0000-4000-8000-000000000001"),
        ),
        ("lastTestOk", json!(true)),
    ] {
        wire[field] = value;
    }
    let Prepared { payload, .. } = prepared(&llm, wire).await;
    assert_eq!(payload["detectedApiType"], "opencode");
    assert_eq!(
        payload["aiProviderId"],
        "a1000000-0000-4000-8000-000000000002"
    );
    assert!(!payload.contains_key("apiType"));
    assert!(!payload.contains_key("lastTestOk"));
    assert!(!payload.contains_key("key"));
}

#[tokio::test]
async fn binary_claude_is_stored_claude_code_with_no_url() {
    let llm = llm();
    for (wire, stored, row, label) in [
        (
            "claude",
            "claude-code",
            "a1000000-0000-4000-8000-000000000004",
            "Binaire Claude Code",
        ),
        (
            "opencode",
            "opencode",
            "a1000000-0000-4000-8000-000000000003",
            "Binaire opencode",
        ),
    ] {
        let mut given = binary(wire);
        // What a binary save carries of api mode is not kept.
        given["url"] = json!(HOME);
        given["key"] = json!("sk-oubliée");
        let Prepared { payload, refusal } = prepared(&llm, given).await;
        assert_eq!(refusal, None);
        assert_eq!(payload["mode"], "binaire", "{wire}");
        assert_eq!(payload["binary"], stored, "{wire}");
        assert_eq!(payload["aiProviderId"], row, "{wire}");
        assert_eq!(payload["activeProviderId"], row, "{wire}");
        assert_eq!(payload["label"], label, "{wire}");
        for field in ["url", "detectedApiType", "apiKey"] {
            assert_eq!(payload[field], Value::Null, "{wire}: {field}");
        }
    }
    assert!(llm.journal().list().is_empty(), "a binary is not asked");
}

#[tokio::test]
async fn another_binary_or_mode_is_kept_for_the_invariants_to_refuse() {
    let llm = llm();
    let Prepared { payload, .. } = prepared(&llm, binary("ollama")).await;
    assert_eq!(payload["binary"], "ollama");
    assert_eq!(payload["aiProviderId"], Value::Null);
    let Prepared { payload, .. } = prepared(&llm, json!({ "mode": "local" })).await;
    assert_eq!(payload["mode"], "local");
    assert_eq!(payload["aiProviderId"], Value::Null);
}

#[tokio::test]
async fn a_save_missing_a_model_says_why() {
    let llm = llm();
    for field in ["generationModel", "summaryModel"] {
        for given in [api(HOME), binary("claude")] {
            let mut absent = given.clone();
            absent.as_object_mut().expect("an object").remove(field);
            let mut empty = given;
            empty[field] = json!("");
            for wire in [absent, empty] {
                let Prepared { payload, refusal } = prepared(&llm, wire.clone()).await;
                assert_eq!(refusal, Some(MISSING_MODELS), "{wire}");
                assert!(
                    payload[field].as_str().is_none_or(str::is_empty),
                    "{wire}: {field}"
                );
            }
        }
    }
}

#[test]
fn names_is_this_data_capability_with_or_without_its_version() {
    assert!(names("ai.saveSettings"));
    assert!(names("ai.saveSettings@1"));
    assert!(!names("ai.saveSettingsNow"));
    assert!(!names("ai.testConnection@1"));
}

#[test]
fn timestamp_is_rfc_3339_in_utc() {
    for (seconds, text) in [
        (0, "1970-01-01T00:00:00Z"),
        (951_782_400, "2000-02-29T00:00:00Z"),
        (1_792_231_200, "2026-10-17T10:00:00Z"),
        (4_102_444_799, "2099-12-31T23:59:59Z"),
        // A century that is no leap year, a fourth one that is, an era's end.
        (4_107_542_399, "2100-02-28T23:59:59Z"),
        (4_107_542_400, "2100-03-01T00:00:00Z"),
        (4_233_686_400, "2104-02-29T00:00:00Z"),
        (13_569_465_599, "2399-12-31T23:59:59Z"),
        (13_574_606_400, "2400-02-29T12:00:00Z"),
        (13_574_649_600, "2400-03-01T00:00:00Z"),
    ] {
        assert_eq!(timestamp(seconds), text);
    }
}
