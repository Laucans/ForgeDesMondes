//! table.setTokenCondition as the server plants it beside its target
//! aggregate, over the in-memory store: a badge set on one token of a
//! character lands on that token's partition alone — its other tokens, the
//! other instance's token of the same character and the instance keep what
//! they hold (`instance-isolated`); badges set in a row all stay
//! (`state-persisted-reset`); a token of another instance, or one the engine
//! does not hold, is refused before anything is queued. The instances and
//! tokens are planted by test-only DataCapabilities, as the aggregate's own
//! tests plant them.

use std::path::Path;
use std::sync::Arc;

use harness_dataguard::aggregate::Aggregate;
use harness_dataguard::api::{Submission, Submitted};
use harness_dataguard::command::{CommandResult, State};
use harness_dataguard::engine::Engine;
use harness_dataguard::manifest::DataCapability;
use harness_dataguard::memory::{FixedClock, MemoryNotifier, MemoryStore};
use harness_dataguard::plugins::{Plugins, RefusalKind};
use harness_dataguard::{Error, Result};
use serde_json::{Value, json};

const CARTE: &str = "d1000000-0000-4000-8000-000000000001";
const SEANCE: &str = "51000000-0000-4000-8000-000000000001";
const ARRIVEE: &str = "5c000000-0000-4000-8000-000000000001";
const EMBUSCADE: &str = "5c000000-0000-4000-8000-000000000002";
/// The tavern's instance, and the ambush's.
const INSTANCE: &str = "b1000000-0000-4000-8000-000000000001";
const AUTRE_INSTANCE: &str = "b1000000-0000-4000-8000-000000000002";
const ELOWEN: &str = "e1000000-0000-4000-8000-000000000001";
const BANDIT: &str = "e1000000-0000-4000-8000-000000000002";
/// The tavern's tokens: Elowen, Bandit 1 and Bandit 2.
const JETON_ELOWEN: &str = "70000000-0000-4000-8000-000000000001";
const BANDIT_1: &str = "70000000-0000-4000-8000-000000000002";
const BANDIT_2: &str = "70000000-0000-4000-8000-000000000003";
/// The ambush's token of the bandit.
const BANDIT_AILLEURS: &str = "70000000-0000-4000-8000-000000000004";
const NOWHERE: &str = "70000000-0000-4000-8000-0000000000ff";

/// The test-only DataCapabilities planting the rows a badge set lands among.
const OPEN_INSTANCE: &str = r#"{"dataCapability":"test.openInstance","owner":"dataguard","version":1,"effect":"insert","target":{"aggregate":"BattlemapInstance","id":"$battlemapInstanceId"},"mode":"overwrite","callableBy":["*"]}"#;
const DROP_TOKEN: &str = r#"{"dataCapability":"test.dropToken","owner":"dataguard","version":1,"effect":"insert","target":{"aggregate":"BattlemapInstance","id":"$tokenId"},"mode":"overwrite","callableBy":["*"]}"#;

fn aggregate() -> Aggregate {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../aggregates/BattlemapInstance/aggregate.json");
    Aggregate::parse(&std::fs::read_to_string(path).expect("aggregate.json")).expect("contract I")
}

fn manifest(json: &str) -> DataCapability {
    DataCapability::parse(json).expect("contract F")
}

fn plugins() -> Plugins {
    [
        aggregate_battlemap_instance::plugin(),
        set_token_condition::plugin(),
    ]
    .into_iter()
    .collect()
}

/// The engine as the server plants this unit and its aggregate, over an
/// in-memory store, the tavern's and the ambush's instances and tokens
/// planted; and the store, whose rows a test reads.
async fn engine() -> (Engine, Arc<MemoryStore>) {
    let store = Arc::new(MemoryStore::default());
    let engine = Engine::new(
        store.clone(),
        Arc::new(MemoryNotifier::default()),
        Arc::new(FixedClock::at(1_792_000_000)),
    )
    .with_aggregate(aggregate())
    .with_data_capability(manifest(include_str!("../data-capability.json")))
    .with_data_capability(manifest(OPEN_INSTANCE))
    .with_data_capability(manifest(DROP_TOKEN))
    .with_plugins(plugins());
    engine.load().await.expect("the in-memory engine");
    for (id, scene) in [(INSTANCE, ARRIVEE), (AUTRE_INSTANCE, EMBUSCADE)] {
        submit(
            &engine,
            "test.openInstance",
            json!({
                "battlemapInstanceId": id,
                "battlemapMediaId": CARTE,
                "sessionId": SEANCE,
                "sceneId": scene,
                "createdAt": "2026-10-21T10:00:00Z",
            }),
            None,
        )
        .await
        .expect("an instance");
    }
    for token in [
        token(JETON_ELOWEN, INSTANCE, ELOWEN, None, "Elowen"),
        token(BANDIT_1, INSTANCE, BANDIT, Some(1), "Bandit 1"),
        token(BANDIT_2, INSTANCE, BANDIT, Some(2), "Bandit 2"),
        token(BANDIT_AILLEURS, AUTRE_INSTANCE, BANDIT, Some(1), "Bandit 1"),
    ] {
        submit(&engine, "test.dropToken", token, None)
            .await
            .expect("a token");
    }
    let planted = engine.run_once().await.expect("planted");
    assert!(
        planted.iter().all(|result| result.status == State::Applied),
        "{planted:?}"
    );
    (engine, store)
}

/// A token as `table.placeTokens` drops it, with no condition: an NPC's
/// numbered, with its HP.
fn token(id: &str, instance: &str, character: &str, number: Option<i64>, name: &str) -> Value {
    let hp = number.map(|_| 11);
    json!({
        "tokenId": id,
        "tokenInstanceId": instance,
        "characterId": character,
        "number": number,
        "name": name,
        "q": 1,
        "r": 1,
        "visible": true,
        "hpCurrent": hp,
        "hpMax": hp,
        "conditions": [],
    })
}

/// A command as the Battlemap sends it, by the DM, on `target`: a planting
/// one by an approver.
async fn submit(
    engine: &Engine,
    data_capability: &str,
    payload: Value,
    target: Option<&str>,
) -> Result<Submitted> {
    let mut body = json!({
        "dataCapability": data_capability,
        "target": { "aggregate": "BattlemapInstance" },
        "payload": payload,
        "caller": { "by": "dm", "from": "Battlemap" },
    });
    // The rows planted hold critical fields, which only an approver writes
    // unheld; a badge is no critical field, which the DM sets as it is.
    if data_capability.starts_with("test.") {
        body["caller"]["roles"] = json!(["role:approver"]);
    }
    if let Some(target) = target {
        body["target"]["id"] = json!(target);
    }
    engine
        .submit(serde_json::from_value::<Submission>(body).expect("a submission"))
        .await
}

/// The badge `state` put on `token` of the tavern's instance, or taken off,
/// queued.
async fn queue(engine: &Engine, token: &str, state: &str, on: bool) -> Submitted {
    submit(
        engine,
        "table.setTokenCondition@1",
        json!({ "token": token, "state": state, "on": on }),
        Some(INSTANCE),
    )
    .await
    .expect("queued")
}

/// The badge `state` put on `token` or taken off, queued then applied.
async fn set(engine: &Engine, token: &str, state: &str, on: bool) -> CommandResult {
    queue(engine, token, state, on).await;
    let mut settled = engine.run_once().await.expect("the applier ran");
    assert_eq!(settled.len(), 1, "{settled:?}");
    settled.remove(0)
}

/// The row of the partition `id`.
fn row(store: &MemoryStore, id: &str) -> Value {
    Value::Object(
        store
            .row_now(&format!("BattlemapInstance/{id}"))
            .into_iter()
            .collect(),
    )
}

/// Every row of the tavern's and the ambush's instances, by partition.
fn every_row(store: &MemoryStore) -> Vec<(&'static str, Value)> {
    [
        INSTANCE,
        AUTRE_INSTANCE,
        JETON_ELOWEN,
        BANDIT_1,
        BANDIT_2,
        BANDIT_AILLEURS,
    ]
    .into_iter()
    .map(|id| (id, row(store, id)))
    .collect()
}

/// The refusal `answer` is, of `kind`: its message.
fn refused(answer: Result<Submitted>, kind: RefusalKind) -> String {
    match answer {
        Err(Error::Refused(refusal)) if refusal.kind == kind => refusal.message,
        other => panic!("a {kind:?} refusal: {other:?}"),
    }
}

#[tokio::test]
async fn instance_isolated_a_badge_on_bandit_2_leaves_bandit_1_and_every_other_row_unchanged() {
    let (engine, store) = engine().await;
    let before = every_row(&store);

    let result = set(&engine, BANDIT_2, "Empoisonné", true).await;
    assert_eq!(result.status, State::Applied, "{result:?}");
    assert_eq!(row(&store, BANDIT_2)["conditions"], json!(["Empoisonné"]));
    // Every row as it was, but Bandit 2's conditions: Bandit 1, the
    // ambush's bandit, their HP and Bandit 2's own HP included.
    let mut expected = before;
    for (id, row) in &mut expected {
        if *id == BANDIT_2 {
            row["conditions"] = json!(["Empoisonné"]);
        }
    }
    assert_eq!(every_row(&store), expected);
    assert_eq!(row(&store, BANDIT_1)["conditions"], json!([]));
}

#[tokio::test]
async fn state_persisted_reset_badges_queued_in_a_row_all_stay_and_one_taken_off_goes() {
    let (engine, store) = engine().await;
    for (state, on) in [
        ("Empoisonné", true),
        ("À terre", true),
        ("Effrayé", true),
        ("À terre", false),
    ] {
        let queued = queue(&engine, BANDIT_2, state, on).await;
        // A badge set is the DM's latest: never held for a confirmation.
        assert_eq!(queued.result.status, State::Queued, "{queued:?}");
    }
    let settled = engine.run_once().await.expect("applied");
    assert_eq!(settled.len(), 4, "{settled:?}");
    assert!(settled.iter().all(|result| result.status == State::Applied));
    assert_eq!(
        row(&store, BANDIT_2)["conditions"],
        json!(["Empoisonné", "Effrayé"])
    );
}

#[tokio::test]
async fn a_badge_already_on_is_not_put_twice_and_one_taken_off_is_gone() {
    let (engine, store) = engine().await;
    for _ in 0..2 {
        assert_eq!(
            set(&engine, BANDIT_1, "À terre", true).await.status,
            State::Applied
        );
    }
    assert_eq!(row(&store, BANDIT_1)["conditions"], json!(["À terre"]));
    assert_eq!(
        set(&engine, BANDIT_1, "À terre", false).await.status,
        State::Applied
    );
    assert_eq!(row(&store, BANDIT_1)["conditions"], json!([]));
}

#[tokio::test]
async fn the_badge_lands_on_the_token_s_partition_named_in_any_case() {
    let (engine, store) = engine().await;
    let queued = submit(
        &engine,
        "table.setTokenCondition@1",
        json!({ "token": format!(" {} ", BANDIT_1.to_uppercase()), "state": " À terre ", "on": true }),
        Some(&INSTANCE.to_uppercase()),
    )
    .await
    .expect("queued");
    assert_eq!(queued.partition, format!("BattlemapInstance/{BANDIT_1}"));
    engine.run_once().await.expect("applied");
    assert_eq!(row(&store, BANDIT_1)["conditions"], json!(["À terre"]));
}

#[tokio::test]
async fn without_a_target_the_payload_s_instance_is_the_one_the_token_must_stand_on() {
    let (engine, store) = engine().await;
    submit(
        &engine,
        "table.setTokenCondition@1",
        json!({ "instance": INSTANCE, "token": JETON_ELOWEN, "state": "Effrayé", "on": true }),
        None,
    )
    .await
    .expect("queued");
    engine.run_once().await.expect("applied");
    assert_eq!(row(&store, JETON_ELOWEN)["conditions"], json!(["Effrayé"]));
}

#[tokio::test]
async fn instance_isolated_a_token_of_another_instance_is_refused_and_stays_as_it_was() {
    let (engine, store) = engine().await;
    let message = refused(
        submit(
            &engine,
            "table.setTokenCondition@1",
            json!({ "token": BANDIT_AILLEURS, "state": "À terre", "on": true }),
            Some(INSTANCE),
        )
        .await,
        RefusalKind::Unprocessable,
    );
    assert!(message.contains("instance-isolated"), "{message}");
    assert!(engine.run_once().await.expect("ran").is_empty(), "queued");
    assert_eq!(row(&store, BANDIT_AILLEURS)["conditions"], json!([]));
}

#[tokio::test]
async fn a_token_the_engine_does_not_hold_is_not_found_nor_is_an_instance() {
    let (engine, store) = engine().await;
    for token in [NOWHERE, INSTANCE] {
        let message = refused(
            submit(
                &engine,
                "table.setTokenCondition@1",
                json!({ "token": token, "state": "À terre", "on": true }),
                Some(INSTANCE),
            )
            .await,
            RefusalKind::NotFound,
        );
        assert!(message.contains("token"), "{token}: {message}");
    }
    assert!(engine.run_once().await.expect("ran").is_empty(), "queued");
    assert!(
        store
            .row_now(&format!("BattlemapInstance/{NOWHERE}"))
            .is_empty()
    );
    assert!(row(&store, INSTANCE).get("conditions").is_none());
}

#[tokio::test]
async fn a_badge_set_naming_no_instance_no_token_no_state_or_no_switch_is_refused() {
    let (engine, _) = engine().await;
    for (payload, target, words) in [
        (
            json!({ "token": BANDIT_2, "state": "À terre", "on": true }),
            None,
            "instance",
        ),
        (
            json!({ "state": "À terre", "on": true }),
            Some(INSTANCE),
            "token",
        ),
        (
            json!({ "token": BANDIT_2, "on": true }),
            Some(INSTANCE),
            "state",
        ),
        (
            json!({ "token": BANDIT_2, "state": "   ", "on": true }),
            Some(INSTANCE),
            "state",
        ),
        (
            json!({ "token": BANDIT_2, "state": "À\u{0}terre", "on": true }),
            Some(INSTANCE),
            "state",
        ),
        (
            json!({ "token": BANDIT_2, "state": "À terre" }),
            Some(INSTANCE),
            "on",
        ),
    ] {
        let message = refused(
            submit(
                &engine,
                "table.setTokenCondition@1",
                payload.clone(),
                target,
            )
            .await,
            RefusalKind::Unprocessable,
        );
        assert!(message.starts_with(words), "{payload}: {message}");
    }
    assert!(engine.run_once().await.expect("ran").is_empty(), "queued");
}
