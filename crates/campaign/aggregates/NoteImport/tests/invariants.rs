//! The NoteImport aggregate's invariant, judged by the DataGuard as `plant()`
//! binds it, over the in-memory store: a command that creates an element
//! when the notes are analysed, for an unchecked proposal, or a second time
//! for the same proposal is rejected under `no-creation-before-validation`,
//! and the row stays as it was. The DataCapabilities here are test-only: no
//! DataCapability of the product writes a note's proposals yet.

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

/// A proposal written by the analysis of a note.
const PROPOSE: &str = r#"{"dataCapability":"test.proposeFromNotes","owner":"dataguard","version":1,"effect":"insert","target":{"aggregate":"NoteImport","id":"$id"},"mode":"overwrite","callableBy":["*"]}"#;
/// A proposal written by the analysis, or written again.
const UPSERT: &str = r#"{"dataCapability":"test.upsertProposal","owner":"dataguard","version":1,"effect":"upsert","target":{"aggregate":"NoteImport","id":"$id"},"mode":"overwrite","callableBy":["*"]}"#;
/// A proposal checked or unchecked by the DM, or validated.
const UPDATE: &str = r#"{"dataCapability":"test.updateProposal","owner":"dataguard","version":1,"effect":"update","target":{"aggregate":"NoteImport","id":"$id"},"touches":["NoteImport.checked","NoteImport.createdEntityId"],"mode":"overwrite","callableBy":["*"]}"#;
/// A proposal removed: the engine's soft delete, a tombstone.
const REMOVE: &str = r#"{"dataCapability":"test.removeProposal","owner":"dataguard","version":1,"effect":"delete","target":{"aggregate":"NoteImport","id":"$id"},"mode":"overwrite","callableBy":["*"]}"#;
/// An element written: another aggregate, which this invariant leaves alone.
const ELEMENT: &str = r#"{"dataCapability":"test.addCampaign","owner":"dataguard","version":1,"effect":"insert","target":{"aggregate":"Campaign","id":"$id"},"mode":"overwrite","callableBy":["*"]}"#;

const VALOMBRE: &str = "c1000000-0000-4000-8000-000000000001";
const NOTES: &str = "a1000000-0000-4000-8000-000000000001";
const GRIMBALD: &str = "f1000000-0000-4000-8000-000000000002";
const CREATED: &str = "e1000000-0000-4000-8000-0000000000aa";
const OTHER: &str = "e1000000-0000-4000-8000-0000000000ab";

/// The engine as the server plants this aggregate — its manifest and its
/// plugin — over the in-memory store.
async fn plant() -> harness_dataguard::Result<Engine> {
    let engine = Engine::new(
        Arc::new(MemoryStore::default()),
        Arc::new(MemoryNotifier::default()),
        Arc::new(SystemClock),
    )
    .with_aggregate(Aggregate::parse(include_str!("../aggregate.json")).expect("contract I"))
    .with_plugins(std::iter::once(aggregate_note_import::plugin()).collect());
    engine.load().await?;
    Ok(engine)
}

/// The engine as the server plants it, with the test-only DataCapabilities.
async fn engine() -> Engine {
    let mut engine = plant().await.expect("the in-memory engine");
    for manifest in [PROPOSE, UPSERT, UPDATE, REMOVE, ELEMENT] {
        engine = engine.with_data_capability(DataCapability::parse(manifest).expect("contract F"));
    }
    engine
}

/// What the single applier made of one command.
async fn written(engine: &Engine, data_capability: &str, payload: Value) -> CommandResult {
    queued(engine, data_capability, payload).await;
    let mut applied = engine.run_once().await.expect("the applier ran");
    assert_eq!(applied.len(), 1, "{applied:?}");
    applied.remove(0)
}

/// One command, queued: what the engine answers.
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
                from: "campaign.noteImport".to_string(),
                permissions: Vec::new(),
                // The critical fields' review is not what these tests judge.
                roles: vec!["role:approver".to_string()],
            },
        })
        .await
        .expect("queued")
}

fn rejected_by(result: &CommandResult, id: &str) {
    assert_eq!(result.status, State::Rejected, "{result:?}");
    assert_eq!(result.violations, [id], "{result:?}");
}

fn applied(result: &CommandResult) {
    assert_eq!(result.status, State::Applied, "{result:?}");
}

/// Grimbald, proposed from Valombre's notes.
fn grimbald(checked: bool) -> Value {
    json!({
        "id": GRIMBALD,
        "noteProposalId": GRIMBALD,
        "noteId": NOTES,
        "type": "pnj",
        "name": "Grimbald",
        "excerpt": "Morlox a vendu la carte à Grimbald",
        "checked": checked,
    })
}

/// An engine where the analysis proposed Grimbald, `checked` or not.
async fn proposed(checked: bool) -> Engine {
    let engine = engine().await;
    applied(&written(&engine, "test.proposeFromNotes", grimbald(checked)).await);
    engine
}

#[tokio::test]
async fn no_creation_before_validation_rejects_an_analysis_that_creates_an_element() {
    let engine = engine().await;
    for checked in [true, false] {
        let mut proposal = grimbald(checked);
        proposal["createdEntityId"] = json!(CREATED);
        rejected_by(
            &written(&engine, "test.proposeFromNotes", proposal).await,
            "no-creation-before-validation",
        );
    }
    // Nor by an upsert of a proposal not written yet.
    let mut proposal = grimbald(true);
    proposal["createdEntityId"] = json!(CREATED);
    rejected_by(
        &written(&engine, "test.upsertProposal", proposal).await,
        "no-creation-before-validation",
    );
    // Nothing was written: the analysis can still propose Grimbald, with a
    // recognised element if it found one.
    let mut proposal = grimbald(false);
    proposal["existingEntityId"] = json!(OTHER);
    applied(&written(&engine, "test.proposeFromNotes", proposal).await);
}

#[tokio::test]
async fn no_creation_before_validation_rejects_creating_an_unchecked_proposal() {
    let engine = proposed(false).await;
    rejected_by(
        &written(
            &engine,
            "test.updateProposal",
            json!({ "id": GRIMBALD, "createdEntityId": CREATED }),
        )
        .await,
        "no-creation-before-validation",
    );
    // Checked by the DM, then validated: created.
    applied(
        &written(
            &engine,
            "test.updateProposal",
            json!({ "id": GRIMBALD, "checked": true }),
        )
        .await,
    );
    applied(
        &written(
            &engine,
            "test.updateProposal",
            json!({ "id": GRIMBALD, "createdEntityId": CREATED }),
        )
        .await,
    );
}

#[tokio::test]
async fn no_creation_before_validation_lets_check_and_create_go_together() {
    let engine = proposed(false).await;
    applied(
        &written(
            &engine,
            "test.updateProposal",
            json!({ "id": GRIMBALD, "checked": true, "createdEntityId": CREATED }),
        )
        .await,
    );
}

#[tokio::test]
async fn no_creation_before_validation_rejects_unchecking_a_created_proposal() {
    let engine = proposed(true).await;
    applied(
        &written(
            &engine,
            "test.updateProposal",
            json!({ "id": GRIMBALD, "createdEntityId": CREATED }),
        )
        .await,
    );
    rejected_by(
        &written(
            &engine,
            "test.updateProposal",
            json!({ "id": GRIMBALD, "checked": false }),
        )
        .await,
        "no-creation-before-validation",
    );
}

#[tokio::test]
async fn no_creation_before_validation_rejects_creating_a_proposal_s_element_twice() {
    let engine = proposed(true).await;
    applied(
        &written(
            &engine,
            "test.updateProposal",
            json!({ "id": GRIMBALD, "createdEntityId": CREATED }),
        )
        .await,
    );
    for again in [json!(OTHER), Value::Null] {
        rejected_by(
            &written(
                &engine,
                "test.updateProposal",
                json!({ "id": GRIMBALD, "createdEntityId": again }),
            )
            .await,
            "no-creation-before-validation",
        );
    }
    // The same element, written again, is no new creation.
    applied(
        &written(
            &engine,
            "test.updateProposal",
            json!({ "id": GRIMBALD, "createdEntityId": CREATED }),
        )
        .await,
    );
}

#[tokio::test]
async fn no_creation_before_validation_warns_no_validation_when_it_is_queued() {
    let engine = proposed(true).await;
    let submitted = engine
        .submit(Submission {
            data_capability: "test.updateProposal".to_string(),
            payload: [
                ("id".to_string(), json!(GRIMBALD)),
                ("checked".to_string(), json!(true)),
                ("createdEntityId".to_string(), json!(CREATED)),
            ]
            .into_iter()
            .collect(),
            target_id: None,
            based_on: None,
            idempotency_key: None,
            caller: Caller {
                by: "dm".to_string(),
                from: "campaign.noteImport".to_string(),
                permissions: Vec::new(),
                roles: vec!["role:approver".to_string()],
            },
        })
        .await
        .expect("queued");
    // The row an update reads when it is queued is empty: it is no row
    // written now, whose element the analysis would create.
    assert!(submitted.warnings.is_empty(), "{:?}", submitted.warnings);
    let mut applied_now = engine.run_once().await.expect("the applier ran");
    assert_eq!(applied_now.len(), 1, "{applied_now:?}");
    applied(&applied_now.remove(0));
}

#[tokio::test]
async fn no_creation_before_validation_still_refuses_a_birth_or_a_second_element() {
    let at_risk = ["invariant(s) at risk: no-creation-before-validation"];
    let mut born = grimbald(true);
    born["createdEntityId"] = json!(CREATED);
    // An insert, and an upsert of a row the engine does not hold: born now.
    for data_capability in ["test.proposeFromNotes", "test.upsertProposal"] {
        let engine = engine().await;
        let submitted = queued(&engine, data_capability, born.clone()).await;
        assert_eq!(submitted.warnings, at_risk, "{data_capability}");
        let mut applied_now = engine.run_once().await.expect("the applier ran");
        assert_eq!(applied_now.len(), 1, "{applied_now:?}");
        rejected_by(&applied_now.remove(0), "no-creation-before-validation");
    }
    // An update replacing the element a proposal created: queued, it reads
    // no row; applied, it reads the element and is refused.
    let engine = proposed(true).await;
    applied(
        &written(
            &engine,
            "test.updateProposal",
            json!({ "id": GRIMBALD, "createdEntityId": CREATED }),
        )
        .await,
    );
    rejected_by(
        &written(
            &engine,
            "test.updateProposal",
            json!({ "id": GRIMBALD, "createdEntityId": OTHER }),
        )
        .await,
        "no-creation-before-validation",
    );
}

#[tokio::test]
async fn no_creation_before_validation_lets_the_dm_check_and_uncheck_freely() {
    let engine = proposed(true).await;
    for checked in [false, true, false] {
        applied(
            &written(
                &engine,
                "test.updateProposal",
                json!({ "id": GRIMBALD, "checked": checked }),
            )
            .await,
        );
    }
}

#[tokio::test]
async fn no_creation_before_validation_leaves_a_proposal_s_removal_to_the_tombstone() {
    let engine = proposed(true).await;
    applied(
        &written(
            &engine,
            "test.updateProposal",
            json!({ "id": GRIMBALD, "createdEntityId": CREATED }),
        )
        .await,
    );
    // Even a removal that clears the element and the check: nothing of the
    // row is left to judge.
    applied(
        &written(
            &engine,
            "test.removeProposal",
            json!({ "id": GRIMBALD, "checked": false, "createdEntityId": Value::Null }),
        )
        .await,
    );
}

#[tokio::test]
async fn no_creation_before_validation_judges_note_imports_alone() {
    // A campaign has no proposal: its fields are not this invariant's.
    let engine = engine().await;
    applied(
        &written(
            &engine,
            "test.addCampaign",
            json!({
                "id": VALOMBRE,
                "name": "Les Cendres de Valombre",
                "kind": "campagne",
                "createdEntityId": CREATED,
            }),
        )
        .await,
    );
}
