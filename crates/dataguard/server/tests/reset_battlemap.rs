//! `table.resetBattlemap` through the wire, over the in-memory engine as
//! `plant()` binds it: what an instance holds is the tables' to tell, and
//! the engine's memory has none — every instance is none to reset, and
//! nothing is queued. What a reset does once the tables hold the instance is
//! `reset_battlemap_projection.rs`'s.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use dataguard_server::{MEMORY, planted, router};
use serde_json::{Value, json};
use tower::ServiceExt;

const INSTANCE: &str = "b1000000-0000-4000-8000-000000000001";

#[tokio::test]
async fn over_the_engine_s_memory_no_instance_is_reset_and_nothing_is_queued() {
    let planted = planted(MEMORY).await.expect("the in-memory engine");
    let engine = std::sync::Arc::new(planted.engine);
    let app = router(
        engine.clone(),
        planted.rows,
        std::sync::Arc::new(ports_llm::fake::FakeLlm::new()),
    );
    for (target, expected, error) in [
        (
            INSTANCE,
            StatusCode::NOT_FOUND,
            "aucune instance de carte de ce nom",
        ),
        (
            "taverne",
            StatusCode::UNPROCESSABLE_ENTITY,
            "instance n'est pas un identifiant",
        ),
    ] {
        let body = json!({
            "dataCapability": "table.resetBattlemap@1",
            "payload": {},
            "target": { "id": target },
            "caller": { "by": "dm", "from": "table.Battlemap", "roles": ["role:approver"] },
        });
        let request = Request::post("/commands")
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap();
        let response = app.clone().oneshot(request).await.expect("an answer");
        assert_eq!(response.status(), expected, "{target}");
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("a body");
        let answer: Value = serde_json::from_slice(&bytes).expect("JSON");
        assert_eq!(answer, json!({ "error": error }), "{target}");
    }
    let ran = engine.run_once().await.expect("the applier ran");
    assert!(ran.is_empty(), "{ran:?}");
}
