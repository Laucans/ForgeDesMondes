//! soundbox.removeSound over the in-memory engine, as the server plants it
//! with its target aggregate: a sound the engine holds is removed as a
//! tombstone; one it does not hold is refused before anything is queued; a
//! removal behind a write in flight is never rejected for it (rule 8).

use std::path::Path;
use std::sync::Arc;

use harness_dataguard::Error;
use harness_dataguard::aggregate::Aggregate;
use harness_dataguard::api::{Submission, Submitted};
use harness_dataguard::command::{CommandResult, State};
use harness_dataguard::engine::Engine;
use harness_dataguard::manifest::DataCapability;
use harness_dataguard::memory::{MemoryNotifier, MemoryStore};
use harness_dataguard::plugins::RefusalKind;
use harness_dataguard::postgres::SystemClock;
use serde_json::{Value, json};

/// A sound added under the id its payload names, as soundbox.addSound
/// leaves it: touching `Sound.id`, as it does.
const ADD: &str = r#"{"dataCapability":"test.addSound","owner":"dataguard","version":1,"effect":"insert","target":{"aggregate":"Sound","id":"$id"},"touches":["Sound.id","Sound.name"],"payload":{},"mode":"overwrite","invariants":[],"permissions":[],"callableBy":["*"],"idempotencyKey":"optional"}"#;
/// A sound renamed: an edit of a field the removal does not touch.
const RENAME: &str = r#"{"dataCapability":"test.renameSound","owner":"dataguard","version":1,"effect":"update","target":{"aggregate":"Sound","id":"$id"},"touches":["Sound.name"],"payload":{},"mode":"overwrite","invariants":[],"permissions":[],"callableBy":["*"],"idempotencyKey":"optional"}"#;

const DOOR: &str = "50000000-0000-4000-8000-0000000000a1";
const VALOMBRE: &str = "c1000000-0000-4000-8000-000000000001";

/// The engine as the server plants this unit and its target aggregate over
/// the in-memory store, with the test-only writes of a sound.
async fn engine() -> (Engine, Arc<MemoryStore>) {
    let store = Arc::new(MemoryStore::default());
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../aggregates/Sound/aggregate.json");
    let aggregate = std::fs::read_to_string(path).expect("Sound's aggregate.json");
    let mut engine = Engine::new(
        store.clone(),
        Arc::new(MemoryNotifier::default()),
        Arc::new(SystemClock),
    )
    .with_aggregate(Aggregate::parse(&aggregate).expect("contract I"));
    for manifest in [include_str!("../data-capability.json"), ADD, RENAME] {
        engine = engine.with_data_capability(DataCapability::parse(manifest).expect("contract F"));
    }
    let engine = engine.with_plugins(
        [aggregate_sound::plugin(), remove_sound::plugin()]
            .into_iter()
            .collect(),
    );
    engine.load().await.expect("the in-memory engine");
    (engine, store)
}

/// A submission of `data_capability` by the DM, an approver unless
/// `approver` is false, on the sound `target` when given.
async fn submit_as(
    engine: &Engine,
    approver: bool,
    data_capability: &str,
    target: Option<&str>,
    payload: Value,
) -> harness_dataguard::Result<Submitted> {
    let roles: Vec<&str> = if approver {
        vec!["role:approver"]
    } else {
        Vec::new()
    };
    let mut submission = json!({
        "dataCapability": data_capability,
        "payload": payload,
        "caller": { "by": "dm", "from": "soundbox.SoundLibrary", "roles": roles },
    });
    if let Some(id) = target {
        submission["targetId"] = json!(id);
    }
    engine
        .submit(serde_json::from_value::<Submission>(submission).expect("a submission"))
        .await
}

async fn submit(
    engine: &Engine,
    data_capability: &str,
    target: Option<&str>,
    payload: Value,
) -> harness_dataguard::Result<Submitted> {
    submit_as(engine, true, data_capability, target, payload).await
}

/// One pass of the single applier, which settles one command.
async fn settled(engine: &Engine) -> CommandResult {
    let mut settled = engine.run_once().await.expect("the applier ran");
    assert_eq!(settled.len(), 1, "{settled:?}");
    settled.remove(0)
}

/// The door, added and applied.
async fn with_door() -> (Engine, Arc<MemoryStore>) {
    let (engine, store) = engine().await;
    submit(&engine, "test.addSound@1", Some(DOOR), door())
        .await
        .expect("queued");
    assert_eq!(settled(&engine).await.status, State::Applied);
    (engine, store)
}

fn door() -> Value {
    json!({
        "id": DOOR, "campaignId": VALOMBRE, "name": "Porte qui grince",
        "filePath": "sons/porte.ogg", "tags": ["porte"], "volume": 0.8,
    })
}

fn not_found(submitted: harness_dataguard::Result<Submitted>) {
    match submitted {
        Err(Error::Refused(refusal)) => {
            assert_eq!(refusal.kind, RefusalKind::NotFound, "{refusal:?}");
            assert_eq!(refusal.message, "ce son n'existe pas");
        }
        other => panic!("expected a refusal, 404: {other:?}"),
    }
}

#[tokio::test]
async fn a_sound_held_is_removed_as_a_tombstone_on_its_partition() {
    let (engine, store) = with_door().await;
    let submitted = submit(&engine, "soundbox.removeSound@1", Some(DOOR), json!({}))
        .await
        .expect("queued");
    assert_eq!(submitted.partition, format!("Sound/{DOOR}"));
    let applied = settled(&engine).await;
    assert_eq!(applied.status, State::Applied, "{applied:?}");
    assert!(store.is_tombstoned(&format!("Sound/{DOOR}")));
}

#[tokio::test]
async fn a_sound_named_in_capitals_or_padded_is_removed_on_its_own_partition() {
    let (engine, store) = with_door().await;
    let named = format!("  {}  ", DOOR.to_uppercase());
    let submitted = submit(&engine, "soundbox.removeSound@1", Some(&named), json!({}))
        .await
        .expect("queued");
    assert_eq!(submitted.partition, format!("Sound/{DOOR}"));
    assert_eq!(settled(&engine).await.status, State::Applied);
    assert!(store.is_tombstoned(&format!("Sound/{DOOR}")));
}

#[tokio::test]
async fn a_sound_named_by_the_payload_s_id_is_removed_whatever_else_it_holds() {
    let (engine, store) = with_door().await;
    submit(
        &engine,
        "soundbox.removeSound@1",
        None,
        json!({ "id": DOOR, "volume": 3, "tags": ["Porte", "Porte"] }),
    )
    .await
    .expect("queued");
    assert_eq!(settled(&engine).await.status, State::Applied);
    assert!(store.is_tombstoned(&format!("Sound/{DOOR}")));
}

#[tokio::test]
async fn a_removal_naming_no_sound_held_is_refused_404_and_nothing_is_queued() {
    let (engine, store) = with_door().await;
    for target in [
        Some("50000000-0000-4000-8000-0000000000ff"),
        Some("porte"),
        Some("50000000-0000-4000-8000-0000000000a"),
        Some("50000000_0000-4000-8000-0000000000a1"),
        Some("5000000g-0000-4000-8000-0000000000a1"),
        Some("   "),
        None,
    ] {
        not_found(submit(&engine, "soundbox.removeSound@1", target, json!({})).await);
    }
    assert!(engine.run_once().await.expect("the applier ran").is_empty());
    assert!(!store.is_tombstoned(&format!("Sound/{DOOR}")));
}

#[tokio::test]
async fn a_sound_whose_addition_is_queued_is_one_to_remove_once_its_author_confirms() {
    let (engine, store) = engine().await;
    submit(&engine, "test.addSound@1", Some(DOOR), door())
        .await
        .expect("queued");
    // The removal overwrites the addition ahead of it, which it touches:
    // it waits for its author, never rejected (rule 8).
    let removal = submit(&engine, "soundbox.removeSound@1", Some(DOOR), json!({}))
        .await
        .expect("queued");
    assert_eq!(removal.result.status, State::AwaitingConfirmation);
    engine
        .confirm(&removal.command_id, "dm")
        .await
        .expect("confirmed");
    let mut applied = engine.run_once().await.expect("the applier ran");
    applied.extend(engine.run_once().await.expect("the applier ran"));
    let states: Vec<State> = applied.iter().map(|result| result.status).collect();
    assert_eq!(states, [State::Applied, State::Applied], "{applied:?}");
    assert!(store.is_tombstoned(&format!("Sound/{DOOR}")));
}

#[tokio::test]
async fn a_removal_behind_an_edit_in_flight_keeps_its_place_and_is_applied_after_it() {
    let (engine, store) = with_door().await;
    submit(
        &engine,
        "test.renameSound@1",
        Some(DOOR),
        json!({ "name": "Porte" }),
    )
    .await
    .expect("queued");
    let removal = submit(&engine, "soundbox.removeSound@1", Some(DOOR), json!({}))
        .await
        .expect("queued");
    assert_eq!(removal.result.status, State::Queued);
    let mut applied = engine.run_once().await.expect("the applier ran");
    applied.extend(engine.run_once().await.expect("the applier ran"));
    let states: Vec<State> = applied.iter().map(|result| result.status).collect();
    assert_eq!(states, [State::Applied, State::Applied], "{applied:?}");
    assert_eq!(applied[1].command_id, removal.command_id);
    assert!(store.is_tombstoned(&format!("Sound/{DOOR}")));
}

#[tokio::test]
async fn a_removal_by_a_caller_who_is_no_approver_waits_for_its_review() {
    // `Sound.id` is critical (aggregate.json): its removal is an
    // approver's to let through.
    let (engine, store) = with_door().await;
    submit_as(
        &engine,
        false,
        "soundbox.removeSound@1",
        Some(DOOR),
        json!({}),
    )
    .await
    .expect("queued");
    let waiting = engine.run_once().await.expect("the applier ran");
    assert!(
        waiting
            .iter()
            .all(|result| result.status == State::AwaitingReview),
        "{waiting:?}"
    );
    assert!(!store.is_tombstoned(&format!("Sound/{DOOR}")));
}
