//! soundbox.setMasterVolume as the server plants it beside its aggregate,
//! over the in-memory store: the DM's master slider, in percent, becomes the
//! campaign's `soundMasterVolume`, its fraction, and nothing else of the
//! campaign moves; a volume out of range is rejected by the campaign's own
//! invariant (`master-volume-range`) and the row stays as it was; a request
//! naming no campaign, or one the engine does not hold, is refused before
//! anything is queued. [`prepare`] alone answers what the command writes.

use std::sync::Arc;

use harness_dataguard::Error;
use harness_dataguard::aggregate::Aggregate;
use harness_dataguard::api::Submission;
use harness_dataguard::command::{CommandResult, State};
use harness_dataguard::engine::Engine;
use harness_dataguard::guard::Caller;
use harness_dataguard::manifest::DataCapability;
use harness_dataguard::memory::{MemoryNotifier, MemoryStore};
use harness_dataguard::plugins::RefusalKind;
use harness_dataguard::postgres::SystemClock;
use harness_dataguard::store::Store;
use serde_json::{Value, json};
use set_master_volume::{DATA_CAPABILITY, MASTER_VOLUME, Prepared, Refusal, Row, prepare};

/// A campaign created under the id its payload names, test-only.
const CREATE: &str = r#"{"dataCapability":"test.createCampaign","owner":"dataguard","version":1,"effect":"insert","target":{"aggregate":"Campaign","id":"$id"},"mode":"overwrite","callableBy":["*"]}"#;

const VALOMBRE: &str = "c1000000-0000-4000-8000-000000000001";
const NOWHERE: &str = "c1000000-0000-4000-8000-0000000000ff";

fn row(value: Value) -> Row {
    let Value::Object(fields) = value else {
        panic!("a row is an object");
    };
    fields.into_iter().collect()
}

/// What `prepare` writes for `volume` on Valombre.
fn written_for(volume: Value) -> Value {
    prepare(
        &row(json!({ "campaign": VALOMBRE, "volume": volume })),
        None,
    )
    .expect("a campaign named")
    .payload[MASTER_VOLUME]
        .clone()
}

#[test]
fn the_percent_is_stored_divided_by_100_and_nothing_else() {
    assert_eq!(
        prepare(&row(json!({ "campaign": VALOMBRE, "volume": 40 })), None),
        Ok(Prepared {
            payload: row(json!({ "soundMasterVolume": 0.4 })),
            target_id: VALOMBRE.to_owned(),
        })
    );
    assert_eq!(written_for(json!(0)), json!(0.0));
    assert_eq!(written_for(json!(100)), json!(1.0));
}

#[test]
fn a_percent_in_range_is_kept_to_the_whole_percent() {
    assert_eq!(written_for(json!(40.4)), json!(0.4));
    assert_eq!(written_for(json!(99.6)), json!(1.0));
    assert_eq!(written_for(json!(0.4)), json!(0.0));
}

#[test]
fn a_percent_out_of_range_is_never_clamped_nor_rounded() {
    assert_eq!(written_for(json!(150)), json!(1.5));
    assert_eq!(written_for(json!(100.4)), json!(1.004));
    assert_eq!(written_for(json!(-0.4)), json!(-0.004));
}

#[test]
fn a_volume_that_is_no_number_is_written_null() {
    assert_eq!(written_for(json!("40")), Value::Null);
    assert_eq!(
        prepare(&row(json!({ "campaign": VALOMBRE })), None)
            .expect("a campaign named")
            .payload,
        row(json!({ "soundMasterVolume": null }))
    );
}

#[test]
fn the_target_names_the_campaign_before_the_payload_lowercase_and_trimmed() {
    let wire = row(json!({ "campaign": NOWHERE, "volume": 40 }));
    let upper = format!("  {}  ", VALOMBRE.to_ascii_uppercase());
    assert_eq!(
        prepare(&wire, Some(&upper)).map(|p| p.target_id),
        Ok(VALOMBRE.to_owned())
    );
    assert_eq!(
        prepare(&wire, Some("  ")).map(|p| p.target_id),
        Ok(NOWHERE.to_owned())
    );
}

#[test]
fn a_request_naming_no_campaign_is_refused() {
    for wire in [
        json!({ "volume": 40 }),
        json!({ "campaign": " ", "volume": 40 }),
    ] {
        assert_eq!(prepare(&row(wire), None), Err(Refusal::NoCampaign));
    }
    assert_eq!(
        Refusal::NoCampaign.to_string(),
        "cette campagne n'existe pas"
    );
}

/// The engine as the server plants the DataCapability and its aggregate,
/// over the in-memory store, with the test-only creation; and that store.
async fn engine() -> (Engine, Arc<MemoryStore>) {
    let store = Arc::new(MemoryStore::default());
    let plugins = [set_master_volume::plugin(), aggregate_campaign::plugin()]
        .into_iter()
        .collect();
    let engine = Engine::new(
        store.clone(),
        Arc::new(MemoryNotifier::default()),
        Arc::new(SystemClock),
    )
    .with_aggregate(Aggregate::parse(aggregate_campaign::AGGREGATE_JSON).expect("contract I"))
    .with_data_capability(
        DataCapability::parse(include_str!("../data-capability.json")).expect("contract F"),
    )
    .with_data_capability(DataCapability::parse(CREATE).expect("contract F"))
    .with_plugins(plugins);
    engine.load().await.expect("the in-memory engine");
    (engine, store)
}

fn submission(data_capability: &str, payload: Value, target: Option<&str>) -> Submission {
    Submission {
        data_capability: data_capability.to_owned(),
        payload: row(payload),
        target_id: target.map(str::to_owned),
        based_on: None,
        idempotency_key: None,
        caller: Caller {
            by: "dm".to_owned(),
            from: "LiveSoundbox".to_owned(),
            permissions: Vec::new(),
            // A campaign's creation writes its critical fields, which wait
            // for an approver; the master volume is low, which nobody reviews.
            roles: if data_capability == DATA_CAPABILITY {
                Vec::new()
            } else {
                vec!["role:approver".to_owned()]
            },
        },
    }
}

/// What the single applier made of one command.
async fn applied(engine: &Engine, data_capability: &str, payload: Value) -> CommandResult {
    engine
        .submit(submission(data_capability, payload, None))
        .await
        .expect("queued");
    let mut results = engine.run_once().await.expect("the applier ran");
    assert_eq!(results.len(), 1, "{results:?}");
    results.remove(0)
}

/// Les Cendres de Valombre, its master volume at 80 %.
fn valombre() -> Value {
    json!({
        "id": VALOMBRE,
        "name": "Les Cendres de Valombre",
        "kind": "campagne",
        "soundMasterVolume": 0.8,
    })
}

async fn with_valombre() -> (Engine, Arc<MemoryStore>) {
    let (engine, store) = engine().await;
    let created = applied(&engine, "test.createCampaign", valombre()).await;
    assert_eq!(created.status, State::Applied, "{created:?}");
    (engine, store)
}

async fn campaign(store: &MemoryStore) -> Value {
    let fields = store
        .row(&format!("Campaign/{VALOMBRE}"))
        .await
        .expect("the row");
    Value::Object(fields.into_iter().collect())
}

#[tokio::test]
async fn a_master_volume_of_40_percent_is_stored_as_0_4_and_nothing_else_moves() {
    let (engine, store) = with_valombre().await;
    let result = applied(
        &engine,
        DATA_CAPABILITY,
        json!({ "campaign": VALOMBRE, "volume": 40 }),
    )
    .await;
    assert_eq!(result.status, State::Applied, "{result:?}");
    let mut expected = valombre();
    expected[MASTER_VOLUME] = json!(0.4);
    assert_eq!(campaign(&store).await, expected);
}

#[tokio::test]
async fn master_volume_range_rejects_a_volume_of_150_percent_and_the_row_stays() {
    let (engine, store) = with_valombre().await;
    let result = applied(
        &engine,
        DATA_CAPABILITY,
        json!({ "campaign": VALOMBRE, "volume": 150 }),
    )
    .await;
    assert_eq!(result.status, State::Rejected, "{result:?}");
    assert_eq!(result.violations, ["master-volume-range"], "{result:?}");
    assert_eq!(campaign(&store).await, valombre());
}

#[tokio::test]
async fn master_volume_range_rejects_a_volume_that_is_no_number() {
    let (engine, store) = with_valombre().await;
    let result = applied(
        &engine,
        DATA_CAPABILITY,
        json!({ "campaign": VALOMBRE, "volume": "fort" }),
    )
    .await;
    assert_eq!(result.violations, ["master-volume-range"], "{result:?}");
    assert_eq!(campaign(&store).await, valombre());
}

#[tokio::test]
async fn a_campaign_the_engine_does_not_hold_is_refused_before_anything_queues() {
    let (engine, store) = with_valombre().await;
    for (payload, target) in [
        (json!({ "campaign": NOWHERE, "volume": 40 }), None),
        (json!({ "volume": 40 }), None),
        (json!({ "volume": 40 }), Some(NOWHERE)),
    ] {
        let refused = engine
            .submit(submission(DATA_CAPABILITY, payload, target))
            .await;
        let Err(Error::Refused(refusal)) = refused else {
            panic!("refused before it queues: {refused:?}");
        };
        assert_eq!(refusal.kind, RefusalKind::NotFound, "{refusal:?}");
    }
    assert!(engine.run_once().await.expect("the applier ran").is_empty());
    assert_eq!(campaign(&store).await, valombre());
}
