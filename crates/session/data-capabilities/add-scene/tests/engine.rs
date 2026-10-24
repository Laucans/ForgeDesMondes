//! session.addScene through the engine, as the server plants its unit and
//! its target aggregate, over the in-memory store: a scene of a session the
//! engine holds is queued on a partition of its own and applied with its
//! places; one of a session there is not, malformed, or linking a place of
//! another campaign is refused before it is queued. Its rank and its
//! tables are `postgres.rs`'s.

use std::sync::Arc;

use harness_dataguard::aggregate::Aggregate;
use harness_dataguard::api::{Submission, Submitted};
use harness_dataguard::command::State;
use harness_dataguard::engine::Engine;
use harness_dataguard::manifest::DataCapability;
use harness_dataguard::memory::{MemoryNotifier, MemoryStore};
use harness_dataguard::plugins::RefusalKind;
use harness_dataguard::postgres::SystemClock;
use harness_dataguard::store::{RandomIds, Store};
use harness_dataguard::{Error, Result};
use serde_json::{Value, json};

const VALOMBRE: &str = "c1000000-0000-4000-8000-000000000001";
const BRUME: &str = "c1000000-0000-4000-8000-000000000002";
const SEANCE: &str = "51000000-0000-4000-8000-00000000000a";
const NOWHERE: &str = "51000000-0000-4000-8000-0000000000ff";
const TAVERNE: &str = "e1000000-0000-4000-8000-00000000000b";
const PHARE: &str = "e1000000-0000-4000-8000-00000000000d";
const MORLOX: &str = "e1000000-0000-4000-8000-00000000000e";

/// A session recorded, test-only: `session.createSession` is a unit of its own.
const ADD_SESSION: &str = r#"{"dataCapability":"test.addSession","owner":"dataguard","version":1,"effect":"insert","target":{"aggregate":"Session","id":"$sessionId"},"mode":"overwrite","callableBy":["*"]}"#;

/// An element saved, test-only: `campaign.saveEntity` is another system's.
const SAVE_ELEMENT: &str = r#"{"dataCapability":"test.saveElement","owner":"dataguard","version":1,"effect":"upsert","target":{"aggregate":"Entity","id":"$id"},"mode":"overwrite","callableBy":["*"]}"#;

/// The engine as the server plants this unit and its target aggregate, with
/// Valombre's session, its tavern and Morlox (an NPC), and Brume's
/// lighthouse.
async fn valombre() -> (Engine, Arc<MemoryStore>) {
    let store = Arc::new(MemoryStore::default());
    let mut engine = Engine::new(
        store.clone(),
        Arc::new(MemoryNotifier::default()),
        Arc::new(SystemClock),
    )
    .with_aggregate(Aggregate::parse(aggregate_session::DOCUMENT).expect("contract I"))
    .with_ids(Arc::new(RandomIds))
    .with_plugins(
        [aggregate_session::plugin(), add_scene::plugin()]
            .into_iter()
            .collect(),
    );
    for manifest in [
        include_str!("../data-capability.json"),
        ADD_SESSION,
        SAVE_ELEMENT,
    ] {
        engine = engine.with_data_capability(DataCapability::parse(manifest).expect("contract F"));
    }
    engine.load().await.expect("the engine loads");
    for (data_capability, payload) in [
        (
            "test.addSession",
            json!({ "sessionId": SEANCE, "sessionCampaignId": VALOMBRE, "sessionStatus": "prep" }),
        ),
        (
            "test.saveElement",
            json!({ "id": TAVERNE, "type": "lieu", "entityCampaignId": VALOMBRE }),
        ),
        (
            "test.saveElement",
            json!({ "id": PHARE, "type": "lieu", "entityCampaignId": BRUME }),
        ),
        (
            "test.saveElement",
            json!({ "id": MORLOX, "type": "pnj", "entityCampaignId": VALOMBRE }),
        ),
    ] {
        submit(&engine, data_capability, payload)
            .await
            .expect("queued");
    }
    for result in engine.run_once().await.expect("the applier ran") {
        assert_eq!(result.status, State::Applied, "{result:?}");
    }
    (engine, store)
}

/// A submission by the DM from the session's preparation.
async fn submit(engine: &Engine, data_capability: &str, payload: Value) -> Result<Submitted> {
    let submission: Submission = serde_json::from_value(json!({
        "dataCapability": data_capability,
        "payload": payload,
        // The critical fields' review is not what these tests judge.
        "caller": { "by": "dm", "from": "session.SceneBoard", "roles": ["role:approver"] },
    }))
    .expect("a submission");
    engine.submit(submission).await
}

/// The refusal `answer` is, of `kind`: its message; nothing was queued.
async fn refused(engine: &Engine, answer: Result<Submitted>, kind: RefusalKind) -> String {
    let message = match answer {
        Err(Error::Refused(refusal)) if refusal.kind == kind => refusal.message,
        other => panic!("a refusal of kind {kind:?}: {other:?}"),
    };
    assert!(
        engine.run_once().await.expect("the applier ran").is_empty(),
        "nothing was queued"
    );
    message
}

#[tokio::test]
async fn a_scene_is_applied_on_a_partition_of_its_own_with_the_places_it_links() {
    let (engine, store) = valombre().await;
    let submitted = submit(
        &engine,
        "session.addScene@1",
        json!({
            "session": SEANCE,
            "title": "L'arrivée",
            "notes": "Il pleut.",
            "places": [TAVERNE.to_uppercase()],
        }),
    )
    .await
    .expect("queued");
    let scene = submitted
        .partition
        .strip_prefix("Session/")
        .expect("a partition of the Session aggregate")
        .to_owned();
    assert_ne!(scene, SEANCE, "a scene is no session's row");
    let applied = engine.run_once().await.expect("the applier ran");
    assert_eq!(applied.len(), 1, "{applied:?}");
    assert_eq!(applied[0].status, State::Applied, "{applied:?}");
    let row = store.row(&submitted.partition).await.expect("the row");
    assert_eq!(
        Value::Object(row.into_iter().collect()),
        json!({
            "sceneId": scene,
            "sceneSessionId": SEANCE,
            "sceneTitle": "L'arrivée",
            "notes": "Il pleut.",
            "entityId": [TAVERNE],
        })
    );
    // The session's own row is untouched.
    assert_eq!(
        store
            .row(&format!("Session/{SEANCE}"))
            .await
            .expect("the row")["sessionStatus"],
        "prep"
    );
}

#[tokio::test]
async fn a_scene_of_a_session_there_is_not_is_not_found_and_nothing_is_queued() {
    let (engine, _) = valombre().await;
    let answer = submit(
        &engine,
        "session.addScene",
        json!({ "session": NOWHERE, "title": "Le gué" }),
    )
    .await;
    assert_eq!(
        refused(&engine, answer, RefusalKind::NotFound).await,
        "séance introuvable"
    );
}

#[tokio::test]
async fn a_place_of_another_campaign_or_a_malformed_scene_is_refused_before_it_is_queued() {
    let (engine, _) = valombre().await;
    for places in [json!([TAVERNE, PHARE]), json!([NOWHERE])] {
        let answer = submit(
            &engine,
            "session.addScene",
            json!({ "session": SEANCE, "title": "Le phare", "places": places }),
        )
        .await;
        let message = refused(&engine, answer, RefusalKind::Unprocessable).await;
        assert!(
            message.starts_with("scene-links-same-campaign"),
            "{message}"
        );
    }
    let answer = submit(
        &engine,
        "session.addScene",
        json!({ "session": SEANCE, "title": "Morlox", "places": [MORLOX] }),
    )
    .await;
    assert_eq!(
        refused(&engine, answer, RefusalKind::Unprocessable).await,
        "places : seuls des lieux de la campagne"
    );
    let answer = submit(
        &engine,
        "session.addScene",
        json!({ "session": SEANCE, "title": " " }),
    )
    .await;
    assert_eq!(
        refused(&engine, answer, RefusalKind::Unprocessable).await,
        "title : une scène a un titre"
    );
}
