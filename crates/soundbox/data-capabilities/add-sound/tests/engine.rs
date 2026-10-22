//! soundbox.addSound through the engine over its memory, as the server
//! plants its unit and its target aggregate: an addition is prepared against
//! its campaign, queued on its new sound's partition, judged by the
//! DataGuard (`sound-volume-range`, `tags-normalized`) and applied. Over
//! memory the campaign is the engine's row; the tables it reaches over
//! PostgreSQL are `postgres.rs`'s.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use harness_dataguard::aggregate::Aggregate;
use harness_dataguard::api::{Submission, Submitted};
use harness_dataguard::command::{CommandResult, State};
use harness_dataguard::engine::Engine;
use harness_dataguard::manifest::DataCapability;
use harness_dataguard::memory::{FixedClock, MemoryNotifier, MemoryStore};
use harness_dataguard::plugins::RefusalKind;
use harness_dataguard::{Error, Result};
use serde_json::{Value, json};

const VALOMBRE: &str = "c1000000-0000-4000-8000-000000000001";
const NOWHERE: &str = "c1000000-0000-4000-8000-0000000000ff";

/// 2026-10-22T20:00:00.123Z, in milliseconds.
const NOW_MILLIS: u64 = 1_792_699_200_123;

/// The Sound aggregate's manifest (contract I), read where it lies.
fn sound_aggregate() -> Aggregate {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../aggregates/Sound/aggregate.json");
    let text = std::fs::read_to_string(path).expect("Sound's aggregate.json");
    Aggregate::parse(&text).expect("contract I")
}

/// The engine as the server plants this unit and its target aggregate, over
/// a memory holding Valombre, its clock at [`NOW_MILLIS`]; and the store
/// under it.
async fn served() -> (Engine, Arc<MemoryStore>) {
    let campaign = BTreeMap::from([
        ("id".to_owned(), json!(VALOMBRE)),
        ("kind".to_owned(), json!("campagne")),
    ]);
    let store =
        Arc::new(MemoryStore::default().with_row(&format!("Campaign/{VALOMBRE}"), campaign));
    let engine = Engine::new(
        store.clone(),
        Arc::new(MemoryNotifier::default()),
        Arc::new(FixedClock::at_millis(NOW_MILLIS)),
    )
    .with_ids(Arc::new(harness_dataguard::store::RandomIds))
    .with_aggregate(sound_aggregate())
    .with_data_capability(
        DataCapability::parse(include_str!("../data-capability.json")).expect("contract F"),
    )
    .with_plugins(
        [aggregate_sound::plugin(), add_sound::plugin()]
            .into_iter()
            .collect(),
    );
    engine.load().await.expect("the engine loads");
    (engine, store)
}

/// A submission of soundbox.addSound by the DM, approver of their own
/// campaign's critical fields, under the idempotency key `key` when given.
async fn submit(engine: &Engine, payload: Value, key: Option<&str>) -> Result<Submitted> {
    let submission: Submission = serde_json::from_value(json!({
        "dataCapability": "soundbox.addSound@1",
        "payload": payload,
        "idempotencyKey": key,
        "caller": { "by": "dm", "from": "soundbox.SoundLibrary", "roles": ["role:approver"] },
    }))
    .expect("a submission");
    engine.submit(submission).await
}

/// One pass of the single applier, which settles one command.
async fn settled(engine: &Engine) -> CommandResult {
    let mut settled = engine.run_once().await.expect("the applier ran");
    assert_eq!(settled.len(), 1, "{settled:?}");
    settled.remove(0)
}

/// Adds `payload`: the sound's partition, and what the applier made of it.
async fn add(engine: &Engine, payload: Value) -> (String, CommandResult) {
    let submitted = submit(engine, payload, None).await.expect("queued");
    assert!(submitted.partition.starts_with("Sound/"), "{submitted:?}");
    (submitted.partition, settled(engine).await)
}

/// Whether the queue holds nothing to apply.
async fn nothing_queued(engine: &Engine) -> bool {
    engine.run_once().await.expect("the applier ran").is_empty()
}

/// The refusal `answer` is, of `kind`: its message.
fn refused(answer: Result<Submitted>, kind: RefusalKind) -> String {
    match answer {
        Err(Error::Refused(refusal)) if refusal.kind == kind => refusal.message,
        other => panic!("a refusal of kind {kind:?}: {other:?}"),
    }
}

fn crowd() -> Value {
    json!({
        "campaign": VALOMBRE,
        "name": "Bruit de foule",
        "tags": ["Taverne"],
        "volume": 55,
        "file": "son.ogg",
    })
}

#[tokio::test]
async fn a_sound_is_applied_its_tags_normalised_its_volume_a_fraction() {
    let (engine, store) = served().await;
    let (partition, result) = add(&engine, crowd()).await;
    assert_eq!(result.status, State::Applied, "{result:?}");
    let id = partition.strip_prefix("Sound/").expect("its id");
    assert_eq!(id.len(), 36, "a fresh uuid: {id}");
    assert_eq!(
        Value::Object(store.row_now(&partition).into_iter().collect()),
        json!({
            "id": id,
            "campaignId": VALOMBRE,
            "name": "Bruit de foule",
            "filePath": "son.ogg",
            "tags": ["taverne"],
            "volume": 0.55,
            "createdAt": "2026-10-22T20:00:00.123Z",
        })
    );
}

#[tokio::test]
async fn tags_normalized_two_spellings_are_one_tag() {
    let (engine, store) = served().await;
    let mut payload = crowd();
    payload["tags"] = json!(["Bois", "bois "]);
    let (partition, result) = add(&engine, payload).await;
    assert_eq!(result.status, State::Applied, "{result:?}");
    assert_eq!(store.row_now(&partition)["tags"], json!(["bois"]));
}

#[tokio::test]
async fn sound_volume_range_a_volume_of_120_is_rejected_and_nothing_written() {
    let (engine, store) = served().await;
    let mut payload = crowd();
    payload["volume"] = json!(120);
    let submitted = submit(&engine, payload, None).await.expect("queued");
    assert!(
        submitted
            .warnings
            .iter()
            .any(|w| w.contains("sound-volume-range")),
        "{submitted:?}"
    );
    let result = settled(&engine).await;
    assert_eq!(result.status, State::Rejected, "{result:?}");
    assert_eq!(result.violations, ["sound-volume-range"]);
    assert!(store.row_now(&submitted.partition).is_empty());
}

#[tokio::test]
async fn tags_normalized_tags_that_are_no_list_of_texts_are_rejected() {
    let (engine, store) = served().await;
    let mut payload = crowd();
    payload["tags"] = json!("Taverne");
    let submitted = submit(&engine, payload, None).await.expect("queued");
    let result = settled(&engine).await;
    assert_eq!(result.status, State::Rejected, "{result:?}");
    assert_eq!(result.violations, ["tags-normalized"]);
    assert!(store.row_now(&submitted.partition).is_empty());
}

#[tokio::test]
async fn an_unknown_campaign_is_not_found_and_nothing_queued() {
    let (engine, _) = served().await;
    let mut payload = crowd();
    payload["campaign"] = json!(NOWHERE);
    let why = refused(submit(&engine, payload, None).await, RefusalKind::NotFound);
    assert_eq!(why, "cette campagne n'existe pas");
    assert!(nothing_queued(&engine).await);
}

#[tokio::test]
async fn the_campaign_is_found_named_in_upper_case() {
    let (engine, _) = served().await;
    let mut payload = crowd();
    payload["campaign"] = json!(VALOMBRE.to_uppercase());
    let (_, result) = add(&engine, payload).await;
    assert_eq!(result.status, State::Applied, "{result:?}");
}

#[tokio::test]
async fn a_sound_with_no_name_or_no_file_is_unprocessable_and_nothing_queued() {
    let (engine, _) = served().await;
    for (field, says) in [
        ("name", "un son a besoin d'un nom"),
        ("file", "un son a besoin de son fichier"),
        ("campaign", "un son nomme sa campagne"),
    ] {
        let mut payload = crowd();
        payload[field] = json!(" ");
        let why = refused(
            submit(&engine, payload, None).await,
            RefusalKind::Unprocessable,
        );
        assert_eq!(why, says);
    }
    assert!(nothing_queued(&engine).await);
}

#[tokio::test]
async fn each_addition_is_a_sound_of_its_own() {
    let (engine, _) = served().await;
    let (first, _) = add(&engine, crowd()).await;
    let (second, _) = add(&engine, crowd()).await;
    assert_ne!(first, second);
}

#[tokio::test]
async fn an_addition_resubmitted_under_its_key_is_replayed_not_added_twice() {
    let (engine, _) = served().await;
    let first = submit(&engine, crowd(), Some("k-1")).await.expect("queued");
    let again = submit(&engine, crowd(), Some("k-1"))
        .await
        .expect("replayed");
    assert!(again.replayed, "{again:?}");
    assert_eq!(again.command_id, first.command_id);
    assert_eq!(settled(&engine).await.status, State::Applied);
    assert!(nothing_queued(&engine).await);
}
