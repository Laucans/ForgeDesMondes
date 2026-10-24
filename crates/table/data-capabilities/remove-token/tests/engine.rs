//! table.removeToken as the server plants it beside its target aggregate,
//! over the engine's store in memory: four Bandit tokens in a combat, the
//! second — rank 2 of 4, holding the turn — removed. Its partition is
//! tombstoned, a soft delete, and nothing else is written: the other tokens,
//! the entries' ranks and the combat's turn are the tables' relations and
//! `table.nextTurn`'s. A removal that names no token, a token of another
//! instance or one the engine does not hold is refused before anything is
//! queued; one by a caller who is no approver waits for its review.

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
use remove_token::{Row, partition};
use serde_json::{Value, json};

const INSTANCE: &str = "b1000000-0000-4000-8000-000000000001";
const AUTRE_INSTANCE: &str = "b1000000-0000-4000-8000-000000000002";
const BANDIT: &str = "e1000000-0000-4000-8000-0000000000b1";
const COMBAT: &str = "c0000000-0000-4000-8000-000000000001";
const NOWHERE: &str = "70000000-0000-4000-8000-0000000000ff";

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
        "hpCurrent": if number == 2 { 0 } else { 11 },
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

/// The instance's combat, round 1, Bandit 2's turn.
fn combat() -> Row {
    row(json!({
        "combatId": COMBAT,
        "combatInstanceId": INSTANCE,
        "round": 1,
        "currentEntryId": entry_id(2),
        "startedAt": "2026-10-21T10:00:00Z",
    }))
}

fn aggregate() -> Aggregate {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../aggregates/BattlemapInstance/aggregate.json");
    Aggregate::parse(&std::fs::read_to_string(path).expect("aggregate.json")).expect("contract I")
}

fn manifest() -> DataCapability {
    DataCapability::parse(include_str!("../data-capability.json")).expect("contract F")
}

/// The engine as the server plants this unit and its aggregate, over a
/// store holding Bandit 1 to 4, their entries ranked 1 to 4 and the
/// combat; and that store.
async fn planted() -> (Engine, Arc<MemoryStore>) {
    let store = (1..=4).fold(
        MemoryStore::default().with_row(&partition(COMBAT), combat()),
        |store, number| {
            store
                .with_row(&partition(&bandit_id(number)), bandit(number))
                .with_row(&partition(&entry_id(number)), entry(number))
        },
    );
    let store = Arc::new(store);
    let plugins: Plugins = [
        aggregate_battlemap_instance::plugin(),
        remove_token::plugin(),
    ]
    .into_iter()
    .collect();
    let engine = Engine::new(
        store.clone(),
        Arc::new(MemoryNotifier::default()),
        Arc::new(FixedClock::at(1_792_000_000)),
    )
    .with_aggregate(aggregate())
    .with_data_capability(manifest())
    .with_plugins(plugins);
    engine.load().await.expect("the engine loads");
    (engine, store)
}

/// A removal as the Battlemap sends it, by the DM with `roles`, to
/// `target`.
async fn remove_by(
    engine: &Engine,
    roles: &[&str],
    target: &str,
    payload: Value,
) -> Result<Submitted> {
    let body = json!({
        "dataCapability": "table.removeToken@1",
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
    remove_by(engine, &["role:approver"], target, payload).await
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

/// Nothing tombstoned and every row as planted.
fn untouched(store: &MemoryStore) {
    for number in 1..=4 {
        assert!(!store.is_tombstoned(&partition(&bandit_id(number))));
        assert_eq!(
            store.row_now(&partition(&bandit_id(number))),
            bandit(number)
        );
        assert_eq!(store.row_now(&partition(&entry_id(number))), entry(number));
    }
    assert_eq!(store.row_now(&partition(COMBAT)), combat());
}

#[tokio::test]
async fn a_token_at_rank_2_of_4_removed_is_tombstoned_and_nothing_else_is_written() {
    let (engine, store) = planted().await;
    let submitted = remove(&engine, INSTANCE, json!({ "token": bandit_id(2) }))
        .await
        .expect("queued");
    // The command is written on the token's partition, not the instance's.
    assert_eq!(submitted.partition, partition(&bandit_id(2)));
    assert_eq!(submitted.result.status, State::Queued);
    assert!(submitted.warnings.is_empty(), "{:?}", submitted.warnings);
    settle(&engine).await;

    // A soft delete: the partition is tombstoned, never erased.
    assert!(store.is_tombstoned(&partition(&bandit_id(2))));
    for number in [1, 3, 4] {
        assert!(!store.is_tombstoned(&partition(&bandit_id(number))));
        assert_eq!(
            store.row_now(&partition(&bandit_id(number))),
            bandit(number)
        );
    }
    // The entries and the turn are the relations' and table.nextTurn's: the
    // removal writes none of them.
    for number in 1..=4 {
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
async fn a_token_targeted_itself_is_removed_too() {
    let (engine, store) = planted().await;
    let submitted = remove(&engine, &bandit_id(3), json!({}))
        .await
        .expect("queued");
    assert_eq!(submitted.partition, partition(&bandit_id(3)));
    settle(&engine).await;
    assert!(store.is_tombstoned(&partition(&bandit_id(3))));
    assert!(!store.is_tombstoned(&partition(&bandit_id(2))));
}

#[tokio::test]
async fn a_removal_by_a_caller_who_is_no_approver_waits_for_its_review() {
    let (engine, store) = planted().await;
    let submitted = remove_by(&engine, &[], INSTANCE, json!({ "token": bandit_id(2) }))
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
        (INSTANCE, json!({ "token": "bandit-2" })),
        ("la-taverne", json!({ "token": bandit_id(2) })),
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
async fn a_token_of_another_instance_or_not_held_is_not_found_and_nothing_is_queued() {
    let (engine, store) = planted().await;
    for (target, token) in [
        (AUTRE_INSTANCE, bandit_id(2)),
        (INSTANCE, NOWHERE.to_owned()),
        // An entry is no token: removing a participant is not this command.
        (INSTANCE, entry_id(2)),
    ] {
        refused(
            remove(&engine, target, json!({ "token": token })).await,
            RefusalKind::NotFound,
        );
    }
    assert!(engine.run_once().await.expect("ran").is_empty());
    untouched(&store);
}
