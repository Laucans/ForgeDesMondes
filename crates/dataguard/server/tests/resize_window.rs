//! live.resizeWindow, judged by the DataGuard as `plant()` binds it over the
//! in-memory store: what `prepare` makes of a drag of the window's corner is
//! written on its session's diffusion row — its width, the largest the box
//! holds at the illustration's ratio; its position, its media and the base
//! as they were; no height — by the DM from their player-view tab, with no
//! review: the manifest touches the width and the stamp alone, neither of
//! them critical. A window that is not open is rejected under
//! window-fields-together, one whose illustration has no proportions under
//! window-proportional, and drags in a row are each applied, the last one's
//! width kept. The live session and the proportions the tables give are
//! given here; the tables' own answer is `resize_window_projection.rs`'s.
//! Over memory, which has no table, the wire protocol refuses every resize
//! before it is queued.

use std::collections::BTreeMap;
use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use dataguard_server::{Engine, MEMORY, Planted, Rows, planted, router};
use harness_dataguard::api::Submission;
use harness_dataguard::command::{CommandResult, State};
use harness_dataguard::guard::Caller;
use resize_window::{Named, Proportions, Window, prepare};
use serde_json::{Value, json};
use tower::ServiceExt;

const VALOMBRE: &str = "c1000000-0000-4000-8000-000000000001";
/// Valombre's live session.
const SEANCE: &str = "51000000-0000-4000-8000-000000000002";
/// The tavern's battlemap instance, of the live session.
const INSTANCE: &str = "b1000000-0000-4000-8000-000000000002";
/// An illustration of Valombre, 800 × 1200.
const PORTRAIT: &str = "d1000000-0000-4000-8000-000000000002";
/// Its proportions, as the tables give them.
const PORTRAIT_SIZE: Proportions = Proportions {
    width_px: 800,
    height_px: 1200,
};

const BEFORE: &str = "2026-10-20T19:59:00.000Z";
const NOW: &str = "2026-10-20T20:00:00.000Z";

fn object(value: Value) -> BTreeMap<String, Value> {
    let Value::Object(value) = value else {
        panic!("an object");
    };
    value.into_iter().collect()
}

/// `state` written on the live session's row by `data_capability`, from the
/// DM's player-view tab with `roles`: what the DataGuard answered.
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
                from: "PlayerStage".to_string(),
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

/// A drag of the corner of Valombre's window, showing `window`, to the box
/// `w` × `h`, prepared for its live session and queued by the DM, who holds
/// no role: what the DataGuard answered.
async fn dragged(engine: &Engine, window: Window, w: f64, h: f64) -> State {
    let named = Named {
        campaign: VALOMBRE.to_string(),
        w,
        h: Some(h),
    };
    let prepared = prepare(&named, Some(SEANCE.to_string()), window, NOW).expect("a live session");
    assert_eq!(prepared.target_id, SEANCE);
    submit(engine, "live.resizeWindow@1", prepared.payload, &[]).await
}

/// A drag of the corner to `w` × `h`: what the applier made of it.
async fn resized(engine: &Engine, window: Window, w: f64, h: f64) -> CommandResult {
    dragged(engine, window, w, h).await;
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

#[tokio::test]
async fn a_square_drawn_on_the_open_window_writes_the_width_at_2_3_and_keeps_its_place() {
    let (engine, rows) = served().await;
    showing_a_battlemap(&engine, true).await;
    let before = row(&rows).await;
    let result = resized(&engine, Window::Open(Some(PORTRAIT_SIZE)), 0.6, 0.6).await;
    assert_eq!(result.status, State::Applied, "{result:?}");
    let row = row(&rows).await;
    // The largest 2/3 window a 0.6 square holds: 0.4 wide, 0.6 high.
    assert_eq!(row["windowWidth"], 0.4);
    assert_eq!(row["updatedAt"], NOW);
    // The position and the content of the window, the base and the framing
    // are as they were; no height is stored.
    for field in [
        "windowX",
        "windowY",
        "windowMediaId",
        "sessionId",
        "baseKind",
        "baseMediaId",
        "baseInstanceId",
        "viewZoom",
        "viewPanX",
        "viewPanY",
    ] {
        assert_eq!(row[field], before[field], "{field}");
    }
    assert!(!row.contains_key("windowHeight"));
}

#[tokio::test]
async fn a_window_that_is_not_open_is_rejected_under_window_fields_together_and_the_row_stays() {
    let (engine, rows) = served().await;
    showing_a_battlemap(&engine, false).await;
    let before = row(&rows).await;
    let result = resized(&engine, Window::Closed, 0.6, 0.6).await;
    assert_eq!(result.status, State::Rejected, "{result:?}");
    assert_eq!(result.violations, ["window-fields-together"]);
    assert_eq!(row(&rows).await, before);
}

#[tokio::test]
async fn a_session_with_no_diffusion_row_has_no_window_to_resize_and_gets_no_row() {
    let (engine, rows) = served().await;
    let result = resized(&engine, Window::Closed, 0.6, 0.6).await;
    assert_eq!(result.status, State::Rejected, "{result:?}");
    assert!(
        result
            .violations
            .contains(&"window-fields-together".to_string()),
        "{result:?}"
    );
    assert!(row(&rows).await.is_empty());
}

#[tokio::test]
async fn a_window_whose_illustration_has_no_proportions_is_rejected_under_window_proportional() {
    let (engine, rows) = served().await;
    showing_a_battlemap(&engine, true).await;
    let before = row(&rows).await;
    let result = resized(&engine, Window::Open(None), 0.6, 0.6).await;
    assert_eq!(result.status, State::Rejected, "{result:?}");
    assert_eq!(result.violations, ["window-proportional"]);
    assert_eq!(row(&rows).await, before);
}

#[tokio::test]
async fn drags_in_a_row_are_queued_and_each_applied_the_last_one_kept() {
    let (engine, rows) = served().await;
    showing_a_battlemap(&engine, true).await;
    let drags = [(0.2, 0.9), (0.5, 0.3), (0.45, 0.9)];
    // Queued before any is applied, and none waits on a confirmation.
    for (w, h) in drags {
        assert_eq!(
            dragged(&engine, Window::Open(Some(PORTRAIT_SIZE)), w, h).await,
            State::Queued
        );
    }
    let mut settled = Vec::new();
    for _ in drags {
        settled.extend(applied(&engine).await);
    }
    assert_eq!(
        settled.iter().map(|r| &r.status).collect::<Vec<_>>(),
        [&State::Applied; 3],
        "{settled:?}"
    );
    let row = row(&rows).await;
    assert_eq!(row["windowWidth"], 0.45);
    assert_eq!(
        (&row["windowX"], &row["windowY"]),
        (&json!(0.6), &json!(0.12))
    );
}

/// `POST /commands` of live.resizeWindow with `payload` by the DM, over the
/// engine's memory: the status and the answer.
async fn post(engine: Engine, rows: Rows, payload: Value) -> (StatusCode, Value) {
    let app = router(
        Arc::new(engine),
        rows,
        Arc::new(ports_llm::fake::FakeLlm::new()),
    );
    let body = json!({
        "dataCapability": "live.resizeWindow@1",
        "target": { "aggregate": "Diffusion" },
        "payload": payload,
        "caller": { "by": "dm", "from": "PlayerStage", "roles": [] },
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
        (json!({ "w": 0.4, "h": 0.6 }), "campaign manque"),
        (
            json!({ "campaign": "valombre", "w": 0.4, "h": 0.6 }),
            "campaign n'est pas un identifiant",
        ),
        (json!({ "campaign": VALOMBRE, "h": 0.6 }), "w manque"),
        (
            json!({ "campaign": VALOMBRE, "w": 0.4, "h": "0.6" }),
            "w ou h n'est pas un nombre",
        ),
        // The engine's memory holds no table, so no live session.
        (
            json!({ "campaign": VALOMBRE, "w": 600, "h": 600 }),
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
