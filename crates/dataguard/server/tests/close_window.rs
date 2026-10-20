//! live.closeWindow, judged by the DataGuard as `plant()` binds it over the
//! in-memory store: what `prepare` makes of a close is written on its
//! session's diffusion row — the window's media, x, y and width emptied
//! together, the base and the framing as they were — by the DM, with no
//! review: none of the fields the manifest touches is critical. A window
//! already closed is closed again, the row unchanged but its stamp. The live
//! session the tables give is given here; the tables' own answer is
//! `close_window_projection.rs`'s. Over memory, which has no table, the wire
//! protocol refuses every close before it is queued.

use std::collections::BTreeMap;
use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use close_window::{Refusal, prepare};
use dataguard_server::{Engine, MEMORY, Planted, Rows, planted, router};
use harness_dataguard::api::Submission;
use harness_dataguard::command::{CommandResult, State};
use harness_dataguard::guard::Caller;
use serde_json::{Value, json};
use tower::ServiceExt;

const VALOMBRE: &str = "c1000000-0000-4000-8000-000000000001";
/// Valombre's live session.
const SEANCE: &str = "51000000-0000-4000-8000-000000000002";
/// The tavern's battlemap instance, of the live session.
const INSTANCE: &str = "b1000000-0000-4000-8000-000000000002";
/// An illustration of Valombre, 800 × 1200.
const PORTRAIT: &str = "d1000000-0000-4000-8000-000000000002";

const BEFORE: &str = "2026-10-20T19:59:00.000Z";
const NOW: &str = "2026-10-20T20:00:00.000Z";

/// The window's fields, which a close empties.
const WINDOW: [&str; 4] = ["windowMediaId", "windowX", "windowY", "windowWidth"];

fn object(value: Value) -> BTreeMap<String, Value> {
    let Value::Object(value) = value else {
        panic!("an object");
    };
    value.into_iter().collect()
}

/// `state` written on the live session's row by `data_capability`, from the
/// DM's live-session bar with `roles`: what the DataGuard answered.
async fn submit(
    engine: &Engine,
    data_capability: &str,
    state: BTreeMap<String, Value>,
    roles: &[&str],
) -> State {
    engine
        .submit(Submission {
            data_capability: data_capability.to_string(),
            payload: state,
            target_id: Some(SEANCE.to_string()),
            based_on: None,
            idempotency_key: None,
            caller: Caller {
                by: "dm".to_string(),
                from: "LiveSessionBar".to_string(),
                permissions: Vec::new(),
                roles: roles.iter().map(|role| (*role).to_string()).collect(),
            },
        })
        .await
        .expect("queued")
        .result
        .status
}

/// What the single applier made of what is queued, one command a partition.
async fn applied(engine: &Engine) -> Vec<CommandResult> {
    engine.run_once().await.expect("the applier ran")
}

/// The engine as the server plants it, and its rows.
async fn served() -> (Engine, Rows) {
    let Planted { engine, rows } = planted(MEMORY).await.expect("the in-memory engine");
    (engine, rows)
}

/// The live session's diffusion row as the engine holds it.
async fn row(rows: &Rows) -> BTreeMap<String, Value> {
    rows.projected(&format!("Diffusion/{SEANCE}"))
        .await
        .expect("the row")
}

/// A close of Valombre's window, prepared for its live session against its
/// row as the engine holds it and queued by the DM, who holds no role: what
/// the applier made of it.
async fn closed(engine: &Engine, rows: &Rows) -> CommandResult {
    let prepared = prepare(Some(SEANCE.to_string()), &row(rows).await, NOW).expect("a window");
    assert_eq!(prepared.target_id, SEANCE);
    assert_eq!(
        submit(engine, "live.closeWindow@1", prepared.payload, &[]).await,
        State::Queued
    );
    let mut results = applied(engine).await;
    assert_eq!(results.len(), 1, "{results:?}");
    results.remove(0)
}

/// The row of a session showing the tavern's battlemap instance, framed —
/// the portrait open in its window when `window` — as the commands of the
/// player view leave it, approved by the DM.
async fn showing_a_battlemap(engine: &Engine, window: bool) {
    let mut state = object(json!({
        "sessionId": SEANCE,
        "baseKind": "battlemap",
        "baseMediaId": null,
        "baseInstanceId": INSTANCE,
        "viewZoom": 1.5,
        "viewPanX": -120.0,
        "viewPanY": 40.0,
        "updatedAt": BEFORE,
    }));
    if window {
        state.extend(object(json!({
            "windowMediaId": PORTRAIT,
            "windowX": 0.6,
            "windowY": 0.12,
            "windowWidth": 0.3,
        })));
    }
    submit(
        engine,
        "live.showWindowOnPlayerView@1",
        state,
        &["role:approver"],
    )
    .await;
    let results = applied(engine).await;
    assert!(
        results.len() == 1 && results[0].status == State::Applied,
        "{results:?}"
    );
}

/// The base and the framing, which a close never writes.
const KEPT: [&str; 7] = [
    "sessionId",
    "baseKind",
    "baseMediaId",
    "baseInstanceId",
    "viewZoom",
    "viewPanX",
    "viewPanY",
];

#[tokio::test]
async fn a_close_of_the_open_window_empties_its_four_fields_and_keeps_the_battlemap_underneath() {
    let (engine, rows) = served().await;
    showing_a_battlemap(&engine, true).await;
    let before = row(&rows).await;
    let result = closed(&engine, &rows).await;
    assert_eq!(result.status, State::Applied, "{result:?}");
    let row = row(&rows).await;
    for field in WINDOW {
        assert_eq!(row.get(field), Some(&Value::Null), "{field}");
    }
    assert_eq!(row["updatedAt"], NOW);
    for field in KEPT {
        assert_eq!(row[field], before[field], "{field}");
    }
    assert_eq!(row["baseKind"], "battlemap");
    assert_eq!(row["baseInstanceId"], INSTANCE);
}

#[tokio::test]
async fn a_window_already_closed_is_closed_again_the_row_unchanged_but_its_stamp() {
    let (engine, rows) = served().await;
    showing_a_battlemap(&engine, false).await;
    let mut before = row(&rows).await;
    let result = closed(&engine, &rows).await;
    assert_eq!(result.status, State::Applied, "{result:?}");
    assert!(result.violations.is_empty(), "{result:?}");
    let after = row(&rows).await;
    assert_eq!(after["updatedAt"], NOW);
    // The window's fields were none and are none; nothing else moved.
    for field in WINDOW {
        before.insert(field.to_string(), Value::Null);
    }
    before.insert("updatedAt".to_string(), Value::from(NOW));
    assert_eq!(after, before);
}

#[tokio::test]
async fn closes_in_a_row_are_each_applied() {
    let (engine, rows) = served().await;
    showing_a_battlemap(&engine, true).await;
    for _ in 0..2 {
        let result = closed(&engine, &rows).await;
        assert_eq!(result.status, State::Applied, "{result:?}");
    }
    let row = row(&rows).await;
    for field in WINDOW {
        assert_eq!(row.get(field), Some(&Value::Null), "{field}");
    }
}

#[tokio::test]
async fn a_session_nothing_was_shown_on_has_no_window_to_close_and_gets_no_row() {
    let (_engine, rows) = served().await;
    assert_eq!(
        prepare(Some(SEANCE.to_string()), &row(&rows).await, NOW),
        Err(Refusal::NothingShown)
    );
    assert!(row(&rows).await.is_empty());
}

/// `POST /commands` of live.closeWindow with `payload` by the DM, over the
/// engine's memory: the status and the answer.
async fn post(engine: Engine, rows: Rows, payload: Value) -> (StatusCode, Value) {
    let app = router(
        Arc::new(engine),
        rows,
        Arc::new(ports_llm::fake::FakeLlm::new()),
    );
    let body = json!({
        "dataCapability": "live.closeWindow@1",
        "target": { "aggregate": "Diffusion" },
        "payload": payload,
        "caller": { "by": "dm", "from": "LiveSessionBar", "roles": [] },
    });
    let request = Request::post("/commands")
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .expect("a request");
    let response = app.oneshot(request).await.expect("an answer");
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("a body");
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

#[tokio::test]
async fn the_wire_protocol_refuses_before_queueing_what_names_no_live_session() {
    for (payload, said) in [
        (json!({}), "campaign manque"),
        (
            json!({ "campaign": "valombre" }),
            "campaign n'est pas un identifiant",
        ),
        // The engine's memory holds no table, so no live session.
        (
            json!({ "campaign": VALOMBRE }),
            "aucune séance n'est en cours",
        ),
    ] {
        let (engine, rows) = served().await;
        let probe = rows.clone();
        let (status, answer) = post(engine, rows, payload.clone()).await;
        assert_eq!(
            status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "{payload}: {answer}"
        );
        assert_eq!(answer["error"], said, "{payload}");
        assert!(row(&probe).await.is_empty(), "{payload}: nothing queued");
    }
}
