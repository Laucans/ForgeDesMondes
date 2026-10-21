//! The Session aggregate's invariants, judged by the DataGuard as `plant()`
//! binds it, over the in-memory store: a command that takes a session back
//! in its lifecycle, closes it with its microphone on, adds a journal line
//! once it is closed, ranks a scene below 1, stores the roster or the media
//! of a session, creates an element from a brief, marks an entry « creee »
//! without its element, or writes audio is rejected under the invariant's
//! id, and the row stays as it was. The DataCapabilities here are test-only:
//! no DataCapability of the product writes a session yet.

use std::sync::Arc;

use harness_dataguard::aggregate::Aggregate;
use harness_dataguard::api::Submission;
use harness_dataguard::command::{CommandResult, State};
use harness_dataguard::engine::Engine;
use harness_dataguard::guard::Caller;
use harness_dataguard::manifest::DataCapability;
use harness_dataguard::memory::{MemoryNotifier, MemoryStore};
use harness_dataguard::postgres::SystemClock;
use serde_json::{Value, json};

/// A session, or an entry of its « à créer » list, written for the first
/// time.
const CREATE: &str = r#"{"dataCapability":"test.createSession","owner":"dataguard","version":1,"effect":"insert","target":{"aggregate":"Session","id":"$sessionId"},"mode":"overwrite","callableBy":["*"]}"#;
const ADD_TO_CREATE: &str = r#"{"dataCapability":"test.addToCreate","owner":"dataguard","version":1,"effect":"insert","target":{"aggregate":"Session","id":"$entityToCreateId"},"mode":"overwrite","callableBy":["*"]}"#;
/// An entry written by the analysis of a brief, or written again.
const UPSERT_TO_CREATE: &str = r#"{"dataCapability":"test.upsertToCreate","owner":"dataguard","version":1,"effect":"upsert","target":{"aggregate":"Session","id":"$entityToCreateId"},"mode":"overwrite","callableBy":["*"]}"#;
/// A session written: its status, its scenes, its presences, its journal.
const UPDATE: &str = r#"{"dataCapability":"test.updateSession","owner":"dataguard","version":1,"effect":"update","target":{"aggregate":"Session","id":"$sessionId"},"mode":"overwrite","callableBy":["*"]}"#;
const UPDATE_TO_CREATE: &str = r#"{"dataCapability":"test.updateToCreate","owner":"dataguard","version":1,"effect":"update","target":{"aggregate":"Session","id":"$entityToCreateId"},"mode":"overwrite","callableBy":["*"]}"#;
/// A session removed: the engine's soft delete, a tombstone.
const REMOVE: &str = r#"{"dataCapability":"test.removeSession","owner":"dataguard","version":1,"effect":"delete","target":{"aggregate":"Session","id":"$sessionId"},"mode":"overwrite","callableBy":["*"]}"#;

const VALOMBRE: &str = "c1000000-0000-4000-8000-000000000001";
/// Valombre's session 1.
const SEANCE: &str = "51000000-0000-4000-8000-000000000001";
/// Its tavern scene.
const ARRIVEE: &str = "5c000000-0000-4000-8000-000000000001";
/// Valombre's PC.
const ELOWEN: &str = "e1000000-0000-4000-8000-000000000001";
/// Valombre's battlemap, at the Taverne.
const CARTE: &str = "d1000000-0000-4000-8000-000000000001";
/// Valombre's place.
const TAVERNE: &str = "e1000000-0000-4000-8000-000000000003";
const BRIEF: &str = "b0000000-0000-4000-8000-000000000001";
/// Grimbald, unknown to the campaign: an entry « à créer ».
const GRIMBALD: &str = "ac000000-0000-4000-8000-000000000001";
/// The element created for Grimbald.
const CREATED: &str = "e1000000-0000-4000-8000-0000000000aa";
const JOURNAL_LINE: &str = "1a000000-0000-4000-8000-000000000001";

/// The engine as the server plants this aggregate — its manifest and its
/// plugin — over the in-memory store.
async fn plant() -> harness_dataguard::Result<Engine> {
    let engine = Engine::new(
        Arc::new(MemoryStore::default()),
        Arc::new(MemoryNotifier::default()),
        Arc::new(SystemClock),
    )
    .with_aggregate(Aggregate::parse(include_str!("../aggregate.json")).expect("contract I"))
    .with_plugins(std::iter::once(aggregate_session::plugin()).collect());
    engine.load().await?;
    Ok(engine)
}

/// The engine as the server plants it, with the test-only DataCapabilities.
async fn engine() -> Engine {
    let mut engine = plant().await.expect("the in-memory engine");
    for manifest in [
        CREATE,
        ADD_TO_CREATE,
        UPSERT_TO_CREATE,
        UPDATE,
        UPDATE_TO_CREATE,
        REMOVE,
    ] {
        engine = engine.with_data_capability(DataCapability::parse(manifest).expect("contract F"));
    }
    engine
}

/// The submission of one command, which the engine refuses before it is
/// queued: why.
async fn refused(engine: &Engine, data_capability: &str, payload: Value) -> String {
    let Value::Object(payload) = payload else {
        panic!("a payload is an object");
    };
    let refusal = engine
        .submit(Submission {
            data_capability: data_capability.to_string(),
            payload: payload.into_iter().collect(),
            caller: Caller {
                by: "dm".to_string(),
                from: "session.prep".to_string(),
                permissions: Vec::new(),
                roles: vec!["role:approver".to_string()],
            },
            ..Submission::default()
        })
        .await
        .expect_err("refused");
    assert!(
        engine.run_once().await.expect("the applier ran").is_empty(),
        "nothing was queued"
    );
    refusal.to_string()
}

/// What the single applier made of one command.
async fn written(engine: &Engine, data_capability: &str, payload: Value) -> CommandResult {
    let Value::Object(payload) = payload else {
        panic!("a payload is an object");
    };
    engine
        .submit(Submission {
            data_capability: data_capability.to_string(),
            payload: payload.into_iter().collect(),
            target_id: None,
            based_on: None,
            idempotency_key: None,
            caller: Caller {
                by: "dm".to_string(),
                from: "session.prep".to_string(),
                permissions: Vec::new(),
                // The critical fields' review is not what these tests judge.
                roles: vec!["role:approver".to_string()],
            },
        })
        .await
        .expect("queued");
    let mut applied = engine.run_once().await.expect("the applier ran");
    assert_eq!(applied.len(), 1, "{applied:?}");
    applied.remove(0)
}

fn rejected_by(result: &CommandResult, ids: &[&str]) {
    assert_eq!(result.status, State::Rejected, "{result:?}");
    assert_eq!(result.violations, ids, "{result:?}");
}

fn applied(result: &CommandResult) {
    assert_eq!(result.status, State::Applied, "{result:?}");
}

/// Valombre's session 1, in preparation.
fn seance() -> Value {
    json!({
        "sessionId": SEANCE,
        "sessionCampaignId": VALOMBRE,
        "sessionPosition": 1,
        "sessionTitle": "L'arrivée à Valombre",
        "sessionStatus": "prep",
        "microphoneOn": false,
    })
}

/// An engine holding session 1, taken to `status` the way a session goes.
async fn session(status: &str) -> Engine {
    let engine = engine().await;
    applied(&written(&engine, "test.createSession", seance()).await);
    if status != "prep" {
        applied(
            &written(
                &engine,
                "test.updateSession",
                json!({ "sessionId": SEANCE, "sessionStatus": "live", "microphoneOn": true }),
            )
            .await,
        );
    }
    if status == "closed" {
        applied(&written(&engine, "test.updateSession", closing()).await);
    }
    engine
}

/// The close of session 1: microphone off, dated, its summary recorded with
/// the summary model.
fn closing() -> Value {
    json!({
        "sessionId": SEANCE,
        "sessionStatus": "closed",
        "microphoneOn": false,
        "closedAt": "2026-10-18T23:00:00Z",
        "summary": "Les héros arrivent à Valombre et rencontrent Morlox.",
        "summaryModel": "claude-haiku-4-5-20251001",
    })
}

/// A line of session 1's journal, transcribed text only.
fn journal_line() -> Value {
    json!({
        "sessionId": SEANCE,
        "sessionJournalLineId": JOURNAL_LINE,
        "sessionJournalLineSessionId": SEANCE,
        "spokenAt": "2026-10-18T21:00:00Z",
        "text": "Morlox vous tend la carte.",
    })
}

/// Grimbald, as the analysis of the brief lists him: « à créer ».
fn grimbald() -> Value {
    json!({
        "entityToCreateId": GRIMBALD,
        "entityToCreateCampaignId": VALOMBRE,
        "entityToCreateSessionId": SEANCE,
        "briefId": BRIEF,
        "type": "pnj",
        "name": "Grimbald",
        "excerpt": "Morlox a vendu la carte à Grimbald",
        "entityToCreateStatus": "a_creer",
    })
}

fn with(mut row: Value, field: &str, value: Value) -> Value {
    row[field] = value;
    row
}

#[tokio::test]
async fn status_lifecycle_goes_from_prep_to_live_to_closed_and_never_back() {
    let engine = session("prep").await;
    applied(
        &written(
            &engine,
            "test.updateSession",
            json!({ "sessionId": SEANCE, "sessionStatus": "live", "microphoneOn": true }),
        )
        .await,
    );
    rejected_by(
        &written(
            &engine,
            "test.updateSession",
            json!({ "sessionId": SEANCE, "sessionStatus": "prep" }),
        )
        .await,
        &["status-lifecycle"],
    );
    applied(&written(&engine, "test.updateSession", closing()).await);
    for back in ["live", "prep"] {
        rejected_by(
            &written(
                &engine,
                "test.updateSession",
                json!({ "sessionId": SEANCE, "sessionStatus": back }),
            )
            .await,
            &["status-lifecycle"],
        );
    }
    // A closed session stays closed: its summary edited, it is still closed.
    applied(
        &written(
            &engine,
            "test.updateSession",
            json!({
                "sessionId": SEANCE,
                "sessionStatus": "closed",
                "summary": "Les héros arrivent à Valombre.",
                "summaryEditedAt": "2026-10-19T09:00:00Z",
            }),
        )
        .await,
    );
}

#[tokio::test]
async fn status_lifecycle_rejects_skipping_live_an_unknown_status_and_none() {
    let prep = session("prep").await;
    for status in [json!("closed"), json!("paused"), json!(2), Value::Null] {
        let mut close = closing();
        close["sessionStatus"] = status;
        rejected_by(
            &written(&prep, "test.updateSession", close).await,
            &["status-lifecycle"],
        );
    }
    // Nor is a session written with a status outside the lifecycle.
    let fresh = engine().await;
    rejected_by(
        &written(
            &fresh,
            "test.createSession",
            with(seance(), "sessionStatus", json!("brouillon")),
        )
        .await,
        &["status-lifecycle"],
    );
}

#[tokio::test]
async fn status_lifecycle_rejects_a_journal_line_once_the_session_is_closed() {
    let engine = session("live").await;
    applied(&written(&engine, "test.updateSession", journal_line()).await);
    applied(&written(&engine, "test.updateSession", closing()).await);
    rejected_by(
        &written(
            &engine,
            "test.updateSession",
            with(journal_line(), "text", json!("Une phrase de trop.")),
        )
        .await,
        &["status-lifecycle"],
    );
    for field in ["spokenAt", "sessionJournalLineSceneId"] {
        rejected_by(
            &written(
                &engine,
                "test.updateSession",
                json!({ "sessionId": SEANCE, field: ARRIVEE }),
            )
            .await,
            &["status-lifecycle"],
        );
    }
}

#[tokio::test]
async fn close_session_turns_the_microphone_off_dates_the_close_and_names_the_summary_model() {
    for (field, value) in [
        ("microphoneOn", json!(true)),
        ("microphoneOn", Value::Null),
        ("closedAt", Value::Null),
        ("summaryModel", Value::Null),
        ("summaryModel", json!("  ")),
    ] {
        let engine = session("live").await;
        rejected_by(
            &written(&engine, "test.updateSession", with(closing(), field, value)).await,
            &["close-session"],
        );
        // The session is still live: closed the way the rule says, it closes.
        applied(&written(&engine, "test.updateSession", closing()).await);
    }
    // A session closed with no summary yet has no model to name.
    let engine = session("live").await;
    applied(
        &written(
            &engine,
            "test.updateSession",
            json!({
                "sessionId": SEANCE,
                "sessionStatus": "closed",
                "microphoneOn": false,
                "closedAt": "2026-10-18T23:00:00Z",
            }),
        )
        .await,
    );
    // Once closed, the microphone stays off.
    rejected_by(
        &written(
            &engine,
            "test.updateSession",
            json!({ "sessionId": SEANCE, "microphoneOn": true }),
        )
        .await,
        &["close-session"],
    );
}

#[tokio::test]
async fn close_session_leaves_a_live_session_s_microphone_to_the_dm() {
    let engine = session("live").await;
    for on in [false, true, false] {
        applied(
            &written(
                &engine,
                "test.updateSession",
                json!({ "sessionId": SEANCE, "microphoneOn": on }),
            )
            .await,
        );
    }
}

#[tokio::test]
async fn scene_order_contiguous_ranks_a_scene_from_one() {
    let engine = session("prep").await;
    for rank in [json!(0), json!(-1), json!(1.5), json!("2")] {
        rejected_by(
            &written(
                &engine,
                "test.updateSession",
                json!({ "sessionId": SEANCE, "sceneId": ARRIVEE, "sceneSessionId": SEANCE, "scenePosition": rank }),
            )
            .await,
            &["scene-order-contiguous"],
        );
    }
    for rank in [1, 3] {
        applied(
            &written(
                &engine,
                "test.updateSession",
                json!({ "sessionId": SEANCE, "sceneId": ARRIVEE, "sceneSessionId": SEANCE, "sceneTitle": "La taverne", "scenePosition": rank }),
            )
            .await,
        );
    }
}

#[tokio::test]
async fn present_only_pj_offers_the_roster_the_view_computes_never_a_stored_one() {
    let engine = session("prep").await;
    // A PC checked present is a presence, which the roster reads.
    applied(
        &written(
            &engine,
            "test.updateSession",
            json!({
                "sessionId": SEANCE,
                "sessionPresentCharacterSessionId": SEANCE,
                "sessionPresentCharacterCharacterId": ELOWEN,
            }),
        )
        .await,
    );
    // present-only-pj: the roster's fields are derived, so the engine
    // refuses a command writing one before it is queued.
    for field in [
        "sessionRosterViewSessionId",
        "sessionRosterViewCharacterId",
        "via",
    ] {
        let why = refused(
            &engine,
            "test.updateSession",
            json!({ "sessionId": SEANCE, field: ELOWEN }),
        )
        .await;
        assert!(why.contains(field), "{why}");
    }
}

#[tokio::test]
async fn session_media_by_place_never_stores_the_media_of_a_session() {
    let engine = session("prep").await;
    // A media linked to a scene, and a place a scene links: the view
    // associates their media.
    applied(
        &written(
            &engine,
            "test.updateSession",
            json!({
                "sessionId": SEANCE,
                "sceneMediaSceneId": ARRIVEE,
                "sceneMediaMediaId": CARTE,
                "sceneEntitySceneId": ARRIVEE,
                "entityId": TAVERNE,
            }),
        )
        .await,
    );
    // session-media-by-place: the view's fields are derived, so the engine
    // refuses a command writing one before it is queued.
    for field in [
        "sessionMediaViewSessionId",
        "sessionMediaViewMediaId",
        "viaPlaceId",
    ] {
        let why = refused(
            &engine,
            "test.updateSession",
            json!({ "sessionId": SEANCE, field: TAVERNE }),
        )
        .await;
        assert!(why.contains(field), "{why}");
    }
}

#[tokio::test]
async fn brief_no_auto_creation_lists_an_unknown_element_a_creer_with_its_type_and_excerpt() {
    let engine = engine().await;
    for (field, value) in [
        ("entityToCreateStatus", json!("creee")),
        ("type", json!("pj")),
        ("type", Value::Null),
        ("excerpt", json!("")),
        ("excerpt", Value::Null),
    ] {
        let mut created = with(grimbald(), field, value);
        if field == "entityToCreateStatus" {
            created["createdEntityId"] = json!(CREATED);
        }
        rejected_by(
            &written(&engine, "test.addToCreate", created.clone()).await,
            &["brief-no-auto-creation"],
        );
        // Nor by an upsert of an entry not written yet.
        rejected_by(
            &written(&engine, "test.upsertToCreate", created).await,
            &["brief-no-auto-creation"],
        );
    }
    // An element named « à créer » is created already: both rules refuse it.
    rejected_by(
        &written(
            &engine,
            "test.addToCreate",
            with(grimbald(), "createdEntityId", json!(CREATED)),
        )
        .await,
        &["brief-no-auto-creation", "to-create-leaves-once-created"],
    );
    // Nothing was written: the analysis can still list Grimbald.
    applied(&written(&engine, "test.addToCreate", grimbald()).await);
}

#[tokio::test]
async fn brief_no_auto_creation_leaves_an_entry_written_again_to_its_creation() {
    // Grimbald listed, then written again whole once the DM created him: an
    // upsert of an entry the engine holds is no analysis creating an element.
    let engine = engine().await;
    applied(&written(&engine, "test.addToCreate", grimbald()).await);
    let mut created = with(grimbald(), "entityToCreateStatus", json!("creee"));
    created["createdEntityId"] = json!(CREATED);
    applied(&written(&engine, "test.upsertToCreate", created).await);
}

#[tokio::test]
async fn to_create_leaves_once_created_if_and_only_if_its_element_is_named() {
    let engine = engine().await;
    applied(&written(&engine, "test.addToCreate", grimbald()).await);
    for change in [
        json!({ "entityToCreateId": GRIMBALD, "entityToCreateStatus": "creee" }),
        json!({ "entityToCreateId": GRIMBALD, "createdEntityId": CREATED }),
        json!({ "entityToCreateId": GRIMBALD, "entityToCreateStatus": "ignoree" }),
    ] {
        rejected_by(
            &written(&engine, "test.updateToCreate", change).await,
            &["to-create-leaves-once-created"],
        );
    }
    // Refined first, then created: its element named with its status.
    applied(
        &written(
            &engine,
            "test.updateToCreate",
            json!({ "entityToCreateId": GRIMBALD, "refinementText": "Un nain, marchand de cartes." }),
        )
        .await,
    );
    applied(
        &written(
            &engine,
            "test.updateToCreate",
            json!({ "entityToCreateId": GRIMBALD, "entityToCreateStatus": "creee", "createdEntityId": CREATED }),
        )
        .await,
    );
    for change in [
        json!({ "entityToCreateId": GRIMBALD, "entityToCreateStatus": "a_creer" }),
        json!({ "entityToCreateId": GRIMBALD, "createdEntityId": Value::Null }),
    ] {
        rejected_by(
            &written(&engine, "test.updateToCreate", change).await,
            &["to-create-leaves-once-created"],
        );
    }
}

#[tokio::test]
async fn text_only_no_audio_rejects_audio_or_anything_but_a_column_s_value() {
    let engine = session("live").await;
    for payload in [
        with(journal_line(), "audio", json!("UklGRiQAAABXQVZF")),
        with(journal_line(), "text", json!({ "bytes": [82, 73, 70, 70] })),
        json!({
            "sessionId": SEANCE,
            "oralBriefId": BRIEF,
            "oralBriefSessionId": SEANCE,
            "transcript": [82, 73, 70, 70],
            "durationSeconds": 42,
            "recordedAt": "2026-10-18T20:00:00Z",
        }),
    ] {
        rejected_by(
            &written(&engine, "test.updateSession", payload).await,
            &["text-only-no-audio"],
        );
    }
    // The brief as its transcribed text: kept.
    applied(
        &written(
            &engine,
            "test.updateSession",
            json!({
                "sessionId": SEANCE,
                "oralBriefId": BRIEF,
                "oralBriefSessionId": SEANCE,
                "transcript": "Ils arrivent à la taverne, Morlox les attend.",
                "durationSeconds": 42,
                "recordedAt": "2026-10-18T20:00:00Z",
            }),
        )
        .await,
    );
}

/// The tavern scene of session 1, linking `links` under `entityId`.
fn arrivee(links: Value) -> Value {
    json!({
        "sceneId": ARRIVEE,
        "sceneSessionId": SEANCE,
        "sceneTitle": "L'arrivée",
        "entityId": links,
    })
}

#[tokio::test]
async fn text_only_no_audio_admits_one_list_the_ids_a_scene_links() {
    let engine = session("prep").await;
    for payload in [
        // A list of anything but ids, even under `entityId`: a text, hex
        // digits too few for a uuid, a number, a list.
        arrivee(json!(["UklGRiQAAABXQVZF"])),
        arrivee(json!(["e1000000"])),
        arrivee(json!([42])),
        arrivee(json!([[TAVERNE]])),
        // A list of ids under any other field.
        with(arrivee(json!([])), "sceneTitle", json!([TAVERNE])),
    ] {
        rejected_by(
            &written(&engine, "test.updateSession", payload).await,
            &["text-only-no-audio"],
        );
    }
    // The ids of the elements it links, in either case: kept.
    applied(
        &written(
            &engine,
            "test.updateSession",
            arrivee(json!([TAVERNE, ELOWEN.to_uppercase()])),
        )
        .await,
    );
}

#[tokio::test]
async fn the_session_invariants_leave_a_session_s_removal_to_the_tombstone() {
    let engine = session("closed").await;
    // Even a removal that writes what a session may not hold: nothing of the
    // row is left to judge.
    applied(
        &written(
            &engine,
            "test.removeSession",
            json!({ "sessionId": SEANCE, "sessionStatus": "prep", "microphoneOn": true }),
        )
        .await,
    );
}
