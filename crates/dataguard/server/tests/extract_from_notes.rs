//! ai.extractFromNotes through the wire protocol, over the in-memory engine:
//! the notes are analysed before anything is queued, then submitted as one
//! command per proposal and one for the note — applied after them, its
//! result the one answered. Notes of no campaign, or holding no word, are
//! refused and nothing is queued; an idempotency key answers the first
//! analysis again.

use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use dataguard_server::{Engine, MEMORY, Planted, Reads, Rows, planted, router_reading};
use harness_dataguard::command::State;
use harness_dataguard::manifest::DataCapability;
use ports_llm::Llm;
use ports_llm::fake::FakeLlm;
use ports_speech_to_text::fake::FakeSpeechToText;
use serde_json::{Value, json};
use tower::ServiceExt;

/// A campaign written: the Campaign aggregate's partition the notes need.
const ADD_CAMPAIGN: &str = r#"{"dataCapability":"test.addCampaign","owner":"dataguard","version":1,"effect":"insert","target":{"aggregate":"Campaign","id":"$id"},"mode":"overwrite","callableBy":["*"]}"#;

const VALOMBRE: &str = "c1000000-0000-4000-8000-000000000001";

/// The DM, approver of their own campaign: the critical fields' review is
/// not what these tests judge.
fn caller() -> Value {
    json!({ "by": "dm", "from": "NoteImport", "roles": ["role:approver"] })
}

fn draft() -> String {
    let fixture: Value =
        serde_json::from_str(include_str!("../../../../mock/fixtures/NoteImport.json"))
            .expect("the fixture is JSON");
    fixture["draft"].as_str().expect("a draft").to_owned()
}

/// The engine over memory, holding Valombre, its rows, and its wire protocol
/// with a speech-to-text fake that hears the draft — and the llm port's fake,
/// whose journal its handle shares.
async fn served() -> (Arc<Engine>, Rows, Router, FakeLlm) {
    let llm = FakeLlm::new();
    let Planted { engine, rows } = planted(MEMORY).await.expect("the in-memory engine");
    let engine = Arc::new(
        engine.with_data_capability(DataCapability::parse(ADD_CAMPAIGN).expect("contract F")),
    );
    let app = router_reading(
        engine.clone(),
        rows.clone(),
        Arc::new(llm.clone()),
        Arc::new(FakeSpeechToText::new(draft())),
        Arc::new(Reads::open(MEMORY).await.expect("no database")),
    );
    let (status, _) = post(
        &app,
        json!({
            "dataCapability": "test.addCampaign@1",
            "payload": { "id": VALOMBRE, "name": "Les Cendres de Valombre", "kind": "campagne" },
            "target": { "id": VALOMBRE },
            "caller": caller(),
        }),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED);
    let applied = engine.run_once().await.expect("the applier ran");
    assert_eq!(applied.len(), 1, "{applied:?}");
    assert_eq!(applied[0].status, State::Applied, "{applied:?}");
    (engine, rows, app, llm)
}

async fn post(app: &Router, body: Value) -> (StatusCode, Value) {
    let request = Request::post("/commands")
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .expect("a request");
    let response = app.clone().oneshot(request).await.expect("an answer");
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("a body");
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

fn extraction(payload: Value) -> Value {
    json!({ "dataCapability": "ai.extractFromNotes@1", "payload": payload, "caller": caller() })
}

#[tokio::test]
async fn the_notes_proposals_are_applied_then_the_note_which_keeps_the_raw_text() {
    let (engine, rows, app, _) = served().await;
    let (status, answer) = post(
        &app,
        extraction(json!({ "campaign": VALOMBRE, "text": draft() })),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{answer}");
    let note = answer["commandId"].as_str().expect("the note's command");
    let partition = answer["partition"].as_str().expect("the note's partition");
    assert!(partition.starts_with("NoteImport/"), "{partition}");

    let applied = engine.run_once().await.expect("the applier ran");
    // At least two proposals, and the note — last.
    assert!(applied.len() >= 3, "{applied:?}");
    assert!(
        applied.iter().all(|r| r.status == State::Applied),
        "{applied:?}"
    );
    assert_eq!(applied.last().map(|r| r.command_id.as_str()), Some(note));
    assert!(engine.run_once().await.expect("the applier ran").is_empty());

    let row = rows.projected(partition).await.expect("the note's row");
    assert_eq!(row["campaignId"], json!(VALOMBRE));
    assert_eq!(row["rawText"], json!(draft()));
    assert_eq!(
        row["importedNoteId"].as_str(),
        partition.strip_prefix("NoteImport/")
    );
    assert!(row["analyzedAt"].is_string(), "{row:?}");
    assert!(
        !row.contains_key("createdEntityId"),
        "no element is created"
    );
}

#[tokio::test]
async fn a_dictation_is_heard_through_the_port_and_only_its_transcript_is_stored() {
    let (engine, rows, app, _) = served().await;
    let (status, answer) = post(
        &app,
        extraction(json!({ "campaign": VALOMBRE, "dictation": STANDARD.encode([3_u8; 64]) })),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{answer}");
    let applied = engine.run_once().await.expect("the applier ran");
    assert!(applied.len() >= 3, "{applied:?}");
    let partition = answer["partition"].as_str().expect("the note's partition");
    let row = rows.projected(partition).await.expect("the note's row");
    assert_eq!(row["rawText"], json!(draft()));
    assert!(!row.contains_key("dictation"), "the audio is not stored");
}

#[tokio::test]
async fn notes_of_a_campaign_the_engine_does_not_hold_are_404_and_nothing_is_queued() {
    let (engine, _, app, _) = served().await;
    let (status, answer) = post(
        &app,
        extraction(json!({ "campaign": "c1000000-0000-4000-8000-0000000000ff", "text": draft() })),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{answer}");
    assert!(engine.run_once().await.expect("the applier ran").is_empty());
}

#[tokio::test]
async fn notes_that_name_no_campaign_or_hold_no_word_are_422_and_nothing_is_queued() {
    let (engine, _, app, _) = served().await;
    for payload in [
        json!({ "text": draft() }),
        json!({ "campaign": VALOMBRE, "text": "   " }),
        json!({ "campaign": VALOMBRE, "dictation": "pas du base64 !" }),
    ] {
        let (status, answer) = post(&app, extraction(payload.clone())).await;
        assert_eq!(
            status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "{payload}: {answer}"
        );
        assert!(answer["error"].is_string(), "{answer}");
    }
    assert!(engine.run_once().await.expect("the applier ran").is_empty());
}

#[tokio::test]
async fn the_same_idempotency_key_answers_the_first_analysis_again() {
    let (engine, _, app, llm) = served().await;
    let mut body = extraction(json!({ "campaign": VALOMBRE, "text": draft() }));
    body["idempotencyKey"] = json!("import-1");
    let (_, first) = post(&app, body.clone()).await;
    let (status, again) = post(&app, body).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{again}");
    assert_eq!(again["commandId"], first["commandId"]);
    assert_eq!(again["replayed"], json!(true));
    assert_eq!(llm.journal().list().len(), 1, "the notes are analysed once");
    let applied = engine.run_once().await.expect("the applier ran");
    // One analysis: its proposals and its note, once.
    assert_eq!(
        applied
            .iter()
            .filter(|r| r.command_id == first["commandId"].as_str().unwrap_or_default())
            .count(),
        1
    );
    let (_, other) = post(
        &app,
        extraction(json!({ "campaign": VALOMBRE, "text": draft() })),
    )
    .await;
    assert_eq!(
        engine.run_once().await.expect("the applier ran").len(),
        applied.len(),
        "a second analysis, without the key, is as long as the first: {other}"
    );
}
