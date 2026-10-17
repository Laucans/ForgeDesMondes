//! The Campaign aggregate's invariants, judged by the DataGuard as `plant()`
//! binds it, over the in-memory store: a command that breaks one is
//! rejected under its id, and the row stays as it was. The DataCapabilities
//! here are test-only — no DataCapability of the product writes a campaign
//! yet — but for `campaign.exportCampaign`, written under the id the
//! invariant knows it by.

use dataguard_server::{Engine, MEMORY, plant};
use harness_dataguard::api::Submission;
use harness_dataguard::command::{CommandResult, State};
use harness_dataguard::guard::Caller;
use harness_dataguard::manifest::DataCapability;
use serde_json::{Value, json};

/// A campaign created under the id its payload names.
const CREATE: &str = r#"{"dataCapability":"test.createCampaign","owner":"dataguard","version":1,"effect":"insert","target":{"aggregate":"Campaign","id":"$id"},"mode":"overwrite","callableBy":["*"]}"#;
/// A campaign's fields changed.
const UPDATE: &str = r#"{"dataCapability":"test.updateCampaign","owner":"dataguard","version":1,"effect":"update","target":{"aggregate":"Campaign","id":"$id"},"mode":"overwrite","callableBy":["*"]}"#;
/// A campaign deleted: the engine's soft delete, a tombstone.
const DELETE: &str = r#"{"dataCapability":"test.deleteCampaign","owner":"dataguard","version":1,"effect":"delete","target":{"aggregate":"Campaign","id":"$id"},"mode":"overwrite","callableBy":["*"]}"#;
/// An export to an archive, under the id the invariant knows it by.
const EXPORT: &str = r#"{"dataCapability":"campaign.exportCampaign","owner":"dataguard","version":1,"effect":"update","target":{"aggregate":"Campaign","id":"$id"},"mode":"overwrite","callableBy":["*"]}"#;
/// An export that would create its campaign rather than stamp it.
const EXPORT_AS_INSERT: &str = r#"{"dataCapability":"campaign.exportCampaign","owner":"dataguard","version":2,"effect":"insert","target":{"aggregate":"Campaign","id":"$id"},"mode":"overwrite","callableBy":["*"]}"#;

const VALOMBRE: &str = "c1000000-0000-4000-8000-000000000001";
const COPY: &str = "c1000000-0000-4000-8000-000000000002";

/// The engine as the server plants it, with the test-only DataCapabilities.
async fn engine() -> Engine {
    let mut engine = plant(MEMORY).await.expect("the in-memory engine");
    for manifest in [CREATE, UPDATE, DELETE, EXPORT] {
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
                from: "campaign.home".to_string(),
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

/// Les Cendres de Valombre, as the home page shows it.
fn valombre() -> Value {
    json!({
        "id": VALOMBRE,
        "name": "Les Cendres de Valombre",
        "kind": "campagne",
        "pitch": "Une cité sous la cendre.",
        "art": "ember",
        "glyph": "V",
        "soundMasterVolume": 0.8,
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

/// An engine where Valombre exists.
async fn with_valombre() -> Engine {
    let engine = engine().await;
    applied(&written(&engine, "test.createCampaign", valombre()).await);
    engine
}

#[tokio::test]
async fn master_volume_range_rejects_a_campaign_written_at_1_2() {
    let engine = engine().await;
    rejected_by(
        &written(
            &engine,
            "test.createCampaign",
            with(valombre(), "soundMasterVolume", json!(1.2)),
        )
        .await,
        "master-volume-range",
    );
    // Nothing was written: the campaign can still be created, in range.
    applied(&written(&engine, "test.createCampaign", valombre()).await);
}

#[tokio::test]
async fn master_volume_range_rejects_a_volume_moved_out_of_range() {
    let engine = with_valombre().await;
    for volume in [
        json!(1.2),
        json!(-0.1),
        json!("1.5"),
        json!("fort"),
        Value::Null,
    ] {
        rejected_by(
            &written(
                &engine,
                "test.updateCampaign",
                json!({ "id": VALOMBRE, "soundMasterVolume": volume }),
            )
            .await,
            "master-volume-range",
        );
    }
}

#[tokio::test]
async fn master_volume_range_lets_0_to_1_through() {
    let engine = with_valombre().await;
    for volume in [json!(0), json!(0.35), json!(1), json!(1.0), json!("0.50")] {
        applied(
            &written(
                &engine,
                "test.updateCampaign",
                json!({ "id": VALOMBRE, "soundMasterVolume": volume }),
            )
            .await,
        );
    }
}

#[tokio::test]
async fn oneshot_single_session_lets_a_campagne_or_a_oneshot_through() {
    let engine = engine().await;
    applied(&written(&engine, "test.createCampaign", valombre()).await);
    applied(
        &written(
            &engine,
            "test.createCampaign",
            with(
                with(valombre(), "id", json!(COPY)),
                "kind",
                json!("oneshot"),
            ),
        )
        .await,
    );
}

#[tokio::test]
async fn oneshot_single_session_rejects_a_campaign_of_no_known_kind() {
    let engine = engine().await;
    for payload in [
        without(valombre(), "kind"),
        with(valombre(), "kind", json!("one-shot")),
        with(valombre(), "kind", json!("")),
        with(valombre(), "kind", Value::Null),
    ] {
        rejected_by(
            &written(&engine, "test.createCampaign", payload).await,
            "oneshot-single-session",
        );
    }
    let engine = with_valombre().await;
    rejected_by(
        &written(
            &engine,
            "test.updateCampaign",
            json!({ "id": VALOMBRE, "kind": "campaign" }),
        )
        .await,
        "oneshot-single-session",
    );
    // A campaign the engine does not hold is not created by a write to it.
    rejected_by(
        &written(
            &engine,
            "test.updateCampaign",
            json!({ "id": COPY, "name": "Nulle part" }),
        )
        .await,
        "oneshot-single-session",
    );
}

#[tokio::test]
async fn import_creates_copy_lets_a_new_campaign_through_whatever_its_origin() {
    let engine = with_valombre().await;
    // The archive names Valombre as its origin, which exists here: the
    // import is a new campaign all the same.
    let copy = with(
        with(
            with(valombre(), "id", json!(COPY)),
            "originCampaignId",
            json!(VALOMBRE),
        ),
        "importedAt",
        json!("2026-10-17T09:00:00Z"),
    );
    applied(&written(&engine, "test.createCampaign", copy).await);
}

#[tokio::test]
async fn import_creates_copy_rejects_an_import_onto_its_original() {
    let engine = with_valombre().await;
    let onto_original = with(
        with(valombre(), "originCampaignId", json!(VALOMBRE)),
        "importedAt",
        json!("2026-10-17T09:00:00Z"),
    );
    rejected_by(
        &written(&engine, "test.createCampaign", onto_original).await,
        "import-creates-copy",
    );
    // Told apart whatever the uuid's case.
    rejected_by(
        &written(
            &engine,
            "test.createCampaign",
            with(
                valombre(),
                "originCampaignId",
                json!(VALOMBRE.to_uppercase()),
            ),
        )
        .await,
        "import-creates-copy",
    );
}

/// Valombre's copy, imported from its archive.
fn copy_of_valombre() -> Value {
    with(
        with(
            with(valombre(), "id", json!(COPY)),
            "originCampaignId",
            json!(VALOMBRE),
        ),
        "importedAt",
        json!("2026-10-17T09:00:00Z"),
    )
}

#[tokio::test]
async fn import_creates_copy_rejects_an_import_onto_any_existing_campaign() {
    let engine = with_valombre().await;
    applied(&written(&engine, "test.createCampaign", copy_of_valombre()).await);
    // A second import of the archive, onto the copy's id: the copy is not
    // the original, but it exists, and stays as it is.
    rejected_by(
        &written(
            &engine,
            "test.createCampaign",
            with(copy_of_valombre(), "name", json!("Écrasée")),
        )
        .await,
        "import-creates-copy",
    );
}

#[tokio::test]
async fn import_creates_copy_lets_a_copy_saved_whole_keep_its_origin() {
    let engine = with_valombre().await;
    applied(&written(&engine, "test.createCampaign", copy_of_valombre()).await);
    // Read whole, renamed, written back whole: the origin did not move.
    let saved = without(
        with(copy_of_valombre(), "name", json!("Valombre, la copie")),
        "kind",
    );
    applied(&written(&engine, "test.updateCampaign", saved).await);
    // Moving it is an import onto a campaign that exists.
    rejected_by(
        &written(
            &engine,
            "test.updateCampaign",
            json!({ "id": COPY, "originCampaignId": "c1000000-0000-4000-8000-000000000009" }),
        )
        .await,
        "import-creates-copy",
    );
}

#[tokio::test]
async fn import_creates_copy_rejects_writing_an_origin_onto_an_existing_campaign() {
    let engine = with_valombre().await;
    for payload in [
        json!({ "id": VALOMBRE, "originCampaignId": COPY }),
        json!({ "id": VALOMBRE, "importedAt": "2026-10-17T09:00:00Z" }),
    ] {
        rejected_by(
            &written(&engine, "test.updateCampaign", payload).await,
            "import-creates-copy",
        );
    }
}

#[tokio::test]
async fn export_stamps_date_lets_the_stamp_through_and_nothing_else_moves() {
    let engine = with_valombre().await;
    applied(
        &written(
            &engine,
            "campaign.exportCampaign",
            json!({ "id": VALOMBRE, "lastExportedAt": "2026-10-17T09:00:00Z" }),
        )
        .await,
    );
    // The campaign named whatever its uuid's case.
    applied(
        &written(
            &engine,
            "campaign.exportCampaign",
            json!({
                "id": VALOMBRE,
                "campaignId": VALOMBRE.to_uppercase(),
                "lastExportedAt": "2026-10-17T10:00:00Z",
            }),
        )
        .await,
    );
}

#[tokio::test]
async fn export_stamps_date_rejects_an_export_of_a_campaign_that_does_not_exist() {
    let engine = with_valombre().await;
    let result = written(
        &engine,
        "campaign.exportCampaign",
        json!({ "id": COPY, "lastExportedAt": "2026-10-17T09:00:00Z" }),
    )
    .await;
    // A row with no kind is refused for that too.
    assert_eq!(result.status, State::Rejected, "{result:?}");
    assert!(
        result
            .violations
            .iter()
            .any(|id| id == "export-stamps-date"),
        "{result:?}"
    );
}

#[tokio::test]
async fn export_stamps_date_rejects_an_export_that_writes_anything_else() {
    let engine = with_valombre().await;
    for payload in [
        json!({ "id": VALOMBRE, "lastExportedAt": "2026-10-17T09:00:00Z", "name": "Autre" }),
        json!({ "id": VALOMBRE, "lastExportedAt": "2026-10-17T09:00:00Z", "soundMasterVolume": 0.2 }),
        json!({ "id": VALOMBRE, "lastExportedAt": "2026-10-17T09:00:00Z", "campaignId": COPY }),
        // An export that stamps no date.
        json!({ "id": VALOMBRE }),
        json!({ "id": VALOMBRE, "lastExportedAt": Value::Null }),
        json!({ "id": VALOMBRE, "lastExportedAt": "" }),
    ] {
        rejected_by(
            &written(&engine, "campaign.exportCampaign", payload).await,
            "export-stamps-date",
        );
    }
}

#[tokio::test]
async fn export_stamps_date_rejects_an_export_that_creates_its_campaign() {
    let engine = engine()
        .await
        .with_data_capability(DataCapability::parse(EXPORT_AS_INSERT).expect("contract F"));
    let result = written(
        &engine,
        "campaign.exportCampaign@2",
        json!({ "id": VALOMBRE, "lastExportedAt": "2026-10-17T09:00:00Z" }),
    )
    .await;
    // A campaign created without its kind is refused for that too.
    assert_eq!(result.status, State::Rejected, "{result:?}");
    assert!(
        result
            .violations
            .iter()
            .any(|id| id == "export-stamps-date"),
        "{result:?}"
    );
}

#[tokio::test]
async fn a_deleted_campaign_is_not_judged_by_its_invariants() {
    let engine = with_valombre().await;
    applied(&written(&engine, "test.deleteCampaign", json!({ "id": VALOMBRE })).await);
    // Nor one the engine never held, whose row it knows by its id alone: no
    // kind for oneshot-single-session to judge, and none needed to delete it.
    applied(&written(&engine, "test.deleteCampaign", json!({ "id": COPY })).await);
}
