//! The Diffusion aggregate's invariants, judged by the DataGuard as
//! `plant()` binds it, over the in-memory store: a command that breaks one
//! is rejected under its id, and the row stays as it was. The
//! DataCapabilities here are test-only — no DataCapability of the product
//! writes a diffusion yet. That the background's media is an illustration
//! and its instance the session's reads rows the engine does not hold beside
//! the diffusion: the engine judges what the row itself says.

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

/// What a session's player view shows, written under the session its
/// payload names.
const SHOW: &str = r#"{"dataCapability":"test.showOnPlayerView","owner":"dataguard","version":1,"effect":"upsert","target":{"aggregate":"Diffusion","id":"$sessionId"},"mode":"overwrite","callableBy":["*"]}"#;
/// The same, written under an id of the payload's own: a row its session
/// may not be.
const SHOW_UNDER: &str = r#"{"dataCapability":"test.showUnder","owner":"dataguard","version":1,"effect":"upsert","target":{"aggregate":"Diffusion","id":"$id"},"mode":"overwrite","callableBy":["*"]}"#;
/// A diffusion deleted: the engine's soft delete, a tombstone.
const DELETE: &str = r#"{"dataCapability":"test.deleteDiffusion","owner":"dataguard","version":1,"effect":"delete","target":{"aggregate":"Diffusion","id":"$sessionId"},"mode":"overwrite","callableBy":["*"]}"#;

/// Valombre's session 1.
const SEANCE: &str = "51000000-0000-4000-8000-000000000001";
/// Valombre's session 2.
const SEANCE_2: &str = "51000000-0000-4000-8000-000000000002";
/// Valombre's session 3, an id with letters.
const SEANCE_3: &str = "51000000-0000-4000-8000-00000000000a";
/// An illustration of 800 × 1200.
const PORTRAIT: &str = "d1000000-0000-4000-8000-000000000002";
/// The tavern's battlemap instance, of session 1.
const INSTANCE: &str = "b1000000-0000-4000-8000-000000000001";

/// The engine as the server plants this aggregate — its manifest and its
/// plugin — over the in-memory store.
async fn plant() -> harness_dataguard::Result<Engine> {
    let engine = Engine::new(
        Arc::new(MemoryStore::default()),
        Arc::new(MemoryNotifier::default()),
        Arc::new(SystemClock),
    )
    .with_aggregate(Aggregate::parse(include_str!("../aggregate.json")).expect("contract I"))
    .with_plugins(std::iter::once(aggregate_diffusion::plugin()).collect());
    engine.load().await?;
    Ok(engine)
}

/// The engine as the server plants it, with the test-only DataCapabilities.
async fn engine() -> Engine {
    let mut engine = plant().await.expect("the in-memory engine");
    for manifest in [SHOW, SHOW_UNDER, DELETE] {
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
                from: "live.player-stage".to_string(),
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

fn rejected_by(result: &CommandResult, id: &str) {
    assert_eq!(result.status, State::Rejected, "{result:?}");
    assert_eq!(result.violations, [id], "{result:?}");
}

fn applied(result: &CommandResult) {
    assert_eq!(result.status, State::Applied, "{result:?}");
}

/// Session 1's player view on the campaign's cover, no window open.
fn cover() -> Value {
    json!({
        "sessionId": SEANCE,
        "baseKind": "cover",
        "baseMediaId": null,
        "baseInstanceId": null,
        "windowMediaId": null,
        "windowX": null,
        "windowY": null,
        "windowWidth": null,
        "viewZoom": 1,
        "viewPanX": 0,
        "viewPanY": 0,
    })
}

/// Session 1's player view on the portrait, in a window over the cover.
fn window() -> Value {
    json!({
        "sessionId": SEANCE,
        "windowMediaId": PORTRAIT,
        "windowX": 0.1,
        "windowY": 0.2,
        "windowWidth": 0.3,
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

/// An engine where session 1 shows its cover.
async fn shown() -> Engine {
    let engine = engine().await;
    applied(&written(&engine, "test.showOnPlayerView", cover()).await);
    engine
}

#[tokio::test]
async fn base_coherent_lets_a_cover_an_illustration_and_a_battlemap_each_show_their_own() {
    let engine = shown().await;
    applied(
        &written(
            &engine,
            "test.showOnPlayerView",
            json!({ "sessionId": SEANCE, "baseKind": "illustration", "baseMediaId": PORTRAIT }),
        )
        .await,
    );
    applied(
        &written(
            &engine,
            "test.showOnPlayerView",
            json!({
                "sessionId": SEANCE,
                "baseKind": "battlemap",
                "baseMediaId": null,
                "baseInstanceId": INSTANCE,
                "viewZoom": "1.500",
                "viewPanX": "-120.00",
                "viewPanY": 40,
            }),
        )
        .await,
    );
    applied(&written(&engine, "test.showOnPlayerView", cover()).await);
}

#[tokio::test]
async fn base_coherent_rejects_a_background_that_is_not_its_kind_s() {
    let shown = shown().await;
    for (kind, media, instance) in [
        // A cover has neither a media nor an instance.
        ("cover", json!(PORTRAIT), Value::Null),
        ("cover", Value::Null, json!(INSTANCE)),
        // An illustration has its media, and its media alone.
        ("illustration", Value::Null, Value::Null),
        ("illustration", Value::Null, json!(INSTANCE)),
        ("illustration", json!(PORTRAIT), json!(INSTANCE)),
        // A battlemap has its instance, and its instance alone.
        ("battlemap", Value::Null, Value::Null),
        ("battlemap", json!(PORTRAIT), Value::Null),
        ("battlemap", json!(PORTRAIT), json!(INSTANCE)),
        // No other background is shown.
        ("video", Value::Null, Value::Null),
    ] {
        rejected_by(
            &written(
                &shown,
                "test.showOnPlayerView",
                json!({
                    "sessionId": SEANCE,
                    "baseKind": kind,
                    "baseMediaId": media,
                    "baseInstanceId": instance,
                }),
            )
            .await,
            "base-coherent",
        );
    }
    // A player view whose kind the row does not say shows nothing coherent.
    let engine = engine().await;
    rejected_by(
        &written(
            &engine,
            "test.showOnPlayerView",
            without(cover(), "baseKind"),
        )
        .await,
        "base-coherent",
    );
    // Nothing was written: the cover can still be shown.
    applied(&written(&engine, "test.showOnPlayerView", cover()).await);
}

#[tokio::test]
async fn window_fields_together_opens_and_closes_a_window_whole() {
    let engine = shown().await;
    applied(&written(&engine, "test.showOnPlayerView", window()).await);
    // Moved and resized, it keeps its three fields.
    applied(
        &written(
            &engine,
            "test.showOnPlayerView",
            json!({ "sessionId": SEANCE, "windowX": "0.4500", "windowWidth": 0.5 }),
        )
        .await,
    );
    applied(
        &written(
            &engine,
            "test.showOnPlayerView",
            json!({
                "sessionId": SEANCE,
                "windowMediaId": null,
                "windowX": null,
                "windowY": null,
                "windowWidth": null,
            }),
        )
        .await,
    );
}

#[tokio::test]
async fn window_fields_together_rejects_a_window_missing_a_field_or_its_media() {
    let engine = shown().await;
    for field in ["windowX", "windowY", "windowWidth"] {
        // An open window missing its position or its width.
        rejected_by(
            &written(
                &engine,
                "test.showOnPlayerView",
                with(window(), field, Value::Null),
            )
            .await,
            "window-fields-together",
        );
        // A closed window keeping one.
        rejected_by(
            &written(
                &engine,
                "test.showOnPlayerView",
                json!({ "sessionId": SEANCE, field: window()[field].clone() }),
            )
            .await,
            "window-fields-together",
        );
    }
    // A window with every field, and no media to show.
    rejected_by(
        &written(
            &engine,
            "test.showOnPlayerView",
            with(window(), "windowMediaId", Value::Null),
        )
        .await,
        "window-fields-together",
    );
    applied(&written(&engine, "test.showOnPlayerView", window()).await);
    // Its media taken away, the window does not stay open on nothing.
    rejected_by(
        &written(
            &engine,
            "test.showOnPlayerView",
            json!({ "sessionId": SEANCE, "windowMediaId": null }),
        )
        .await,
        "window-fields-together",
    );
}

#[tokio::test]
async fn window_proportional_rejects_a_stored_height_or_a_width_its_height_cannot_follow() {
    let engine = shown().await;
    // The height follows from the width and the media's proportions: it is
    // never stored.
    rejected_by(
        &written(
            &engine,
            "test.showOnPlayerView",
            with(window(), "windowHeight", json!(0.45)),
        )
        .await,
        "window-proportional",
    );
    // A height said to be none is none stored.
    applied(
        &written(
            &engine,
            "test.showOnPlayerView",
            with(window(), "windowHeight", Value::Null),
        )
        .await,
    );
    for width in [json!(0), json!(-0.3), json!("large"), json!("inf")] {
        rejected_by(
            &written(
                &engine,
                "test.showOnPlayerView",
                with(window(), "windowWidth", width),
            )
            .await,
            "window-proportional",
        );
    }
    applied(
        &written(
            &engine,
            "test.showOnPlayerView",
            with(window(), "windowWidth", json!("0.3000")),
        )
        .await,
    );
}

#[tokio::test]
async fn single_diffusion_source_keeps_one_row_per_session() {
    let empty = engine().await;
    let engine = shown().await;
    // Another session has a row of its own.
    applied(
        &written(
            &engine,
            "test.showOnPlayerView",
            with(cover(), "sessionId", json!(SEANCE_2)),
        )
        .await,
    );
    // A row under the session's id is its own.
    applied(
        &written(
            &engine,
            "test.showUnder",
            with(window(), "id", json!(SEANCE)),
        )
        .await,
    );
    // A row under another id may not say it is session 1's: a second row of
    // the session would give the two views two sources.
    rejected_by(
        &written(
            &engine,
            "test.showUnder",
            with(cover(), "id", json!(SEANCE_2)),
        )
        .await,
        "single-diffusion-source",
    );
    rejected_by(
        &written(
            &engine,
            "test.showUnder",
            with(cover(), "id", json!("51000000-0000-4000-8000-0000000000ff")),
        )
        .await,
        "single-diffusion-source",
    );
    // A new row says whose it is, in the very text it is keyed by: the
    // same session written another way would be a second row.
    rejected_by(
        &written(
            &empty,
            "test.showUnder",
            with(without(cover(), "sessionId"), "id", json!(SEANCE)),
        )
        .await,
        "single-diffusion-source",
    );
    applied(
        &written(
            &engine,
            "test.showOnPlayerView",
            with(cover(), "sessionId", json!(SEANCE_3)),
        )
        .await,
    );
    rejected_by(
        &written(
            &engine,
            "test.showUnder",
            with(
                with(cover(), "sessionId", json!(SEANCE_3)),
                "id",
                json!(SEANCE_3.to_uppercase()),
            ),
        )
        .await,
        "single-diffusion-source",
    );
}

#[tokio::test]
async fn a_diffusion_deleted_is_a_tombstone_no_invariant_judges() {
    let engine = shown().await;
    applied(
        &written(
            &engine,
            "test.deleteDiffusion",
            json!({ "sessionId": SEANCE }),
        )
        .await,
    );
    // Nor one the engine never held, whose row is its session alone: no
    // kind for base-coherent to judge, and none needed to delete it.
    applied(
        &written(
            &engine,
            "test.deleteDiffusion",
            json!({ "sessionId": SEANCE_2 }),
        )
        .await,
    );
}
