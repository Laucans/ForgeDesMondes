//! session.addPlaceToSession through the engine, as the server plants its
//! unit and its target aggregate, over the in-memory store: a place of the
//! campaign is queued on the partition of the scene it joins and applied
//! there, after its last element, once; a scene of another session, a place
//! of another campaign or no place at all is refused before it is queued.
//! Its links in the tables and the media they bring are `postgres.rs`'s.

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
const AUTRE: &str = "51000000-0000-4000-8000-00000000000b";
const NOWHERE: &str = "51000000-0000-4000-8000-0000000000ff";
const BOURG: &str = "5c000000-0000-4000-8000-00000000000a";
const GUE: &str = "5c000000-0000-4000-8000-00000000000b";
const TAVERNE: &str = "e1000000-0000-4000-8000-00000000000b";
const MOULIN: &str = "e1000000-0000-4000-8000-00000000000c";
const PHARE: &str = "e1000000-0000-4000-8000-00000000000d";
const MORLOX: &str = "e1000000-0000-4000-8000-00000000000e";

/// A session recorded, test-only: `session.createSession` is a unit of its own.
const ADD_SESSION: &str = r#"{"dataCapability":"test.addSession","owner":"dataguard","version":1,"effect":"insert","target":{"aggregate":"Session","id":"$sessionId"},"mode":"overwrite","callableBy":["*"]}"#;

/// A scene recorded, test-only: `session.addScene` is a unit of its own.
const ADD_SCENE: &str = r#"{"dataCapability":"test.addScene","owner":"dataguard","version":1,"effect":"insert","target":{"aggregate":"Session","id":"$sceneId"},"mode":"overwrite","callableBy":["*"]}"#;

/// An element saved, test-only: `campaign.saveEntity` is another system's.
const SAVE_ELEMENT: &str = r#"{"dataCapability":"test.saveElement","owner":"dataguard","version":1,"effect":"upsert","target":{"aggregate":"Entity","id":"$id"},"mode":"overwrite","callableBy":["*"]}"#;

/// The engine as the server plants this unit and its target aggregate,
/// with Valombre's two sessions — the bourg, a scene of the first linking
/// the mill, and the ford, a scene of the second —, its tavern, its mill
/// and Morlox (an NPC), and Brume's lighthouse.
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
        [aggregate_session::plugin(), add_place_to_session::plugin()]
            .into_iter()
            .collect(),
    );
    for manifest in [
        include_str!("../data-capability.json"),
        ADD_SESSION,
        ADD_SCENE,
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
            "test.addSession",
            json!({ "sessionId": AUTRE, "sessionCampaignId": VALOMBRE, "sessionStatus": "prep" }),
        ),
        (
            "test.addScene",
            json!({ "sceneId": BOURG, "sceneSessionId": SEANCE, "sceneTitle": "Le bourg",
                    "notes": "Il pleut.", "entityId": [MOULIN] }),
        ),
        (
            "test.addScene",
            json!({ "sceneId": GUE, "sceneSessionId": AUTRE, "sceneTitle": "Le gué",
                    "notes": null }),
        ),
        (
            "test.saveElement",
            json!({ "id": TAVERNE, "type": "lieu", "entityCampaignId": VALOMBRE }),
        ),
        (
            "test.saveElement",
            json!({ "id": MOULIN, "type": "lieu", "entityCampaignId": VALOMBRE }),
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
        let submission: Submission = serde_json::from_value(json!({
            "dataCapability": data_capability,
            "payload": payload,
            "caller": { "by": "dm", "from": "test", "roles": ["role:approver"] },
        }))
        .expect("a submission");
        engine.submit(submission).await.expect("queued");
    }
    for result in engine.run_once().await.expect("the applier ran") {
        assert_eq!(result.status, State::Applied, "{result:?}");
    }
    (engine, store)
}

/// A submission by the DM from the Médias page, naming the session as its
/// target as the MediaLibrary does: the command still writes the scene's
/// partition, its manifest's `$sceneId`.
async fn submit(engine: &Engine, data_capability: &str, payload: Value) -> Result<Submitted> {
    let submission: Submission = serde_json::from_value(json!({
        "dataCapability": data_capability,
        "payload": payload,
        "target": { "aggregate": "Session", "id": SEANCE },
        // The critical fields' review is not what these tests judge.
        "caller": { "by": "dm", "from": "MediaLibrary", "roles": ["role:approver"] },
    }))
    .expect("a submission");
    engine.submit(submission).await
}

/// Puts `place` in `scene` and applies it: the scene's row.
async fn linked(engine: &Engine, store: &MemoryStore, scene: &str, place: &str) -> Value {
    let submitted = submit(
        engine,
        "session.addPlaceToSession@1",
        json!({ "session": SEANCE, "scene": scene, "place": place }),
    )
    .await
    .expect("queued");
    assert_eq!(submitted.partition, format!("Session/{scene}"));
    let applied = engine.run_once().await.expect("the applier ran");
    assert_eq!(applied.len(), 1, "{applied:?}");
    assert_eq!(applied[0].status, State::Applied, "{applied:?}");
    let row = store.row(&submitted.partition).await.expect("the row");
    Value::Object(row.into_iter().collect())
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
async fn a_place_joins_the_scene_after_its_last_element_and_again_stays_once() {
    let (engine, store) = valombre().await;
    let bourg = json!({
        "sceneId": BOURG,
        "sceneSessionId": SEANCE,
        "sceneTitle": "Le bourg",
        "notes": "Il pleut.",
        "entityId": [MOULIN, TAVERNE],
    });
    assert_eq!(
        linked(&engine, &store, BOURG, &TAVERNE.to_uppercase()).await,
        bourg
    );
    // Asked again, the tavern keeps its rank: a single link remains.
    assert_eq!(linked(&engine, &store, BOURG, TAVERNE).await, bourg);
    // The session's own row is untouched.
    assert_eq!(
        store
            .row(&format!("Session/{SEANCE}"))
            .await
            .expect("the row"),
        [
            ("sessionCampaignId".to_owned(), json!(VALOMBRE)),
            ("sessionId".to_owned(), json!(SEANCE)),
            ("sessionStatus".to_owned(), json!("prep")),
        ]
        .into_iter()
        .collect()
    );
}

#[tokio::test]
async fn scene_links_same_campaign_refuses_a_scene_of_another_session_or_a_place_of_another_campaign()
 {
    let (engine, _) = valombre().await;
    for (scene, place) in [(GUE, TAVERNE), (BOURG, PHARE), (BOURG, NOWHERE)] {
        let answer = submit(
            &engine,
            "session.addPlaceToSession",
            json!({ "session": SEANCE, "scene": scene, "place": place }),
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
        "session.addPlaceToSession",
        json!({ "session": SEANCE, "scene": BOURG, "place": MORLOX }),
    )
    .await;
    assert_eq!(
        refused(&engine, answer, RefusalKind::Unprocessable).await,
        "place : seul un lieu de la campagne"
    );
}

#[tokio::test]
async fn a_session_or_a_scene_there_is_not_is_not_found_and_nothing_is_queued() {
    let (engine, _) = valombre().await;
    for (payload, said) in [
        (
            json!({ "session": NOWHERE, "scene": BOURG, "place": TAVERNE }),
            "séance introuvable",
        ),
        (
            json!({ "session": SEANCE, "scene": NOWHERE, "place": TAVERNE }),
            "scène introuvable",
        ),
        // A session's own row is no scene.
        (
            json!({ "session": SEANCE, "scene": SEANCE, "place": TAVERNE }),
            "scène introuvable",
        ),
        // No scene named: the tables hold none of this session's here.
        (
            json!({ "session": SEANCE, "place": TAVERNE }),
            "la séance n'a pas encore de scène",
        ),
    ] {
        let answer = submit(&engine, "session.addPlaceToSession", payload).await;
        assert_eq!(refused(&engine, answer, RefusalKind::NotFound).await, said);
    }
    let answer = submit(
        &engine,
        "session.addPlaceToSession",
        json!({ "session": SEANCE, "scene": BOURG }),
    )
    .await;
    assert_eq!(
        refused(&engine, answer, RefusalKind::Unprocessable).await,
        "place : l'identifiant d'un lieu"
    );
}
