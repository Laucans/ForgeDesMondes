//! soundbox.updateSound through the engine, over the in-memory store, as the
//! server plants its unit and its target aggregate's judge: an edit writes
//! the fields it names on the sound's partition, its tags normalised, and
//! keeps the others; a volume out of range is rejected by
//! `sound-volume-range`, tags that are no list of texts by
//! `tags-normalized`; a second edit of one sound in flight queues behind the
//! first and asks its author to confirm the overwrite, never rejected for
//! concurrency; a sound the engine does not hold is refused before anything
//! is queued.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use harness_dataguard::aggregate::Aggregate;
use harness_dataguard::api::{Submission, Submitted};
use harness_dataguard::command::{CommandResult, State};
use harness_dataguard::engine::Engine;
use harness_dataguard::manifest::DataCapability;
use harness_dataguard::memory::{MemoryNotifier, MemoryStore};
use harness_dataguard::plugins::RefusalKind;
use harness_dataguard::postgres::SystemClock;
use harness_dataguard::{Error, Result};
use serde_json::{Value, json};

const VALOMBRE: &str = "c1000000-0000-4000-8000-000000000001";
/// « Grincement », Valombre's door, as it was added.
const PORTE: &str = "50000000-0000-4000-8000-000000000002";
/// A sound no one added.
const NULLE_PART: &str = "50000000-0000-4000-8000-0000000000ff";

/// The Sound aggregate's document, beside this unit's folder.
fn aggregate() -> Aggregate {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../aggregates/Sound/aggregate.json");
    Aggregate::parse(&std::fs::read_to_string(path).expect("Sound's aggregate.json"))
        .expect("contract I")
}

/// The door as the engine holds it, under the Sound aggregate's fields.
fn door() -> BTreeMap<String, Value> {
    let Value::Object(row) = json!({
        "id": PORTE,
        "campaignId": VALOMBRE,
        "name": "Grincement",
        "filePath": "sons/porte.ogg",
        "tags": ["bois"],
        "volume": 1.0,
        "createdAt": "2026-10-17T20:00:00Z",
    }) else {
        unreachable!("an object")
    };
    row.into_iter().collect()
}

/// The engine as the server plants this unit and its target aggregate over
/// a store holding the door.
async fn planted() -> (Engine, Arc<MemoryStore>) {
    let store = Arc::new(MemoryStore::default().with_row(&format!("Sound/{PORTE}"), door()));
    let engine = Engine::new(
        store.clone(),
        Arc::new(MemoryNotifier::default()),
        Arc::new(SystemClock),
    )
    .with_aggregate(aggregate())
    .with_data_capability(
        DataCapability::parse(include_str!("../data-capability.json")).expect("contract F"),
    )
    .with_plugins(
        [aggregate_sound::plugin(), update_sound::plugin()]
            .into_iter()
            .collect(),
    );
    engine.load().await.expect("the engine loads");
    (engine, store)
}

/// An edit of `sound` with `payload`, as the SoundLibrary sends it — by an
/// approver: the file is a critical field, which anyone else's edit leaves
/// to a review.
async fn edit(engine: &Engine, sound: &str, payload: Value) -> Result<Submitted> {
    engine
        .submit(
            serde_json::from_value::<Submission>(json!({
                "dataCapability": "soundbox.updateSound@1",
                "targetId": sound,
                "payload": payload,
                "caller": { "by": "dm", "from": "SoundLibrary", "roles": ["role:approver"] },
            }))
            .expect("a submission"),
        )
        .await
}

/// Every command the single applier settles in one pass.
async fn settled(engine: &Engine) -> Vec<CommandResult> {
    engine.run_once().await.expect("the applier ran")
}

/// The door with `changes` over it.
fn door_with(changes: Value) -> BTreeMap<String, Value> {
    let mut row = door();
    for (field, value) in changes.as_object().expect("changes") {
        row.insert(field.clone(), value.clone());
    }
    row
}

#[tokio::test]
async fn an_edit_writes_the_new_name_and_the_tags_normalised_and_keeps_the_rest() {
    let (engine, store) = planted().await;

    let submitted = edit(
        &engine,
        PORTE,
        json!({ "name": "Porte qui grince", "tags": ["Porte", "porte"] }),
    )
    .await
    .expect("queued");

    assert_eq!(submitted.partition, format!("Sound/{PORTE}"));
    let applied = settled(&engine).await;
    assert_eq!(applied.len(), 1, "{applied:?}");
    assert_eq!(applied[0].status, State::Applied, "{applied:?}");
    assert_eq!(
        store.row_now(&submitted.partition),
        door_with(json!({ "name": "Porte qui grince", "tags": ["porte"] }))
    );
}

#[tokio::test]
async fn every_field_of_the_form_is_written_the_volume_as_a_fraction() {
    let (engine, store) = planted().await;

    edit(
        &engine,
        &PORTE.to_uppercase(),
        json!({ "name": "Porte", "file": "sons/porte-2.ogg", "tags": ["Intérieur"], "volume": 40 }),
    )
    .await
    .expect("queued");

    assert_eq!(settled(&engine).await[0].status, State::Applied);
    assert_eq!(
        store.row_now(&format!("Sound/{PORTE}")),
        door_with(json!({
            "name": "Porte",
            "filePath": "sons/porte-2.ogg",
            "tags": ["intérieur"],
            "volume": 0.4,
        }))
    );
}

#[tokio::test]
async fn sound_volume_range_rejects_a_volume_of_minus_5_and_the_sound_stays_as_it_was() {
    let (engine, store) = planted().await;

    let submitted = edit(&engine, PORTE, json!({ "volume": -5 }))
        .await
        .expect("queued, at risk");

    assert_eq!(
        submitted.warnings,
        ["invariant(s) at risk: sound-volume-range"],
        "{submitted:?}"
    );
    let rejected = settled(&engine).await;
    assert_eq!(rejected[0].status, State::Rejected, "{rejected:?}");
    assert_eq!(rejected[0].violations, ["sound-volume-range"]);
    assert_eq!(store.row_now(&submitted.partition), door());
}

#[tokio::test]
async fn tags_normalized_rejects_tags_that_are_no_list_of_texts() {
    let (engine, store) = planted().await;

    edit(&engine, PORTE, json!({ "tags": ["Porte", 3] }))
        .await
        .expect("queued, at risk");

    let rejected = settled(&engine).await;
    assert_eq!(rejected[0].status, State::Rejected, "{rejected:?}");
    assert_eq!(rejected[0].violations, ["tags-normalized"]);
    assert_eq!(store.row_now(&format!("Sound/{PORTE}")), door());
}

#[tokio::test]
async fn a_second_edit_in_flight_queues_and_asks_its_author_to_confirm_the_overwrite() {
    let (engine, store) = planted().await;
    let first = edit(&engine, PORTE, json!({ "name": "Porte qui grince" }))
        .await
        .expect("queued");
    assert_eq!(first.result.status, State::Queued, "{first:?}");

    let second = edit(&engine, PORTE, json!({ "name": "Porte en bois" }))
        .await
        .expect("never rejected for concurrency");

    assert_eq!(
        second.result.status,
        State::AwaitingConfirmation,
        "{second:?}"
    );
    assert_eq!(second.partition, first.partition);
    assert!(!second.warnings.is_empty(), "{second:?}");
    let confirmed = engine
        .confirm(&second.command_id, "dm")
        .await
        .expect("its author confirms");
    assert_eq!(confirmed.status, State::Confirmed, "{confirmed:?}");
    let mut applied = settled(&engine).await;
    applied.extend(settled(&engine).await);
    let statuses: Vec<(&str, State)> = applied
        .iter()
        .map(|result| (result.command_id.as_str(), result.status))
        .collect();
    assert_eq!(
        statuses,
        [
            (first.command_id.as_str(), State::Applied),
            (second.command_id.as_str(), State::Applied),
        ]
    );
    assert_eq!(
        store.row_now(&first.partition),
        door_with(json!({ "name": "Porte en bois" }))
    );
}

#[tokio::test]
async fn a_sound_the_engine_does_not_hold_is_refused_before_anything_queues() {
    let (engine, _) = planted().await;

    let refused = edit(&engine, NULLE_PART, json!({ "name": "Cloche" })).await;

    let Err(Error::Refused(refusal)) = refused else {
        panic!("refused before it queues: {refused:?}");
    };
    assert_eq!(refusal.kind, RefusalKind::NotFound, "{refusal:?}");
    assert_eq!(refusal.message, "ce son n'existe pas");
    assert!(settled(&engine).await.is_empty());
}

#[tokio::test]
async fn an_edit_changing_nothing_is_refused_before_anything_queues() {
    let (engine, store) = planted().await;

    let refused = edit(&engine, PORTE, json!({ "playing": true })).await;

    let Err(Error::Refused(refusal)) = refused else {
        panic!("refused before it queues: {refused:?}");
    };
    assert_eq!(refusal.kind, RefusalKind::Unprocessable, "{refusal:?}");
    assert!(settled(&engine).await.is_empty());
    assert_eq!(store.row_now(&format!("Sound/{PORTE}")), door());
}
