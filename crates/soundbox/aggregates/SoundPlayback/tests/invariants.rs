//! The SoundPlayback aggregate's invariants, judged by the DataGuard as
//! `plant()` binds it, over the in-memory store: a command that breaks one
//! is rejected under its id, and the row stays as it was. The
//! DataCapabilities here are test-only — no DataCapability of the product
//! writes a playback yet.

use std::sync::Arc;

use harness_dataguard::aggregate::Aggregate;
use harness_dataguard::api::{Submission, Submitted};
use harness_dataguard::command::{CommandResult, State};
use harness_dataguard::engine::Engine;
use harness_dataguard::guard::Caller;
use harness_dataguard::manifest::DataCapability;
use harness_dataguard::memory::{MemoryNotifier, MemoryStore};
use harness_dataguard::postgres::SystemClock;
use serde_json::{Value, json};

/// A sound played: a playback added under the id its payload names.
const PLAY: &str = r#"{"dataCapability":"test.playSound","owner":"dataguard","version":1,"effect":"insert","target":{"aggregate":"SoundPlayback","id":"$id"},"mode":"overwrite","callableBy":["*"]}"#;
/// A playback's fields changed: its stop, by « Tout arrêter » or alone.
const UPDATE: &str = r#"{"dataCapability":"test.updatePlayback","owner":"dataguard","version":1,"effect":"update","target":{"aggregate":"SoundPlayback","id":"$id"},"mode":"overwrite","callableBy":["*"]}"#;
/// A playback removed: the engine's soft delete, a tombstone.
const REMOVE: &str = r#"{"dataCapability":"test.removePlayback","owner":"dataguard","version":1,"effect":"delete","target":{"aggregate":"SoundPlayback","id":"$id"},"mode":"overwrite","callableBy":["*"]}"#;
/// A sound added: another aggregate.
const ADD_SOUND: &str = r#"{"dataCapability":"test.addSound","owner":"dataguard","version":1,"effect":"insert","target":{"aggregate":"Sound","id":"$id"},"mode":"overwrite","callableBy":["*"]}"#;

/// « Bruit de foule », playing from the soundbox.
const FOULE: &str = "5b000000-0000-4000-8000-000000000001";
/// « Porte qui grince », triggered from the pastille.
const PORTE: &str = "5b000000-0000-4000-8000-000000000002";
/// « Porte qui grince » again, over the first.
const PORTE_ENCORE: &str = "5b000000-0000-4000-8000-000000000003";
/// Session 1 of Valombre, live.
const SEANCE: &str = "5e000000-0000-4000-8000-000000000001";
const SON_FOULE: &str = "50000000-0000-4000-8000-000000000001";
const SON_PORTE: &str = "50000000-0000-4000-8000-000000000002";
const VALOMBRE: &str = "c1000000-0000-4000-8000-000000000001";

const STARTED: &str = "2026-10-18T20:00:00Z";
const STOPPED: &str = "2026-10-18T20:05:00Z";

/// The engine as the server plants this aggregate — its manifest and its
/// plugin — over the in-memory store.
async fn plant() -> harness_dataguard::Result<Engine> {
    let engine = Engine::new(
        Arc::new(MemoryStore::default()),
        Arc::new(MemoryNotifier::default()),
        Arc::new(SystemClock),
    )
    .with_aggregate(Aggregate::parse(include_str!("../aggregate.json")).expect("contract I"))
    .with_plugins(std::iter::once(aggregate_sound_playback::plugin()).collect());
    engine.load().await?;
    Ok(engine)
}

/// The engine as the server plants it, with the test-only DataCapabilities.
async fn engine() -> Engine {
    let mut engine = plant().await.expect("the in-memory engine");
    for manifest in [PLAY, UPDATE, REMOVE, ADD_SOUND] {
        engine = engine.with_data_capability(DataCapability::parse(manifest).expect("contract F"));
    }
    engine
}

/// The command queued, not applied yet: what its author is told.
async fn queued(engine: &Engine, data_capability: &str, payload: Value) -> Submitted {
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
                from: "live.soundbox".to_string(),
                permissions: Vec::new(),
                // The critical fields' review is not what these tests judge.
                roles: vec!["role:approver".to_string()],
            },
        })
        .await
        .expect("queued")
}

/// What the single applier made of one command.
async fn written(engine: &Engine, data_capability: &str, payload: Value) -> CommandResult {
    queued(engine, data_capability, payload).await;
    let mut applied = engine.run_once().await.expect("the applier ran");
    assert_eq!(applied.len(), 1, "{applied:?}");
    applied.remove(0)
}

/// What the single applier made of every command queued, in order.
async fn drained(engine: &Engine) -> Vec<CommandResult> {
    let mut results = Vec::new();
    loop {
        let applied = engine.run_once().await.expect("the applier ran");
        if applied.is_empty() {
            return results;
        }
        results.extend(applied);
    }
}

fn rejected_by(result: &CommandResult, ids: &[&str]) {
    assert_eq!(result.status, State::Rejected, "{result:?}");
    assert_eq!(result.violations, ids, "{result:?}");
}

fn applied(result: &CommandResult) {
    assert_eq!(result.status, State::Applied, "{result:?}");
}

/// Playback `id` of `sound` in the live session, started from `origin`.
fn playback(id: &str, sound: &str, origin: &str) -> Value {
    json!({
        "id": id,
        "sessionId": SEANCE,
        "soundId": sound,
        "origin": origin,
        "startedAt": STARTED,
    })
}

fn with(mut base: Value, field: &str, value: Value) -> Value {
    base[field] = value;
    base
}

/// An engine where « bruit de foule » plays from the soundbox.
async fn with_foule() -> Engine {
    let engine = engine().await;
    applied(
        &written(
            &engine,
            "test.playSound",
            playback(FOULE, SON_FOULE, "soundbox"),
        )
        .await,
    );
    engine
}

/// The stop of `id`, at `at`.
async fn stop(engine: &Engine, id: &str, at: Value) -> CommandResult {
    written(
        engine,
        "test.updatePlayback",
        json!({"id": id, "stoppedAt": at}),
    )
    .await
}

#[tokio::test]
async fn simultaneous_playback_lets_a_pastille_sound_join_the_one_playing() {
    let engine = with_foule().await;
    // R111: « porte qui grince » from the pastille, while the crowd plays;
    // then the same door again, over the first.
    applied(
        &written(
            &engine,
            "test.playSound",
            playback(PORTE, SON_PORTE, "pastille"),
        )
        .await,
    );
    applied(
        &written(
            &engine,
            "test.playSound",
            playback(PORTE_ENCORE, SON_PORTE, "pastille"),
        )
        .await,
    );
    // Adding them stopped nothing: each one is still playing, so each stops
    // now, once.
    for id in [FOULE, PORTE, PORTE_ENCORE] {
        applied(&stop(&engine, id, json!(STOPPED)).await);
    }
}

#[tokio::test]
async fn simultaneous_playback_lets_a_stop_queued_behind_its_playback_apply() {
    let engine = engine().await;
    // The door played and stopped before the applier ran: the stop is not a
    // playback added.
    let play = queued(
        &engine,
        "test.playSound",
        playback(PORTE, SON_PORTE, "pastille"),
    )
    .await;
    let stop = queued(
        &engine,
        "test.updatePlayback",
        json!({"id": PORTE, "stoppedAt": STOPPED}),
    )
    .await;
    for submitted in [&play, &stop] {
        assert!(submitted.warnings.is_empty(), "{submitted:?}");
    }
    let results = drained(&engine).await;
    assert_eq!(results.len(), 2, "{results:?}");
    results.iter().for_each(applied);
}

#[tokio::test]
async fn simultaneous_playback_warns_of_nothing_when_a_playing_one_is_stopped() {
    let engine = with_foule().await;
    let stop = queued(
        &engine,
        "test.updatePlayback",
        json!({"id": FOULE, "stoppedAt": STOPPED}),
    )
    .await;
    assert!(stop.warnings.is_empty(), "{stop:?}");
    applied(&drained(&engine).await[0]);
}

#[tokio::test]
async fn simultaneous_playback_rejects_a_playback_added_already_stopped() {
    let engine = engine().await;
    rejected_by(
        &written(
            &engine,
            "test.playSound",
            with(
                playback(PORTE, SON_PORTE, "pastille"),
                "stoppedAt",
                json!(STOPPED),
            ),
        )
        .await,
        &["simultaneous-playback"],
    );
    // Nothing was written: the door can still be played, playing.
    applied(
        &written(
            &engine,
            "test.playSound",
            playback(PORTE, SON_PORTE, "pastille"),
        )
        .await,
    );
}

#[tokio::test]
async fn simultaneous_playback_lets_a_playing_one_stop_on_its_own() {
    let engine = with_foule().await;
    // Not born now: an update may give it its stop.
    applied(&stop(&engine, FOULE, json!(STOPPED)).await);
}

#[tokio::test]
async fn stop_all_stops_every_playback_still_playing_in_the_session() {
    let engine = with_foule().await;
    applied(
        &written(
            &engine,
            "test.playSound",
            playback(PORTE, SON_PORTE, "pastille"),
        )
        .await,
    );
    // « Tout arrêter »: each playback still playing gets the same stop.
    for id in [FOULE, PORTE] {
        applied(&stop(&engine, id, json!(STOPPED)).await);
    }
    // Restating the stop it has changes nothing, and passes.
    applied(&stop(&engine, PORTE, json!(STOPPED)).await);
}

#[tokio::test]
async fn stop_all_rejects_a_stopped_playback_restarted_and_keeps_it_stopped() {
    let engine = with_foule().await;
    applied(&stop(&engine, FOULE, json!(STOPPED)).await);
    rejected_by(&stop(&engine, FOULE, Value::Null).await, &["stop-all"]);
    // The row stayed stopped: a later « Tout arrêter » cannot stop it again.
    rejected_by(
        &stop(&engine, FOULE, json!("2026-10-18T20:09:00Z")).await,
        &["stop-all"],
    );
}

#[tokio::test]
async fn stop_all_rejects_a_stopped_playback_stopped_again_later() {
    let engine = with_foule().await;
    applied(&stop(&engine, FOULE, json!(STOPPED)).await);
    rejected_by(
        &stop(&engine, FOULE, json!("2026-10-18T20:09:00Z")).await,
        &["stop-all"],
    );
    // The row kept its first stop: restating it passes.
    applied(&stop(&engine, FOULE, json!(STOPPED)).await);
}

#[tokio::test]
async fn dm_side_only_accepts_the_soundbox_and_the_pastille() {
    let engine = engine().await;
    for (id, origin) in [(FOULE, "soundbox"), (PORTE, "pastille")] {
        applied(&written(&engine, "test.playSound", playback(id, SON_PORTE, origin)).await);
    }
}

#[tokio::test]
async fn dm_side_only_rejects_a_playback_from_the_player_view() {
    let engine = engine().await;
    for origin in [
        json!("player"),
        json!("joueur"),
        json!(""),
        Value::Null,
        json!(1),
    ] {
        rejected_by(
            &written(
                &engine,
                "test.playSound",
                with(playback(PORTE, SON_PORTE, "pastille"), "origin", origin),
            )
            .await,
            &["dm-side-only"],
        );
    }
    // Nor from nowhere said.
    let Value::Object(mut silent) = playback(PORTE, SON_PORTE, "pastille") else {
        unreachable!()
    };
    silent.remove("origin");
    rejected_by(
        &written(&engine, "test.playSound", Value::Object(silent)).await,
        &["dm-side-only"],
    );
    // Nothing was written: the door can still be played from the pastille.
    applied(
        &written(
            &engine,
            "test.playSound",
            playback(PORTE, SON_PORTE, "pastille"),
        )
        .await,
    );
}

#[tokio::test]
async fn dm_side_only_rejects_a_playback_moved_to_the_player_view_and_keeps_the_row() {
    let engine = with_foule().await;
    rejected_by(
        &written(
            &engine,
            "test.updatePlayback",
            json!({"id": FOULE, "origin": "player"}),
        )
        .await,
        &["dm-side-only"],
    );
    // The row kept the soundbox: a stop, judged on the whole row it leaves,
    // applies.
    applied(&stop(&engine, FOULE, json!(STOPPED)).await);
}

#[tokio::test]
async fn a_playback_breaking_two_invariants_is_rejected_under_both() {
    let engine = engine().await;
    rejected_by(
        &written(
            &engine,
            "test.playSound",
            with(
                playback(PORTE, SON_PORTE, "player"),
                "stoppedAt",
                json!(STOPPED),
            ),
        )
        .await,
        &["simultaneous-playback", "dm-side-only"],
    );
}

#[tokio::test]
async fn removing_a_playback_is_a_tombstone_and_passes() {
    let engine = with_foule().await;
    applied(&stop(&engine, FOULE, json!(STOPPED)).await);
    // What a removal carries is not judged: nothing of the row is left.
    applied(
        &written(
            &engine,
            "test.removePlayback",
            json!({"id": FOULE, "origin": "player", "stoppedAt": null}),
        )
        .await,
    );
}

#[tokio::test]
async fn another_aggregate_s_origin_and_stop_are_not_judged() {
    let engine = engine().await;
    applied(
        &written(
            &engine,
            "test.addSound",
            json!({
                "id": SON_PORTE,
                "campaignId": VALOMBRE,
                "name": "Porte qui grince",
                "filePath": "sons/porte.ogg",
                "tags": ["porte"],
                "volume": 0.8,
                "origin": "player",
                "stoppedAt": STOPPED,
            }),
        )
        .await,
    );
}
