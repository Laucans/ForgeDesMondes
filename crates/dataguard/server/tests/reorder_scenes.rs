//! session.reorderScenes through the wire protocol, over the engine's
//! in-memory store: the order of a session's scenes is written whole, in one
//! command on the session's partition, and the DataGuard rejects one that
//! omits a scene, repeats one or takes one from another session
//! (`scene-order-contiguous`), leaving the order as it was. Two reorders
//! submitted at once queue on the session: neither is rejected, the second
//! waits for its author to confirm it overwrites the first. What the tables
//! make of an order is `reorder_scenes_projection.rs`'s.
//!
//! The scenes are written by a test-only DataCapability, each on its own
//! partition, as a scene's own row holds it: no DataCapability of this
//! branch adds a scene yet (`session.addScene`, #160).

use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use dataguard_server::{Engine, MEMORY, Planted, Rows, planted, router};
use harness_dataguard::api::Submission;
use harness_dataguard::command::{CommandResult, State};
use harness_dataguard::guard::Caller;
use harness_dataguard::manifest::DataCapability;
use serde_json::{Value, json};
use tower::ServiceExt;

/// A scene written for the first time, on its own partition: a stand-in for
/// session.addScene.
const ADD_SCENE: &str = r#"{"dataCapability":"test.addScene","owner":"dataguard","version":1,"effect":"insert","target":{"aggregate":"Session","id":"$sceneId"},"mode":"overwrite","callableBy":["*"]}"#;
/// A session's own row written, its status: what a later write of the
/// session that is no reorder leaves.
const UPDATE_SESSION: &str = r#"{"dataCapability":"test.updateSession","owner":"dataguard","version":1,"effect":"update","target":{"aggregate":"Session","id":"$sessionId"},"mode":"overwrite","callableBy":["*"]}"#;

/// Valombre's session 1, and session 2.
const SEANCE: &str = "51000000-0000-4000-8000-000000000001";
const SEANCE_2: &str = "51000000-0000-4000-8000-000000000002";
/// Session 1's three scenes, ranked 1, 2 and 3.
const A: &str = "5c000000-0000-4000-8000-00000000000a";
const B: &str = "5c000000-0000-4000-8000-00000000000b";
const C: &str = "5c000000-0000-4000-8000-00000000000c";
/// A fourth scene of session 1, added later.
const D: &str = "5c000000-0000-4000-8000-00000000000d";
/// Session 2's scene.
const X: &str = "5c000000-0000-4000-8000-0000000000ff";

/// The engine as the server plants it, with the test-only DataCapabilities,
/// its rows and its wire protocol.
struct Served {
    engine: Arc<Engine>,
    rows: Rows,
    app: Router,
}

impl Served {
    /// Session 1 holding A, B and C, ranked 1 to 3, and session 2 its X.
    async fn new() -> Served {
        let Planted { engine, rows } = planted(MEMORY).await.expect("the in-memory engine");
        let mut engine = engine;
        for manifest in [ADD_SCENE, UPDATE_SESSION] {
            engine =
                engine.with_data_capability(DataCapability::parse(manifest).expect("contract F"));
        }
        let engine = Arc::new(engine);
        let app = router(
            engine.clone(),
            rows.clone(),
            Arc::new(ports_llm::fake::FakeLlm::new()),
        );
        let served = Served { engine, rows, app };
        for (rank, scene) in [A, B, C].into_iter().enumerate() {
            served.add_scene(scene, SEANCE, rank + 1).await;
        }
        served.add_scene(X, SEANCE_2, 1).await;
        served
    }

    /// `scene` written as session `session`'s, at `rank`, straight to the
    /// engine.
    async fn add_scene(&self, scene: &str, session: &str, rank: usize) {
        let result = self
            .straight(
                "test.addScene",
                json!({
                    "sceneId": scene,
                    "sceneSessionId": session,
                    "sceneTitle": format!("Scène {rank}"),
                    "scenePosition": rank,
                }),
            )
            .await;
        assert_eq!(result.status, State::Applied, "{result:?}");
    }

    /// `payload` of `data_capability` submitted straight to the engine by
    /// the DM, who approves the critical fields: what the applier made of
    /// it.
    async fn straight(&self, data_capability: &str, payload: Value) -> CommandResult {
        let Value::Object(payload) = payload else {
            panic!("a payload is an object");
        };
        self.engine
            .submit(Submission {
                data_capability: data_capability.to_string(),
                payload: payload.into_iter().collect(),
                target_id: None,
                based_on: None,
                idempotency_key: None,
                caller: Caller {
                    by: "dm".to_string(),
                    from: "SceneBoard".to_string(),
                    permissions: Vec::new(),
                    roles: vec!["role:approver".to_string()],
                },
            })
            .await
            .expect("queued");
        let mut settled = self.engine.run_once().await.expect("the applier ran");
        assert_eq!(settled.len(), 1, "{settled:?}");
        settled.remove(0)
    }

    async fn post(&self, uri: &str, body: Value) -> (StatusCode, Value) {
        let request = Request::post(uri)
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .expect("a request");
        let response = self.app.clone().oneshot(request).await.expect("an answer");
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("a body");
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    /// `payload` submitted as the SceneBoard sends it, by the DM — no role
    /// of approver: a scene's rank is no critical field — with `key` as its
    /// idempotency key: the answer, its command queued but not applied.
    async fn submit(&self, payload: Value, key: Option<&str>) -> (StatusCode, Value) {
        let mut body = json!({
            "dataCapability": "session.reorderScenes@1",
            "payload": payload,
            "caller": { "by": "dm", "from": "SceneBoard", "roles": [] },
        });
        if let Some(key) = key {
            body["idempotencyKey"] = json!(key);
        }
        self.post("/commands", body).await
    }

    /// `payload` submitted, then one pass of the applier: the answer, and
    /// the commands it settled.
    async fn reorder(&self, payload: Value) -> (StatusCode, Value, Vec<CommandResult>) {
        let (status, answer) = self.submit(payload, None).await;
        let settled = self.engine.run_once().await.expect("the applier ran");
        (status, answer, settled)
    }

    /// The order session `session`'s row holds — as the commands queued on
    /// it will leave it: its scenes' ids, each at its rank less one.
    async fn order(&self, session: &str) -> Value {
        let row = self
            .rows
            .projected(&format!("Session/{session}"))
            .await
            .expect("the row");
        row.get("scenePosition").cloned().unwrap_or(Value::Null)
    }
}

fn rejected_by_order(settled: &[CommandResult]) {
    assert_eq!(settled.len(), 1, "{settled:?}");
    assert_eq!(settled[0].status, State::Rejected, "{settled:?}");
    assert_eq!(
        settled[0].violations,
        ["scene-order-contiguous"],
        "{settled:?}"
    );
}

#[tokio::test]
async fn an_order_c_a_b_ranks_c_first_a_second_b_third_in_a_single_write() {
    let served = Served::new().await;
    let (status, answer, settled) = served
        .reorder(json!({ "session": SEANCE, "order": [C, A, B] }))
        .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{answer}");
    assert_eq!(answer["partition"], format!("Session/{SEANCE}"));
    // One command, applied once: every rank is written by it.
    assert_eq!(settled.len(), 1, "{settled:?}");
    assert_eq!(settled[0].status, State::Applied, "{settled:?}");
    assert_eq!(served.order(SEANCE).await, json!([C, A, B]));
    // Ordered again: the new order is the whole of it.
    let (_, _, settled) = served
        .reorder(json!({ "session": SEANCE, "order": [B, C, A] }))
        .await;
    assert_eq!(settled[0].status, State::Applied, "{settled:?}");
    assert_eq!(served.order(SEANCE).await, json!([B, C, A]));
}

#[tokio::test]
async fn an_order_is_written_under_its_ids_lowercase_whatever_the_case_given() {
    let served = Served::new().await;
    let (_, _, settled) = served
        .reorder(json!({
            "session": SEANCE.to_uppercase(),
            "order": [C.to_uppercase(), A, format!(" {B} ")],
        }))
        .await;
    assert_eq!(settled[0].status, State::Applied, "{settled:?}");
    assert_eq!(served.order(SEANCE).await, json!([C, A, B]));
}

#[tokio::test]
async fn scene_order_contiguous_an_order_omitting_repeating_or_borrowing_a_scene_is_rejected_and_no_rank_changes()
 {
    let served = Served::new().await;
    let (_, _, settled) = served
        .reorder(json!({ "session": SEANCE, "order": [C, A, B] }))
        .await;
    assert_eq!(settled[0].status, State::Applied, "{settled:?}");
    for order in [
        // Omits B.
        json!([A, C]),
        // Repeats A, in place of B.
        json!([A, C, A]),
        // Repeats A, beside every scene.
        json!([A, B, C, A]),
        // Takes session 2's scene, in place of B and beside every scene.
        json!([A, X, C]),
        json!([A, B, C, X]),
        // Names what is no scene.
        json!([A, B, C, "5c000000-0000-4000-8000-000000000999"]),
        json!([A, B, "sc3"]),
    ] {
        let (status, answer, settled) = served
            .reorder(json!({ "session": SEANCE, "order": order }))
            .await;
        assert_eq!(status, StatusCode::ACCEPTED, "{answer}");
        rejected_by_order(&settled);
        assert_eq!(served.order(SEANCE).await, json!([C, A, B]), "{order}");
    }
    // Session 2's own order is no order of session 1's scenes either.
    let (_, _, settled) = served
        .reorder(json!({ "session": SEANCE_2, "order": [A] }))
        .await;
    rejected_by_order(&settled);
    assert_eq!(served.order(SEANCE_2).await, Value::Null);
    // A session that holds no scene has none to order.
    let (_, _, settled) = served
        .reorder(json!({ "session": "51000000-0000-4000-8000-0000000000ee", "order": [A, B, C] }))
        .await;
    rejected_by_order(&settled);
}

#[tokio::test]
async fn an_order_is_judged_against_the_scenes_of_the_session_when_it_is_applied() {
    let served = Served::new().await;
    // Two orders of the three scenes: the second waits for its author behind
    // the first.
    served
        .submit(json!({ "session": SEANCE, "order": [C, A, B] }), None)
        .await;
    let (_, waiting) = served
        .submit(json!({ "session": SEANCE, "order": [B, A, C] }), None)
        .await;
    assert_eq!(
        waiting["result"]["status"], "awaiting_confirmation",
        "{waiting}"
    );
    let settled = served.engine.run_once().await.expect("the applier ran");
    assert_eq!(settled[0].status, State::Applied, "{settled:?}");
    // A fourth scene joins the session before the second is confirmed.
    served.add_scene(D, SEANCE, 4).await;
    let waiting = waiting["commandId"].as_str().expect("a command id");
    served
        .post(
            &format!("/commands/{waiting}/confirm"),
            json!({ "by": "dm" }),
        )
        .await;
    // Applied now, it omits D: three of the four scenes are no order.
    let settled = served.engine.run_once().await.expect("the applier ran");
    rejected_by_order(&settled);
    assert_eq!(served.order(SEANCE).await, json!([C, A, B]));
    // The order of the four is.
    let (_, _, settled) = served
        .reorder(json!({ "session": SEANCE, "order": [D, C, A, B] }))
        .await;
    assert_eq!(settled[0].status, State::Applied, "{settled:?}");
    assert_eq!(served.order(SEANCE).await, json!([D, C, A, B]));
}

#[tokio::test]
async fn a_later_write_of_the_session_is_not_judged_on_the_order_it_holds() {
    let served = Served::new().await;
    let (_, _, settled) = served
        .reorder(json!({ "session": SEANCE, "order": [C, A, B] }))
        .await;
    assert_eq!(settled[0].status, State::Applied, "{settled:?}");
    // D joins the session: the order held omits it now.
    served.add_scene(D, SEANCE, 4).await;
    // A write of the session that is no reorder is not judged on it.
    let written = served
        .straight(
            "test.updateSession",
            json!({ "sessionId": SEANCE, "sessionTitle": "L'arrivée à Valombre" }),
        )
        .await;
    assert_eq!(written.status, State::Applied, "{written:?}");
    assert_eq!(served.order(SEANCE).await, json!([C, A, B]));
}

#[tokio::test]
async fn two_reorders_submitted_at_once_queue_on_the_session_and_neither_is_rejected() {
    let served = Served::new().await;
    let (first_status, first) = served
        .submit(json!({ "session": SEANCE, "order": [C, A, B] }), None)
        .await;
    let (second_status, second) = served
        .submit(json!({ "session": SEANCE, "order": [B, A, C] }), None)
        .await;
    assert_eq!(first_status, StatusCode::ACCEPTED, "{first}");
    assert_eq!(second_status, StatusCode::ACCEPTED, "{second}");
    // Both on the session's partition, one behind the other.
    assert_eq!(first["partition"], second["partition"]);
    assert_eq!(first["result"]["status"], "queued", "{first}");
    // The second overwrites the first: its author confirms it, it is not
    // rejected.
    assert_eq!(
        second["result"]["status"], "awaiting_confirmation",
        "{second}"
    );
    // The first is applied; the second waits for its author.
    let settled = served.engine.run_once().await.expect("the applier ran");
    assert_eq!(settled.len(), 1, "{settled:?}");
    assert_eq!(
        settled[0].command_id,
        first["commandId"].as_str().expect("an id")
    );
    assert_eq!(settled[0].status, State::Applied, "{settled:?}");

    let second = second["commandId"].as_str().expect("a command id");
    let (status, confirmed) = served
        .post(
            &format!("/commands/{second}/confirm"),
            json!({ "by": "dm" }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{confirmed}");
    let settled = served.engine.run_once().await.expect("the applier ran");
    assert_eq!(settled.len(), 1, "{settled:?}");
    assert_eq!(settled[0].command_id, second);
    assert_eq!(settled[0].status, State::Applied, "{settled:?}");
    assert_eq!(served.order(SEANCE).await, json!([B, A, C]));
}

#[tokio::test]
async fn a_reorder_naming_no_session_or_no_order_of_ids_is_refused_and_not_queued() {
    let served = Served::new().await;
    for payload in [
        json!({ "order": [C, A, B] }),
        json!({ "session": "  ", "order": [C, A, B] }),
        json!({ "session": 7, "order": [C, A, B] }),
        json!({ "session": SEANCE }),
        json!({ "session": SEANCE, "order": [] }),
        json!({ "session": SEANCE, "order": C }),
        json!({ "session": SEANCE, "order": [C, 1, B] }),
        json!({ "session": SEANCE, "order": [C, "", B] }),
        json!({ "session": SEANCE, "order": [C, "a\u{0}", B] }),
    ] {
        let (status, answer, settled) = served.reorder(payload.clone()).await;
        assert_eq!(
            status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "{payload}: {answer}"
        );
        assert!(answer["error"].is_string(), "{answer}");
        assert!(settled.is_empty(), "{payload}: {settled:?}");
    }
    assert_eq!(served.order(SEANCE).await, Value::Null);
}

#[tokio::test]
async fn the_same_idempotency_key_twice_is_one_reorder() {
    let served = Served::new().await;
    let (_, first) = served
        .submit(
            json!({ "session": SEANCE, "order": [C, A, B] }),
            Some("drag-1"),
        )
        .await;
    let (status, again) = served
        .submit(
            json!({ "session": SEANCE, "order": [C, A, B] }),
            Some("drag-1"),
        )
        .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{again}");
    assert_eq!(again["commandId"], first["commandId"]);
    assert_eq!(again["replayed"], true, "{again}");
    let settled = served.engine.run_once().await.expect("the applier ran");
    assert_eq!(settled.len(), 1, "{settled:?}");
    assert_eq!(settled[0].status, State::Applied, "{settled:?}");
}
