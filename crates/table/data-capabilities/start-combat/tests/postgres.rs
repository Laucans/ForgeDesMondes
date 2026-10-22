//! table.startCombat over PostgreSQL (`DATABASE_URL`), as the server plants
//! it beside its target aggregate: the combat is written on its own
//! partition and each participant's entry on its own, a follow-up of the
//! start, every one judged by the aggregate. R105's table — Elowen a PC
//! entered at 18, the Bandit (DEX 12) rolling 13 and Vesna (DEX 16), hidden,
//! rolling 11 — starts at round 1 on Elowen, Vesna before the Bandit; dice
//! left to the server land on a d20 plus the modifier; a second start is
//! refused while the first is queued or running, and taken once it ended.
//! Each test works in a scratch database of its own, migrated as the server
//! migrates it, dropped when it ends.

use std::collections::VecDeque;
use std::path::Path;
use std::sync::Arc;

use harness_dataguard::aggregate::Aggregate;
use harness_dataguard::api::{FollowUp, Submission, Submitted};
use harness_dataguard::command::State;
use harness_dataguard::engine::Engine;
use harness_dataguard::manifest::DataCapability;
use harness_dataguard::memory::MemoryNotifier;
use harness_dataguard::plugins::{Plugin, Plugins, RefusalKind};
use harness_dataguard::postgres::{PgStore, ReadOnly, SystemClock};
use harness_dataguard::tokio_postgres::{self, Client, NoTls};
use harness_dataguard::{Error, Result};
use serde_json::{Value, json};
use start_combat::{Dice, combat_id, entry_id, partition};

const VALOMBRE: &str = "c1000000-0000-4000-8000-000000000001";
/// Valombre's battlemap with its grid.
const GUE: &str = "d1000000-0000-4000-8000-00000000000a";
const SEANCE: &str = "5e000000-0000-4000-8000-000000000001";
const ARRIVEE: &str = "5c000000-0000-4000-8000-000000000001";
const INSTANCE: &str = "b1000000-0000-4000-8000-000000000001";
const ELOWEN: &str = "e1000000-0000-4000-8000-000000000001";
const BANDIT: &str = "e1000000-0000-4000-8000-0000000000b1";
const VESNA: &str = "e1000000-0000-4000-8000-0000000000b2";
/// Elowen's token, the Bandit's and Vesna's, in the order of their ids: the
/// order the dice are thrown in.
const T_ELOWEN: &str = "70000000-0000-4000-8000-000000000001";
const T_BANDIT: &str = "70000000-0000-4000-8000-000000000002";
const T_VESNA: &str = "70000000-0000-4000-8000-000000000003";

/// The test-only DataCapabilities planting the rows a start lands among,
/// and ending a combat.
const OPEN_INSTANCE: &str = r#"{"dataCapability":"test.openInstance","owner":"dataguard","version":1,"effect":"insert","target":{"aggregate":"BattlemapInstance","id":"$battlemapInstanceId"},"mode":"overwrite","callableBy":["*"]}"#;
const DROP_TOKEN: &str = r#"{"dataCapability":"test.dropToken","owner":"dataguard","version":1,"effect":"insert","target":{"aggregate":"BattlemapInstance","id":"$tokenId"},"mode":"overwrite","callableBy":["*"]}"#;
const REMOVE_TOKEN: &str = r#"{"dataCapability":"test.removeToken","owner":"dataguard","version":1,"effect":"delete","target":{"aggregate":"BattlemapInstance","id":"$token"},"mode":"overwrite","callableBy":["*"]}"#;
const ENTER: &str = r#"{"dataCapability":"test.enter","owner":"dataguard","version":1,"effect":"insert","target":{"aggregate":"BattlemapInstance","id":"$initiativeEntryId"},"mode":"overwrite","callableBy":["*"]}"#;
const END_COMBAT: &str = r#"{"dataCapability":"test.endCombat","owner":"dataguard","version":1,"effect":"update","target":{"aggregate":"BattlemapInstance","id":"$combat"},"touches":["BattlemapInstance.endedAt","BattlemapInstance.currentEntryId"],"mode":"overwrite","callableBy":["*"]}"#;

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

/// Dice that throw what the test scripts, in order; then 10s and 0s.
#[derive(Default)]
struct Scripted {
    d20s: VecDeque<i64>,
    draws: VecDeque<i64>,
}

impl Scripted {
    fn new(d20s: &[i64], draws: &[i64]) -> Self {
        Self {
            d20s: d20s.iter().copied().collect(),
            draws: draws.iter().copied().collect(),
        }
    }
}

impl Dice for Scripted {
    fn d20(&mut self) -> i64 {
        self.d20s.pop_front().unwrap_or(10)
    }

    fn draw(&mut self) -> i64 {
        self.draws.pop_front().unwrap_or(0)
    }
}

/// A token of `character` on the tavern's instance, as `table.placeTokens`
/// drops it; an NPC's tracks 11 HP.
fn token(id: &str, character: &str, name: &str, visible: bool) -> Value {
    let npc = character != ELOWEN;
    json!({
        "tokenId": id,
        "tokenInstanceId": INSTANCE,
        "characterId": character,
        "number": if npc { json!(1) } else { Value::Null },
        "name": name,
        "q": 5,
        "r": 5,
        "visible": visible,
        "hpCurrent": if npc { json!(11) } else { Value::Null },
        "hpMax": if npc { json!(11) } else { Value::Null },
        "conditions": [],
    })
}

/// A database of its own, migrated, holding Valombre, its battlemap with
/// its grid, the session and its scene, and the characters with their DEX.
struct Scratch {
    name: String,
    url: String,
    client: Client,
}

impl Scratch {
    async fn new(name: &str) -> Scratch {
        let name = format!("start_combat_test_{name}_{}", std::process::id());
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
        client
            .batch_execute(&format!(
                "INSERT INTO campaign (id, name, kind) VALUES
                     ('{VALOMBRE}', 'Les Cendres de Valombre', 'campagne');
                 INSERT INTO media (id, campaign_id, kind, title, file_path, width_px, height_px) VALUES
                     ('{GUE}', '{VALOMBRE}', 'battlemap', 'Le Gué des Corbeaux', 'media/gue.png', 960, 640);
                 INSERT INTO battlemap_grid (media_id, hex_size_px, offset_x, offset_y, orientation, color, opacity) VALUES
                     ('{GUE}', 30, 12, 8, 'pointe', 'text', 0.4);
                 INSERT INTO session (id, campaign_id, position, title, status, microphone_on) VALUES
                     ('{SEANCE}', '{VALOMBRE}', 1, 'Le Gué', 'live', true);
                 INSERT INTO scene (id, session_id, position, title) VALUES
                     ('{ARRIVEE}', '{SEANCE}', 1, 'Arrivée');
                 INSERT INTO entity (id, campaign_id, type, name) VALUES
                     ('{ELOWEN}', '{VALOMBRE}', 'pj', 'Elowen Ventargent'),
                     ('{BANDIT}', '{VALOMBRE}', 'pnj', 'Bandit'),
                     ('{VESNA}', '{VALOMBRE}', 'pnj', 'Chef bandit');
                 INSERT INTO character (entity_id, hp_max, dexterity, token_color) VALUES
                     ('{ELOWEN}', 44, 14, 'success'),
                     ('{BANDIT}', 11, 12, 'danger'),
                     ('{VESNA}', 65, 16, 'danger');
                 INSERT INTO entity_version (entity_id, version_number, source, snapshot, diff)
                     SELECT id, 1, 'dm', '{{}}', '[]' FROM entity;"
            ))
            .await
            .expect("Valombre");
        Scratch { name, url, client }
    }

    /// The engine as the server plants this unit and its target aggregate
    /// over this database, its dice `start_combat`'s, with the test-only
    /// DataCapabilities.
    async fn serve(&self, start_combat: Plugin) -> Engine {
        let plugins: Plugins = [aggregate_battlemap_instance::plugin(), start_combat]
            .into_iter()
            .collect();
        let store = PgStore::open(connect(&self.url).await)
            .await
            .expect("the store")
            .with_projections(&plugins)
            .await
            .expect("warmed");
        let engine = Engine::new(
            Arc::new(store),
            Arc::new(MemoryNotifier::default()),
            Arc::new(SystemClock),
        )
        .with_rows(Arc::new(ReadOnly::new(connect(&self.url).await)))
        .with_aggregate(aggregate())
        .with_data_capability(manifest(include_str!("../data-capability.json")))
        .with_data_capability(manifest(OPEN_INSTANCE))
        .with_data_capability(manifest(DROP_TOKEN))
        .with_data_capability(manifest(REMOVE_TOKEN))
        .with_data_capability(manifest(ENTER))
        .with_data_capability(manifest(END_COMBAT))
        .with_plugins(plugins);
        engine.load().await.expect("the engine loads");
        engine
    }

    /// The tavern scene's instance of the ford, and on it Elowen, the
    /// Bandit and Vesna — hidden —, as the engine holds them.
    async fn plant(&self, engine: &Engine) {
        submit(
            engine,
            "test.openInstance",
            json!({
                "battlemapInstanceId": INSTANCE,
                "battlemapMediaId": GUE,
                "sessionId": SEANCE,
                "sceneId": ARRIVEE,
                "createdAt": "2026-10-21T10:00:00Z",
            }),
            None,
        )
        .await
        .expect("the instance");
        for token in [
            token(T_ELOWEN, ELOWEN, "Elowen", true),
            token(T_BANDIT, BANDIT, "Bandit 1", true),
            token(T_VESNA, VESNA, "Chef bandit 1", false),
        ] {
            submit(engine, "test.dropToken", token, None)
                .await
                .expect("a token");
        }
        applied(engine).await;
    }

    /// The row the engine holds on `partition`; `None` when it was never
    /// written or is tombstoned.
    async fn row(&self, partition: &str) -> Option<Value> {
        self.client
            .query_opt(
                "SELECT fields::text FROM dataguard_rows WHERE partition = $1 AND NOT tombstoned",
                &[&partition],
            )
            .await
            .expect("the engine's rows")
            .map(|row| serde_json::from_str(row.get::<_, &str>(0)).expect("JSON"))
    }

    /// The entries the engine holds of `combat`, rank 1 first.
    async fn entries(&self, combat: &str) -> Vec<Value> {
        let mut entries: Vec<Value> = self
            .client
            .query(
                "SELECT fields::text FROM dataguard_rows
                 WHERE NOT tombstoned AND fields ->> 'initiativeEntryCombatId' = $1",
                &[&combat],
            )
            .await
            .expect("the engine's rows")
            .iter()
            .map(|row| serde_json::from_str(row.get::<_, &str>(0)).expect("JSON"))
            .collect();
        entries.sort_by_key(|entry| entry["rank"].as_i64());
        entries
    }

    async fn drop(self) {
        drop(self.client);
        connect(&url())
            .await
            .batch_execute(&format!("DROP DATABASE {} WITH (FORCE)", self.name))
            .await
            .expect("the scratch database dropped");
    }
}

/// Runs the queue until it is drained, every command applied.
async fn applied(engine: &Engine) {
    loop {
        let run = engine.run_once().await.expect("a run");
        if run.is_empty() {
            return;
        }
        assert!(
            run.iter().all(|result| result.status == State::Applied),
            "{run:?}"
        );
    }
}

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

/// A start of the tavern's instance, as the Battlemap sends it.
async fn start(engine: &Engine, payload: Value) -> Result<Submitted> {
    submit(engine, "table.startCombat@1", payload, Some(INSTANCE)).await
}

/// R105's start: Elowen entered at 18, the Bandit rolling 13, Vesna 11.
fn r105() -> Value {
    json!({ "initiative": { ELOWEN: 18 }, "rolls": { T_BANDIT: 13, T_VESNA: 11 } })
}

/// The refusal `result` is, by its kind and its message.
fn refusal(result: Result<Submitted>) -> (RefusalKind, String) {
    match result {
        Err(Error::Refused(refusal)) => (refusal.kind, refusal.message),
        other => panic!("refused, not {other:?}"),
    }
}

/// The tokens of `entries`, rank 1 first.
fn order(entries: &[Value]) -> Vec<&str> {
    entries
        .iter()
        .map(|entry| entry["initiativeEntryTokenId"].as_str().expect("a token"))
        .collect()
}

#[tokio::test(flavor = "current_thread")]
async fn r105_the_pc_first_dex_16_before_dex_12_round_1_on_rank_1_the_hidden_one_ranked() {
    let scratch = Scratch::new("r105").await;
    let engine = scratch
        .serve(start_combat::plugin_with(Scripted::new(&[], &[1, 99, 2])))
        .await;
    scratch.plant(&engine).await;

    let started = start(&engine, r105()).await.expect("started");
    let combat = combat_id(INSTANCE, 0);
    assert_eq!(started.partition, partition(&combat));
    assert!(
        !started
            .warnings
            .iter()
            .any(|warning| warning.starts_with("invariant(s) at risk")),
        "{:?}",
        started.warnings
    );
    // One seat per participant, each queued on its entry's partition.
    assert_eq!(started.then.len(), 3, "{:?}", started.then);
    for follow_up in &started.then {
        let FollowUp::Submitted(seat) = follow_up else {
            panic!("a seat refused: {follow_up:?}");
        };
        assert!(seat.warnings.is_empty(), "{:?}", seat.warnings);
    }
    applied(&engine).await;

    let entries = scratch.entries(&combat).await;
    // The draws would put the Bandit first: DEX comes before them.
    assert_eq!(order(&entries), [T_ELOWEN, T_VESNA, T_BANDIT]);
    let ranks: Vec<&Value> = entries.iter().map(|entry| &entry["rank"]).collect();
    assert_eq!(ranks, [&json!(1), &json!(2), &json!(3)]);
    let (elowen, vesna, bandit) = (&entries[0], &entries[1], &entries[2]);
    // A PC rolls nothing: its initiative is the DM's figure.
    assert_eq!(elowen["roll"], Value::Null);
    assert_eq!(elowen["initiative"], 18);
    assert_eq!(elowen["dexScore"], 14);
    assert_eq!(
        (&vesna["roll"], &vesna["initiative"], &vesna["dexScore"]),
        (&json!(11), &json!(14), &json!(16))
    );
    assert_eq!(
        (&bandit["roll"], &bandit["initiative"], &bandit["dexScore"]),
        (&json!(13), &json!(14), &json!(12))
    );
    for entry in &entries {
        let token = entry["initiativeEntryTokenId"].as_str().expect("a token");
        assert_eq!(entry["initiativeEntryId"], entry_id(&combat, token));
    }
    let row = scratch.row(&partition(&combat)).await.expect("the combat");
    assert_eq!(row["combatId"], combat);
    assert_eq!(row["combatInstanceId"], INSTANCE);
    assert_eq!(row["round"], 1);
    assert_eq!(row["currentEntryId"], elowen["initiativeEntryId"]);
    assert!(row["startedAt"].as_str().is_some(), "{row}");
    assert_eq!(row.get("endedAt"), None);
    // Vesna stays hidden: the start writes no token.
    assert_eq!(
        scratch.row(&partition(T_VESNA)).await.expect("Vesna")["visible"],
        false
    );
    scratch.drop().await;
}

#[tokio::test(flavor = "current_thread")]
async fn npc_initiative_bounds_dice_left_to_the_server_land_on_a_d20_plus_the_modifier() {
    let scratch = Scratch::new("dice").await;
    let engine = scratch.serve(start_combat::plugin()).await;
    scratch.plant(&engine).await;

    start(&engine, json!({ "initiative": { ELOWEN: 9 } }))
        .await
        .expect("started");
    applied(&engine).await;

    let entries = scratch.entries(&combat_id(INSTANCE, 0)).await;
    assert_eq!(entries.len(), 3);
    for entry in &entries {
        let (roll, initiative, dex) = (
            entry["roll"].as_i64(),
            entry["initiative"].as_i64().expect("an initiative"),
            entry["dexScore"].as_i64().expect("a DEX"),
        );
        if entry["initiativeEntryTokenId"] == T_ELOWEN {
            assert_eq!((roll, initiative), (None, 9));
            continue;
        }
        let roll = roll.expect("an NPC rolls");
        assert!((1..=20).contains(&roll), "{entry}");
        assert_eq!(initiative, roll + (dex - 10).div_euclid(2), "{entry}");
    }
    scratch.drop().await;
}

#[tokio::test(flavor = "current_thread")]
async fn npc_initiative_bounds_an_npc_with_no_roll_sent_throws_the_injected_dice() {
    let scratch = Scratch::new("scripted").await;
    // The Bandit's token comes before Vesna's: it throws first.
    let engine = scratch
        .serve(start_combat::plugin_with(Scripted::new(&[1, 20], &[])))
        .await;
    scratch.plant(&engine).await;

    start(&engine, json!({ "initiative": { ELOWEN: 18 } }))
        .await
        .expect("started");
    applied(&engine).await;

    let entries = scratch.entries(&combat_id(INSTANCE, 0)).await;
    assert_eq!(order(&entries), [T_VESNA, T_ELOWEN, T_BANDIT]);
    assert_eq!(
        (&entries[0]["roll"], &entries[0]["initiative"]),
        (&json!(20), &json!(23))
    );
    assert_eq!(
        (&entries[2]["roll"], &entries[2]["initiative"]),
        (&json!(1), &json!(2))
    );
    scratch.drop().await;
}

#[tokio::test(flavor = "current_thread")]
async fn one_active_combat_a_second_start_is_refused_queued_or_running_and_taken_once_ended() {
    let scratch = Scratch::new("one_active").await;
    let engine = scratch.serve(start_combat::plugin()).await;
    scratch.plant(&engine).await;

    let first = combat_id(INSTANCE, 0);
    start(&engine, r105()).await.expect("the first start");
    // Queued, not yet applied: the second is refused all the same.
    let (kind, why) = refusal(start(&engine, r105()).await);
    assert_eq!(kind, RefusalKind::Unprocessable);
    assert!(why.starts_with("one-active-combat"), "{why}");
    applied(&engine).await;
    let (kind, why) = refusal(start(&engine, r105()).await);
    assert_eq!(kind, RefusalKind::Unprocessable);
    assert!(why.starts_with("one-active-combat"), "{why}");
    assert_eq!(scratch.entries(&first).await.len(), 3);

    // Ended, the instance starts its next combat, under a new id.
    submit(
        &engine,
        "test.endCombat",
        json!({ "endedAt": "2026-10-21T11:00:00Z", "currentEntryId": null }),
        Some(&first),
    )
    .await
    .expect("ended");
    applied(&engine).await;
    let second = start(&engine, r105()).await.expect("the next start");
    let next = combat_id(INSTANCE, 1);
    assert_eq!(second.partition, partition(&next));
    applied(&engine).await;
    assert_eq!(scratch.entries(&next).await.len(), 3);
    // The first combat and its entries stay as they were.
    assert!(scratch.row(&partition(&first)).await.expect("the first")["endedAt"].is_string());
    assert_eq!(scratch.entries(&first).await.len(), 3);
    scratch.drop().await;
}

#[tokio::test(flavor = "current_thread")]
async fn one_active_combat_an_open_combat_the_tables_hold_starts_none() {
    let scratch = Scratch::new("table_combat").await;
    let engine = scratch.serve(start_combat::plugin()).await;
    scratch.plant(&engine).await;
    // An instance and a combat of it the tables hold, the engine not.
    scratch
        .client
        .batch_execute(&format!(
            "INSERT INTO battlemap_instance (id, battlemap_media_id, session_id, scene_id) VALUES
                 ('{INSTANCE}', '{GUE}', '{SEANCE}', '{ARRIVEE}');
             INSERT INTO combat (id, instance_id, round, started_at) VALUES
                 ('c0000000-0000-4000-8000-000000000001', '{INSTANCE}', 2, now());"
        ))
        .await
        .expect("a combat in the tables");
    let (kind, why) = refusal(start(&engine, r105()).await);
    assert_eq!(kind, RefusalKind::Unprocessable);
    assert!(why.starts_with("one-active-combat"), "{why}");

    // Ended in the tables, it is none.
    scratch
        .client
        .batch_execute("UPDATE combat SET ended_at = now()")
        .await
        .expect("ended");
    start(&engine, r105()).await.expect("started");
    scratch.drop().await;
}

#[tokio::test(flavor = "current_thread")]
async fn a_start_of_no_instance_or_no_token_is_refused_and_a_removed_token_is_none() {
    let scratch = Scratch::new("refused").await;
    let engine = scratch.serve(start_combat::plugin()).await;

    // No such instance: 404.
    let (kind, _) = refusal(start(&engine, r105()).await);
    assert_eq!(kind, RefusalKind::NotFound);
    // The ids the DM sends are uuids: 422.
    let (kind, why) = refusal(start(&engine, json!({ "initiative": { "elowen": 18 } })).await);
    assert_eq!(kind, RefusalKind::Unprocessable);
    assert!(why.starts_with("initiative"), "{why}");

    scratch.plant(&engine).await;
    // Elowen's initiative is the DM's to enter.
    let (kind, why) = refusal(start(&engine, json!({})).await);
    assert_eq!(kind, RefusalKind::Unprocessable);
    assert!(why.contains(ELOWEN), "{why}");

    // Every token taken off the map: no one to fight.
    for token in [T_ELOWEN, T_BANDIT, T_VESNA] {
        submit(&engine, "test.removeToken", json!({}), Some(token))
            .await
            .expect("removed");
    }
    applied(&engine).await;
    let (kind, why) = refusal(start(&engine, json!({})).await);
    assert_eq!(kind, RefusalKind::Unprocessable);
    assert!(why.starts_with("aucun token"), "{why}");
    scratch.drop().await;
}

#[tokio::test(flavor = "current_thread")]
async fn a_seat_sent_again_or_of_no_open_combat_is_refused() {
    let scratch = Scratch::new("seat").await;
    let engine = scratch.serve(start_combat::plugin()).await;
    scratch.plant(&engine).await;
    start(&engine, r105()).await.expect("started");
    applied(&engine).await;
    let combat = combat_id(INSTANCE, 0);
    let entries = scratch.entries(&combat).await;

    // A seat sent again, as a follow-up replayed: seated already.
    let (kind, why) = refusal(
        submit(
            &engine,
            "table.startCombat",
            json!({ "entry": entries[2] }),
            None,
        )
        .await,
    );
    assert_eq!(kind, RefusalKind::Unprocessable);
    assert!(why.contains("déjà placée"), "{why}");

    // An entry forged for a combat that was never started.
    let mut forged = entries[2].clone();
    let nowhere = combat_id(INSTANCE, 7);
    forged["initiativeEntryCombatId"] = json!(nowhere);
    forged["initiativeEntryId"] = json!(entry_id(&nowhere, T_BANDIT));
    let (kind, why) = refusal(
        submit(
            &engine,
            "table.startCombat",
            json!({ "entry": forged }),
            None,
        )
        .await,
    );
    assert_eq!(kind, RefusalKind::Unprocessable);
    assert!(why.starts_with("instance-isolated"), "{why}");
    assert_eq!(
        scratch.row(&partition(&entry_id(&nowhere, T_BANDIT))).await,
        None
    );
    scratch.drop().await;
}

#[tokio::test(flavor = "current_thread")]
async fn one_active_combat_two_starts_at_the_same_moment_the_second_waits_for_its_author() {
    let scratch = Scratch::new("at_once").await;
    let engine = scratch.serve(start_combat::plugin()).await;
    scratch.plant(&engine).await;

    // Both are prepared before either is queued: the same combat, the same
    // entries.
    let (first, second) = tokio::join!(start(&engine, r105()), start(&engine, r105()));
    let (first, second) = (first.expect("the first"), second.expect("the second"));
    let combat = partition(&combat_id(INSTANCE, 0));
    assert_eq!((&first.partition, &second.partition), (&combat, &combat));
    assert_eq!(first.result.status, State::Queued);
    // Never rejected for concurrency: the second waits for its author to
    // confirm or cancel, and its seats are the first one's already.
    assert_eq!(second.result.status, State::AwaitingConfirmation);
    assert!(
        matches!(&second.then[..], [FollowUp::Refused { error, .. }] if error.contains("déjà placée")),
        "{:?}",
        second.then
    );
    applied(&engine).await;
    let entries = scratch.entries(&combat_id(INSTANCE, 0)).await;
    assert_eq!(order(&entries), [T_ELOWEN, T_VESNA, T_BANDIT]);
    scratch.drop().await;
}

#[tokio::test(flavor = "current_thread")]
async fn a_combat_id_whose_entries_were_seated_is_taken_by_no_new_combat() {
    let scratch = Scratch::new("orphans").await;
    let engine = scratch.serve(start_combat::plugin()).await;
    scratch.plant(&engine).await;
    // An entry of the instance's first combat, whose own row never landed.
    let orphan = combat_id(INSTANCE, 0);
    submit(
        &engine,
        "test.enter",
        json!({
            "initiativeEntryId": entry_id(&orphan, T_BANDIT),
            "initiativeEntryCombatId": orphan,
            "initiativeEntryTokenId": T_BANDIT,
            "roll": 10,
            "initiative": 11,
            "dexScore": 12,
            "tiebreak": 0,
            "rank": 1,
        }),
        None,
    )
    .await
    .expect("an orphan entry");
    applied(&engine).await;

    let started = start(&engine, r105()).await.expect("started");
    let next = combat_id(INSTANCE, 1);
    assert_eq!(started.partition, partition(&next));
    applied(&engine).await;
    assert_eq!(scratch.entries(&next).await.len(), 3);
    scratch.drop().await;
}

#[tokio::test(flavor = "current_thread")]
async fn an_entry_of_null_is_a_start_not_a_seat() {
    let scratch = Scratch::new("entry_null").await;
    let engine = scratch.serve(start_combat::plugin()).await;
    scratch.plant(&engine).await;
    let mut payload = r105();
    payload["entry"] = Value::Null;
    let started = start(&engine, payload).await.expect("started");
    assert_eq!(started.partition, partition(&combat_id(INSTANCE, 0)));
    assert_eq!(started.then.len(), 3);
    scratch.drop().await;
}
