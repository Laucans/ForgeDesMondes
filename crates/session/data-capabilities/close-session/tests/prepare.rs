//! `prepare`: a live session's close, its journal summarised through the llm
//! port's fake by the active provider's summary model; a session not live
//! closed with nothing sent; a provider missing or failing, a close with no
//! summary.

use std::collections::BTreeMap;

use close_session::{
    Line, NO_SUMMARY, Prepared, Refusal, journal_text, names, prepare, provider, session_id,
};
use ports_llm::fake::FakeLlm;
use ports_llm::{
    ApiType, Binary, ConnectionReport, GenerateAnswer, GenerateRequest, Llm, LlmError, Provider,
    ProviderConfig, RequestJournal, RequestKind,
};
use serde_json::{Value, json};

const NOW: &str = "2026-10-17T23:00:00.000Z";

fn row(value: Value) -> BTreeMap<String, Value> {
    let Value::Object(row) = value else {
        panic!("a row is an object");
    };
    row.into_iter().collect()
}

fn session(status: &str) -> BTreeMap<String, Value> {
    row(json!({
        "sessionTitle": "L'embuscade du Gué",
        "sessionStatus": status,
        "microphoneOn": status == "live",
    }))
}

/// The cloud API, as the tables hold it: its type detected, its summary
/// model Haiku.
fn cloud() -> BTreeMap<String, Value> {
    row(json!({
        "mode": "api",
        "url": "https://api.anthropic.com/v1",
        "detectedApiType": "cloud",
        "apiKey": "sk-ant-test",
        "generationModel": "claude-sonnet-5-5",
        "summaryModel": "claude-haiku-4-5-20251001",
    }))
}

/// The opencode binary, as ai.saveSettings stores it: « binaire ».
fn opencode() -> BTreeMap<String, Value> {
    row(json!({
        "mode": "binaire",
        "binary": "opencode",
        "generationModel": "qwen2.5-32b-instruct",
        "summaryModel": "qwen2.5-14b-instruct",
    }))
}

fn journal() -> Vec<Line> {
    vec![
        Line {
            at: "20:14:03".into(),
            text: "Elowen entre dans la taverne.".into(),
        },
        Line {
            at: "20:16:40".into(),
            text: "Morlox lui tend la carte.".into(),
        },
    ]
}

/// The close every session gets: closed, dated, its microphone off.
fn closes(prepared: &Prepared) {
    assert_eq!(prepared.payload["sessionStatus"], "closed", "{prepared:?}");
    assert_eq!(prepared.payload["closedAt"], NOW, "{prepared:?}");
    assert_eq!(prepared.payload["microphoneOn"], false, "{prepared:?}");
}

/// A port whose provider never answers.
struct Down(RequestJournal);

impl Llm for Down {
    async fn test_connection(&self, _: &ProviderConfig) -> ConnectionReport {
        unreachable!("a close tests no connection")
    }

    async fn generate(
        &self,
        _: &ProviderConfig,
        _: &GenerateRequest,
    ) -> Result<GenerateAnswer, LlmError> {
        Err(LlmError::UnrecognisedApiType)
    }

    fn journal(&self) -> &RequestJournal {
        &self.0
    }
}

/// A port whose provider answers under a model name of its own.
struct Renaming(RequestJournal);

impl Llm for Renaming {
    async fn test_connection(&self, _: &ProviderConfig) -> ConnectionReport {
        unreachable!("a close tests no connection")
    }

    async fn generate(
        &self,
        _: &ProviderConfig,
        _: &GenerateRequest,
    ) -> Result<GenerateAnswer, LlmError> {
        Ok(GenerateAnswer {
            text: "Elowen reçoit la carte de Morlox.".into(),
            model: "claude-haiku-4-5-resolved".into(),
        })
    }

    fn journal(&self) -> &RequestJournal {
        &self.0
    }
}

#[tokio::test]
async fn close_session_names_the_configured_summary_model_whatever_the_provider_calls_it() {
    let llm = Renaming(RequestJournal::new());
    let prepared = prepare(&llm, &session("live"), &journal(), &cloud(), NOW)
        .await
        .expect("a live session closes");
    assert_eq!(
        prepared.payload["summary"],
        "Elowen reçoit la carte de Morlox."
    );
    assert_eq!(
        prepared.payload["summaryModel"],
        "claude-haiku-4-5-20251001"
    );
}

#[test]
fn the_session_named_by_the_payload_is_its_uuid_lowercased() {
    let named = row(json!({ "session": "5E000000-0000-4000-8000-000000000004" }));
    assert_eq!(
        session_id(&named).as_deref(),
        Some("5e000000-0000-4000-8000-000000000004")
    );
    assert_eq!(session_id(&BTreeMap::new()), None);
}

#[test]
fn it_names_session_close_session_with_or_without_its_version() {
    assert!(names("session.closeSession"));
    assert!(names("session.closeSession@1"));
    assert!(!names("session.getLiveSession"));
}

#[tokio::test]
async fn close_session_stores_the_summary_with_the_configured_summary_model() {
    let llm = FakeLlm::new();
    let prepared = prepare(&llm, &session("live"), &journal(), &cloud(), NOW)
        .await
        .expect("a live session closes");
    closes(&prepared);
    assert!(
        prepared.payload["summary"]
            .as_str()
            .is_some_and(|summary| !summary.trim().is_empty()),
        "{prepared:?}"
    );
    assert_eq!(
        prepared.payload["summaryModel"],
        "claude-haiku-4-5-20251001"
    );
    assert_eq!(prepared.warning, None);
    // The close's fields and the summary's, nothing else.
    assert_eq!(
        prepared
            .payload
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        [
            "closedAt",
            "microphoneOn",
            "sessionStatus",
            "summary",
            "summaryModel"
        ]
    );
}

#[tokio::test]
async fn text_only_no_audio_only_the_journal_s_lines_reach_the_provider_as_one_journal_request() {
    let llm = FakeLlm::new();
    prepare(&llm, &session("live"), &journal(), &opencode(), NOW)
        .await
        .expect("a live session closes");
    let sent = llm.journal().list();
    assert_eq!(sent.len(), 1, "{sent:?}");
    assert_eq!(sent[0].kind, RequestKind::Journal);
    assert_eq!(sent[0].provider, Provider::OpencodeBinary);
    assert_eq!(
        sent[0].text,
        "Journal de la séance « L'embuscade du Gué »\n\
         20:14:03 Elowen entre dans la taverne.\n\
         20:16:40 Morlox lui tend la carte."
    );
}

#[tokio::test]
async fn status_lifecycle_a_session_not_live_is_closed_with_no_summary_and_nothing_sent() {
    for status in ["prep", "closed"] {
        let llm = FakeLlm::new();
        let prepared = prepare(&llm, &session(status), &journal(), &cloud(), NOW)
            .await
            .expect("queued for the DataGuard to judge");
        closes(&prepared);
        assert!(!prepared.payload.contains_key("summary"), "{status}");
        assert_eq!(prepared.warning, None, "{status}");
        assert!(llm.journal().list().is_empty(), "{status}");
    }
}

#[tokio::test]
async fn a_session_with_no_status_is_none_to_close() {
    let llm = FakeLlm::new();
    let refused = prepare(&llm, &BTreeMap::new(), &[], &cloud(), NOW).await;
    assert_eq!(refused, Err(Refusal::UnknownSession));
    assert_eq!(Refusal::UnknownSession.to_string(), "séance inconnue");
    assert!(llm.journal().list().is_empty());
}

#[tokio::test]
async fn close_session_with_no_summary_model_or_a_provider_down_closes_all_the_same() {
    let mut no_model = cloud();
    no_model.insert("summaryModel".into(), "".into());
    for settings in [BTreeMap::new(), no_model] {
        let prepared = prepare(
            &FakeLlm::new(),
            &session("live"),
            &journal(),
            &settings,
            NOW,
        )
        .await
        .expect("a live session closes");
        closes(&prepared);
        assert!(!prepared.payload.contains_key("summary"), "{prepared:?}");
        assert_eq!(prepared.warning, Some(NO_SUMMARY));
    }
    let prepared = prepare(
        &Down(RequestJournal::new()),
        &session("live"),
        &journal(),
        &cloud(),
        NOW,
    )
    .await
    .expect("a live session closes");
    closes(&prepared);
    assert!(
        !prepared.payload.contains_key("summaryModel"),
        "{prepared:?}"
    );
    assert_eq!(prepared.warning, Some(NO_SUMMARY));
}

#[test]
fn the_active_provider_is_read_as_the_tables_hold_it() {
    let Some(ProviderConfig::Api {
        url,
        api_type,
        key,
        models,
    }) = provider(&cloud())
    else {
        panic!("the cloud API");
    };
    assert_eq!(url, "https://api.anthropic.com/v1");
    assert_eq!(api_type, Some(ApiType::Cloud));
    assert_eq!(
        key.map(|key| key.expose().to_owned()).as_deref(),
        Some("sk-ant-test")
    );
    assert_eq!(models.summary, "claude-haiku-4-5-20251001");
    let mut server = cloud();
    server.insert("detectedApiType".into(), "opencode".into());
    assert!(
        matches!(
            provider(&server),
            Some(ProviderConfig::Api {
                api_type: Some(ApiType::Opencode),
                ..
            })
        ),
        "{server:?}"
    );

    let mut claude = opencode();
    claude.insert("binary".into(), "claude-code".into());
    for (settings, binary) in [(opencode(), Binary::Opencode), (claude, Binary::ClaudeCode)] {
        assert!(
            matches!(provider(&settings), Some(ProviderConfig::Binary { binary: b, .. }) if b == binary),
            "{settings:?}"
        );
    }
    let mut unknown = opencode();
    unknown.insert("mode".into(), "carrier-pigeon".into());
    assert_eq!(provider(&unknown), None);
}

#[test]
fn an_empty_journal_is_said_so_to_the_summary_model() {
    assert_eq!(
        journal_text(&session("live"), &[]),
        "Journal de la séance « L'embuscade du Gué »\n(aucune ligne transcrite)"
    );
}
