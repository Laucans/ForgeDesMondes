//! session.closeSession through the wire protocol, over the in-memory store:
//! a live session is closed with its microphone off, its close dated and the
//! summary of its journal stored with the summary model of the active
//! provider — the journal's text alone sent to the `llm` port, as a
//! `Journal` request. A session in prep or already closed is refused under
//! `status-lifecycle` and its row stays as it was; a closed session takes no
//! more journal lines; a session no one holds is refused before it is
//! queued. The session is written by a test-only DataCapability: no
//! DataCapability of the product creates one yet.

use std::collections::BTreeMap;
use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use dataguard_server::{Engine, MEMORY, Planted, Rows, planted, router};
use harness_dataguard::api::Submission;
use harness_dataguard::command::{CommandResult, State};
use harness_dataguard::guard::Caller;
use harness_dataguard::manifest::DataCapability;
use ports_llm::fake::FakeLlm;
use ports_llm::{Llm, Provider, RequestKind};
use serde_json::{Value, json};
use tower::ServiceExt;

/// A session written, test-only: its status, its microphone, its journal.
const UPDATE: &str = r#"{"dataCapability":"test.updateSession","owner":"dataguard","version":1,"effect":"upsert","target":{"aggregate":"Session","id":"$sessionId"},"mode":"overwrite","callableBy":["*"]}"#;

const VALOMBRE: &str = "c1000000-0000-4000-8000-000000000001";
/// Valombre's session 4, and session 3 before it.
const SEANCE: &str = "51000000-0000-4000-8000-000000000004";
const NOWHERE: &str = "51000000-0000-4000-8000-0000000000ff";
const JOURNAL_LINE: &str = "1a000000-0000-4000-8000-000000000001";

/// The engine as the server plants it, with the test-only DataCapability,
/// its rows, its wire protocol, and the fake `llm` port it reaches.
struct Served {
    engine: Arc<Engine>,
    rows: Rows,
    app: Router,
    llm: Arc<FakeLlm>,
}

async fn serve() -> Served {
    let Planted { engine, rows } = planted(MEMORY).await.expect("the in-memory engine");
    let engine =
        Arc::new(engine.with_data_capability(DataCapability::parse(UPDATE).expect("contract F")));
    let llm = Arc::new(FakeLlm::new());
    let app = router(engine.clone(), rows.clone(), llm.clone());
    Served {
        engine,
        rows,
        app,
        llm,
    }
}

/// `payload` written straight to session 4, as an approver: what the applier
/// made of it.
async fn write(engine: &Engine, payload: Value) -> CommandResult {
    let Value::Object(payload) = payload else {
        panic!("a payload is an object");
    };
    engine
        .submit(Submission {
            data_capability: "test.updateSession".to_string(),
            payload: payload.into_iter().collect(),
            target_id: Some(SEANCE.to_string()),
            based_on: None,
            idempotency_key: None,
            caller: Caller {
                by: "dm".to_string(),
                from: "session.prep".to_string(),
                permissions: Vec::new(),
                roles: vec!["role:approver".to_string()],
            },
        })
        .await
        .expect("queued");
    engine.run_once().await.expect("the applier ran").remove(0)
}

/// Session 4 of Valombre in `status`, its microphone on once live.
async fn session(engine: &Engine, status: &str) {
    let row = json!({
        "sessionId": SEANCE,
        "sessionCampaignId": VALOMBRE,
        "sessionPosition": 4,
        "sessionTitle": "L'embuscade du Gué",
        "sessionStatus": "prep",
        "microphoneOn": false,
    });
    assert_eq!(write(engine, row).await.status, State::Applied);
    if status != "prep" {
        let live = json!({ "sessionStatus": "live", "microphoneOn": true });
        assert_eq!(write(engine, live).await.status, State::Applied);
    }
    if status == "closed" {
        let closed = json!({
            "sessionStatus": "closed",
            "microphoneOn": false,
            "closedAt": "2026-10-17T23:00:00Z",
        });
        assert_eq!(write(engine, closed).await.status, State::Applied);
    }
}

/// `body` posted to `uri`, and the answer.
async fn call(app: &Router, uri: &str, body: Value) -> (StatusCode, Value) {
    let request = Request::post(uri)
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

/// The active provider saved through ai.saveSettings, as the settings screen
/// saves it.
async fn settings(engine: &Engine, app: &Router, payload: Value) {
    let body = json!({
        "dataCapability": "ai.saveSettings@1",
        "payload": payload,
        "caller": { "by": "dm", "from": "AiSettings", "roles": ["role:approver"] },
    });
    let (status, answer) = call(app, "/commands", body).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{answer}");
    let result = engine.run_once().await.expect("the applier ran").remove(0);
    assert_eq!(result.status, State::Applied, "{result:?}");
}

/// The Claude Code binary, its summary model Haiku.
fn claude_code() -> Value {
    json!({
        "mode": "binary",
        "binary": "claude",
        "generationModel": "claude-sonnet-5-5",
        "summaryModel": "claude-haiku-4-5-20251001",
    })
}

/// « Clore la séance » on `target`, as the LiveSessionBar sends it — an
/// empty payload — by an approver: the session's status and microphone are
/// critical fields, whose change a caller who approves nothing leaves
/// awaiting its review. The answer, and what the applier made of it when it
/// was queued.
async fn close(
    engine: &Engine,
    app: &Router,
    target: &str,
) -> (StatusCode, Value, Option<CommandResult>) {
    let body = json!({
        "dataCapability": "session.closeSession@1",
        "payload": {},
        "target": { "aggregate": "Session", "id": target },
        "caller": { "by": "dm", "from": "LiveSessionBar", "roles": ["role:approver"] },
    });
    let (status, answer) = call(app, "/commands", body).await;
    if status != StatusCode::ACCEPTED {
        return (status, answer, None);
    }
    let result = engine.run_once().await.expect("the applier ran").remove(0);
    (status, answer, Some(result))
}

/// Session 4's row as the engine holds it.
async fn row(rows: &Rows) -> BTreeMap<String, Value> {
    rows.projected(&format!("Session/{SEANCE}"))
        .await
        .expect("the row")
}

fn closed(result: Option<CommandResult>) {
    let result = result.expect("queued");
    assert_eq!(result.status, State::Applied, "{result:?}");
}

fn refused_by_status_lifecycle(result: Option<CommandResult>) {
    let result = result.expect("queued");
    assert_eq!(result.status, State::Rejected, "{result:?}");
    assert_eq!(result.violations, ["status-lifecycle"], "{result:?}");
}

#[tokio::test]
async fn close_session_turns_the_microphone_off_dates_the_close_and_stores_the_summary_with_its_model()
 {
    let Served {
        engine, rows, app, ..
    } = serve().await;
    settings(&engine, &app, claude_code()).await;
    session(&engine, "live").await;

    let (status, answer, result) = close(&engine, &app, SEANCE).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{answer}");
    closed(result);

    let row = row(&rows).await;
    assert_eq!(row["sessionStatus"], "closed", "{row:?}");
    assert_eq!(row["microphoneOn"], false, "{row:?}");
    assert!(
        row["closedAt"]
            .as_str()
            .is_some_and(|at| at.starts_with("20")),
        "{row:?}"
    );
    assert!(
        row["summary"]
            .as_str()
            .is_some_and(|s| !s.trim().is_empty()),
        "{row:?}"
    );
    assert_eq!(row["summaryModel"], "claude-haiku-4-5-20251001", "{row:?}");
    // What the session was keeps: its title, its campaign.
    assert_eq!(row["sessionTitle"], "L'embuscade du Gué", "{row:?}");
    assert_eq!(row["sessionCampaignId"], VALOMBRE, "{row:?}");
}

#[tokio::test]
async fn text_only_no_audio_the_journal_s_text_alone_goes_to_the_provider_as_a_journal_request() {
    let Served {
        engine, app, llm, ..
    } = serve().await;
    settings(&engine, &app, claude_code()).await;
    session(&engine, "live").await;

    let before = llm.journal().list().len();
    closed(close(&engine, &app, SEANCE).await.2);

    let sent = llm.journal().list();
    let sent = &sent[before..];
    assert_eq!(sent.len(), 1, "{sent:?}");
    assert_eq!(sent[0].kind, RequestKind::Journal, "{sent:?}");
    assert_eq!(sent[0].provider, Provider::ClaudeCodeBinary, "{sent:?}");
    assert!(sent[0].text.contains("L'embuscade du Gué"), "{sent:?}");
}

#[tokio::test]
async fn status_lifecycle_refuses_to_close_a_session_in_prep_and_nothing_is_written() {
    let Served {
        engine,
        rows,
        app,
        llm,
    } = serve().await;
    settings(&engine, &app, claude_code()).await;
    session(&engine, "prep").await;
    let before = row(&rows).await;
    let sent = llm.journal().list().len();

    refused_by_status_lifecycle(close(&engine, &app, SEANCE).await.2);

    assert_eq!(row(&rows).await, before);
    // Nothing to summarise: nothing reached the provider.
    assert_eq!(llm.journal().list().len(), sent);
}

#[tokio::test]
async fn status_lifecycle_refuses_to_close_a_closed_session_again_and_nothing_is_written() {
    let Served {
        engine,
        rows,
        app,
        llm,
    } = serve().await;
    settings(&engine, &app, claude_code()).await;
    session(&engine, "closed").await;
    let before = row(&rows).await;
    let sent = llm.journal().list().len();

    refused_by_status_lifecycle(close(&engine, &app, SEANCE).await.2);

    assert_eq!(row(&rows).await, before);
    assert_eq!(llm.journal().list().len(), sent);
}

#[tokio::test]
async fn status_lifecycle_a_closed_session_takes_no_more_journal_lines() {
    let Served {
        engine, rows, app, ..
    } = serve().await;
    settings(&engine, &app, claude_code()).await;
    session(&engine, "live").await;
    closed(close(&engine, &app, SEANCE).await.2);
    let before = row(&rows).await;

    let line = json!({
        "sessionJournalLineId": JOURNAL_LINE,
        "sessionJournalLineSessionId": SEANCE,
        "spokenAt": "2026-10-17T23:05:00Z",
        "text": "Encore une chose…",
    });
    let result = write(&engine, line).await;
    assert_eq!(result.status, State::Rejected, "{result:?}");
    assert_eq!(result.violations, ["status-lifecycle"], "{result:?}");
    assert_eq!(row(&rows).await, before);
}

#[tokio::test]
async fn a_session_no_one_holds_is_refused_before_it_is_queued() {
    let Served {
        engine, app, llm, ..
    } = serve().await;
    settings(&engine, &app, claude_code()).await;
    let sent = llm.journal().list().len();

    let (status, answer, _) = close(&engine, &app, NOWHERE).await;

    assert_eq!(status, StatusCode::NOT_FOUND, "{answer}");
    assert!(engine.run_once().await.expect("the applier ran").is_empty());
    assert_eq!(llm.journal().list().len(), sent);
}

#[tokio::test]
async fn close_session_without_a_provider_still_closes_and_says_no_summary_was_made() {
    let Served {
        engine,
        rows,
        app,
        llm,
    } = serve().await;
    session(&engine, "live").await;

    let (status, answer, result) = close(&engine, &app, SEANCE).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{answer}");
    closed(result);
    assert_eq!(
        answer["warnings"],
        json!([close_session::NO_SUMMARY]),
        "{answer}"
    );

    let row = row(&rows).await;
    assert_eq!(row["sessionStatus"], "closed", "{row:?}");
    assert_eq!(row["microphoneOn"], false, "{row:?}");
    assert!(row.get("summary").is_none_or(Value::is_null), "{row:?}");
    assert!(llm.journal().list().is_empty());
}

#[tokio::test]
async fn close_session_finds_its_session_in_any_case_or_named_by_the_payload() {
    let Served {
        engine, rows, app, ..
    } = serve().await;
    settings(&engine, &app, claude_code()).await;
    session(&engine, "live").await;

    let (status, answer, result) = close(&engine, &app, &SEANCE.to_uppercase()).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{answer}");
    assert_eq!(answer["partition"], format!("Session/{SEANCE}"), "{answer}");
    closed(result);
    assert_eq!(row(&rows).await["sessionStatus"], "closed");

    // Named by the payload, as the manifest's target `$session` reads it.
    let body = json!({
        "dataCapability": "session.closeSession@1",
        "payload": { "session": SEANCE },
        "caller": { "by": "dm", "from": "LiveSessionBar", "roles": ["role:approver"] },
    });
    let (status, answer) = call(&app, "/commands", body).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{answer}");
    assert_eq!(answer["partition"], format!("Session/{SEANCE}"), "{answer}");
    refused_by_status_lifecycle(engine.run_once().await.expect("the applier ran").pop());
}
