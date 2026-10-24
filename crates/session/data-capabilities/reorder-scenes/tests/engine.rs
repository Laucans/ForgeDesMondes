//! session.reorderScenes through the engine, over the in-memory store: the
//! session and its scenes are rows the engine holds (test-only
//! DataCapabilities write them), and the order is written on the session's
//! scene order, a partition of its own. A session there is not is refused,
//! 404; a malformed order, or one that repeats a scene or names another
//! session's, 422 — neither is queued. Two reorders of one session queue there:
//! the second waits for its author to confirm it overwrites the first, and
//! neither is rejected.

use std::sync::Arc;

use aggregate_session::order_id;
use harness_dataguard::Error;
use harness_dataguard::aggregate::Aggregate;
use harness_dataguard::api::{Submission, Submitted};
use harness_dataguard::command::State;
use harness_dataguard::engine::Engine;
use harness_dataguard::manifest::DataCapability;
use harness_dataguard::memory::{MemoryNotifier, MemoryStore};
use harness_dataguard::plugins::RefusalKind;
use harness_dataguard::postgres::SystemClock;
use harness_dataguard::store::Store;
use serde_json::{Value, json};

/// A session, or a scene, written as the session's DataCapabilities write
/// them.
const CREATE_SESSION: &str = r#"{"dataCapability":"test.createSession","owner":"dataguard","version":1,"effect":"insert","target":{"aggregate":"Session","id":"$sessionId"},"mode":"overwrite","callableBy":["*"]}"#;
const ADD_SCENE: &str = r#"{"dataCapability":"test.addScene","owner":"dataguard","version":1,"effect":"insert","target":{"aggregate":"Session","id":"$sceneId"},"mode":"overwrite","callableBy":["*"]}"#;

const VALOMBRE: &str = "c1000000-0000-4000-8000-000000000001";
const SEANCE: &str = "51000000-0000-4000-8000-000000000004";
const AUTRE: &str = "51000000-0000-4000-8000-000000000005";
const A: &str = "5c000000-0000-4000-8000-00000000000a";
const B: &str = "5c000000-0000-4000-8000-00000000000b";
const C: &str = "5c000000-0000-4000-8000-00000000000c";
/// Session 5's scene.
const X: &str = "5c000000-0000-4000-8000-0000000000ff";

/// The engine as the server plants this unit and its target aggregate,
/// over memory, with session 4 and its scenes A, B, C, and session 5 and
/// its scene X.
async fn engine() -> (Engine, Arc<MemoryStore>) {
    let store = Arc::new(MemoryStore::default());
    let mut engine = Engine::new(
        store.clone(),
        Arc::new(MemoryNotifier::default()),
        Arc::new(SystemClock),
    )
    .with_aggregate(Aggregate::parse(aggregate_session::DOCUMENT).expect("contract I"))
    .with_plugins(
        [aggregate_session::plugin(), reorder_scenes::plugin()]
            .into_iter()
            .collect(),
    );
    for manifest in [
        include_str!("../data-capability.json"),
        CREATE_SESSION,
        ADD_SCENE,
    ] {
        engine = engine.with_data_capability(DataCapability::parse(manifest).expect("contract F"));
    }
    engine.load().await.expect("the engine loads");
    for session in [SEANCE, AUTRE] {
        write(
            &engine,
            "test.createSession@1",
            json!({ "sessionId": session, "sessionCampaignId": VALOMBRE }),
        )
        .await;
    }
    for (scene, session) in [(A, SEANCE), (B, SEANCE), (C, SEANCE), (X, AUTRE)] {
        write(
            &engine,
            "test.addScene@1",
            json!({ "sceneId": scene, "sceneSessionId": session, "sceneTitle": scene }),
        )
        .await;
    }
    assert_eq!(engine.run_once().await.expect("applied").len(), 6);
    (engine, store)
}

fn submission(data_capability: &str, payload: Value, roles: &[&str]) -> Submission {
    serde_json::from_value(json!({
        "dataCapability": data_capability,
        "payload": payload,
        "caller": { "by": "dm", "from": "session.SceneBoard", "roles": roles },
    }))
    .expect("a submission")
}

/// A test-only row written, queued.
async fn write(engine: &Engine, data_capability: &str, payload: Value) {
    engine
        .submit(submission(data_capability, payload, &["role:approver"]))
        .await
        .expect("queued");
}

/// A reorder, as the session's preparation sends it: by the DM, who holds
/// no approver's role.
async fn reorder(engine: &Engine, payload: Value) -> harness_dataguard::Result<Submitted> {
    engine
        .submit(submission("session.reorderScenes@1", payload, &[]))
        .await
}

/// The partition session 4's order is written on.
fn order_partition() -> String {
    format!("Session/{}", order_id(SEANCE).expect("a uuid"))
}

/// The refusal of `payload`, of `kind`, nothing queued: why.
async fn refused(engine: &Engine, payload: Value, kind: RefusalKind) -> String {
    let refusal = reorder(engine, payload.clone()).await;
    let Err(Error::Refused(refusal)) = refusal else {
        panic!("{payload} refused: {refusal:?}");
    };
    assert_eq!(refusal.kind, kind, "{payload}: {refusal:?}");
    assert!(
        engine.run_once().await.expect("the applier ran").is_empty(),
        "{payload}: nothing was queued"
    );
    refusal.message
}

#[tokio::test]
async fn a_reorder_is_written_whole_on_the_session_s_order_by_a_dm_with_no_review() {
    let (engine, store) = engine().await;
    let submitted = reorder(&engine, json!({ "session": SEANCE, "order": [C, A, B] }))
        .await
        .expect("queued");
    assert_eq!(submitted.partition, order_partition());
    assert_eq!(submitted.result.status, State::Queued, "{submitted:?}");
    assert!(submitted.warnings.is_empty(), "{:?}", submitted.warnings);
    let applied = engine.run_once().await.expect("applied");
    assert_eq!(applied.len(), 1, "one write: {applied:?}");
    assert_eq!(applied[0].status, State::Applied, "{applied:?}");
    assert_eq!(
        store.row(&order_partition()).await.expect("the order"),
        [("scenePosition".to_owned(), json!([C, A, B]))]
            .into_iter()
            .collect()
    );
    // The session's own row is left as it was.
    assert!(
        !store
            .row(&format!("Session/{SEANCE}"))
            .await
            .expect("the session")
            .contains_key("scenePosition")
    );
}

#[tokio::test]
async fn a_scene_still_queued_is_one_of_its_session_s_by_its_row() {
    let (engine, store) = engine().await;
    const D: &str = "5c000000-0000-4000-8000-00000000000d";
    write(
        &engine,
        "test.addScene@1",
        json!({ "sceneId": D, "sceneSessionId": SEANCE, "sceneTitle": "D" }),
    )
    .await;
    reorder(&engine, json!({ "session": SEANCE, "order": [D, C, A, B] }))
        .await
        .expect("queued behind D");
    assert_eq!(engine.run_once().await.expect("applied").len(), 2);
    assert_eq!(
        store.row(&order_partition()).await.expect("the order")["scenePosition"],
        json!([D, C, A, B])
    );
}

#[tokio::test]
async fn a_reorder_of_a_session_there_is_not_is_not_found() {
    let (engine, _) = engine().await;
    const NOWHERE: &str = "51000000-0000-4000-8000-000000000009";
    for payload in [
        json!({ "session": NOWHERE, "order": [A, B, C] }),
        json!({ "session": "seance-4", "order": [A, B, C] }),
    ] {
        refused(&engine, payload, RefusalKind::NotFound).await;
    }
}

#[tokio::test]
async fn an_order_repeating_a_scene_or_naming_another_session_s_is_refused() {
    let (engine, store) = engine().await;
    // Over memory no table lists the session's scenes: an order omitting
    // one is judged over PostgreSQL (tests/postgres.rs).
    for order in [json!([C, A, B, A]), json!([C, A, B, X]), json!([X])] {
        let why = refused(
            &engine,
            json!({ "session": SEANCE, "order": order }),
            RefusalKind::Unprocessable,
        )
        .await;
        assert!(why.starts_with("scene-order-contiguous : "), "{why}");
    }
    for payload in [
        json!({ "session": SEANCE, "order": [] }),
        json!({ "session": SEANCE, "order": [C, A, B], "title": "Ordre" }),
    ] {
        refused(&engine, payload, RefusalKind::Unprocessable).await;
    }
    assert!(
        store
            .row(&order_partition())
            .await
            .expect("none")
            .is_empty()
    );
}

#[tokio::test]
async fn two_reorders_at_once_queue_on_the_session_s_order_and_neither_is_rejected() {
    let (engine, store) = engine().await;
    let first = reorder(&engine, json!({ "session": SEANCE, "order": [C, A, B] }))
        .await
        .expect("queued");
    let second = reorder(&engine, json!({ "session": SEANCE, "order": [B, C, A] }))
        .await
        .expect("queued behind the first");
    assert_eq!(first.result.status, State::Queued);
    assert_eq!(second.partition, first.partition);
    assert_eq!(
        second.result.status,
        State::AwaitingConfirmation,
        "{second:?}"
    );
    assert!(
        second
            .warnings
            .iter()
            .any(|warning| warning.contains("confirm")),
        "{:?}",
        second.warnings
    );
    engine
        .confirm(&second.command_id, "dm")
        .await
        .expect("its author confirms the overwrite");
    let mut applied = Vec::new();
    for _ in 0..3 {
        applied.extend(engine.run_once().await.expect("the applier ran"));
    }
    assert_eq!(applied.len(), 2, "{applied:?}");
    assert!(
        applied.iter().all(|result| result.status == State::Applied),
        "{applied:?}"
    );
    assert_eq!(applied[0].command_id, first.command_id);
    assert_eq!(
        store.row(&order_partition()).await.expect("the order")["scenePosition"],
        json!([B, C, A])
    );
}
