//! `table.nextTurn` through the wire, over the in-memory engine as `plant()`
//! binds it: the engine's memory holds no table to find an instance's open
//! combat in, so every « Suivant » there is refused before it is queued, as
//! one on an instance with no open combat is — and one naming no instance,
//! before anything is read. The turn itself is judged over PostgreSQL
//! (`next_turn_projection.rs`).

use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use dataguard_server::{Engine, MEMORY, planted, router};
use serde_json::{Value, json};
use tower::ServiceExt;

const INSTANCE: &str = "b1000000-0000-4000-8000-000000000001";

async fn served() -> (Arc<Engine>, Router) {
    let planted = planted(MEMORY).await.expect("the in-memory engine");
    let engine = Arc::new(planted.engine);
    let app = router(
        engine.clone(),
        planted.rows,
        Arc::new(ports_llm::fake::FakeLlm::new()),
    );
    (engine, app)
}

/// `POST /commands` of table.nextTurn with `payload`, on `target` when one
/// is given, by the DM from the battlemap: its status and answer.
async fn command(app: &Router, target: Option<&str>, payload: Value) -> (StatusCode, Value) {
    let mut body = json!({
        "dataCapability": "table.nextTurn@1",
        "payload": payload,
        "caller": { "by": "dm", "from": "table.Battlemap", "roles": ["role:approver"] },
    });
    if let Some(target) = target {
        body["target"] = json!({ "id": target });
    }
    let request = Request::post("/commands")
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
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

#[tokio::test]
async fn next_turn_on_an_instance_with_no_open_combat_is_refused_409_and_nothing_is_queued() {
    let (engine, app) = served().await;
    for (target, payload) in [
        (Some(INSTANCE), json!({})),
        (None, json!({ "instance": INSTANCE })),
    ] {
        let (status, body) = command(&app, target, payload).await;
        assert_eq!(status, StatusCode::CONFLICT, "{body}");
        assert!(
            body["error"].as_str().is_some_and(|e| e.contains("combat")),
            "{body}"
        );
    }
    let ran = engine.run_once().await.expect("the applier ran");
    assert!(ran.is_empty(), "nothing was queued: {ran:?}");
}

#[tokio::test]
async fn next_turn_naming_no_instance_is_refused_422_and_nothing_is_queued() {
    let (engine, app) = served().await;
    for (target, payload) in [
        (None, json!({})),
        (Some("la-taverne"), json!({})),
        (None, json!({ "instance": 12 })),
    ] {
        let (status, body) = command(&app, target, payload).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    }
    let ran = engine.run_once().await.expect("the applier ran");
    assert!(ran.is_empty(), "nothing was queued: {ran:?}");
}
