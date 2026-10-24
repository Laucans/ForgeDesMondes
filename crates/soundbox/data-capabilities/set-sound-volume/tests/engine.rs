//! soundbox.setSoundVolume through the engine, over the in-memory store, as
//! the server plants its unit and its target aggregate's judge: a move of
//! one sound's slider writes its volume, in percent divided by 100, and
//! nothing else of it nor of any other sound; a volume out of range, or no
//! number, is rejected by `sound-volume-range`; two moves of one slider queue
//! and the last one wins, nobody asked to confirm it; a sound the engine does
//! not hold is refused before anything is queued.

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
/// « Grincement », Valombre's door, at 80 %.
const PORTE: &str = "50000000-0000-4000-8000-000000000002";
/// « Pluie », Valombre's rain, at 50 %.
const PLUIE: &str = "50000000-0000-4000-8000-000000000003";
/// A sound no one added.
const NULLE_PART: &str = "50000000-0000-4000-8000-0000000000ff";

/// The Sound aggregate's document, beside this unit's folder.
fn aggregate() -> Aggregate {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../aggregates/Sound/aggregate.json");
    Aggregate::parse(&std::fs::read_to_string(path).expect("Sound's aggregate.json"))
        .expect("contract I")
}

/// A sound of Valombre as the engine holds it, under the Sound aggregate's
/// fields.
fn sound(id: &str, name: &str, volume: f64) -> BTreeMap<String, Value> {
    let Value::Object(row) = json!({
        "id": id,
        "campaignId": VALOMBRE,
        "name": name,
        "filePath": format!("sons/{}.ogg", name.to_lowercase()),
        "tags": ["bois"],
        "volume": volume,
        "createdAt": "2026-10-17T20:00:00Z",
    }) else {
        unreachable!("an object")
    };
    row.into_iter().collect()
}

fn door() -> BTreeMap<String, Value> {
    sound(PORTE, "Grincement", 0.8)
}

fn rain() -> BTreeMap<String, Value> {
    sound(PLUIE, "Pluie", 0.5)
}

/// The engine as the server plants this unit and its target aggregate over
/// a store holding the door and the rain.
async fn planted() -> (Engine, Arc<MemoryStore>) {
    let store = Arc::new(
        MemoryStore::default()
            .with_row(&format!("Sound/{PORTE}"), door())
            .with_row(&format!("Sound/{PLUIE}"), rain()),
    );
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
        [aggregate_sound::plugin(), set_sound_volume::plugin()]
            .into_iter()
            .collect(),
    );
    engine.load().await.expect("the engine loads");
    (engine, store)
}

/// A move of `sound`'s slider to `payload`, as the LiveSoundbox sends it.
async fn set(engine: &Engine, sound: &str, payload: Value) -> Result<Submitted> {
    engine
        .submit(
            serde_json::from_value::<Submission>(json!({
                "dataCapability": "soundbox.setSoundVolume@1",
                "targetId": sound,
                "payload": payload,
                "caller": { "by": "dm", "from": "LiveSoundbox" },
            }))
            .expect("a submission"),
        )
        .await
}

/// Every command the single applier settles in one pass.
async fn settled(engine: &Engine) -> Vec<CommandResult> {
    engine.run_once().await.expect("the applier ran")
}

/// `row` with its volume at `volume`.
fn at(mut row: BTreeMap<String, Value>, volume: f64) -> BTreeMap<String, Value> {
    row.insert("volume".into(), json!(volume));
    row
}

#[tokio::test]
async fn a_sound_at_80_set_to_40_stores_0_4_and_no_other_field_changes() {
    let (engine, store) = planted().await;

    set(&engine, PORTE, json!({ "volume": 40 }))
        .await
        .expect("queued");
    let results = settled(&engine).await;

    assert_eq!(results.len(), 1, "{results:?}");
    assert_eq!(results[0].status, State::Applied, "{results:?}");
    assert_eq!(store.row_now(&format!("Sound/{PORTE}")), at(door(), 0.4));
    assert_eq!(store.row_now(&format!("Sound/{PLUIE}")), rain());
}

#[tokio::test]
async fn sound_volume_range_rejects_101_and_the_sound_keeps_its_volume() {
    let (engine, store) = planted().await;

    set(&engine, PORTE, json!({ "volume": 101 }))
        .await
        .expect("queued");
    let results = settled(&engine).await;

    assert_eq!(results[0].status, State::Rejected, "{results:?}");
    assert_eq!(results[0].violations, ["sound-volume-range"], "{results:?}");
    assert_eq!(store.row_now(&format!("Sound/{PORTE}")), door());
}

#[tokio::test]
async fn sound_volume_range_rejects_a_volume_that_is_no_number_or_none() {
    for payload in [
        json!({ "volume": "40" }),
        json!({ "volume": null }),
        json!({}),
        json!({ "volume": -1 }),
    ] {
        let (engine, store) = planted().await;

        set(&engine, PORTE, payload.clone()).await.expect("queued");
        let results = settled(&engine).await;

        assert_eq!(results[0].status, State::Rejected, "{payload}: {results:?}");
        assert_eq!(results[0].violations, ["sound-volume-range"], "{payload}");
        assert_eq!(
            store.row_now(&format!("Sound/{PORTE}")),
            door(),
            "{payload}"
        );
    }
}

#[tokio::test]
async fn the_bounds_of_the_slider_are_held() {
    for (percent, stored) in [(0, 0.0), (100, 1.0)] {
        let (engine, store) = planted().await;

        set(&engine, PORTE, json!({ "volume": percent }))
            .await
            .expect("queued");
        let results = settled(&engine).await;

        assert_eq!(results[0].status, State::Applied, "{percent}: {results:?}");
        assert_eq!(store.row_now(&format!("Sound/{PORTE}")), at(door(), stored));
    }
}

#[tokio::test]
async fn two_moves_of_one_slider_queue_and_the_last_one_wins_unconfirmed() {
    let (engine, store) = planted().await;

    let first = set(&engine, PORTE, json!({ "volume": 30 }))
        .await
        .expect("queued");
    let second = set(&engine, PORTE, json!({ "volume": 60 }))
        .await
        .expect("queued, never rejected for concurrency");
    let results = settled(&engine).await;

    assert_eq!(first.result.status, State::Queued, "{first:?}");
    assert_eq!(second.result.status, State::Queued, "{second:?}");
    assert_eq!(second.partition, first.partition);
    let statuses: Vec<(&str, State)> = results
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
    assert_eq!(store.row_now(&format!("Sound/{PORTE}")), at(door(), 0.6));
}

#[tokio::test]
async fn the_sound_may_be_named_by_the_payload_in_whatever_case() {
    let (engine, store) = planted().await;

    engine
        .submit(
            serde_json::from_value::<Submission>(json!({
                "dataCapability": "soundbox.setSoundVolume@1",
                "payload": { "sound": PORTE.to_uppercase(), "volume": 25 },
                "caller": { "by": "dm", "from": "LiveSoundbox" },
            }))
            .expect("a submission"),
        )
        .await
        .expect("queued");
    let results = settled(&engine).await;

    assert_eq!(results[0].status, State::Applied, "{results:?}");
    assert_eq!(store.row_now(&format!("Sound/{PORTE}")), at(door(), 0.25));
}

#[tokio::test]
async fn a_sound_the_engine_does_not_hold_is_refused_before_anything_queues() {
    let (engine, _store) = planted().await;

    let refused = set(&engine, NULLE_PART, json!({ "volume": 40 })).await;

    let Err(Error::Refused(refusal)) = refused else {
        panic!("refused before it queues: {refused:?}");
    };
    assert_eq!(refusal.kind, RefusalKind::NotFound, "{refusal:?}");
    assert_eq!(refusal.message, "ce son n'existe pas");
    assert!(settled(&engine).await.is_empty());
}
