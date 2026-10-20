//! The Sound aggregate's invariants, judged by the DataGuard as `plant()`
//! binds it, over the in-memory store: a command that breaks one is
//! rejected under its id, and the row stays as it was. The DataCapabilities
//! here are test-only — no DataCapability of the product writes a sound yet.

use dataguard_server::{Engine, MEMORY, plant};
use harness_dataguard::api::Submission;
use harness_dataguard::command::{CommandResult, State};
use harness_dataguard::guard::Caller;
use harness_dataguard::manifest::DataCapability;
use serde_json::{Value, json};

/// A sound added under the id its payload names.
const ADD: &str = r#"{"dataCapability":"test.addSound","owner":"dataguard","version":1,"effect":"insert","target":{"aggregate":"Sound","id":"$id"},"mode":"overwrite","callableBy":["*"]}"#;
/// A sound's fields changed.
const UPDATE: &str = r#"{"dataCapability":"test.updateSound","owner":"dataguard","version":1,"effect":"update","target":{"aggregate":"Sound","id":"$id"},"mode":"overwrite","callableBy":["*"]}"#;
/// A sound removed: the engine's soft delete, a tombstone.
const REMOVE: &str = r#"{"dataCapability":"test.removeSound","owner":"dataguard","version":1,"effect":"delete","target":{"aggregate":"Sound","id":"$id"},"mode":"overwrite","callableBy":["*"]}"#;
/// A campaign's fields changed: another aggregate, with a field of the
/// same name.
const UPDATE_CAMPAIGN: &str = r#"{"dataCapability":"test.updateCampaign","owner":"dataguard","version":1,"effect":"update","target":{"aggregate":"Campaign","id":"$id"},"mode":"overwrite","callableBy":["*"]}"#;

const DOOR: &str = "50000000-0000-4000-8000-000000000001";
const VALOMBRE: &str = "c1000000-0000-4000-8000-000000000001";

/// The engine as the server plants it, with the test-only DataCapabilities.
async fn engine() -> Engine {
    let mut engine = plant(MEMORY).await.expect("the in-memory engine");
    for manifest in [ADD, UPDATE, REMOVE, UPDATE_CAMPAIGN] {
        engine = engine.with_data_capability(DataCapability::parse(manifest).expect("contract F"));
    }
    engine
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
                from: "soundbox.library".to_string(),
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

/// The creaking door of the soundbox, as the library shows it.
fn door() -> Value {
    json!({
        "id": DOOR,
        "campaignId": VALOMBRE,
        "name": "Porte qui grince",
        "filePath": "sons/porte-qui-grince.ogg",
        "tags": ["porte", "taverne"],
        "volume": 0.8,
    })
}

fn with(mut base: Value, field: &str, value: Value) -> Value {
    base[field] = value;
    base
}

/// The door's row is still one both invariants hold on: a rename, judged on
/// the whole row it leaves, applies.
async fn still_in_range(engine: &Engine) {
    applied(
        &written(
            engine,
            "test.updateSound",
            json!({"id": DOOR, "name": "Porte de la taverne"}),
        )
        .await,
    );
}

/// An engine where the door exists.
async fn with_door() -> Engine {
    let engine = engine().await;
    applied(&written(&engine, "test.addSound", door()).await);
    engine
}

#[tokio::test]
async fn sound_volume_range_rejects_a_sound_added_at_1_2() {
    let engine = engine().await;
    rejected_by(
        &written(&engine, "test.addSound", with(door(), "volume", json!(1.2))).await,
        &["sound-volume-range"],
    );
    // Nothing was written: the sound can still be added, in range.
    applied(&written(&engine, "test.addSound", door()).await);
}

#[tokio::test]
async fn sound_volume_range_rejects_a_volume_moved_below_0_and_keeps_the_row() {
    let engine = with_door().await;
    for volume in [json!(-0.1), json!("1.01"), json!("fort"), Value::Null] {
        rejected_by(
            &written(
                &engine,
                "test.updateSound",
                json!({"id": DOOR, "volume": volume}),
            )
            .await,
            &["sound-volume-range"],
        );
    }
    still_in_range(&engine).await;
}

#[tokio::test]
async fn sound_volume_range_lets_0_to_1_through_as_a_number_or_its_text() {
    let engine = with_door().await;
    for volume in [json!(0), json!(1), json!(0.35), json!("0.50")] {
        applied(
            &written(
                &engine,
                "test.updateSound",
                json!({"id": DOOR, "volume": volume}),
            )
            .await,
        );
    }
}

#[tokio::test]
async fn tags_normalized_rejects_a_tag_in_capitals() {
    let engine = engine().await;
    for tags in [json!(["Porte"]), json!(["porte", "ÉCHO"])] {
        rejected_by(
            &written(&engine, "test.addSound", with(door(), "tags", tags)).await,
            &["tags-normalized"],
        );
    }
}

#[tokio::test]
async fn tags_normalized_rejects_a_tag_twice() {
    let engine = with_door().await;
    rejected_by(
        &written(
            &engine,
            "test.updateSound",
            json!({"id": DOOR, "tags": ["porte", "taverne", "porte"]}),
        )
        .await,
        &["tags-normalized"],
    );
    still_in_range(&engine).await;
}

#[tokio::test]
async fn tags_normalized_rejects_tags_that_are_not_a_list_of_texts() {
    let engine = with_door().await;
    for tags in [
        json!("porte"),
        json!(["porte", 3]),
        json!([null]),
        Value::Null,
    ] {
        rejected_by(
            &written(
                &engine,
                "test.updateSound",
                json!({"id": DOOR, "tags": tags}),
            )
            .await,
            &["tags-normalized"],
        );
    }
}

#[tokio::test]
async fn tags_normalized_lets_lowercase_distinct_tags_through() {
    let engine = with_door().await;
    for tags in [json!([]), json!(["écho", "grotte"]), json!(["porte-2"])] {
        applied(
            &written(
                &engine,
                "test.updateSound",
                json!({"id": DOOR, "tags": tags}),
            )
            .await,
        );
    }
}

#[tokio::test]
async fn both_invariants_are_named_when_both_break() {
    let engine = engine().await;
    let sound = with(with(door(), "volume", json!(2)), "tags", json!(["Porte"]));
    rejected_by(
        &written(&engine, "test.addSound", sound).await,
        &["sound-volume-range", "tags-normalized"],
    );
}

#[tokio::test]
async fn a_removed_sound_is_not_judged_by_its_invariants() {
    let engine = with_door().await;
    // The door's removal carrying what neither invariant would let through:
    // a tombstone, judged on nothing of the row.
    applied(
        &written(
            &engine,
            "test.removeSound",
            json!({"id": DOOR, "volume": 3, "tags": ["Porte", "Porte"]}),
        )
        .await,
    );
    // Nor one the engine never held, known by its id alone.
    applied(
        &written(
            &engine,
            "test.removeSound",
            json!({"id": "50000000-0000-4000-8000-000000000002"}),
        )
        .await,
    );
}

#[tokio::test]
async fn another_aggregate_s_volume_and_tags_are_not_the_sound_s() {
    let engine = engine().await;
    applied(
        &written(
            &engine,
            "test.updateCampaign",
            json!({
                "id": VALOMBRE,
                "kind": "campagne",
                "volume": 7,
                "tags": ["Porte", "Porte"],
            }),
        )
        .await,
    );
}
