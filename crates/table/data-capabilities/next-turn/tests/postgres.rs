//! table.nextTurn over PostgreSQL (`DATABASE_URL`), as the server plants it
//! beside its target aggregate: a combat of three Bandits on the tavern's
//! instance, round 1, the first's turn. « Suivant » passes the turn in rank
//! order and counts a round at each lap (`next-turn`), skips a dead one
//! (`dead-derived`), gives a hidden one its turn (`hidden-keeps-rank`),
//! reads what is queued ahead of it — a press, a blow, an end — and is
//! refused when the instance has no open combat. Each test works in a
//! scratch database of its own, migrated as the server migrates it, dropped
//! when it ends.

use std::path::Path;
use std::sync::Arc;

use harness_dataguard::aggregate::Aggregate;
use harness_dataguard::api::{Submission, Submitted};
use harness_dataguard::command::State;
use harness_dataguard::engine::Engine;
use harness_dataguard::manifest::DataCapability;
use harness_dataguard::memory::{FixedClock, MemoryNotifier, MemoryStore};
use harness_dataguard::plugins::{Plugins, RefusalKind};
use harness_dataguard::postgres::{PgStore, ReadOnly, SystemClock};
use harness_dataguard::tokio_postgres::{self, Client, NoTls};
use harness_dataguard::{Error, Result};
use next_turn::partition;
use serde_json::{Value, json};

const INSTANCE: &str = "b1000000-0000-4000-8000-000000000001";
const AUTRE_INSTANCE: &str = "b1000000-0000-4000-8000-000000000002";
const BANDIT: &str = "e1000000-0000-4000-8000-0000000000b1";
const COMBAT: &str = "c0000000-0000-4000-8000-000000000001";

/// The test-only DataCapabilities planting the rows a turn reads, and
/// changing them as the other table commands do.
const DROP_TOKEN: &str = r#"{"dataCapability":"test.dropToken","owner":"dataguard","version":1,"effect":"insert","target":{"aggregate":"BattlemapInstance","id":"$tokenId"},"mode":"overwrite","callableBy":["*"]}"#;
const OPEN_COMBAT: &str = r#"{"dataCapability":"test.openCombat","owner":"dataguard","version":1,"effect":"insert","target":{"aggregate":"BattlemapInstance","id":"$combatId"},"mode":"overwrite","callableBy":["*"]}"#;
const SEAT: &str = r#"{"dataCapability":"test.seat","owner":"dataguard","version":1,"effect":"insert","target":{"aggregate":"BattlemapInstance","id":"$initiativeEntryId"},"mode":"overwrite","callableBy":["*"]}"#;
const WOUND: &str = r#"{"dataCapability":"test.wound","owner":"dataguard","version":1,"effect":"update","target":{"aggregate":"BattlemapInstance","id":"$tokenId"},"touches":["BattlemapInstance.hpCurrent"],"mode":"overwrite","callableBy":["*"]}"#;
const HIDE: &str = r#"{"dataCapability":"test.hide","owner":"dataguard","version":1,"effect":"update","target":{"aggregate":"BattlemapInstance","id":"$tokenId"},"touches":["BattlemapInstance.visible"],"mode":"overwrite","callableBy":["*"]}"#;
const END: &str = r#"{"dataCapability":"test.endCombat","owner":"dataguard","version":1,"effect":"update","target":{"aggregate":"BattlemapInstance","id":"$combatId"},"touches":["BattlemapInstance.endedAt"],"mode":"overwrite","callableBy":["*"]}"#;

/// The id of Bandit `number`.
fn bandit_id(number: u8) -> String {
    format!("70000000-0000-4000-8000-00000000000{number}")
}

/// The id of Bandit `number`'s initiative entry.
fn entry_id(number: u8) -> String {
    format!("ee000000-0000-4000-8000-00000000000{number}")
}

fn url() -> String {
    std::env::var("DATABASE_URL").expect("DATABASE_URL names the harness's PostgreSQL")
}

async fn connect(url: &str) -> Client {
    let (client, connection) = tokio_postgres::connect(url, NoTls)
        .await
        .expect("PostgreSQL");
    tokio::spawn(connection);
    client
}

/// `DATABASE_URL` naming the database `name` instead.
fn naming(name: &str) -> String {
    let url = url();
    let (address, query) = url.split_once('?').unwrap_or((&url, ""));
    let (server, _) = address.rsplit_once('/').expect("a URL naming its database");
    let query = if query.is_empty() {
        String::new()
    } else {
        format!("?{query}")
    };
    format!("{server}/{name}{query}")
}

/// The BattlemapInstance aggregate's document, read beside this unit.
fn aggregate() -> Aggregate {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../aggregates/BattlemapInstance/aggregate.json");
    Aggregate::parse(&std::fs::read_to_string(path).expect("aggregate.json")).expect("contract I")
}

fn manifest(json: &str) -> DataCapability {
    DataCapability::parse(json).expect("contract F")
}

fn plugins() -> Plugins {
    [aggregate_battlemap_instance::plugin(), next_turn::plugin()]
        .into_iter()
        .collect()
}

/// Bandit `number` on the tavern's instance, at `hp` of 11.
fn bandit(number: u8, hp: i64) -> Value {
    json!({
        "tokenId": bandit_id(number),
        "tokenInstanceId": INSTANCE,
        "characterId": BANDIT,
        "number": number,
        "name": format!("Bandit {number}"),
        "q": 5,
        "r": number,
        "visible": true,
        "hpCurrent": hp,
        "hpMax": 11,
        "conditions": [],
    })
}

/// Bandit `number`'s place in the combat: rank `number`.
fn entry(number: u8) -> Value {
    json!({
        "initiativeEntryId": entry_id(number),
        "initiativeEntryCombatId": COMBAT,
        "initiativeEntryTokenId": bandit_id(number),
        "roll": 10,
        "initiative": 11,
        "dexScore": 12,
        "tiebreak": 0.5,
        "rank": number,
    })
}

/// A database of its own, migrated.
struct Scratch {
    name: String,
    url: String,
}

impl Scratch {
    async fn new(name: &str) -> Scratch {
        let name = format!("next_turn_test_{name}_{}", std::process::id());
        let admin = connect(&url()).await;
        for sql in [
            format!("DROP DATABASE IF EXISTS {name} WITH (FORCE)"),
            format!("CREATE DATABASE {name}"),
        ] {
            admin.batch_execute(&sql).await.expect("a scratch database");
        }
        let url = naming(&name);
        let mut client = connect(&url).await;
        let migrations = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../data/migrations");
        data::migrate(&mut client, &migrations)
            .await
            .expect("migrated");
        Scratch { name, url }
    }

    /// The engine as the server plants this unit and its target aggregate
    /// over this database, with the test-only DataCapabilities.
    async fn serve(&self) -> Engine {
        let store = PgStore::open(connect(&self.url).await)
            .await
            .expect("the store")
            .with_projections(&plugins())
            .await
            .expect("warmed");
        let engine = Engine::new(
            Arc::new(store),
            Arc::new(MemoryNotifier::default()),
            Arc::new(SystemClock),
        )
        .with_rows(Arc::new(ReadOnly::new(connect(&self.url).await)))
        .with_aggregate(aggregate())
        .with_data_capability(manifest(include_str!("../data-capability.json")));
        [DROP_TOKEN, OPEN_COMBAT, SEAT, WOUND, HIDE, END]
            .into_iter()
            .fold(engine, |engine, test| {
                engine.with_data_capability(manifest(test))
            })
            .with_plugins(plugins())
    }

    /// Bandit 1 to 3 on the tavern's instance, at `hp`, ranked 1 to 3 in
    /// its combat, round 1, Bandit 1's turn, as the engine holds them.
    async fn plant(&self, engine: &Engine, hp: [i64; 3]) {
        engine.load().await.expect("the engine loads");
        for (number, hp) in (1..=3).zip(hp) {
            submit(engine, "test.dropToken", bandit(number, hp), None)
                .await
                .expect("a token");
        }
        submit(
            engine,
            "test.openCombat",
            json!({
                "combatId": COMBAT,
                "combatInstanceId": INSTANCE,
                "round": 1,
                "currentEntryId": entry_id(1),
                "startedAt": "2026-10-21T10:00:00Z",
            }),
            None,
        )
        .await
        .expect("the combat");
        for number in 1..=3 {
            submit(engine, "test.seat", entry(number), None)
                .await
                .expect("an entry");
        }
        settle(engine, 7).await;
    }

    /// The combat's turn and round, as the engine holds them.
    async fn turn(&self) -> (Value, Value) {
        let fields: Value = connect(&self.url)
            .await
            .query_one(
                "SELECT fields FROM dataguard_rows WHERE partition = $1",
                &[&partition(COMBAT)],
            )
            .await
            .expect("the combat's row")
            .get(0);
        (fields["currentEntryId"].clone(), fields["round"].clone())
    }

    /// Dropped, tried again a few times: a role that is no superuser may
    /// not terminate a backend another role holds there for a moment.
    async fn drop(self) {
        let admin = connect(&url()).await;
        let sql = format!("DROP DATABASE {} WITH (FORCE)", self.name);
        for _ in 0..4 {
            if admin.batch_execute(&sql).await.is_ok() {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        }
        admin
            .batch_execute(&sql)
            .await
            .expect("the scratch database dropped");
    }
}

/// A command by the DM, on `target`.
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

/// « Suivant » on the tavern's instance, as the Battlemap sends it.
async fn press(engine: &Engine) -> Result<Submitted> {
    submit(engine, "table.nextTurn@1", json!({}), Some(INSTANCE)).await
}

/// The applier's run, every one of the `count` commands it settles applied.
async fn settle(engine: &Engine, count: usize) {
    let settled = engine.run_once().await.expect("the applier ran");
    assert_eq!(settled.len(), count, "{settled:?}");
    assert!(
        settled.iter().all(|result| result.status == State::Applied),
        "{settled:?}"
    );
}

/// « Suivant » pressed and applied, then the turn and the round.
async fn pressed(scratch: &Scratch, engine: &Engine) -> (Value, Value) {
    let submitted = press(engine).await.expect("queued");
    assert!(submitted.warnings.is_empty(), "{submitted:?}");
    settle(engine, 1).await;
    scratch.turn().await
}

#[tokio::test]
async fn next_turn_three_presses_from_the_first_go_second_third_first_and_round_2() {
    let scratch = Scratch::new("lap").await;
    let engine = scratch.serve().await;
    scratch.plant(&engine, [11, 11, 11]).await;
    assert_eq!(scratch.turn().await, (json!(entry_id(1)), json!(1)));

    assert_eq!(
        pressed(&scratch, &engine).await,
        (json!(entry_id(2)), json!(1))
    );
    assert_eq!(
        pressed(&scratch, &engine).await,
        (json!(entry_id(3)), json!(1))
    );
    assert_eq!(
        pressed(&scratch, &engine).await,
        (json!(entry_id(1)), json!(2))
    );
    scratch.drop().await;
}

#[tokio::test]
async fn next_turn_three_presses_queued_at_once_each_build_on_the_one_ahead() {
    let scratch = Scratch::new("queued").await;
    let engine = scratch.serve().await;
    scratch.plant(&engine, [11, 11, 11]).await;

    for _ in 0..3 {
        let submitted = press(&engine).await.expect("queued");
        assert!(submitted.warnings.is_empty(), "{submitted:?}");
    }
    settle(&engine, 3).await;
    assert_eq!(scratch.turn().await, (json!(entry_id(1)), json!(2)));
    scratch.drop().await;
}

#[tokio::test]
async fn dead_derived_the_second_at_0_hp_is_skipped_from_the_first() {
    let scratch = Scratch::new("dead").await;
    let engine = scratch.serve().await;
    scratch.plant(&engine, [11, 0, 11]).await;

    assert_eq!(
        pressed(&scratch, &engine).await,
        (json!(entry_id(3)), json!(1))
    );
    // On the wrap, the first takes it: the dead second never does.
    assert_eq!(
        pressed(&scratch, &engine).await,
        (json!(entry_id(1)), json!(2))
    );
    assert_eq!(
        pressed(&scratch, &engine).await,
        (json!(entry_id(3)), json!(2))
    );
    scratch.drop().await;
}

#[tokio::test]
async fn dead_derived_a_blow_queued_ahead_that_kills_the_second_is_read() {
    let scratch = Scratch::new("blow").await;
    let engine = scratch.serve().await;
    scratch.plant(&engine, [11, 11, 11]).await;

    submit(
        &engine,
        "test.wound",
        json!({ "tokenId": bandit_id(2), "hpCurrent": 0 }),
        None,
    )
    .await
    .expect("queued");
    press(&engine).await.expect("queued");
    settle(&engine, 2).await;
    assert_eq!(scratch.turn().await, (json!(entry_id(3)), json!(1)));
    scratch.drop().await;
}

#[tokio::test]
async fn hidden_keeps_rank_a_hidden_participant_gets_its_turn_at_its_rank() {
    let scratch = Scratch::new("hidden").await;
    let engine = scratch.serve().await;
    scratch.plant(&engine, [11, 11, 11]).await;
    submit(
        &engine,
        "test.hide",
        json!({ "tokenId": bandit_id(2), "visible": false }),
        None,
    )
    .await
    .expect("hidden");
    settle(&engine, 1).await;

    assert_eq!(
        pressed(&scratch, &engine).await,
        (json!(entry_id(2)), json!(1))
    );
    scratch.drop().await;
}

#[tokio::test]
async fn next_turn_with_no_open_combat_is_refused_and_nothing_is_queued() {
    let scratch = Scratch::new("none").await;
    let engine = scratch.serve().await;
    scratch.plant(&engine, [11, 11, 11]).await;

    // Another instance, with no combat.
    match submit(&engine, "table.nextTurn@1", json!({}), Some(AUTRE_INSTANCE)).await {
        Err(Error::Refused(refusal)) => assert_eq!(refusal.kind, RefusalKind::Unprocessable),
        other => panic!("no open combat: {other:?}"),
    }
    // The tavern's, once its end is queued, then once it is applied.
    submit(
        &engine,
        "test.endCombat",
        json!({ "combatId": COMBAT, "endedAt": "2026-10-21T11:00:00Z" }),
        None,
    )
    .await
    .expect("ended");
    for _ in 0..2 {
        match press(&engine).await {
            Err(Error::Refused(refusal)) => {
                assert_eq!(refusal.kind, RefusalKind::Unprocessable);
            }
            other => panic!("an ended combat is not open: {other:?}"),
        }
        engine.run_once().await.expect("the applier ran");
    }
    assert_eq!(scratch.turn().await, (json!(entry_id(1)), json!(1)));
    scratch.drop().await;
}

#[tokio::test]
async fn a_combat_whose_participants_are_all_dead_is_refused() {
    let scratch = Scratch::new("all_dead").await;
    let engine = scratch.serve().await;
    scratch.plant(&engine, [0, 0, 0]).await;

    match press(&engine).await {
        Err(Error::Refused(refusal)) => assert_eq!(refusal.kind, RefusalKind::Unprocessable),
        other => panic!("no one alive: {other:?}"),
    }
    assert!(engine.run_once().await.expect("ran").is_empty());
    scratch.drop().await;
}

#[tokio::test]
async fn over_memory_no_combat_is_found_and_the_press_is_refused() {
    let engine = Engine::new(
        Arc::new(MemoryStore::default()),
        Arc::new(MemoryNotifier::default()),
        Arc::new(FixedClock::at(1_792_000_000)),
    )
    .with_aggregate(aggregate())
    .with_data_capability(manifest(include_str!("../data-capability.json")))
    .with_plugins(plugins());
    engine.load().await.expect("the engine loads");
    match press(&engine).await {
        Err(Error::Refused(refusal)) => assert_eq!(refusal.kind, RefusalKind::Unprocessable),
        other => panic!("no combat over memory: {other:?}"),
    }
    match submit(&engine, "table.nextTurn@1", json!({}), Some("taverne")).await {
        Err(Error::Refused(refusal)) => assert_eq!(refusal.kind, RefusalKind::Unprocessable),
        other => panic!("no uuid: {other:?}"),
    }
}
