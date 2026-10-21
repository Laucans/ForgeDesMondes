//! The Media aggregate's invariants, judged by the DataGuard as `plant()`
//! binds it, over the in-memory store: a command that breaks one is
//! rejected under its id, and the row stays as it was. The DataCapabilities
//! here are test-only — no DataCapability of the product writes a media yet.
//! `media-place-is-lieu` reads the place, which the engine does not hold
//! beside the media: the table judges it (`tests/media_migration.rs`), and
//! the engine lets it through. `feetPerHex` is derived: the engine refuses
//! any command writing it before it is queued, so `hex-is-five-feet` judges
//! a row no command may change.

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

/// A media uploaded under the id its payload names.
const CREATE: &str = r#"{"dataCapability":"test.uploadMedia","owner":"dataguard","version":1,"effect":"insert","target":{"aggregate":"Media","id":"$id"},"mode":"overwrite","callableBy":["*"]}"#;
/// A media's fields, or its grid's, changed.
const UPDATE: &str = r#"{"dataCapability":"test.updateMedia","owner":"dataguard","version":1,"effect":"update","target":{"aggregate":"Media","id":"$id"},"mode":"overwrite","callableBy":["*"]}"#;
/// A media deleted: the engine's soft delete, a tombstone.
const DELETE: &str = r#"{"dataCapability":"test.deleteMedia","owner":"dataguard","version":1,"effect":"delete","target":{"aggregate":"Media","id":"$id"},"mode":"overwrite","callableBy":["*"]}"#;

const CARTE: &str = "a1000000-0000-4000-8000-000000000001";
const PORTRAIT: &str = "a1000000-0000-4000-8000-000000000002";
const VALOMBRE: &str = "c1000000-0000-4000-8000-000000000001";
const TAVERNE: &str = "e1000000-0000-4000-8000-000000000002";

/// The engine as the server plants this aggregate — its manifest and its
/// plugin — over the in-memory store.
async fn plant() -> harness_dataguard::Result<Engine> {
    let engine = Engine::new(
        Arc::new(MemoryStore::default()),
        Arc::new(MemoryNotifier::default()),
        Arc::new(SystemClock),
    )
    .with_aggregate(Aggregate::parse(include_str!("../aggregate.json")).expect("contract I"))
    .with_plugins(std::iter::once(aggregate_media::plugin()).collect());
    engine.load().await?;
    Ok(engine)
}

/// The engine as the server plants it, with the test-only DataCapabilities.
async fn engine() -> Engine {
    let mut engine = plant().await.expect("the in-memory engine");
    for manifest in [CREATE, UPDATE, DELETE] {
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
                from: "media.library".to_string(),
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
                from: "media.library".to_string(),
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

/// The tavern's battlemap, uploaded with its grid.
fn carte() -> Value {
    json!({
        "id": CARTE,
        "campaignId": VALOMBRE,
        "kind": "battlemap",
        "title": "La Taverne du Sanglier",
        "filePath": "media/taverne.png",
        "widthPx": 2048,
        "heightPx": 1536,
        "placeId": TAVERNE,
        "mediaId": CARTE,
        "hexSizePx": 64,
        "offsetX": 12.5,
        "offsetY": 0,
        "orientation": "pointe",
        "color": "#202020",
        "opacity": 0.4,
    })
}

/// Elowen's portrait: an illustration, with no grid.
fn portrait() -> Value {
    json!({
        "id": PORTRAIT,
        "campaignId": VALOMBRE,
        "kind": "illustration",
        "title": "Elowen",
        "filePath": "media/elowen.png",
        "widthPx": 512,
        "heightPx": 512,
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

/// An engine where the battlemap and the portrait exist.
async fn uploaded() -> Engine {
    let engine = engine().await;
    applied(&written(&engine, "test.uploadMedia", carte()).await);
    applied(&written(&engine, "test.uploadMedia", portrait()).await);
    engine
}

#[tokio::test]
async fn a_battlemap_with_its_grid_and_an_illustration_without_are_uploaded() {
    let engine = uploaded().await;
    // Its place is the table's to judge (media-place-is-lieu): the engine
    // does not hold the element.
    applied(
        &written(
            &engine,
            "test.updateMedia",
            json!({ "id": PORTRAIT, "placeId": "e1000000-0000-4000-8000-0000000000ff" }),
        )
        .await,
    );
}

#[tokio::test]
async fn grid_only_for_battlemap_rejects_a_battlemap_uploaded_without_its_grid() {
    let engine = engine().await;
    for field in [
        "hexSizePx",
        "offsetX",
        "offsetY",
        "orientation",
        "color",
        "opacity",
    ] {
        rejected_by(
            &written(&engine, "test.uploadMedia", without(carte(), field)).await,
            "grid-only-for-battlemap",
        );
        rejected_by(
            &written(
                &engine,
                "test.uploadMedia",
                with(carte(), field, Value::Null),
            )
            .await,
            "grid-only-for-battlemap",
        );
    }
    // Nothing was written: the battlemap can still be uploaded, gridded.
    applied(&written(&engine, "test.uploadMedia", carte()).await);
}

#[tokio::test]
async fn grid_only_for_battlemap_rejects_a_grid_removed_from_a_battlemap() {
    let engine = uploaded().await;
    rejected_by(
        &written(
            &engine,
            "test.updateMedia",
            json!({ "id": CARTE, "hexSizePx": null }),
        )
        .await,
        "grid-only-for-battlemap",
    );
}

#[tokio::test]
async fn grid_only_for_battlemap_rejects_a_grid_on_any_other_media() {
    let engine = engine().await;
    for field in ["hexSizePx", "orientation", "mediaId"] {
        rejected_by(
            &written(
                &engine,
                "test.uploadMedia",
                with(portrait(), field, carte()[field].clone()),
            )
            .await,
            "grid-only-for-battlemap",
        );
    }
    // A battlemap's grid sets it, not another media.
    rejected_by(
        &written(
            &engine,
            "test.uploadMedia",
            with(carte(), "mediaId", json!(PORTRAIT)),
        )
        .await,
        "grid-only-for-battlemap",
    );
    // A media whose kind the row does not say has no grid either.
    rejected_by(
        &written(&engine, "test.uploadMedia", without(carte(), "kind")).await,
        "grid-only-for-battlemap",
    );
    let engine = uploaded().await;
    // A battlemap made an illustration keeps a grid it may not have.
    rejected_by(
        &written(
            &engine,
            "test.updateMedia",
            json!({ "id": CARTE, "kind": "illustration" }),
        )
        .await,
        "grid-only-for-battlemap",
    );
    // An illustration made a battlemap has none yet.
    rejected_by(
        &written(
            &engine,
            "test.updateMedia",
            json!({ "id": PORTRAIT, "kind": "battlemap" }),
        )
        .await,
        "grid-only-for-battlemap",
    );
}

#[tokio::test]
async fn hex_is_five_feet_holds_since_no_command_may_write_a_hex_s_worth() {
    let engine = engine().await;
    // hex-is-five-feet: `feetPerHex` is derived, so the engine refuses any
    // command writing it, whatever it says — 5 feet included — on a
    // battlemap or another media, before it is queued.
    for feet in [
        json!(6),
        json!(0),
        json!("10"),
        json!("cinq"),
        json!(5.5),
        json!(5),
    ] {
        let why = refused(
            &engine,
            "test.uploadMedia",
            with(carte(), "feetPerHex", feet),
        )
        .await;
        assert!(why.contains("feetPerHex"), "{why}");
    }
    let why = refused(
        &engine,
        "test.uploadMedia",
        with(portrait(), "feetPerHex", json!(5)),
    )
    .await;
    assert!(why.contains("feetPerHex"), "{why}");
    // Whatever the hex's size, offset or orientation, it is worth 5 feet.
    applied(
        &written(
            &engine,
            "test.uploadMedia",
            with(
                with(
                    with(carte(), "hexSizePx", json!(133.33)),
                    "offsetX",
                    json!(-40),
                ),
                "orientation",
                json!("plat"),
            ),
        )
        .await,
    );
    let why = refused(
        &engine,
        "test.updateMedia",
        json!({ "id": CARTE, "feetPerHex": "5" }),
    )
    .await;
    assert!(why.contains("feetPerHex"), "{why}");
}

#[tokio::test]
async fn grid_values_range_rejects_a_hex_of_no_size_or_an_opacity_out_of_0_to_1() {
    let engine = engine().await;
    for (field, value) in [
        ("hexSizePx", json!(0)),
        ("hexSizePx", json!(-12)),
        ("hexSizePx", json!("grand")),
        ("hexSizePx", json!("inf")),
        ("hexSizePx", json!("NaN")),
        ("opacity", json!(1.01)),
        ("opacity", json!(-0.01)),
        ("opacity", json!("1.5")),
    ] {
        rejected_by(
            &written(&engine, "test.uploadMedia", with(carte(), field, value)).await,
            "grid-values-range",
        );
    }
    applied(&written(&engine, "test.uploadMedia", carte()).await);
    for (field, value) in [
        ("hexSizePx", json!(0.01)),
        ("hexSizePx", json!("64.00")),
        ("opacity", json!(0)),
        ("opacity", json!("1.00")),
    ] {
        applied(
            &written(
                &engine,
                "test.updateMedia",
                json!({ "id": CARTE, field: value }),
            )
            .await,
        );
    }
}

#[tokio::test]
async fn a_media_deleted_is_a_tombstone_no_invariant_judges() {
    let engine = uploaded().await;
    applied(&written(&engine, "test.deleteMedia", json!({ "id": CARTE })).await);
}
