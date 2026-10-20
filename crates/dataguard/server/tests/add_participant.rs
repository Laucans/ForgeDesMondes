//! `table.addParticipant` through the wire, over the in-memory engine as
//! `plant()` binds it: what names no uuid, no whole number, or no token of
//! the instance targeted is refused before anything is queued; and over the
//! engine's memory, which has no table to read a combat from, a token of the
//! instance has none running to join. The joining itself — its roll, its
//! place, the moves — is `add_participant_projection.rs`'s, over PostgreSQL.
//! The tokens are written by a test-only DataCapability, as
//! `battlemap_instance_invariants.rs` writes them.

use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use dataguard_server::{Engine, MEMORY, planted, router};
use harness_dataguard::api::Submission;
use harness_dataguard::command::State;
use harness_dataguard::guard::Caller;
use harness_dataguard::manifest::DataCapability;
use serde_json::{Value, json};
use tower::ServiceExt;

const INSTANCE: &str = "b1000000-0000-4000-8000-000000000001";
/// Another scene's instance.
const AUTRE_INSTANCE: &str = "b1000000-0000-4000-8000-000000000002";
const VESNA: &str = "e1000000-0000-4000-8000-000000000004";
const CHEF_BANDIT_1: &str = "70000000-0000-4000-8000-000000000003";
const NO_TOKEN: &str = "70000000-0000-4000-8000-0000000000ff";

/// The engine as the server plants it, with the test-only DataCapability
/// that drops a token, and the wire it is served by.
async fn served() -> (Arc<Engine>, Router) {
    let planted = planted(MEMORY).await.expect("the in-memory engine");
    let manifest = json!({
        "dataCapability": "test.dropToken",
        "owner": "dataguard",
        "version": 1,
        "effect": "insert",
        "target": { "aggregate": "BattlemapInstance", "id": "$tokenId" },
        "mode": "overwrite",
        "callableBy": ["*"],
    });
    let engine =
        Arc::new(planted.engine.with_data_capability(
            DataCapability::parse(&manifest.to_string()).expect("contract F"),
        ));
    let app = router(
        engine.clone(),
        planted.rows,
        Arc::new(ports_llm::fake::FakeLlm::new()),
    );
    (engine, app)
}

/// Chef bandit 1 on the instance, applied.
async fn dropped(engine: &Engine) {
    let token = json!({
        "tokenId": CHEF_BANDIT_1,
        "tokenInstanceId": INSTANCE,
        "characterId": VESNA,
        "name": "Chef bandit 1",
        "q": 1,
        "r": 1,
        "visible": true,
        "conditions": [],
    });
    let Value::Object(payload) = token else {
        unreachable!()
    };
    engine
        .submit(Submission {
            data_capability: "test.dropToken".to_string(),
            payload: payload.into_iter().collect(),
            target_id: None,
            based_on: None,
            idempotency_key: None,
            caller: Caller {
                by: "dm".to_string(),
                from: "table.battlemap".to_string(),
                permissions: Vec::new(),
                roles: vec!["role:approver".to_string()],
            },
        })
        .await
        .expect("queued");
    let results = engine.run_once().await.expect("the applier ran");
    assert!(results.iter().all(|result| result.status == State::Applied));
}

/// `POST /commands` of table.addParticipant with `payload`, on `target`
/// when one is given, by the DM: its status and answer.
async fn command(app: &Router, target: Option<&str>, payload: Value) -> (StatusCode, Value) {
    let mut body = json!({
        "dataCapability": "table.addParticipant@1",
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
async fn what_names_no_uuid_no_whole_number_or_no_token_of_the_instance_is_refused() {
    let (engine, app) = served().await;
    dropped(&engine).await;
    for (target, payload, expected) in [
        (
            Some(INSTANCE),
            json!({ "token": "chef" }),
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
        (
            Some("taverne"),
            json!({ "token": CHEF_BANDIT_1 }),
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
        (None, json!({}), StatusCode::UNPROCESSABLE_ENTITY),
        (
            Some(INSTANCE),
            json!({ "token": CHEF_BANDIT_1, "initiative": "15" }),
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
        (
            Some(INSTANCE),
            json!({ "token": NO_TOKEN }),
            StatusCode::NOT_FOUND,
        ),
        (
            Some(AUTRE_INSTANCE),
            json!({ "token": CHEF_BANDIT_1 }),
            StatusCode::NOT_FOUND,
        ),
        // Over memory, no table to read a combat from: none running.
        (
            Some(INSTANCE),
            json!({ "token": CHEF_BANDIT_1 }),
            StatusCode::CONFLICT,
        ),
        (Some(CHEF_BANDIT_1), json!({}), StatusCode::CONFLICT),
    ] {
        let (status, body) = command(&app, target, payload.clone()).await;
        assert_eq!(status, expected, "{target:?} {payload}: {body}");
        assert!(body["error"].is_string(), "{body}");
    }
    assert!(engine.run_once().await.expect("the applier ran").is_empty());
}
