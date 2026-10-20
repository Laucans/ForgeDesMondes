//! live.showCover, judged by the DataGuard as `plant()` binds it over the
//! in-memory store: what `prepare` makes of a return to the cover is written
//! on its session's diffusion row — made when there was none, its base
//! emptied otherwise, its window and its framing kept — and two in a row are
//! both applied. The live session the tables give is given here; the tables'
//! own answer is `show_cover_projection.rs`'s. Over memory, which has no
//! table, the wire protocol refuses every return to the cover before it is
//! queued.

use std::collections::BTreeMap;
use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use dataguard_server::{Engine, MEMORY, Planted, Rows, planted, router};
use harness_dataguard::api::Submission;
use harness_dataguard::command::{CommandResult, State};
use harness_dataguard::guard::Caller;
use serde_json::{Value, json};
use show_cover::prepare;
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

fn object(value: Value) -> BTreeMap<String, Value> {
    let Value::Object(value) = value else {
        panic!("an object");
    };
    value.into_iter().collect()
}

/// `state` written on the live session's row by the DM from the session bar
/// — the DM approves their own campaign's critical fields, the session and
/// the kind of its base.
async fn submit(engine: &Engine, state: BTreeMap<String, Value>) {
    engine
        .submit(Submission {
            data_capability: "live.showCover@1".to_string(),
            payload: state,
            target_id: Some(SEANCE.to_string()),
            based_on: None,
            idempotency_key: None,
            caller: Caller {
                by: "dm".to_string(),
                from: "LiveSessionBar".to_string(),
                permissions: Vec::new(),
                roles: vec!["role:approver".to_string()],
            },
        })
        .await
        .expect("queued");
}

/// What the single applier made of what is queued, one command a partition.
async fn applied(engine: &Engine) -> Vec<CommandResult> {
    engine.run_once().await.expect("the applier ran")
}

/// A return to the cover, prepared for the live session: what the applier
/// made of it.
async fn covered(engine: &Engine) -> CommandResult {
    let prepared = prepare(Some(SEANCE.to_string()), NOW).expect("a live session");
    assert_eq!(prepared.target_id, SEANCE);
    submit(engine, prepared.payload).await;
    let mut results = applied(engine).await;
    assert_eq!(results.len(), 1, "{results:?}");
    results.remove(0)
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

/// The row a session showing the tavern's battlemap instance, framed, with
/// the portrait open in the floating window, holds — as a send to the player
/// view and an opened window leave it. It is written here through the one
/// DataCapability of Diffusion this checkout holds, carrying that state.
async fn showing_a_battlemap_with_a_window(engine: &Engine) {
    submit(
        engine,
        object(json!({
            "sessionId": SEANCE,
            "baseKind": "battlemap",
            "baseMediaId": null,
            "baseInstanceId": INSTANCE,
            "viewZoom": 1.5,
            "viewPanX": -120.0,
            "viewPanY": 40.0,
            "windowMediaId": PORTRAIT,
            "windowX": 0.1,
            "windowY": 0.2,
            "windowWidth": 0.3,
            "updatedAt": BEFORE,
        })),
    )
    .await;
    let results = applied(engine).await;
    assert!(
        results.len() == 1 && results[0].status == State::Applied,
        "{results:?}"
    );
}

#[tokio::test]
async fn a_session_with_no_diffusion_row_gets_one_showing_the_cover() {
    let (engine, rows) = served().await;
    assert!(row(&rows).await.is_empty(), "no row before");
    let result = covered(&engine).await;
    assert_eq!(result.status, State::Applied, "{result:?}");
    assert_eq!(
        row(&rows).await,
        object(json!({
            "sessionId": SEANCE,
            "baseKind": "cover",
            "baseMediaId": null,
            "baseInstanceId": null,
            "updatedAt": NOW,
        }))
    );
}

#[tokio::test]
async fn a_session_showing_a_battlemap_goes_back_to_the_cover_its_window_and_framing_kept() {
    let (engine, rows) = served().await;
    showing_a_battlemap_with_a_window(&engine).await;
    let result = covered(&engine).await;
    assert_eq!(result.status, State::Applied, "{result:?}");
    let row = row(&rows).await;
    // base-coherent: the cover, neither a media nor an instance.
    assert_eq!(row["baseKind"], "cover");
    assert_eq!(row["baseMediaId"], Value::Null);
    assert_eq!(row["baseInstanceId"], Value::Null);
    assert_eq!(row["updatedAt"], NOW);
    // The window (live.closeWindow's) and the framing are not touched.
    assert_eq!(row["windowMediaId"], PORTRAIT);
    assert_eq!(row["windowX"], 0.1);
    assert_eq!(row["windowY"], 0.2);
    assert_eq!(row["windowWidth"], 0.3);
    assert_eq!(row["viewZoom"], 1.5);
    assert_eq!(row["viewPanX"], -120.0);
    assert_eq!(row["viewPanY"], 40.0);
    // single-diffusion-source: still the session's one row.
    assert_eq!(row["sessionId"], SEANCE);
}

#[tokio::test]
async fn two_returns_to_the_cover_in_a_row_are_both_applied_on_the_one_row() {
    let (engine, rows) = served().await;
    for _ in 0..2 {
        let prepared = prepare(Some(SEANCE.to_string()), NOW).expect("a live session");
        submit(&engine, prepared.payload).await;
    }
    // overwrite: neither waits for a confirmation.
    let results = applied(&engine).await;
    assert_eq!(results.len(), 2, "{results:?}");
    assert!(
        results.iter().all(|result| result.status == State::Applied),
        "{results:?}"
    );
    assert_eq!(row(&rows).await["baseKind"], "cover");
}

#[tokio::test]
async fn base_coherent_refuses_a_cover_that_keeps_a_media_and_the_row_stays() {
    let (engine, rows) = served().await;
    showing_a_battlemap_with_a_window(&engine).await;
    let before = row(&rows).await;
    let mut state = prepare(Some(SEANCE.to_string()), NOW)
        .expect("a live session")
        .payload;
    state.insert("baseMediaId".to_string(), Value::from(PORTRAIT));
    submit(&engine, state).await;
    let results = applied(&engine).await;
    assert_eq!(results.len(), 1, "{results:?}");
    assert_eq!(results[0].status, State::Rejected, "{results:?}");
    assert_eq!(results[0].violations, vec!["base-coherent".to_string()]);
    assert_eq!(row(&rows).await, before);
}

/// `POST /commands` of live.showCover with `payload` by the DM, over the
/// engine's memory: the status and the answer.
async fn post(engine: Engine, rows: Rows, payload: Value) -> (StatusCode, Value) {
    let app = router(
        Arc::new(engine),
        rows,
        Arc::new(ports_llm::fake::FakeLlm::new()),
    );
    let body = json!({
        "dataCapability": "live.showCover@1",
        "target": { "aggregate": "Diffusion" },
        "payload": payload,
        "caller": { "by": "dm", "from": "LiveSessionBar", "roles": ["role:approver"] },
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
