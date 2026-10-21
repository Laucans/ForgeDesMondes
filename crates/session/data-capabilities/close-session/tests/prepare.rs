//! session.closeSession's preparation, on the `llm` port's fake: what a
//! close writes on the session, what reaches the provider — the journal's
//! text, as a Journal request, and nothing for a session that is not live —
//! and which provider and model the AI settings name.

use std::collections::BTreeMap;

use close_session::{
    Line, NO_SUMMARY, NO_SUMMARY_PROVIDER, Prepared, Unclosable, closable, journal_text, prepare,
    provider, timestamp,
};
use ports_llm::fake::FakeLlm;
use ports_llm::{ApiType, Binary, Llm, Models, Provider, ProviderConfig, RequestKind, Secret};
use serde_json::{Value, json};

const CLOSED_AT: &str = "2026-10-17T22:40:00.123Z";

fn object(value: Value) -> BTreeMap<String, Value> {
    value
        .as_object()
        .expect("an object")
        .clone()
        .into_iter()
        .collect()
}

fn session(status: &str) -> BTreeMap<String, Value> {
    object(json!({
        "sessionTitle": "L'embuscade du Gué",
        "sessionStatus": status,
        "microphoneOn": true,
    }))
}

fn journal() -> Vec<Line> {
    [
        ("20:04", "On reprend sous le pont."),
        ("21:05", "Morlox finit par avouer."),
    ]
    .map(|(at, text)| Line {
        at: at.to_owned(),
        text: text.to_owned(),
    })
    .to_vec()
}

/// The home server's endpoint, as a save of the settings leaves it.
fn home() -> BTreeMap<String, Value> {
    object(json!({
        "mode": "api",
        "url": "http://192.168.1.40:4096",
        "detectedApiType": "opencode",
        "apiKey": null,
        "binary": null,
        "generationModel": "qwen2.5-32b-instruct",
        "summaryModel": "qwen2.5-14b-instruct",
    }))
}

fn claude_code() -> BTreeMap<String, Value> {
    object(json!({
        "mode": "binaire",
        "binary": "claude-code",
        "generationModel": "claude-sonnet-5-5",
        "summaryModel": "claude-haiku-4-5-20251001",
    }))
}

fn closed(extra: Value) -> BTreeMap<String, Value> {
    let mut payload = object(json!({
        "sessionStatus": "closed",
        "closedAt": CLOSED_AT,
        "microphoneOn": false,
    }));
    payload.extend(object(extra));
    payload
}

#[tokio::test]
async fn close_session_a_live_session_is_closed_its_mic_off_its_journal_summarised_with_the_summary_model()
 {
    for (settings, model, provider) in [
        (home(), "qwen2.5-14b-instruct", Provider::OpencodeServer),
        (
            claude_code(),
            "claude-haiku-4-5-20251001",
            Provider::ClaudeCodeBinary,
        ),
    ] {
        let llm = FakeLlm::new();
        let prepared = prepare(&llm, &session("live"), &journal(), &settings, CLOSED_AT)
            .await
            .expect("a live session closes");
        let summary = prepared.payload["summary"].as_str().expect("a summary");
        assert!(summary.contains(model), "{summary}");
        assert_eq!(
            prepared,
            Prepared {
                payload: closed(json!({ "summary": summary, "summaryModel": model })),
                warning: None,
            }
        );
        // text-only-no-audio: the journal's text, once, as a Journal request.
        let sent = llm.journal().list();
        assert_eq!(sent.len(), 1, "{sent:?}");
        assert_eq!(sent[0].kind, RequestKind::Journal);
        assert_eq!(sent[0].provider, provider);
        assert_eq!(sent[0].text, journal_text(&session("live"), &journal()));
    }
}

#[tokio::test]
async fn status_lifecycle_a_session_in_prep_or_closed_is_refused_and_nothing_reaches_the_provider()
{
    let llm = FakeLlm::new();
    for status in ["prep", "closed"] {
        assert_eq!(
            prepare(&llm, &session(status), &journal(), &home(), CLOSED_AT).await,
            Err(Unclosable::NotLive(status.to_owned()))
        );
    }
    assert_eq!(
        prepare(&llm, &BTreeMap::new(), &journal(), &home(), CLOSED_AT).await,
        Err(Unclosable::Unknown)
    );
    assert!(llm.journal().list().is_empty());
}

#[test]
fn only_a_live_session_is_closable() {
    assert_eq!(closable(&session("live")), Ok(()));
    assert_eq!(
        closable(&session("prep")),
        Err(Unclosable::NotLive("prep".to_owned()))
    );
    assert_eq!(
        closable(&object(json!({ "sessionStatus": 3 }))),
        Err(Unclosable::Unknown)
    );
}

#[tokio::test]
async fn close_session_with_no_provider_or_one_that_fails_closes_without_a_summary_and_says_so() {
    // An API URL whose type the port cannot tell: the fake refuses it.
    let unknown = object(json!({
        "mode": "api",
        "url": "http://example.org",
        "summaryModel": "m",
    }));
    for (settings, warning) in [
        (BTreeMap::new(), NO_SUMMARY_PROVIDER),
        (unknown, NO_SUMMARY),
    ] {
        let prepared = prepare(&FakeLlm::new(), &session("live"), &[], &settings, CLOSED_AT)
            .await
            .expect("closed all the same");
        assert_eq!(
            prepared,
            Prepared {
                payload: closed(json!({})),
                warning: Some(warning),
            }
        );
    }
}

/// A port whose provider answers nothing but blanks.
struct Blank(FakeLlm);

impl Llm for Blank {
    async fn test_connection(&self, config: &ProviderConfig) -> ports_llm::ConnectionReport {
        self.0.test_connection(config).await
    }

    async fn generate(
        &self,
        config: &ProviderConfig,
        request: &ports_llm::GenerateRequest,
    ) -> Result<ports_llm::GenerateAnswer, ports_llm::LlmError> {
        let mut answer = self.0.generate(config, request).await?;
        answer.text = " \n ".to_owned();
        Ok(answer)
    }

    fn journal(&self) -> &ports_llm::RequestJournal {
        self.0.journal()
    }
}

#[tokio::test]
async fn a_blank_answer_is_no_summary() {
    let prepared = prepare(
        &Blank(FakeLlm::new()),
        &session("live"),
        &journal(),
        &home(),
        CLOSED_AT,
    )
    .await
    .expect("closed all the same");
    assert_eq!(prepared.warning, Some(NO_SUMMARY));
    assert!(!prepared.payload.contains_key("summary"));
}

#[test]
fn the_settings_name_the_provider_and_its_two_models() {
    let models = |generation: &str, summary: &str| Models {
        generation: generation.to_owned(),
        summary: summary.to_owned(),
    };
    assert_eq!(
        provider(&home()),
        Some(ProviderConfig::Api {
            url: "http://192.168.1.40:4096".to_owned(),
            api_type: Some(ApiType::Opencode),
            key: None,
            models: models("qwen2.5-32b-instruct", "qwen2.5-14b-instruct"),
        })
    );
    let cloud = object(json!({
        "mode": "api",
        "url": "https://api.anthropic.com/v1",
        "detectedApiType": "cloud",
        "apiKey": "sk-ant-secret",
        "generationModel": "g",
        "summaryModel": "s",
    }));
    assert_eq!(
        provider(&cloud),
        Some(ProviderConfig::Api {
            url: "https://api.anthropic.com/v1".to_owned(),
            api_type: Some(ApiType::Cloud),
            key: Some(Secret::new("sk-ant-secret")),
            models: models("g", "s"),
        })
    );
    // A type not known yet: the port detects it.
    let mut undetected = cloud.clone();
    undetected.insert("detectedApiType".to_owned(), json!("autre"));
    undetected.insert("apiKey".to_owned(), json!(""));
    assert_eq!(
        provider(&undetected),
        Some(ProviderConfig::Api {
            url: "https://api.anthropic.com/v1".to_owned(),
            api_type: None,
            key: None,
            models: models("g", "s"),
        })
    );
    for (mode, name, binary) in [
        ("binaire", "claude-code", Binary::ClaudeCode),
        ("binary", "claude", Binary::ClaudeCode),
        ("binaire", "opencode", Binary::Opencode),
    ] {
        let settings = object(json!({
            "mode": mode, "binary": name, "generationModel": "g", "summaryModel": "s",
        }));
        assert_eq!(
            provider(&settings),
            Some(ProviderConfig::Binary {
                binary,
                models: models("g", "s"),
            }),
            "{mode} {name}"
        );
    }
}

#[test]
fn settings_that_reach_no_provider_or_name_no_summary_model_name_none() {
    let mut no_summary = home();
    no_summary.insert("summaryModel".to_owned(), json!("  "));
    let mut no_url = home();
    no_url.insert("url".to_owned(), json!(""));
    let mut no_binary = claude_code();
    no_binary.insert("binary".to_owned(), json!("vim"));
    let mut no_mode = home();
    no_mode.insert("mode".to_owned(), json!("pigeon"));
    for settings in [no_summary, no_url, no_binary, no_mode, BTreeMap::new()] {
        assert_eq!(provider(&settings), None, "{settings:?}");
    }
}

#[test]
fn the_text_sent_is_the_title_then_each_line_with_its_time() {
    assert_eq!(
        journal_text(&session("live"), &journal()),
        "Journal de la séance « L'embuscade du Gué »\n\
         20:04 On reprend sous le pont.\n\
         21:05 Morlox finit par avouer."
    );
    assert_eq!(
        journal_text(&session("live"), &[]),
        "Journal de la séance « L'embuscade du Gué »\n(aucune ligne transcrite)"
    );
}

#[test]
fn a_close_is_dated_in_utc_to_the_millisecond() {
    assert_eq!(timestamp(0), "1970-01-01T00:00:00.000Z");
    assert_eq!(timestamp(1_792_231_200_123), "2026-10-17T10:00:00.123Z");
    // A leap day, and the last millisecond of a year.
    assert_eq!(timestamp(951_782_400_000), "2000-02-29T00:00:00.000Z");
    assert_eq!(timestamp(1_798_761_599_999), "2026-12-31T23:59:59.999Z");
    // Across a century the calendar skips a leap day, and on the last day
    // of a 400-year era it keeps it.
    assert_eq!(timestamp(4_107_542_400_000), "2100-03-01T00:00:00.000Z");
    assert_eq!(timestamp(13_574_606_400_000), "2400-02-29T12:00:00.000Z");
    assert_eq!(timestamp(13_574_649_600_000), "2400-03-01T00:00:00.000Z");
}
