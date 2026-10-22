//! table.changeHitPoints as the server plants it beside its target
//! aggregate, over the in-memory store: one NPC token's HP change on its own
//! partition, and no other token of the same character
//! (`hp-change-one-token`); its HP land between 0 and its max, and at 0 it
//! is dead by its HP alone (`pj-hp-not-tracked`, `dead-derived`); a PC's
//! token is rejected and keeps no HP; blows sent at once all land; a token
//! of another instance, or one the engine does not hold, is refused before
//! anything is queued (`instance-isolated`). The instances and tokens are
//! planted by test-only DataCapabilities, as the aggregate's own tests plant
//! them.

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
/// The tavern's tokens: Elowen, a PC, and Bandit 1 and 2.
const JETON_ELOWEN: &str = "70000000-0000-4000-8000-000000000001";
const BANDIT_1: &str = "70000000-0000-4000-8000-000000000002";
const BANDIT_2: &str = "70000000-0000-4000-8000-000000000003";
/// The ambush's bandit.
const BANDIT_AILLEURS: &str = "70000000-0000-4000-8000-000000000004";
const NOWHERE: &str = "70000000-0000-4000-8000-0000000000ff";

/// The test-only DataCapabilities planting the rows a change lands among.
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
        change_hit_points::plugin(),
    ]
    .into_iter()
    .collect()
}

/// The engine as the server plants this unit and its aggregate, over an
/// in-memory store, the tavern's and the ambush's instances and tokens
/// planted, Bandit 1 at `hp` of 20; and the store, whose rows a test reads.
async fn engine(hp: i64) -> (Engine, Arc<MemoryStore>) {
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
        token(JETON_ELOWEN, INSTANCE, ELOWEN, None),
        token(BANDIT_1, INSTANCE, BANDIT, Some((1, hp))),
        token(BANDIT_2, INSTANCE, BANDIT, Some((2, 20))),
        token(BANDIT_AILLEURS, AUTRE_INSTANCE, BANDIT, Some((1, 20))),
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

/// A token as `table.placeTokens` drops it: a PC's with no number and no
/// HP, an NPC's numbered, at `hp` of 20.
fn token(id: &str, instance: &str, character: &str, npc: Option<(i64, i64)>) -> Value {
    json!({
        "tokenId": id,
        "tokenInstanceId": instance,
        "characterId": character,
        "number": npc.map(|(number, _)| number),
        "name": npc.map_or_else(|| "Elowen".to_owned(), |(number, _)| format!("Bandit {number}")),
        "q": 1,
        "r": 1,
        "visible": true,
        "hpCurrent": npc.map(|(_, hp)| hp),
        "hpMax": npc.map(|_| 20),
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

/// `delta` applied to `token` of the tavern's instance, queued.
async fn change(engine: &Engine, token: &str, delta: i64) -> Submitted {
    submit(
        engine,
        "table.changeHitPoints@1",
        json!({ "token": token, "delta": delta }),
        Some(INSTANCE),
    )
    .await
    .expect("queued")
}

/// `delta` applied to `token` of the tavern's instance, queued then settled.
async fn changed(engine: &Engine, token: &str, delta: i64) -> CommandResult {
    change(engine, token, delta).await;
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

/// The current HP of the token `id`.
fn hp(store: &MemoryStore, id: &str) -> Value {
    row(store, id)["hpCurrent"].clone()
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
async fn hp_change_one_token_an_npc_at_20_takes_7_and_no_other_token_of_its_character_changes() {
    let (engine, store) = engine(20).await;
    let before = every_row(&store);

    let queued = change(&engine, BANDIT_1, -7).await;
    assert_eq!(queued.partition, format!("BattlemapInstance/{BANDIT_1}"));
    assert!(queued.warnings.is_empty(), "{queued:?}");
    let settled = engine.run_once().await.expect("applied");
    assert_eq!(settled.len(), 1, "{settled:?}");
    assert_eq!(settled[0].status, State::Applied, "{settled:?}");
    assert_eq!(hp(&store, BANDIT_1), json!(13));
    // Every row as it was, but Bandit 1's HP: Bandit 2, the same
    // character's, and the ambush's bandit untouched.
    let mut expected = before;
    for (id, row) in &mut expected {
        if *id == BANDIT_1 {
            row["hpCurrent"] = json!(13);
        }
    }
    assert_eq!(every_row(&store), expected);
}

#[tokio::test]
async fn pj_hp_not_tracked_an_npc_at_3_taking_10_is_at_0_never_below() {
    let (engine, store) = engine(3).await;
    let result = changed(&engine, BANDIT_1, -10).await;
    assert_eq!(result.status, State::Applied, "{result:?}");
    assert_eq!(hp(&store, BANDIT_1), json!(0));
    assert_eq!(hp(&store, BANDIT_2), json!(20));
}

#[tokio::test]
async fn pj_hp_not_tracked_an_npc_at_13_healed_50_is_at_its_max_of_20() {
    let (engine, store) = engine(13).await;
    let result = changed(&engine, BANDIT_1, 50).await;
    assert_eq!(result.status, State::Applied, "{result:?}");
    assert_eq!(hp(&store, BANDIT_1), json!(20));
}

#[tokio::test]
async fn dead_derived_an_npc_at_0_is_dead_by_its_hp_alone_and_healing_brings_it_back() {
    let (engine, store) = engine(5).await;
    assert_eq!(changed(&engine, BANDIT_1, -5).await.status, State::Applied);
    let dead = row(&store, BANDIT_1);
    assert_eq!(dead["hpCurrent"], json!(0));
    // Death is the table's, derived from the HP: the command writes none of
    // what the aggregate derives.
    for derived in ["dead", "skipped", "healthBand", "shownToPlayers"] {
        assert!(dead.get(derived).is_none(), "{derived} written: {dead}");
    }
    // A blow on the dead lands nowhere below 0; healing raises it.
    assert_eq!(changed(&engine, BANDIT_1, -3).await.status, State::Applied);
    assert_eq!(hp(&store, BANDIT_1), json!(0));
    assert_eq!(changed(&engine, BANDIT_1, 4).await.status, State::Applied);
    assert_eq!(hp(&store, BANDIT_1), json!(4));
}

#[tokio::test]
async fn pj_hp_not_tracked_a_pc_s_change_is_rejected_and_its_token_keeps_no_hp() {
    let (engine, store) = engine(20).await;
    let before = every_row(&store);
    for delta in [-5, 0, 5] {
        // Queued — the DM's command is answered, then settles rejected.
        let queued = change(&engine, JETON_ELOWEN, delta).await;
        assert_eq!(queued.result.status, State::Queued, "{queued:?}");
        let mut settled = engine.run_once().await.expect("the applier ran");
        assert_eq!(settled.len(), 1, "{settled:?}");
        let result = settled.remove(0);
        assert_eq!(result.status, State::Rejected, "{delta}: {result:?}");
        assert!(
            result.violations.contains(&"pj-hp-not-tracked".to_owned()),
            "{result:?}"
        );
    }
    assert!(row(&store, JETON_ELOWEN)["hpCurrent"].is_null());
    assert_eq!(every_row(&store), before);
}

#[tokio::test]
async fn blows_sent_at_once_all_land_and_still_stop_at_0() {
    let (engine, store) = engine(20).await;
    for delta in [-7, -6, -4, -10] {
        let queued = change(&engine, BANDIT_1, delta).await;
        // A change of HP is relative: never held for a confirmation.
        assert_eq!(queued.result.status, State::Queued, "{queued:?}");
        assert!(queued.warnings.is_empty(), "{queued:?}");
    }
    let settled = engine.run_once().await.expect("applied");
    assert_eq!(settled.len(), 4, "{settled:?}");
    assert!(
        settled.iter().all(|result| result.status == State::Applied),
        "{settled:?}"
    );
    // 20 − 7 − 6 − 4 = 3, and the last blow of 10 stops at 0.
    assert_eq!(hp(&store, BANDIT_1), json!(0));
    assert_eq!(hp(&store, BANDIT_2), json!(20));
}

#[tokio::test]
async fn without_a_target_the_payload_s_instance_is_the_one_the_token_must_stand_on() {
    let (engine, store) = engine(20).await;
    submit(
        &engine,
        "table.changeHitPoints@1",
        json!({ "instance": INSTANCE, "token": BANDIT_2, "delta": -2 }),
        None,
    )
    .await
    .expect("queued");
    engine.run_once().await.expect("applied");
    assert_eq!(hp(&store, BANDIT_2), json!(18));
}

#[tokio::test]
async fn instance_isolated_a_token_of_another_instance_is_refused_and_stays_as_it_was() {
    let (engine, store) = engine(20).await;
    let message = refused(
        submit(
            &engine,
            "table.changeHitPoints@1",
            json!({ "token": BANDIT_AILLEURS, "delta": -4 }),
            Some(INSTANCE),
        )
        .await,
        RefusalKind::Unprocessable,
    );
    assert!(message.contains("instance-isolated"), "{message}");
    assert!(engine.run_once().await.expect("ran").is_empty(), "queued");
    assert_eq!(hp(&store, BANDIT_AILLEURS), json!(20));
}

#[tokio::test]
async fn a_token_the_engine_does_not_hold_is_not_found_nor_is_an_instance() {
    let (engine, store) = engine(20).await;
    for token in [NOWHERE, INSTANCE] {
        let message = refused(
            submit(
                &engine,
                "table.changeHitPoints@1",
                json!({ "token": token, "delta": -1 }),
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
    assert!(row(&store, INSTANCE).get("hpCurrent").is_none());
}

#[tokio::test]
async fn a_change_naming_no_instance_no_token_or_no_whole_delta_is_refused() {
    let (engine, _) = engine(20).await;
    for (payload, target, words) in [
        (json!({ "token": BANDIT_1, "delta": -1 }), None, "instance"),
        (json!({ "delta": -1 }), Some(INSTANCE), "token"),
        (json!({ "token": BANDIT_1 }), Some(INSTANCE), "delta"),
        (
            json!({ "token": BANDIT_1, "delta": "-1" }),
            Some(INSTANCE),
            "delta",
        ),
    ] {
        let message = refused(
            submit(&engine, "table.changeHitPoints@1", payload.clone(), target).await,
            RefusalKind::Unprocessable,
        );
        assert!(message.starts_with(words), "{payload}: {message}");
    }
    assert!(engine.run_once().await.expect("ran").is_empty(), "queued");
}
