//! table.moveToken as the server plants it beside its target aggregate, over
//! the in-memory store: a token of the instance moves to the hex dropped on,
//! on its own partition, and nothing else moves (`single-position-source`);
//! the last of two drops is where it stands; a token of another instance, or
//! one the engine does not hold, is refused before anything is queued
//! (`instance-isolated`). The tokens and the instances are planted by
//! test-only DataCapabilities, as the aggregate's own tests plant them.

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
const MORLOX: &str = "e1000000-0000-4000-8000-000000000002";
/// The tavern's tokens: Elowen, Morlox 1 and Morlox 2.
const JETON_ELOWEN: &str = "70000000-0000-4000-8000-000000000001";
const MORLOX_1: &str = "70000000-0000-4000-8000-000000000002";
const MORLOX_2: &str = "70000000-0000-4000-8000-000000000003";
/// The ambush's token of Morlox.
const MORLOX_AILLEURS: &str = "70000000-0000-4000-8000-000000000004";
const NOWHERE: &str = "70000000-0000-4000-8000-0000000000ff";

/// The test-only DataCapabilities planting the rows a move lands on.
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
    [aggregate_battlemap_instance::plugin(), move_token::plugin()]
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
        token(JETON_ELOWEN, INSTANCE, ELOWEN, None, (1, 1)),
        token(MORLOX_1, INSTANCE, MORLOX, Some(1), (5, 5)),
        token(MORLOX_2, INSTANCE, MORLOX, Some(2), (5, 6)),
        token(MORLOX_AILLEURS, AUTRE_INSTANCE, MORLOX, Some(1), (0, 0)),
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

/// A token as `table.placeTokens` drops it: an NPC's numbered, with its HP.
fn token(id: &str, instance: &str, character: &str, number: Option<i64>, hex: (i64, i64)) -> Value {
    let name = number.map_or_else(|| "Elowen".to_owned(), |n| format!("Morlox {n}"));
    let hp = number.map(|_| 7);
    json!({
        "tokenId": id,
        "tokenInstanceId": instance,
        "characterId": character,
        "number": number,
        "name": name,
        "q": hex.0,
        "r": hex.1,
        "visible": true,
        "hpCurrent": hp,
        "hpMax": hp,
        "conditions": [],
    })
}

/// A command as the Battlemap sends it, by the DM, on `target`.
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
        "caller": { "by": "dm", "from": "Battlemap", "roles": ["role:approver"] },
    });
    if let Some(target) = target {
        body["target"]["id"] = json!(target);
    }
    engine
        .submit(serde_json::from_value::<Submission>(body).expect("a submission"))
        .await
}

/// A move of `token` to `hex` on the tavern's instance, queued then applied.
async fn moved(engine: &Engine, token: &str, hex: (i64, i64)) -> CommandResult {
    submit(
        engine,
        "table.moveToken@1",
        json!({ "token": token, "q": hex.0, "r": hex.1 }),
        Some(INSTANCE),
    )
    .await
    .expect("queued");
    let mut settled = engine.run_once().await.expect("the applier ran");
    assert_eq!(settled.len(), 1, "{settled:?}");
    settled.remove(0)
}

/// Where the token `id` stands.
fn hex(store: &MemoryStore, id: &str) -> (Value, Value) {
    let row = store.row_now(&format!("BattlemapInstance/{id}"));
    (row["q"].clone(), row["r"].clone())
}

/// The refusal `answer` is, of `kind`: its message.
fn refused(answer: Result<Submitted>, kind: RefusalKind) -> String {
    match answer {
        Err(Error::Refused(refusal)) if refusal.kind == kind => refusal.message,
        other => panic!("a {kind:?} refusal: {other:?}"),
    }
}

#[tokio::test]
async fn single_position_source_the_token_moves_to_the_hex_and_no_other_token_moves() {
    let (engine, store) = engine().await;
    let before = |id| store.row_now(&format!("BattlemapInstance/{id}"));
    let (elowen, morlox_2, ailleurs, instance) = (
        before(JETON_ELOWEN),
        before(MORLOX_2),
        before(MORLOX_AILLEURS),
        before(INSTANCE),
    );
    let morlox_1 = before(MORLOX_1);

    let result = moved(&engine, MORLOX_1, (2, 3)).await;
    assert_eq!(result.status, State::Applied, "{result:?}");
    assert_eq!(hex(&store, MORLOX_1), (json!(2), json!(3)));
    // The token keeps everything but its hex.
    let mut after = store.row_now(&format!("BattlemapInstance/{MORLOX_1}"));
    for axis in ["q", "r"] {
        after.insert(axis.into(), morlox_1[axis].clone());
    }
    assert_eq!(after, morlox_1);
    // No other token moved, and the instance is as it was.
    assert_eq!(before(JETON_ELOWEN), elowen);
    assert_eq!(before(MORLOX_2), morlox_2);
    assert_eq!(before(MORLOX_AILLEURS), ailleurs);
    assert_eq!(before(INSTANCE), instance);
}

#[tokio::test]
async fn the_move_lands_on_the_token_s_partition_named_in_any_case() {
    let (engine, store) = engine().await;
    let queued = submit(
        &engine,
        "table.moveToken@1",
        json!({ "token": format!(" {} ", JETON_ELOWEN.to_uppercase()), "q": -1, "r": 4 }),
        Some(&INSTANCE.to_uppercase()),
    )
    .await
    .expect("queued");
    assert_eq!(
        queued.partition,
        format!("BattlemapInstance/{JETON_ELOWEN}")
    );
    engine.run_once().await.expect("applied");
    assert_eq!(hex(&store, JETON_ELOWEN), (json!(-1), json!(4)));
}

#[tokio::test]
async fn without_a_target_the_payload_s_instance_is_the_one_the_token_must_stand_on() {
    let (engine, store) = engine().await;
    submit(
        &engine,
        "table.moveToken@1",
        json!({ "instance": INSTANCE, "token": MORLOX_2, "q": 8, "r": 1 }),
        None,
    )
    .await
    .expect("queued");
    engine.run_once().await.expect("applied");
    assert_eq!(hex(&store, MORLOX_2), (json!(8), json!(1)));
}

#[tokio::test]
async fn state_persisted_reset_the_last_of_two_drops_queued_is_where_the_token_stands() {
    let (engine, store) = engine().await;
    for hex in [(3, 3), (4, 2)] {
        let queued = submit(
            &engine,
            "table.moveToken@1",
            json!({ "token": MORLOX_1, "q": hex.0, "r": hex.1 }),
            Some(INSTANCE),
        )
        .await
        .expect("queued");
        // A drop is the DM's latest: never held for a confirmation.
        assert_eq!(queued.result.status, State::Queued, "{queued:?}");
    }
    let settled = engine.run_once().await.expect("applied");
    assert_eq!(settled.len(), 2, "{settled:?}");
    assert!(settled.iter().all(|result| result.status == State::Applied));
    assert_eq!(hex(&store, MORLOX_1), (json!(4), json!(2)));
}

#[tokio::test]
async fn instance_isolated_a_token_of_another_instance_is_refused_and_stays_where_it_was() {
    let (engine, store) = engine().await;
    let message = refused(
        submit(
            &engine,
            "table.moveToken@1",
            json!({ "token": MORLOX_AILLEURS, "q": 2, "r": 3 }),
            Some(INSTANCE),
        )
        .await,
        RefusalKind::Unprocessable,
    );
    assert!(message.contains("instance-isolated"), "{message}");
    assert!(engine.run_once().await.expect("ran").is_empty(), "queued");
    assert_eq!(hex(&store, MORLOX_AILLEURS), (json!(0), json!(0)));
}

#[tokio::test]
async fn a_token_the_engine_does_not_hold_is_not_found_and_an_instance_is_no_token() {
    let (engine, store) = engine().await;
    for token in [NOWHERE, INSTANCE] {
        let message = refused(
            submit(
                &engine,
                "table.moveToken@1",
                json!({ "token": token, "q": 2, "r": 3 }),
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
}

#[tokio::test]
async fn a_move_naming_no_instance_no_token_or_no_hex_is_refused() {
    let (engine, _) = engine().await;
    for (payload, target, words) in [
        (
            json!({ "token": MORLOX_1, "q": 2, "r": 3 }),
            None,
            "instance",
        ),
        (json!({ "q": 2, "r": 3 }), Some(INSTANCE), "token"),
        (json!({ "token": MORLOX_1, "r": 3 }), Some(INSTANCE), "q"),
        (
            json!({ "token": MORLOX_1, "q": 2, "r": 0.5 }),
            Some(INSTANCE),
            "r",
        ),
    ] {
        let message = refused(
            submit(&engine, "table.moveToken@1", payload.clone(), target).await,
            RefusalKind::Unprocessable,
        );
        assert!(message.starts_with(words), "{payload}: {message}");
    }
    assert!(engine.run_once().await.expect("ran").is_empty(), "queued");
}
