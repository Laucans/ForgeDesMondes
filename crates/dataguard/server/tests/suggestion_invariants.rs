//! The Suggestion aggregate's invariants, judged by the DataGuard as
//! `plant()` binds it, over the in-memory store: a command that breaks one
//! is rejected under its id, and the row stays as it was. The
//! DataCapabilities here are test-only — no DataCapability of the product
//! writes a suggestion yet.

use dataguard_server::{Engine, MEMORY, plant};
use harness_dataguard::api::Submission;
use harness_dataguard::command::{CommandResult, State};
use harness_dataguard::guard::Caller;
use harness_dataguard::manifest::DataCapability;
use serde_json::{Value, json};

/// A suggestion added under the id its payload names.
const ADD: &str = r#"{"dataCapability":"test.addSuggestion","owner":"dataguard","version":1,"effect":"insert","target":{"aggregate":"Suggestion","id":"$id"},"mode":"overwrite","callableBy":["*"]}"#;
/// A suggestion's fields changed.
const UPDATE: &str = r#"{"dataCapability":"test.updateSuggestion","owner":"dataguard","version":1,"effect":"update","target":{"aggregate":"Suggestion","id":"$id"},"mode":"overwrite","callableBy":["*"]}"#;
/// A suggestion removed: the engine's soft delete, a tombstone.
const REMOVE: &str = r#"{"dataCapability":"test.removeSuggestion","owner":"dataguard","version":1,"effect":"delete","target":{"aggregate":"Suggestion","id":"$id"},"mode":"overwrite","callableBy":["*"]}"#;
/// A sound's fields changed: another aggregate, with fields of no
/// suggestion.
const UPDATE_SOUND: &str = r#"{"dataCapability":"test.updateSound","owner":"dataguard","version":1,"effect":"update","target":{"aggregate":"Sound","id":"$id"},"mode":"overwrite","callableBy":["*"]}"#;

/// Morlox heard during Valombre's session 1.
const HEARD: &str = "5a000000-0000-4000-8000-000000000001";
const SEANCE: &str = "51000000-0000-4000-8000-000000000001";
const MORLOX: &str = "e1000000-0000-4000-8000-000000000002";
const PLUIE: &str = "50000000-0000-4000-8000-000000000001";

/// The engine as the server plants it, with the test-only DataCapabilities.
async fn engine() -> Engine {
    let mut engine = plant(MEMORY).await.expect("the in-memory engine");
    for manifest in [ADD, UPDATE, REMOVE, UPDATE_SOUND] {
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
                from: "live.suggestions".to_string(),
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

/// Morlox, recognised in what the DM said.
fn heard() -> Value {
    json!({
        "id": HEARD,
        "sessionId": SEANCE,
        "entityId": MORLOX,
        "matchedText": "Morlox",
        "detectedAt": "2026-10-18T20:31:07Z",
    })
}

fn with(mut base: Value, field: &str, value: Value) -> Value {
    base[field] = value;
    base
}

fn without(mut base: Value, field: &str) -> Value {
    base.as_object_mut().expect("an object").remove(field);
    base
}

/// An engine where Morlox's suggestion exists.
async fn with_heard() -> Engine {
    let engine = engine().await;
    applied(&written(&engine, "test.addSuggestion", heard()).await);
    engine
}

/// The suggestion's row is still one every invariant holds on: its words
/// corrected, judged on the whole row it leaves, apply.
async fn still_whole(engine: &Engine) {
    applied(
        &written(
            engine,
            "test.updateSuggestion",
            json!({"id": HEARD, "matchedText": "Morlox le borgne"}),
        )
        .await,
    );
}

#[tokio::test]
async fn suggestion_target_lets_an_element_or_a_sound_through() {
    let engine = engine().await;
    applied(&written(&engine, "test.addSuggestion", heard()).await);
    let rain = with(
        with(without(heard(), "entityId"), "soundId", json!(PLUIE)),
        "id",
        json!("5a000000-0000-4000-8000-000000000002"),
    );
    applied(&written(&engine, "test.addSuggestion", rain).await);
    // A null element beside the sound is no element.
    let rain = with(
        with(
            with(heard(), "entityId", Value::Null),
            "soundId",
            json!(PLUIE),
        ),
        "id",
        json!("5a000000-0000-4000-8000-000000000003"),
    );
    applied(&written(&engine, "test.addSuggestion", rain).await);
}

#[tokio::test]
async fn suggestion_target_rejects_both_an_element_and_a_sound() {
    let engine = engine().await;
    rejected_by(
        &written(
            &engine,
            "test.addSuggestion",
            with(heard(), "soundId", json!(PLUIE)),
        )
        .await,
        &["suggestion-target"],
    );
    // Nothing was written: the suggestion can still be added, on Morlox.
    applied(&written(&engine, "test.addSuggestion", heard()).await);
}

#[tokio::test]
async fn suggestion_target_rejects_no_target_and_keeps_the_row() {
    let engine = with_heard().await;
    for target in [Value::Null, json!(""), json!(42), json!("Morlox")] {
        rejected_by(
            &written(
                &engine,
                "test.updateSuggestion",
                json!({"id": HEARD, "entityId": target}),
            )
            .await,
            &["suggestion-target"],
        );
    }
    still_whole(&engine).await;
}

#[tokio::test]
async fn suggestion_target_rejects_a_suggestion_of_no_session() {
    let engine = engine().await;
    for session in [
        None,
        Some(Value::Null),
        Some(json!("  ")),
        Some(json!("seance-1")),
    ] {
        let suggestion = match session {
            None => without(heard(), "sessionId"),
            Some(session) => with(heard(), "sessionId", session),
        };
        rejected_by(
            &written(&engine, "test.addSuggestion", suggestion).await,
            &["suggestion-target"],
        );
    }
}

#[tokio::test]
async fn suggestion_target_rejects_a_move_to_a_sound_that_keeps_the_element() {
    let engine = with_heard().await;
    rejected_by(
        &written(
            &engine,
            "test.updateSuggestion",
            json!({"id": HEARD, "soundId": PLUIE}),
        )
        .await,
        &["suggestion-target"],
    );
    // Moved whole — the element left, the sound taken — it applies.
    applied(
        &written(
            &engine,
            "test.updateSuggestion",
            json!({"id": HEARD, "entityId": null, "soundId": PLUIE}),
        )
        .await,
    );
}

#[tokio::test]
async fn pile_and_history_derived_rejects_a_stored_pile_or_rank() {
    let engine = engine().await;
    for (field, value) in [
        ("pile", json!([HEARD])),
        ("rank", json!(1)),
        ("historyPosition", json!(7)),
    ] {
        rejected_by(
            &written(&engine, "test.addSuggestion", with(heard(), field, value)).await,
            &["pile-and-history-derived"],
        );
    }
}

#[tokio::test]
async fn pile_and_history_derived_rejects_a_rank_set_later_and_keeps_the_row() {
    let engine = with_heard().await;
    rejected_by(
        &written(
            &engine,
            "test.updateSuggestion",
            json!({"id": HEARD, "inPile": true}),
        )
        .await,
        &["pile-and-history-derived"],
    );
    still_whole(&engine).await;
}

#[tokio::test]
async fn pile_and_history_derived_lets_an_undeclared_field_through_when_it_holds_nothing() {
    let engine = engine().await;
    applied(
        &written(
            &engine,
            "test.addSuggestion",
            with(heard(), "rank", Value::Null),
        )
        .await,
    );
    // An instant PostgreSQL reads, written with a space and an offset.
    applied(
        &written(
            &engine,
            "test.updateSuggestion",
            json!({"id": HEARD, "detectedAt": "2026-10-18 20:31:07.5+02"}),
        )
        .await,
    );
}

#[tokio::test]
async fn pile_and_history_derived_rejects_a_suggestion_of_no_instant() {
    let engine = engine().await;
    rejected_by(
        &written(
            &engine,
            "test.addSuggestion",
            without(heard(), "detectedAt"),
        )
        .await,
        &["pile-and-history-derived"],
    );
    let engine = with_heard().await;
    for instant in [
        Value::Null,
        json!(""),
        json!(1_729_283_467),
        json!("hier soir"),
        json!("18/10/2026 20:31"),
    ] {
        rejected_by(
            &written(
                &engine,
                "test.updateSuggestion",
                json!({"id": HEARD, "detectedAt": instant}),
            )
            .await,
            &["pile-and-history-derived"],
        );
    }
    still_whole(&engine).await;
}

#[tokio::test]
async fn live_only_local_audio_rejects_a_suggestion_that_keeps_no_words() {
    let engine = engine().await;
    for words in [Value::Null, json!(""), json!("   ")] {
        rejected_by(
            &written(
                &engine,
                "test.addSuggestion",
                with(heard(), "matchedText", words),
            )
            .await,
            &["live-only-local-audio"],
        );
    }
    rejected_by(
        &written(
            &engine,
            "test.addSuggestion",
            without(heard(), "matchedText"),
        )
        .await,
        &["live-only-local-audio"],
    );
}

#[tokio::test]
async fn live_only_local_audio_rejects_a_recording_in_place_of_the_words() {
    let engine = with_heard().await;
    for recording in [json!([82, 73, 70, 70]), json!({"wav": "UklGRg=="})] {
        rejected_by(
            &written(
                &engine,
                "test.updateSuggestion",
                json!({"id": HEARD, "matchedText": recording}),
            )
            .await,
            &["live-only-local-audio"],
        );
    }
    still_whole(&engine).await;
}

#[tokio::test]
async fn every_invariant_is_named_when_all_break() {
    let engine = engine().await;
    let suggestion = json!({
        "id": HEARD,
        "sessionId": SEANCE,
        "audio": "UklGRg==",
    });
    rejected_by(
        &written(&engine, "test.addSuggestion", suggestion).await,
        &[
            "suggestion-target",
            "pile-and-history-derived",
            "live-only-local-audio",
        ],
    );
}

#[tokio::test]
async fn a_removed_suggestion_is_not_judged_by_its_invariants() {
    let engine = with_heard().await;
    // Morlox's removal carrying what no invariant would let through: a
    // tombstone, judged on nothing of the row.
    applied(
        &written(
            &engine,
            "test.removeSuggestion",
            json!({"id": HEARD, "soundId": PLUIE, "rank": 1, "matchedText": ""}),
        )
        .await,
    );
    // Nor one the engine never held, known by its id alone.
    applied(
        &written(
            &engine,
            "test.removeSuggestion",
            json!({"id": "5a000000-0000-4000-8000-0000000000ff"}),
        )
        .await,
    );
}

#[tokio::test]
async fn another_aggregate_s_rows_are_not_the_suggestion_s() {
    let engine = engine().await;
    applied(
        &written(
            &engine,
            "test.updateSound",
            json!({
                "id": PLUIE,
                "name": "Pluie",
                "entityId": MORLOX,
                "rank": 1,
                "matchedText": "",
            }),
        )
        .await,
    );
}
