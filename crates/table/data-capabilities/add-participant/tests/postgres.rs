//! table.addParticipant over PostgreSQL (`DATABASE_URL`), as the server
//! plants it beside its target aggregate: the newcomer's entry is written on
//! its own partition and each participant it lands ahead of moves one rank
//! down, a follow-up of the joining, every one judged by the aggregate. R105's
//! combat — Elowen a PC at 18, Vesna (DEX 16) at 14, the Bandit (DEX 12) at
//! 14 — takes the wolf (DEX 14) rolling 13 second; a PC joins with the DM's
//! figure; a token in the combat, queued or applied, joins no more; an
//! instance with no open combat takes no one; a participant
//! `table.removeParticipant` took out stays out. Each test works in a scratch
//! database of its own, migrated as the server migrates it, dropped when it
//! ends.

use std::collections::VecDeque;
use std::path::Path;
use std::sync::Arc;

use add_participant::{Dice, entry_id, partition};
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

const VALOMBRE: &str = "c1000000-0000-4000-8000-000000000001";
/// Valombre's battlemap with its grid.
const GUE: &str = "d1000000-0000-4000-8000-00000000000a";
const SEANCE: &str = "5e000000-0000-4000-8000-000000000001";
const ARRIVEE: &str = "5c000000-0000-4000-8000-000000000001";
const INSTANCE: &str = "b1000000-0000-4000-8000-000000000001";
const COMBAT: &str = "c0000000-0000-4000-8000-000000000001";
const ELOWEN: &str = "e1000000-0000-4000-8000-000000000001";
const GARRICK: &str = "e1000000-0000-4000-8000-000000000002";
const BANDIT: &str = "e1000000-0000-4000-8000-0000000000b1";
const VESNA: &str = "e1000000-0000-4000-8000-0000000000b2";
const LOUP: &str = "e1000000-0000-4000-8000-0000000000b3";
const OMBRE: &str = "e1000000-0000-4000-8000-0000000000b4";
const T_ELOWEN: &str = "70000000-0000-4000-8000-000000000001";
const T_BANDIT: &str = "70000000-0000-4000-8000-000000000002";
const T_VESNA: &str = "70000000-0000-4000-8000-000000000003";
const T_LOUP: &str = "70000000-0000-4000-8000-000000000004";
const T_OMBRE: &str = "70000000-0000-4000-8000-000000000005";
const T_GARRICK: &str = "70000000-0000-4000-8000-000000000006";

/// The test-only DataCapabilities planting the rows a joining lands among,
/// and ending a combat.
const OPEN_INSTANCE: &str = r#"{"dataCapability":"test.openInstance","owner":"dataguard","version":1,"effect":"insert","target":{"aggregate":"BattlemapInstance","id":"$battlemapInstanceId"},"mode":"overwrite","callableBy":["*"]}"#;
const DROP_TOKEN: &str = r#"{"dataCapability":"test.dropToken","owner":"dataguard","version":1,"effect":"insert","target":{"aggregate":"BattlemapInstance","id":"$tokenId"},"mode":"overwrite","callableBy":["*"]}"#;
const REMOVE_TOKEN: &str = r#"{"dataCapability":"test.removeToken","owner":"dataguard","version":1,"effect":"delete","target":{"aggregate":"BattlemapInstance","id":"$token"},"mode":"overwrite","callableBy":["*"]}"#;
const OPEN_COMBAT: &str = r#"{"dataCapability":"test.openCombat","owner":"dataguard","version":1,"effect":"insert","target":{"aggregate":"BattlemapInstance","id":"$combatId"},"mode":"overwrite","callableBy":["*"]}"#;
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

/// `table.removeParticipant`'s manifest, read beside this unit.
fn remove_participant() -> DataCapability {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../remove-participant/data-capability.json");
    manifest(&std::fs::read_to_string(path).expect("data-capability.json"))
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
fn token(id: &str, character: &str, name: &str) -> Value {
    let npc = ![ELOWEN, GARRICK].contains(&character);
    json!({
        "tokenId": id,
        "tokenInstanceId": INSTANCE,
        "characterId": character,
        "number": if npc { json!(1) } else { Value::Null },
        "name": name,
        "q": 5,
        "r": 5,
        "visible": true,
        "hpCurrent": if npc { json!(11) } else { Value::Null },
        "hpMax": if npc { json!(11) } else { Value::Null },
        "conditions": [],
    })
}

/// The entry of `token` in R105's combat, as `table.startCombat` seats it.
fn entry(token: &str, roll: Value, initiative: i64, dex: i64, draw: i64, rank: i64) -> Value {
    json!({
        "initiativeEntryId": entry_id(COMBAT, token),
        "initiativeEntryCombatId": COMBAT,
        "initiativeEntryTokenId": token,
        "roll": roll,
        "initiative": initiative,
        "dexScore": dex,
        "tiebreak": draw,
        "rank": rank,
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
        let name = format!("add_participant_test_{name}_{}", std::process::id());
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
                     ('{GARRICK}', '{VALOMBRE}', 'pj', 'Garrick'),
                     ('{BANDIT}', '{VALOMBRE}', 'pnj', 'Bandit'),
                     ('{VESNA}', '{VALOMBRE}', 'pnj', 'Chef bandit'),
                     ('{LOUP}', '{VALOMBRE}', 'pnj', 'Loup'),
                     ('{OMBRE}', '{VALOMBRE}', 'pnj', 'Ombre');
                 INSERT INTO character (entity_id, hp_max, dexterity, token_color) VALUES
                     ('{ELOWEN}', 44, 14, 'success'),
                     ('{GARRICK}', 52, 10, 'success'),
                     ('{BANDIT}', 11, 12, 'danger'),
                     ('{VESNA}', 65, 16, 'danger'),
                     ('{LOUP}', 11, 14, 'danger'),
                     ('{OMBRE}', 11, 8, 'danger');
                 INSERT INTO entity_version (entity_id, version_number, source, snapshot, diff)
                     SELECT id, 1, 'dm', '{{}}', '[]' FROM entity;"
            ))
            .await
            .expect("Valombre");
        Scratch { name, url, client }
    }

    /// The engine as the server plants this unit and its target aggregate
    /// over this database, its dice `add_participant`'s, with
    /// `table.removeParticipant` and the test-only DataCapabilities.
    async fn serve(&self, add_participant: Plugin) -> Engine {
        let plugins: Plugins = [
            aggregate_battlemap_instance::plugin(),
            add_participant,
            remove_participant::plugin(),
        ]
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
        .with_data_capability(remove_participant())
        .with_data_capability(manifest(OPEN_INSTANCE))
        .with_data_capability(manifest(DROP_TOKEN))
        .with_data_capability(manifest(REMOVE_TOKEN))
        .with_data_capability(manifest(OPEN_COMBAT))
        .with_data_capability(manifest(ENTER))
        .with_data_capability(manifest(END_COMBAT))
        .with_plugins(plugins);
        engine.load().await.expect("the engine loads");
        engine
    }

    /// The tavern scene's instance of the ford, and on it every character's
    /// token, as the engine holds them.
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
            token(T_ELOWEN, ELOWEN, "Elowen"),
            token(T_BANDIT, BANDIT, "Bandit 1"),
            token(T_VESNA, VESNA, "Chef bandit 1"),
            token(T_LOUP, LOUP, "Loup 1"),
            token(T_OMBRE, OMBRE, "Ombre 1"),
            token(T_GARRICK, GARRICK, "Garrick"),
        ] {
            submit(engine, "test.dropToken", token, None)
                .await
                .expect("a token");
        }
        applied(engine).await;
    }

    /// R105's combat, as `table.startCombat` leaves it: Elowen at 18,
    /// Vesna rolling 11 (14), the Bandit rolling 13 (14), round 1 on Elowen.
    async fn fight(&self, engine: &Engine) {
        submit(
            engine,
            "test.openCombat",
            json!({
                "combatId": COMBAT,
                "combatInstanceId": INSTANCE,
                "round": 1,
                "currentEntryId": entry_id(COMBAT, T_ELOWEN),
                "startedAt": "2026-10-22T10:00:00Z",
            }),
            None,
        )
        .await
        .expect("the combat");
        for entry in [
            entry(T_ELOWEN, Value::Null, 18, 14, 1, 1),
            entry(T_VESNA, json!(11), 14, 16, 99, 2),
            entry(T_BANDIT, json!(13), 14, 12, 2, 3),
        ] {
            submit(engine, "test.enter", entry, None)
                .await
                .expect("an entry");
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

    /// The entries the engine holds of the combat, rank 1 first.
    async fn entries(&self) -> Vec<Value> {
        let mut entries: Vec<Value> = self
            .client
            .query(
                "SELECT fields::text FROM dataguard_rows
                 WHERE NOT tombstoned AND fields ->> 'initiativeEntryCombatId' = $1",
                &[&COMBAT],
            )
            .await
            .expect("the engine's rows")
            .iter()
            .map(|row| serde_json::from_str(row.get::<_, &str>(0)).expect("JSON"))
            .collect();
        entries.sort_by_key(|entry| entry["rank"].as_i64());
        entries
    }

    /// Drops the database — again, a moment later, while a backend a role
    /// that is no superuser cannot end (an autovacuum worker) is still in it.
    async fn drop(self) {
        drop(self.client);
        let admin = connect(&url()).await;
        let sql = format!("DROP DATABASE {} WITH (FORCE)", self.name);
        for _ in 0..20 {
            if admin.batch_execute(&sql).await.is_ok() {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(250));
        }
        admin
            .batch_execute(&sql)
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

/// `token` joining the tavern's combat, as the Battlemap sends it.
async fn join(engine: &Engine, payload: Value) -> Result<Submitted> {
    submit(engine, "table.addParticipant@1", payload, Some(INSTANCE)).await
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

/// The ranks of `entries`, in order.
fn ranks(entries: &[Value]) -> Vec<i64> {
    entries
        .iter()
        .map(|entry| entry["rank"].as_i64().expect("a rank"))
        .collect()
}

/// Asserts `submitted` and every follow-up of it were taken with no warning.
fn clean(submitted: &Submitted) {
    assert!(submitted.warnings.is_empty(), "{:?}", submitted.warnings);
    for follow_up in &submitted.then {
        let FollowUp::Submitted(moved) = follow_up else {
            panic!("a move refused: {follow_up:?}");
        };
        assert!(moved.warnings.is_empty(), "{:?}", moved.warnings);
    }
}

#[tokio::test(flavor = "current_thread")]
async fn npc_initiative_bounds_initiative_tiebreak_an_npc_joins_mid_order_the_ones_after_move_down()
{
    let scratch = Scratch::new("mid_order").await;
    let engine = scratch
        .serve(add_participant::plugin_with(Scripted::new(&[13], &[42])))
        .await;
    scratch.plant(&engine).await;
    scratch.fight(&engine).await;
    let combat = scratch.row(&partition(COMBAT)).await.expect("the combat");

    // The wolf, DEX 14 (+2), rolls 13: 15, after Elowen, before Vesna.
    let joined = join(&engine, json!({ "token": T_LOUP }))
        .await
        .expect("joined");
    assert_eq!(joined.partition, partition(&entry_id(COMBAT, T_LOUP)));
    assert_eq!(joined.then.len(), 2, "{:?}", joined.then);
    clean(&joined);
    applied(&engine).await;

    let entries = scratch.entries().await;
    assert_eq!(order(&entries), [T_ELOWEN, T_LOUP, T_VESNA, T_BANDIT]);
    assert_eq!(ranks(&entries), [1, 2, 3, 4]);
    let loup = &entries[1];
    assert_eq!(loup["initiativeEntryId"], entry_id(COMBAT, T_LOUP));
    assert_eq!(loup["initiativeEntryCombatId"], COMBAT);
    assert_eq!(
        (&loup["roll"], &loup["initiative"], &loup["dexScore"]),
        (&json!(13), &json!(15), &json!(14))
    );
    assert_eq!(loup["tiebreak"], 42);
    // The others moved their rank alone.
    let vesna = &entries[2];
    assert_eq!(
        (&vesna["roll"], &vesna["initiative"], &vesna["tiebreak"]),
        (&json!(11), &json!(14), &json!(99))
    );
    // The turn and the round stay.
    assert_eq!(
        scratch.row(&partition(COMBAT)).await.expect("the combat"),
        combat
    );
    scratch.drop().await;
}

#[tokio::test(flavor = "current_thread")]
async fn npc_initiative_bounds_a_pc_joins_with_the_dm_s_figure_and_the_server_s_dice_roll_a_d20() {
    let scratch = Scratch::new("pc_and_dice").await;
    let engine = scratch.serve(add_participant::plugin()).await;
    scratch.plant(&engine).await;
    scratch.fight(&engine).await;

    // Garrick, DEX 10, entered at 14: after Vesna's and the Bandit's DEX.
    let joined = join(&engine, json!({ "token": T_GARRICK, "initiative": 14 }))
        .await
        .expect("joined");
    assert!(joined.then.is_empty(), "{:?}", joined.then);
    clean(&joined);
    applied(&engine).await;
    let garrick = scratch
        .row(&partition(&entry_id(COMBAT, T_GARRICK)))
        .await
        .expect("Garrick's entry");
    assert_eq!(
        (
            &garrick["roll"],
            &garrick["initiative"],
            &garrick["dexScore"]
        ),
        (&Value::Null, &json!(14), &json!(10))
    );
    assert_eq!(garrick["rank"], 4);

    // The wolf's d20 is the server's.
    join(&engine, json!({ "token": T_LOUP }))
        .await
        .expect("joined");
    applied(&engine).await;
    let entries = scratch.entries().await;
    assert_eq!(ranks(&entries), [1, 2, 3, 4, 5]);
    let loup = entries
        .iter()
        .find(|entry| entry["initiativeEntryTokenId"] == T_LOUP)
        .expect("the wolf");
    let roll = loup["roll"].as_i64().expect("a roll");
    assert!((1..=20).contains(&roll), "{roll}");
    assert_eq!(loup["initiative"], roll + 2);
    scratch.drop().await;
}

#[tokio::test(flavor = "current_thread")]
async fn two_joinings_queued_one_after_the_other_both_take_their_place() {
    let scratch = Scratch::new("two").await;
    let engine = scratch
        .serve(add_participant::plugin_with(Scripted::new(
            &[13, 20],
            &[42, 7],
        )))
        .await;
    scratch.plant(&engine).await;
    scratch.fight(&engine).await;

    // The wolf at 15, then the shadow (DEX 8, -1) at 19, before anything
    // is applied: the shadow finds the wolf queued, and moves it too.
    let wolf = join(&engine, json!({ "token": T_LOUP }))
        .await
        .expect("the wolf");
    clean(&wolf);
    let shadow = join(&engine, json!({ "token": T_OMBRE }))
        .await
        .expect("the shadow");
    assert!(shadow.warnings.is_empty(), "{:?}", shadow.warnings);
    // Elowen, the wolf, Vesna and the Bandit move down. The wolf's entry is
    // still queued: the engine judges its step on the step alone and warns,
    // and applies it on the entry once the entry has landed.
    let moved: Vec<(&str, &[String])> = shadow
        .then
        .iter()
        .map(|follow_up| match follow_up {
            FollowUp::Submitted(moved) => (moved.partition.as_str(), moved.warnings.as_slice()),
            refused => panic!("a move refused: {refused:?}"),
        })
        .collect();
    let wolf_entry = partition(&entry_id(COMBAT, T_LOUP));
    let moved_entries: Vec<&str> = moved.iter().map(|(partition, _)| *partition).collect();
    assert_eq!(
        moved_entries,
        [
            partition(&entry_id(COMBAT, T_ELOWEN)).as_str(),
            wolf_entry.as_str(),
            partition(&entry_id(COMBAT, T_VESNA)).as_str(),
            partition(&entry_id(COMBAT, T_BANDIT)).as_str(),
        ]
    );
    for (partition, warnings) in &moved {
        assert_eq!(
            warnings.is_empty(),
            *partition != wolf_entry,
            "{warnings:?}"
        );
    }
    applied(&engine).await;

    let entries = scratch.entries().await;
    assert_eq!(
        order(&entries),
        [T_OMBRE, T_ELOWEN, T_LOUP, T_VESNA, T_BANDIT]
    );
    assert_eq!(ranks(&entries), [1, 2, 3, 4, 5]);
    assert_eq!(entries[0]["initiative"], 19);
    scratch.drop().await;
}

#[tokio::test(flavor = "current_thread")]
async fn a_token_already_in_the_combat_is_refused_applied_or_queued() {
    let scratch = Scratch::new("again").await;
    let engine = scratch
        .serve(add_participant::plugin_with(Scripted::new(&[], &[])))
        .await;
    scratch.plant(&engine).await;
    scratch.fight(&engine).await;

    let (kind, message) = refusal(join(&engine, json!({ "token": T_BANDIT })).await);
    assert_eq!(kind, RefusalKind::Unprocessable);
    assert!(message.contains("déjà dans le combat"), "{message}");

    join(&engine, json!({ "token": T_LOUP }))
        .await
        .expect("the wolf queued");
    let (kind, message) = refusal(join(&engine, json!({ "token": T_LOUP })).await);
    assert_eq!(kind, RefusalKind::Unprocessable);
    assert!(message.contains("déjà dans le combat"), "{message}");
    applied(&engine).await;
    let (kind, _) = refusal(join(&engine, json!({ "token": T_LOUP })).await);
    assert_eq!(kind, RefusalKind::Unprocessable);
    assert_eq!(scratch.entries().await.len(), 4);
    scratch.drop().await;
}

#[tokio::test(flavor = "current_thread")]
async fn a_joining_with_no_open_combat_is_refused_before_ended_or_ending() {
    let scratch = Scratch::new("no_combat").await;
    let engine = scratch.serve(add_participant::plugin()).await;
    scratch.plant(&engine).await;

    let no_combat = |result| {
        let (kind, message) = refusal(result);
        assert_eq!(kind, RefusalKind::Unprocessable);
        assert!(message.contains("aucun combat"), "{message}");
    };
    no_combat(join(&engine, json!({ "token": T_LOUP })).await);

    scratch.fight(&engine).await;
    // An end queued ahead closes it.
    submit(
        &engine,
        "test.endCombat",
        json!({ "endedAt": "2026-10-22T11:00:00Z", "currentEntryId": null }),
        Some(COMBAT),
    )
    .await
    .expect("the end");
    no_combat(join(&engine, json!({ "token": T_LOUP })).await);
    applied(&engine).await;
    no_combat(join(&engine, json!({ "token": T_LOUP })).await);
    assert_eq!(
        scratch.row(&partition(&entry_id(COMBAT, T_LOUP))).await,
        None
    );
    scratch.drop().await;
}

#[tokio::test(flavor = "current_thread")]
async fn a_token_not_on_the_instance_is_not_found_and_a_pc_or_npc_initiative_misgiven_refused() {
    let scratch = Scratch::new("refused").await;
    let engine = scratch.serve(add_participant::plugin()).await;
    scratch.plant(&engine).await;
    scratch.fight(&engine).await;

    let (kind, _) = refusal(
        join(
            &engine,
            json!({ "token": "70000000-0000-4000-8000-0000000000ff" }),
        )
        .await,
    );
    assert_eq!(kind, RefusalKind::NotFound);
    let (kind, _) = refusal(
        submit(
            &engine,
            "table.addParticipant@1",
            json!({ "token": T_LOUP }),
            Some("b2000000-0000-4000-8000-000000000002"),
        )
        .await,
    );
    assert_eq!(kind, RefusalKind::NotFound);
    // The instance under `instance` when the command names none.
    submit(
        &engine,
        "table.addParticipant@1",
        json!({ "token": T_LOUP, "instance": INSTANCE }),
        None,
    )
    .await
    .expect("the wolf by its instance");

    let (kind, message) = refusal(join(&engine, json!({ "token": T_GARRICK })).await);
    assert_eq!(kind, RefusalKind::Unprocessable);
    assert!(message.starts_with("npc-initiative-bounds"), "{message}");
    let (kind, message) =
        refusal(join(&engine, json!({ "token": T_OMBRE, "initiative": 12 })).await);
    assert_eq!(kind, RefusalKind::Unprocessable);
    assert!(message.starts_with("npc-initiative-bounds"), "{message}");
    let (kind, _) = refusal(join(&engine, json!({ "token": "Ombre" })).await);
    assert_eq!(kind, RefusalKind::Unprocessable);
    scratch.drop().await;
}

#[tokio::test(flavor = "current_thread")]
async fn initiative_tiebreak_a_move_sent_alone_is_refused() {
    let scratch = Scratch::new("forged").await;
    let engine = scratch
        .serve(add_participant::plugin_with(Scripted::new(&[13], &[42])))
        .await;
    scratch.plant(&engine).await;
    scratch.fight(&engine).await;
    join(&engine, json!({ "token": T_LOUP }))
        .await
        .expect("joined");
    applied(&engine).await;

    let newcomer = entry_id(COMBAT, T_LOUP);
    for payload in [
        json!({ "shift": entry_id(COMBAT, T_VESNA), "newcomer": newcomer }),
        json!({ "shift": entry_id(COMBAT, T_VESNA), "newcomer": "Loup" }),
        json!({ "shift": entry_id(COMBAT, T_VESNA) }),
        json!({ "shift": "Vesna", "newcomer": newcomer }),
        json!({ "shift": COMBAT, "newcomer": newcomer }),
    ] {
        let (kind, message) =
            refusal(submit(&engine, "table.addParticipant@1", payload.clone(), None).await);
        assert_eq!(kind, RefusalKind::Unprocessable, "{payload}");
        assert!(message.starts_with("initiative-tiebreak"), "{message}");
    }
    assert_eq!(ranks(&scratch.entries().await), [1, 2, 3, 4]);
    scratch.drop().await;
}

#[tokio::test(flavor = "current_thread")]
async fn initiative_tiebreak_a_move_that_would_make_room_is_refused_without_its_joining_s_seal() {
    let scratch = Scratch::new("unsealed").await;
    let engine = scratch
        .serve(add_participant::plugin_with(Scripted::new(&[13], &[42])))
        .await;
    scratch.plant(&engine).await;
    scratch.fight(&engine).await;
    // Garrick enters at the Bandit's rank by hand: the Bandit holds its rank
    // in the queue, so a move of it passes every check but the seal.
    submit(
        &engine,
        "test.enter",
        entry(T_GARRICK, Value::Null, 10, 10, 3, 3),
        None,
    )
    .await
    .expect("Garrick");
    applied(&engine).await;
    let ranks_before = ranks(&scratch.entries().await);

    // Only a joining may move them: a move sent by hand, with no seal, a
    // made-up one or the entry's id for one, is refused.
    let newcomer = entry_id(COMBAT, T_ELOWEN);
    for payload in [
        json!({ "shift": entry_id(COMBAT, T_BANDIT), "newcomer": newcomer }),
        json!({ "shift": entry_id(COMBAT, T_BANDIT), "newcomer": newcomer, "seal": "forged" }),
        json!({ "shift": entry_id(COMBAT, T_BANDIT), "newcomer": newcomer, "seal": null }),
        json!({ "shift": entry_id(COMBAT, T_BANDIT), "newcomer": newcomer, "seal": entry_id(COMBAT, T_BANDIT) }),
    ] {
        let (kind, message) =
            refusal(submit(&engine, "table.addParticipant@1", payload.clone(), None).await);
        assert_eq!(kind, RefusalKind::Unprocessable, "{payload}");
        assert!(message.starts_with("initiative-tiebreak"), "{message}");
    }
    applied(&engine).await;
    assert_eq!(ranks(&scratch.entries().await), ranks_before);
    scratch.drop().await;
}

#[tokio::test(flavor = "current_thread")]
async fn two_joinings_of_one_token_prepared_together_enter_once() {
    let scratch = Scratch::new("together").await;
    let engine = scratch
        .serve(add_participant::plugin_with(Scripted::new(
            &[13, 20],
            &[42, 7],
        )))
        .await;
    scratch.plant(&engine).await;
    scratch.fight(&engine).await;

    // Both are prepared before either is queued: the queue shows neither the
    // wolf's entry, and only the claim tells the second.
    let (first, second) = tokio::join!(
        join(&engine, json!({ "token": T_LOUP })),
        join(&engine, json!({ "token": T_LOUP })),
    );
    let (joined, refused) = match (first, second) {
        (Ok(joined), refused) | (refused, Ok(joined)) => (joined, refusal(refused)),
        both => panic!("one of the two joins: {both:?}"),
    };
    clean(&joined);
    assert_eq!(refused.0, RefusalKind::Unprocessable);
    applied(&engine).await;

    let entries = scratch.entries().await;
    assert_eq!(order(&entries), [T_ELOWEN, T_LOUP, T_VESNA, T_BANDIT]);
    assert_eq!(ranks(&entries), [1, 2, 3, 4]);
    let wolf = &entries[1];
    // A d20 of 13 or 20 plus the wolf's +2: never both added.
    assert!([15, 22].contains(&wolf["initiative"].as_i64().expect("an initiative")));
    assert!([13, 20].contains(&wolf["roll"].as_i64().expect("a roll")));
    scratch.drop().await;
}

#[tokio::test(flavor = "current_thread")]
async fn initiative_tiebreak_the_entry_of_a_token_off_the_map_neither_ranks_nor_makes_room() {
    let scratch = Scratch::new("off_the_map").await;
    let engine = scratch
        .serve(add_participant::plugin_with(Scripted::new(&[13], &[42])))
        .await;
    scratch.plant(&engine).await;
    scratch.fight(&engine).await;
    // Vesna's token leaves the map: her entry stays among the engine's rows
    // at rank 2, which the carousel no longer shows.
    submit(&engine, "test.removeToken", json!({}), Some(T_VESNA))
        .await
        .expect("Vesna removed");
    applied(&engine).await;
    let vesna = scratch
        .row(&partition(&entry_id(COMBAT, T_VESNA)))
        .await
        .expect("Vesna's entry");

    // The wolf at 15 takes rank 2, free among the participants on the map:
    // the Bandit stays at 3.
    let joined = join(&engine, json!({ "token": T_LOUP }))
        .await
        .expect("joined");
    assert!(joined.then.is_empty(), "{:?}", joined.then);
    clean(&joined);
    applied(&engine).await;
    let wolf = entry_id(COMBAT, T_LOUP);
    assert_eq!(
        scratch.row(&partition(&wolf)).await.expect("the wolf")["rank"],
        2
    );
    assert_eq!(
        scratch
            .row(&partition(&entry_id(COMBAT, T_BANDIT)))
            .await
            .expect("the Bandit")["rank"],
        3
    );
    assert_eq!(
        scratch.row(&partition(&entry_id(COMBAT, T_VESNA))).await,
        Some(vesna)
    );

    // Vesna's entry at the wolf's rank makes no room for anyone.
    let (kind, message) = refusal(
        submit(
            &engine,
            "table.addParticipant@1",
            json!({ "shift": wolf, "newcomer": entry_id(COMBAT, T_ELOWEN) }),
            None,
        )
        .await,
    );
    assert_eq!(kind, RefusalKind::Unprocessable);
    assert!(message.starts_with("initiative-tiebreak"), "{message}");
    scratch.drop().await;
}

#[tokio::test(flavor = "current_thread")]
async fn initiative_tiebreak_a_participant_removed_stays_out_and_the_joinings_after_it_are_taken() {
    let scratch = Scratch::new("removed").await;
    let engine = scratch
        .serve(add_participant::plugin_with(Scripted::new(
            &[20, 17],
            &[42, 7],
        )))
        .await;
    scratch.plant(&engine).await;
    scratch.fight(&engine).await;
    // Vesna leaves the combat as the DM takes her out: her entry's partition
    // is tombstoned, rank 2 left empty.
    let vesna = partition(&entry_id(COMBAT, T_VESNA));
    submit(
        &engine,
        "table.removeParticipant@1",
        json!({ "participant": entry_id(COMBAT, T_VESNA) }),
        Some(INSTANCE),
    )
    .await
    .expect("Vesna removed");
    applied(&engine).await;
    assert_eq!(scratch.row(&vesna).await, None);

    // The wolf, 20 + 2, lands ahead of Vesna's old rank, ahead of Elowen:
    // Elowen alone moves down, into the room Vesna left.
    let wolf = join(&engine, json!({ "token": T_LOUP }))
        .await
        .expect("the wolf");
    clean(&wolf);
    assert_eq!(wolf.then.len(), 1, "{:?}", wolf.then);
    applied(&engine).await;
    // The shadow, 17 - 1, right after: after Elowen, ahead of the Bandit.
    let shadow = join(&engine, json!({ "token": T_OMBRE }))
        .await
        .expect("the shadow");
    clean(&shadow);
    assert_eq!(shadow.then.len(), 1, "{:?}", shadow.then);
    applied(&engine).await;

    let entries = scratch.entries().await;
    assert_eq!(order(&entries), [T_LOUP, T_ELOWEN, T_OMBRE, T_BANDIT]);
    assert_eq!(ranks(&entries), [1, 2, 3, 4]);
    // No move brought Vesna back.
    assert_eq!(scratch.row(&vesna).await, None);
    let tombstoned: bool = scratch
        .client
        .query_one(
            "SELECT tombstoned FROM dataguard_rows WHERE partition = $1",
            &[&vesna],
        )
        .await
        .expect("Vesna's partition")
        .get(0);
    assert!(tombstoned);
    scratch.drop().await;
}
