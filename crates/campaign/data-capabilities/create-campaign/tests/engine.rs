//! campaign.createCampaign through the engine, as the server plants its unit
//! and its target aggregate, over the engine's memory: a creation is given
//! its ids and its time, queued on its new campaign's partition, judged by
//! the DataGuard (`oneshot-single-session`, `master-volume-range`) and
//! applied — a oneshot with its single session. The tables it reaches are
//! `postgres.rs`'s.

use std::collections::BTreeMap;
use std::sync::Arc;

use harness_dataguard::aggregate::Aggregate;
use harness_dataguard::api::{Submission, Submitted};
use harness_dataguard::command::{CommandResult, State};
use harness_dataguard::engine::Engine;
use harness_dataguard::manifest::DataCapability;
use harness_dataguard::memory::{FixedClock, MemoryNotifier, MemoryStore};
use harness_dataguard::plugins::RefusalKind;
use harness_dataguard::store::{RandomIds, Store};
use harness_dataguard::{Error, Result};
use serde_json::{Value, json};

/// 2026-10-21T09:00:00.123Z, in milliseconds.
const NOW_MILLIS: u64 = 1_792_573_200_123;
const NOW: &str = "2026-10-21T09:00:00.123Z";

/// What a creation writes of a oneshot's session.
const SESSION: [&str; 6] = [
    "sessionId",
    "sessionCampaignId",
    "sessionPosition",
    "sessionTitle",
    "sessionStatus",
    "microphoneOn",
];

/// The engine as the server plants this unit and its target aggregate, over
/// its memory, its clock at [`NOW_MILLIS`]; and the store under it.
async fn served() -> (Engine, Arc<MemoryStore>) {
    let store = Arc::new(MemoryStore::default());
    let engine = Engine::new(
        store.clone(),
        Arc::new(MemoryNotifier::default()),
        Arc::new(FixedClock::at_millis(NOW_MILLIS)),
    )
    .with_ids(Arc::new(RandomIds))
    .with_aggregate(Aggregate::parse(aggregate_campaign::AGGREGATE_JSON).expect("contract I"))
    .with_data_capability(
        DataCapability::parse(include_str!("../data-capability.json")).expect("contract F"),
    )
    .with_plugins(
        [aggregate_campaign::plugin(), create_campaign::plugin()]
            .into_iter()
            .collect(),
    );
    engine.load().await.expect("the engine loads");
    (engine, store)
}

/// A submission of campaign.createCampaign by the DM — approver of their
/// own campaigns' critical fields when `roles` says so.
async fn submit(
    engine: &Engine,
    payload: Value,
    key: Option<&str>,
    roles: &[&str],
) -> Result<Submitted> {
    let submission: Submission = serde_json::from_value(json!({
        "dataCapability": "campaign.createCampaign@1",
        "payload": payload,
        "idempotencyKey": key,
        "caller": { "by": "dm", "from": "CampaignList", "roles": roles },
    }))
    .expect("a submission");
    engine.submit(submission).await
}

/// Creates `payload` as the DM, approver: its partition, and what the
/// applier made of it.
async fn create(engine: &Engine, payload: Value) -> (String, CommandResult) {
    let submitted = submit(engine, payload, None, &["role:approver"])
        .await
        .expect("queued");
    assert!(submitted.warnings.is_empty(), "{submitted:?}");
    (submitted.partition, settled(engine).await)
}

/// One pass of the single applier, which settles one command.
async fn settled(engine: &Engine) -> CommandResult {
    let mut settled = engine.run_once().await.expect("the applier ran");
    assert_eq!(settled.len(), 1, "{settled:?}");
    settled.remove(0)
}

/// Whether the queue holds nothing to apply.
async fn nothing_queued(engine: &Engine) -> bool {
    engine.run_once().await.expect("the applier ran").is_empty()
}

/// The row the engine holds on `partition`.
async fn row(store: &MemoryStore, partition: &str) -> BTreeMap<String, Value> {
    store.row(partition).await.expect("the row")
}

/// The refusal `answer` is, of `kind`: its message.
fn refused(answer: Result<Submitted>, kind: RefusalKind) -> String {
    match answer {
        Err(Error::Refused(refusal)) if refusal.kind == kind => refusal.message,
        other => panic!("a refusal of kind {kind:?}: {other:?}"),
    }
}

/// A uuid written 8-4-4-4-12 in lowercase hex digits.
fn lowercase_uuid(text: &str) -> bool {
    text.len() == 36
        && text.char_indices().all(|(i, c)| match i {
            8 | 13 | 18 | 23 => c == '-',
            _ => c.is_ascii_digit() || ('a'..='f').contains(&c),
        })
}

#[tokio::test]
async fn a_campagne_is_created_whole_on_its_new_partition_with_no_session() {
    let (engine, store) = served().await;
    let (partition, result) = create(
        &engine,
        json!({ "name": "Les Brumes de Corbière", "type": "campagne", "pitch": "Un port noyé.", "art": "forest", "glyph": "B" }),
    )
    .await;
    assert_eq!(result.status, State::Applied, "{result:?}");
    let id = partition.strip_prefix("Campaign/").expect("a campaign");
    assert!(lowercase_uuid(id), "{partition}");
    let row = row(&store, &partition).await;
    assert_eq!(row["id"], json!(id));
    assert_eq!(row["name"], "Les Brumes de Corbière");
    assert_eq!(row["kind"], "campagne");
    assert_eq!(row["pitch"], "Un port noyé.");
    assert_eq!(row["art"], "forest");
    assert_eq!(row["glyph"], "B");
    assert_eq!(row["createdAt"], NOW);
    for field in SESSION {
        assert!(!row.contains_key(field), "{field}: {row:?}");
    }
}

#[tokio::test]
async fn oneshot_single_session_a_oneshot_is_created_with_its_one_session_in_prep_at_position_1_microphone_off()
 {
    let (engine, store) = served().await;
    let (partition, result) = create(&engine, json!({ "name": "Le Bal", "type": "oneshot" })).await;
    assert_eq!(result.status, State::Applied, "{result:?}");
    let id = partition.strip_prefix("Campaign/").expect("a campaign");
    let row = row(&store, &partition).await;
    assert_eq!(row["kind"], "oneshot");
    let session = row["sessionId"].as_str().expect("its session's id");
    assert!(lowercase_uuid(session), "{session}");
    assert_ne!(session, id);
    assert_eq!(row["sessionCampaignId"], json!(id));
    assert_eq!(row["sessionPosition"], 1);
    assert_eq!(row["sessionTitle"], "Le Bal");
    assert_eq!(row["sessionStatus"], "prep");
    assert_eq!(row["microphoneOn"], false);
}

#[tokio::test]
async fn master_volume_range_a_new_campaign_starts_at_full_volume_inside_0_to_1() {
    let (engine, store) = served().await;
    for kind in ["campagne", "oneshot"] {
        // A volume the wire names is not the DM's to set at creation: the
        // engine warns of the key, the creation starts at full volume.
        let submitted = submit(
            &engine,
            json!({ "name": "Le Bal", "type": kind, "soundMasterVolume": 7 }),
            None,
            &["role:approver"],
        )
        .await
        .expect("queued");
        assert!(
            submitted
                .warnings
                .iter()
                .all(|warning| warning.contains("soundMasterVolume not declared")),
            "{submitted:?}"
        );
        let result = settled(&engine).await;
        assert_eq!(result.status, State::Applied, "{result:?}");
        let volume = row(&store, &submitted.partition).await["soundMasterVolume"]
            .as_f64()
            .expect("a volume");
        assert!((0.0..=1.0).contains(&volume), "{kind}: {volume}");
        assert!((volume - 1.0).abs() < f64::EPSILON, "{kind}: {volume}");
    }
}

#[tokio::test]
async fn two_creations_are_two_campaigns_each_under_its_own_id() {
    let (engine, _) = served().await;
    let (first, _) = create(&engine, json!({ "name": "Le Bal", "type": "oneshot" })).await;
    let (second, _) = create(&engine, json!({ "name": "Le Bal", "type": "oneshot" })).await;
    assert_ne!(first, second);
}

#[tokio::test]
async fn the_same_idempotency_key_twice_creates_one_campaign_and_one_session() {
    let (engine, store) = served().await;
    let payload = json!({ "name": "Le Bal", "type": "oneshot" });
    let first = submit(&engine, payload.clone(), Some("k-1"), &["role:approver"])
        .await
        .expect("queued");
    let again = submit(&engine, payload.clone(), Some("k-1"), &["role:approver"])
        .await
        .expect("replayed");
    assert!(again.replayed, "{again:?}");
    assert_eq!(again.command_id, first.command_id);
    assert_eq!(again.partition, first.partition);
    assert_eq!(settled(&engine).await.status, State::Applied);
    assert!(nothing_queued(&engine).await);
    // Applied, a replay answers the first creation's result and queues none.
    let after = submit(&engine, payload, Some("k-1"), &["role:approver"])
        .await
        .expect("replayed");
    assert_eq!(after.partition, first.partition);
    assert_eq!(after.result.status, State::Applied, "{after:?}");
    assert!(nothing_queued(&engine).await);
    let session = row(&store, &first.partition).await["sessionId"].clone();
    assert!(session.is_string(), "{session}");
}

#[tokio::test]
async fn a_creation_with_no_name_or_of_a_kind_there_is_not_is_refused_and_nothing_is_queued() {
    let (engine, _) = served().await;
    for (payload, why) in [
        (json!({ "type": "campagne" }), "nom"),
        (json!({ "name": "  ", "type": "oneshot" }), "nom"),
        (json!({ "name": "Le Bal" }), "oneshot"),
        (json!({ "name": "Le Bal", "type": "saga" }), "oneshot"),
    ] {
        let message = refused(
            submit(&engine, payload.clone(), None, &["role:approver"]).await,
            RefusalKind::Unprocessable,
        );
        assert!(message.contains(why), "{payload}: {message}");
    }
    assert!(nothing_queued(&engine).await);
}

#[tokio::test]
async fn a_creation_by_a_caller_who_is_no_approver_waits_for_review() {
    // `Campaign.id` and `Campaign.kind` are critical: the engine opens a
    // review for every command touching them by a caller without
    // `role:approver`.
    let (engine, _) = served().await;
    let submitted = submit(
        &engine,
        json!({ "name": "Le Bal", "type": "campagne" }),
        None,
        &[],
    )
    .await
    .expect("queued");
    assert_eq!(
        submitted.result.status,
        State::AwaitingReview,
        "{submitted:?}"
    );
}
