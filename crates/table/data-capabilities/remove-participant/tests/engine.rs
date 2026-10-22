//! table.removeParticipant as the server plants it beside its target
//! aggregate, over the engine's store in memory: four Bandits in a combat,
//! ranked 1 to 4, Bandit 1's turn. Taking out Bandit 2 — rank 2 of 4 —
//! tombstones its entry, a soft delete, and nothing else is written: its
//! token stays on the map, the other entries keep their ranks (the order
//! closes up by itself, `orderIndex`) and the turn is the combat's. The
//! participant holding the turn is refused until « Suivant » passes it, sent
//! first: then it is taken out, and the next living one has the turn. A
//! removal that names no uuid, a participant of another instance, of an
//! ended combat, or one the engine does not hold, is refused before
//! anything is queued; one by a caller who is no approver waits for its
//! review.

use std::path::Path;
use std::sync::Arc;

use harness_dataguard::aggregate::Aggregate;
use harness_dataguard::api::{Submission, Submitted};
use harness_dataguard::command::State;
use harness_dataguard::engine::Engine;
use harness_dataguard::manifest::DataCapability;
use harness_dataguard::memory::{FixedClock, MemoryNotifier, MemoryStore};
use harness_dataguard::plugins::{Plugins, RefusalKind};
use harness_dataguard::{Error, Result};
use remove_participant::{Row, partition};
use serde_json::{Value, json};

const INSTANCE: &str = "b1000000-0000-4000-8000-000000000001";
const AUTRE_INSTANCE: &str = "b1000000-0000-4000-8000-000000000002";
const BANDIT: &str = "e1000000-0000-4000-8000-0000000000b1";
const COMBAT: &str = "c0000000-0000-4000-8000-000000000001";
const NOWHERE: &str = "70000000-0000-4000-8000-0000000000ff";

/// The test-only DataCapabilities standing for « Suivant » and the combat's
/// end, written on the combat's partition as table.nextTurn and
/// table.endCombat write it.
const NEXT_TURN: &str = r#"{"dataCapability":"test.nextTurn","owner":"dataguard","version":1,"effect":"update","target":{"aggregate":"BattlemapInstance","id":"$combat"},"touches":["BattlemapInstance.currentEntryId","BattlemapInstance.round"],"mode":"relative","callableBy":["*"]}"#;
const END_COMBAT: &str = r#"{"dataCapability":"test.endCombat","owner":"dataguard","version":1,"effect":"update","target":{"aggregate":"BattlemapInstance","id":"$combat"},"touches":["BattlemapInstance.endedAt","BattlemapInstance.currentEntryId"],"mode":"overwrite","callableBy":["*"]}"#;

/// The id of Bandit `number`.
fn bandit_id(number: u8) -> String {
    format!("70000000-0000-4000-8000-00000000000{number}")
}

/// The id of Bandit `number`'s initiative entry.
fn entry_id(number: u8) -> String {
    format!("ee000000-0000-4000-8000-00000000000{number}")
}

fn row(value: Value) -> Row {
    let Value::Object(map) = value else {
        unreachable!("a row is an object");
    };
    map.into_iter().collect()
}

/// Bandit `number` as table.placeTokens left it on the instance.
fn bandit(number: u8) -> Row {
    row(json!({
        "tokenId": bandit_id(number),
        "tokenInstanceId": INSTANCE,
        "characterId": BANDIT,
        "number": number,
        "name": format!("Bandit {number}"),
        "q": 5,
        "r": i64::from(number),
        "visible": true,
        "hpCurrent": 11,
        "hpMax": 11,
        "conditions": [],
    }))
}

/// Bandit `number`'s place in the combat: rank `number`.
fn entry(number: u8) -> Row {
    row(json!({
        "initiativeEntryId": entry_id(number),
        "initiativeEntryCombatId": COMBAT,
        "initiativeEntryTokenId": bandit_id(number),
        "roll": 10,
        "initiative": 20 - i64::from(number),
        "dexScore": 12,
        "tiebreak": 0.5,
        "rank": number,
    }))
}

/// The instance's combat, round 1, Bandit 1's turn.
fn combat() -> Row {
    row(json!({
        "combatId": COMBAT,
        "combatInstanceId": INSTANCE,
        "round": 1,
        "currentEntryId": entry_id(1),
        "startedAt": "2026-10-21T10:00:00Z",
    }))
}

fn aggregate() -> Aggregate {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../aggregates/BattlemapInstance/aggregate.json");
    Aggregate::parse(&std::fs::read_to_string(path).expect("aggregate.json")).expect("contract I")
}

fn manifest(json: &str) -> DataCapability {
    DataCapability::parse(json).expect("contract F")
}

/// The engine as the server plants this unit and its aggregate, over a
/// store holding Bandit 1 to 4, their entries ranked 1 to 4 and `combat`;
/// and that store.
async fn planted_with(combat: Row) -> (Engine, Arc<MemoryStore>) {
    let store = (1..=4).fold(
        MemoryStore::default().with_row(&partition(COMBAT), combat),
        |store, number| {
            store
                .with_row(&partition(&bandit_id(number)), bandit(number))
                .with_row(&partition(&entry_id(number)), entry(number))
        },
    );
    let store = Arc::new(store);
    let plugins: Plugins = [
        aggregate_battlemap_instance::plugin(),
        remove_participant::plugin(),
    ]
    .into_iter()
    .collect();
    let engine = Engine::new(
        store.clone(),
        Arc::new(MemoryNotifier::default()),
        Arc::new(FixedClock::at(1_792_000_000)),
    )
    .with_aggregate(aggregate())
    .with_data_capability(manifest(include_str!("../data-capability.json")))
    .with_data_capability(manifest(NEXT_TURN))
    .with_data_capability(manifest(END_COMBAT))
    .with_plugins(plugins);
    engine.load().await.expect("the engine loads");
    (engine, store)
}

async fn planted() -> (Engine, Arc<MemoryStore>) {
    planted_with(combat()).await
}

/// A command by the DM with `roles`, from the Battlemap, on `target`.
async fn submit_by(
    engine: &Engine,
    roles: &[&str],
    data_capability: &str,
    target: &str,
    payload: Value,
) -> Result<Submitted> {
    let body = json!({
        "dataCapability": data_capability,
        "target": { "aggregate": "BattlemapInstance", "id": target },
        "payload": payload,
        "caller": { "by": "dm", "from": "Battlemap", "roles": roles },
    });
    engine
        .submit(serde_json::from_value::<Submission>(body).expect("a submission"))
        .await
}

/// A removal by the DM, who approves what the table's critical fields need.
async fn remove(engine: &Engine, target: &str, payload: Value) -> Result<Submitted> {
    submit_by(
        engine,
        &["role:approver"],
        "table.removeParticipant@1",
        target,
        payload,
    )
    .await
}

/// Every command queued, applied.
async fn settle(engine: &Engine) {
    for result in engine.run_once().await.expect("the applier ran") {
        assert_eq!(result.status, State::Applied, "{result:?}");
    }
}

/// The refusal `answer` is, of `kind`: its message.
fn refused(answer: Result<Submitted>, kind: RefusalKind) -> String {
    match answer {
        Err(Error::Refused(refusal)) if refusal.kind == kind => refusal.message,
        other => panic!("a {kind:?} refusal: {other:?}"),
    }
}

/// Nothing tombstoned, and every token and entry as planted.
fn untouched(store: &MemoryStore) {
    for number in 1..=4 {
        assert!(!store.is_tombstoned(&partition(&bandit_id(number))));
        assert_eq!(
            store.row_now(&partition(&bandit_id(number))),
            bandit(number)
        );
        assert!(!store.is_tombstoned(&partition(&entry_id(number))));
        assert_eq!(store.row_now(&partition(&entry_id(number))), entry(number));
    }
}

#[tokio::test]
async fn a_participant_at_rank_2_of_4_is_taken_out_and_its_token_stays() {
    let (engine, store) = planted().await;
    let submitted = remove(&engine, INSTANCE, json!({ "participant": entry_id(2) }))
        .await
        .expect("queued");
    // The command is written on the entry's partition, not the instance's.
    assert_eq!(submitted.partition, partition(&entry_id(2)));
    assert_eq!(submitted.result.status, State::Queued);
    assert!(submitted.warnings.is_empty(), "{:?}", submitted.warnings);
    settle(&engine).await;

    // A soft delete: the entry's partition is tombstoned, never erased.
    assert!(store.is_tombstoned(&partition(&entry_id(2))));
    // Its token stays on the map, every other one too.
    for number in 1..=4 {
        assert!(!store.is_tombstoned(&partition(&bandit_id(number))));
        assert_eq!(
            store.row_now(&partition(&bandit_id(number))),
            bandit(number)
        );
    }
    // The others keep their ranks, 1, 3, 4: in the previous order, which the
    // carousel counts 1, 2, 3 (orderIndex, derived).
    for number in [1, 3, 4] {
        assert!(!store.is_tombstoned(&partition(&entry_id(number))));
        assert_eq!(store.row_now(&partition(&entry_id(number))), entry(number));
    }
    assert_eq!(store.row_now(&partition(COMBAT)), combat());
    // One write: the removal's.
    let applied: Vec<String> = store
        .journal()
        .into_iter()
        .filter(|result| result.status == State::Applied)
        .map(|result| result.command_id)
        .collect();
    assert_eq!(applied, [submitted.command_id]);
}

#[tokio::test]
async fn an_entry_targeted_itself_is_taken_out_too() {
    let (engine, store) = planted().await;
    let submitted = remove(&engine, &entry_id(3), json!({}))
        .await
        .expect("queued");
    assert_eq!(submitted.partition, partition(&entry_id(3)));
    settle(&engine).await;
    assert!(store.is_tombstoned(&partition(&entry_id(3))));
    assert!(!store.is_tombstoned(&partition(&entry_id(2))));
    assert!(!store.is_tombstoned(&partition(&bandit_id(3))));
}

#[tokio::test]
async fn the_participant_holding_the_turn_is_taken_out_once_suivant_passed_it() {
    let (engine, store) = planted().await;
    // Bandit 1's turn: it is not taken out while it holds it.
    let message = refused(
        remove(&engine, INSTANCE, json!({ "participant": entry_id(1) })).await,
        RefusalKind::Unprocessable,
    );
    assert!(message.starts_with("next-turn:"), "{message}");
    assert!(engine.run_once().await.expect("ran").is_empty());
    untouched(&store);

    // « Suivant » first, as the Battlemap sends it: Bandit 2 has the turn
    // in the queue, and Bandit 1 is then taken out behind it.
    submit_by(
        &engine,
        &["role:approver"],
        "test.nextTurn",
        COMBAT,
        json!({ "currentEntryId": entry_id(2), "round": 0 }),
    )
    .await
    .expect("« Suivant » queued");
    let submitted = remove(&engine, INSTANCE, json!({ "participant": entry_id(1) }))
        .await
        .expect("queued");
    assert!(submitted.warnings.is_empty(), "{:?}", submitted.warnings);
    settle(&engine).await;

    assert!(store.is_tombstoned(&partition(&entry_id(1))));
    assert!(!store.is_tombstoned(&partition(&bandit_id(1))));
    let combat = store.row_now(&partition(COMBAT));
    assert_eq!(combat["currentEntryId"], json!(entry_id(2)));
    assert_eq!(combat["round"], json!(1));
}

#[tokio::test]
async fn a_participant_of_an_ended_combat_an_end_queued_included_is_refused() {
    let ended = {
        let mut combat = combat();
        combat.insert("endedAt".to_owned(), json!("2026-10-21T11:00:00Z"));
        combat.insert("currentEntryId".to_owned(), Value::Null);
        combat
    };
    let (engine, store) = planted_with(ended).await;
    let message = refused(
        remove(&engine, INSTANCE, json!({ "participant": entry_id(2) })).await,
        RefusalKind::Unprocessable,
    );
    assert_eq!(message, "ce combat est terminé");
    untouched(&store);

    // An end queued, not yet applied, closes the combat too.
    let (engine, store) = planted().await;
    submit_by(
        &engine,
        &["role:approver"],
        "test.endCombat",
        COMBAT,
        json!({ "endedAt": "2026-10-21T11:00:00Z", "currentEntryId": null }),
    )
    .await
    .expect("the end queued");
    let message = refused(
        remove(&engine, INSTANCE, json!({ "participant": entry_id(2) })).await,
        RefusalKind::Unprocessable,
    );
    assert_eq!(message, "ce combat est terminé");
    settle(&engine).await;
    untouched(&store);
}

#[tokio::test]
async fn a_removal_by_a_caller_who_is_no_approver_waits_for_its_review() {
    let (engine, store) = planted().await;
    let submitted = submit_by(
        &engine,
        &[],
        "table.removeParticipant@1",
        INSTANCE,
        json!({ "participant": entry_id(2) }),
    )
    .await
    .expect("queued");
    assert_eq!(submitted.result.status, State::AwaitingReview);
    engine.run_once().await.expect("the applier ran");
    untouched(&store);
}

#[tokio::test]
async fn an_id_that_is_no_uuid_is_unprocessable_and_nothing_is_queued() {
    let (engine, store) = planted().await;
    for (target, payload) in [
        (INSTANCE, json!({ "participant": "bandit-2" })),
        ("la-taverne", json!({ "participant": entry_id(2) })),
        ("la-taverne", json!({})),
    ] {
        let message = refused(
            remove(&engine, target, payload).await,
            RefusalKind::Unprocessable,
        );
        assert!(message.contains("n'est pas un identifiant"), "{message}");
    }
    assert!(engine.run_once().await.expect("ran").is_empty());
    untouched(&store);
}

#[tokio::test]
async fn a_participant_of_another_instance_or_not_held_is_not_found() {
    let (engine, store) = planted().await;
    for (target, participant) in [
        (AUTRE_INSTANCE, entry_id(2)),
        (INSTANCE, NOWHERE.to_owned()),
        // The combat is no participant.
        (INSTANCE, COMBAT.to_owned()),
    ] {
        refused(
            remove(&engine, target, json!({ "participant": participant })).await,
            RefusalKind::NotFound,
        );
    }
    assert!(engine.run_once().await.expect("ran").is_empty());
    untouched(&store);
}

#[tokio::test]
async fn a_token_s_entry_cannot_be_looked_up_over_memory() {
    let (engine, store) = planted().await;
    let message = refused(
        remove(&engine, INSTANCE, json!({ "participant": bandit_id(2) })).await,
        RefusalKind::Unprocessable,
    );
    assert_eq!(message, "les participants du combat n'ont pas pu être lus");
    assert!(engine.run_once().await.expect("ran").is_empty());
    untouched(&store);
}

#[tokio::test]
async fn a_removal_sent_again_before_the_first_is_applied_waits_for_its_author() {
    let (engine, store) = planted().await;
    let first = remove(&engine, INSTANCE, json!({ "participant": entry_id(2) }))
        .await
        .expect("queued");
    assert_eq!(first.result.status, State::Queued);
    // The entry is still the queue's to remove: the second is never refused
    // for it (rule 8), its author confirms or cancels it.
    let again = remove(&engine, INSTANCE, json!({ "participant": entry_id(2) }))
        .await
        .expect("parked");
    assert_eq!(again.partition, partition(&entry_id(2)));
    assert_eq!(again.result.status, State::AwaitingConfirmation);
    settle(&engine).await;
    assert!(store.is_tombstoned(&partition(&entry_id(2))));
    assert!(!store.is_tombstoned(&partition(&bandit_id(2))));
}
