//! table.resetBattlemap over PostgreSQL (`DATABASE_URL`), as the server
//! plants it beside its target aggregate: the ford's instance of the tavern
//! scene, three Bandits on it, an ended combat and the running one; beside
//! it, the camp scene's instance of the same battlemap, with a Bandit and a
//! combat of its own. The reset tombstones every entry, combat and token of
//! the tavern's instance — the first by the command, the others by its
//! follow-ups, each carrying when the reset was asked into the journal —
//! and nothing else: the instance stays, the camp's rows stay, the
//! battlemap and its grid stay (`instance-isolated`). Rows the tables hold
//! without the engine (fixtures) are tombstoned as well; a map with nothing
//! on it, an unknown instance, a row of another instance, are refused. Each
//! test works in a scratch database of its own, migrated as the server
//! migrates it, dropped when it ends.

use std::path::Path;
use std::sync::Arc;

use harness_dataguard::aggregate::Aggregate;
use harness_dataguard::api::{FollowUp, Submission, Submitted};
use harness_dataguard::command::State;
use harness_dataguard::engine::Engine;
use harness_dataguard::manifest::DataCapability;
use harness_dataguard::memory::MemoryNotifier;
use harness_dataguard::plugins::{Plugins, RefusalKind};
use harness_dataguard::postgres::{PgStore, ReadOnly, SystemClock};
use harness_dataguard::tokio_postgres::{self, Client, NoTls};
use harness_dataguard::{Error, Result};
use reset_battlemap::partition;
use serde_json::{Value, json};

const VALOMBRE: &str = "c1000000-0000-4000-8000-000000000001";
/// Valombre's battlemap with its grid.
const GUE: &str = "d1000000-0000-4000-8000-00000000000a";
const SEANCE: &str = "5e000000-0000-4000-8000-000000000001";
const TAVERNE: &str = "5c000000-0000-4000-8000-000000000001";
const CAMP: &str = "5c000000-0000-4000-8000-000000000002";
const PONT: &str = "5c000000-0000-4000-8000-000000000003";
/// The tavern scene's instance of the ford, the one reset.
const INSTANCE: &str = "b1000000-0000-4000-8000-000000000001";
/// The camp scene's instance of the same ford.
const CAMP_INSTANCE: &str = "b1000000-0000-4000-8000-000000000002";
/// The bridge scene's instance, which only the tables hold.
const PONT_INSTANCE: &str = "b1000000-0000-4000-8000-000000000003";
const NOWHERE: &str = "b1000000-0000-4000-8000-0000000000ff";
const BANDIT: &str = "e1000000-0000-4000-8000-0000000000b1";
/// The tavern's combat that ended, and the one running.
const ENDED: &str = "c0000000-0000-4000-8000-000000000001";
const RUNNING: &str = "c0000000-0000-4000-8000-000000000002";
const CAMP_COMBAT: &str = "c0000000-0000-4000-8000-000000000009";
const CAMP_BANDIT: &str = "70000000-0000-4000-8000-000000000009";
const CAMP_ENTRY: &str = "ee000000-0000-4000-8000-000000000009";

/// The test-only DataCapabilities planting the rows a reset lands on, and a
/// token's move.
const OPEN_INSTANCE: &str = r#"{"dataCapability":"test.openInstance","owner":"dataguard","version":1,"effect":"insert","target":{"aggregate":"BattlemapInstance","id":"$battlemapInstanceId"},"mode":"overwrite","callableBy":["*"]}"#;
const DROP_TOKEN: &str = r#"{"dataCapability":"test.dropToken","owner":"dataguard","version":1,"effect":"insert","target":{"aggregate":"BattlemapInstance","id":"$tokenId"},"mode":"overwrite","callableBy":["*"]}"#;
const START_COMBAT: &str = r#"{"dataCapability":"test.startCombat","owner":"dataguard","version":1,"effect":"insert","target":{"aggregate":"BattlemapInstance","id":"$combatId"},"mode":"overwrite","callableBy":["*"]}"#;
const ENTER: &str = r#"{"dataCapability":"test.enter","owner":"dataguard","version":1,"effect":"insert","target":{"aggregate":"BattlemapInstance","id":"$initiativeEntryId"},"mode":"overwrite","callableBy":["*"]}"#;
const MOVE: &str = r#"{"dataCapability":"test.move","owner":"dataguard","version":1,"effect":"update","target":{"aggregate":"BattlemapInstance","id":"$tokenId"},"touches":["BattlemapInstance.q","BattlemapInstance.r"],"mode":"confirm_on_stale","callableBy":["*"]}"#;

/// The id of the tavern's Bandit `number`.
fn bandit_id(number: u8) -> String {
    format!("70000000-0000-4000-8000-00000000000{number}")
}

/// The id of Bandit `number`'s entry in the running combat.
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
    [
        aggregate_battlemap_instance::plugin(),
        reset_battlemap::plugin(),
    ]
    .into_iter()
    .collect()
}

/// The instance `id` of the ford for the scene `scene`.
fn instance(id: &str, scene: &str) -> Value {
    json!({
        "battlemapInstanceId": id,
        "battlemapMediaId": GUE,
        "sessionId": SEANCE,
        "sceneId": scene,
        "createdAt": "2026-10-21T10:00:00Z",
    })
}

/// The Bandit `id` on the instance `on`, numbered `number`.
fn bandit(id: &str, on: &str, number: u8) -> Value {
    json!({
        "tokenId": id,
        "tokenInstanceId": on,
        "characterId": BANDIT,
        "number": number,
        "name": format!("Bandit {number}"),
        "q": 5,
        "r": number,
        "visible": true,
        "hpCurrent": 11,
        "hpMax": 11,
        "conditions": [],
    })
}

/// The combat `id` of the instance `on`, ended or running on `current`.
fn combat(id: &str, on: &str, current: Option<&str>) -> Value {
    json!({
        "combatId": id,
        "combatInstanceId": on,
        "round": 1,
        "currentEntryId": current,
        "startedAt": "2026-10-21T10:05:00Z",
        "endedAt": current.map_or(json!("2026-10-21T10:20:00Z"), |_| Value::Null),
    })
}

/// The entry `id` of the token `token` in the combat `of`, ranked `rank`.
fn entry(id: &str, of: &str, token: &str, rank: u8) -> Value {
    json!({
        "initiativeEntryId": id,
        "initiativeEntryCombatId": of,
        "initiativeEntryTokenId": token,
        "roll": 10,
        "initiative": 10,
        "dexScore": 10,
        "tiebreak": f64::from(rank) / 10.0,
        "rank": rank,
    })
}

/// A database of its own, migrated, holding Valombre, its battlemap with
/// its grid, the session and its three scenes, and the Bandit its tokens
/// stand for.
struct Scratch {
    name: String,
    url: String,
    client: Client,
}

impl Scratch {
    async fn new(name: &str) -> Scratch {
        let name = format!("reset_battlemap_test_{name}_{}", std::process::id());
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
                     ('{TAVERNE}', '{SEANCE}', 1, 'La taverne'),
                     ('{CAMP}', '{SEANCE}', 2, 'Le camp'),
                     ('{PONT}', '{SEANCE}', 3, 'Le pont');
                 INSERT INTO entity (id, campaign_id, type, name) VALUES ('{BANDIT}', '{VALOMBRE}', 'pnj', 'Bandit');
                 INSERT INTO character (entity_id, token_color) VALUES ('{BANDIT}', '#888');
                 INSERT INTO entity_version (entity_id, version_number, source, snapshot, diff)
                     VALUES ('{BANDIT}', 1, 'dm', '{{}}', '[]');"
            ))
            .await
            .expect("Valombre");
        Scratch { name, url, client }
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
        .with_data_capability(manifest(include_str!("../data-capability.json")))
        .with_data_capability(manifest(OPEN_INSTANCE))
        .with_data_capability(manifest(DROP_TOKEN))
        .with_data_capability(manifest(START_COMBAT))
        .with_data_capability(manifest(ENTER))
        .with_data_capability(manifest(MOVE))
        .with_plugins(plugins());
        engine.load().await.expect("the engine loads");
        engine
    }

    /// The tavern's instance, Bandit 1 to 3 on it, its ended combat (Bandit
    /// 1 in it) and its running one (all three, Bandit 1's turn); the
    /// camp's instance, its Bandit and its running combat — as the engine
    /// holds them.
    async fn plant(&self, engine: &Engine) {
        let mut rows = vec![
            ("test.openInstance", instance(INSTANCE, TAVERNE)),
            ("test.openInstance", instance(CAMP_INSTANCE, CAMP)),
        ];
        for number in 1..=3 {
            rows.push((
                "test.dropToken",
                bandit(&bandit_id(number), INSTANCE, number),
            ));
        }
        rows.push(("test.dropToken", bandit(CAMP_BANDIT, CAMP_INSTANCE, 1)));
        rows.push(("test.startCombat", combat(ENDED, INSTANCE, None)));
        rows.push((
            "test.startCombat",
            combat(RUNNING, INSTANCE, Some(&entry_id(1))),
        ));
        rows.push((
            "test.startCombat",
            combat(CAMP_COMBAT, CAMP_INSTANCE, Some(CAMP_ENTRY)),
        ));
        rows.push((
            "test.enter",
            entry(
                "ee000000-0000-4000-8000-000000000000",
                ENDED,
                &bandit_id(1),
                1,
            ),
        ));
        for number in 1..=3 {
            rows.push((
                "test.enter",
                entry(&entry_id(number), RUNNING, &bandit_id(number), number),
            ));
        }
        rows.push(("test.enter", entry(CAMP_ENTRY, CAMP_COMBAT, CAMP_BANDIT, 1)));
        for (data_capability, row) in rows {
            submit(engine, data_capability, row, None)
                .await
                .expect("planted");
        }
        let planted = engine.run_once().await.expect("planted");
        assert!(
            planted.iter().all(|result| result.status == State::Applied),
            "{planted:?}"
        );
    }

    /// The bridge's instance, the Bandit on it, its combat and its entry, as
    /// the tables alone hold them: fixtures the engine never wrote.
    async fn fixtures(&self) {
        self.client
            .batch_execute(&format!(
                "INSERT INTO battlemap_instance (id, battlemap_media_id, session_id, scene_id) VALUES
                     ('{PONT_INSTANCE}', '{GUE}', '{SEANCE}', '{PONT}');
                 INSERT INTO token (id, instance_id, character_id, number, name, q, r, visible, hp_current, hp_max, conditions)
                     VALUES ('{token}', '{PONT_INSTANCE}', '{BANDIT}', 1, 'Bandit 1', 0, 0, true, 11, 11, '{{}}');
                 INSERT INTO combat (id, instance_id, round, started_at) VALUES
                     ('{combat}', '{PONT_INSTANCE}', 1, '2026-10-21T10:05:00Z');
                 INSERT INTO initiative_entry (id, combat_id, token_id, roll, initiative, dex_score, tiebreak, rank)
                     VALUES ('{entry}', '{combat}', '{token}', 10, 10, 10, 0.5, 1);",
                token = PONT_TOKEN,
                combat = PONT_COMBAT,
                entry = PONT_ENTRY,
            ))
            .await
            .expect("the bridge's fixtures");
    }

    /// The row the engine holds on `partition`, and whether it is archived;
    /// `None` when it was never written.
    async fn engine_row(&self, partition: &str) -> Option<(Value, bool)> {
        self.client
            .query_opt(
                "SELECT fields::text, tombstoned FROM dataguard_rows WHERE partition = $1",
                &[&partition],
            )
            .await
            .expect("the engine's rows")
            .map(|row| {
                (
                    serde_json::from_str(row.get::<_, &str>(0)).expect("JSON"),
                    row.get(1),
                )
            })
    }

    /// The engine's live row on `partition` holding `fields`, written
    /// straight into `dataguard_rows`: the tables are left as they are.
    async fn hold(&self, partition: &str, fields: &Value) {
        self.client
            .execute(
                "INSERT INTO dataguard_rows (partition, fields) VALUES ($1, $2::text::jsonb)",
                &[&partition, &fields.to_string()],
            )
            .await
            .expect("the engine's row");
    }

    /// The payloads of the resets' commands the journal holds, applied, by
    /// partition.
    async fn journal(&self) -> Vec<(String, Value)> {
        self.client
            .query(
                "SELECT partition, (entry -> 'payload')::text FROM dataguard_queue
                 WHERE data_capability LIKE 'table.resetBattlemap%' AND state = 'applied'
                 ORDER BY partition",
                &[],
            )
            .await
            .expect("the journal")
            .iter()
            .map(|row| {
                (
                    row.get(0),
                    serde_json::from_str(row.get::<_, &str>(1)).expect("JSON"),
                )
            })
            .collect()
    }

    /// The product's tables a reset could reach, as JSON text.
    async fn tables(&self) -> Vec<String> {
        self.client
            .query(
                "SELECT to_jsonb(m)::text FROM media m
                 UNION ALL SELECT to_jsonb(g)::text FROM battlemap_grid g
                 UNION ALL SELECT to_jsonb(i)::text FROM battlemap_instance i
                 UNION ALL SELECT to_jsonb(t)::text FROM token t
                 UNION ALL SELECT to_jsonb(c)::text FROM combat c
                 UNION ALL SELECT to_jsonb(e)::text FROM initiative_entry e
                 ORDER BY 1",
                &[],
            )
            .await
            .expect("the tables")
            .iter()
            .map(|row| row.get(0))
            .collect()
    }

    /// Drops the database, again while an autovacuum worker the local role
    /// cannot end still holds it.
    async fn drop(self) {
        drop(self.client);
        let admin = connect(&url()).await;
        let sql = format!("DROP DATABASE {} WITH (FORCE)", self.name);
        for _ in 0..20 {
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

const PONT_TOKEN: &str = "70000000-0000-4000-8000-000000000031";
const PONT_COMBAT: &str = "c0000000-0000-4000-8000-000000000031";
const PONT_ENTRY: &str = "ee000000-0000-4000-8000-000000000031";

/// A command by the DM, on `target`.
async fn submit(
    engine: &Engine,
    data_capability: &str,
    payload: Value,
    target: Option<&str>,
) -> Result<Submitted> {
    submit_by("dm", engine, data_capability, payload, target).await
}

/// A command by `by`, on `target`.
async fn submit_by(
    by: &str,
    engine: &Engine,
    data_capability: &str,
    payload: Value,
    target: Option<&str>,
) -> Result<Submitted> {
    let mut body = json!({
        "dataCapability": data_capability,
        "target": { "aggregate": "BattlemapInstance" },
        "payload": payload,
        "caller": { "by": by, "from": "Battlemap", "roles": ["role:approver"] },
    });
    if let Some(target) = target {
        body["target"]["id"] = json!(target);
    }
    engine
        .submit(serde_json::from_value::<Submission>(body).expect("a submission"))
        .await
}

/// « Réinitialiser la carte » on the instance `instance`.
async fn reset(engine: &Engine, instance: &str) -> Result<Submitted> {
    submit(engine, "table.resetBattlemap@1", json!({}), Some(instance)).await
}

/// The partitions `submitted` and its follow-ups landed on, in order; every
/// follow-up was queued.
fn landed(submitted: &Submitted) -> Vec<String> {
    let mut partitions = vec![submitted.partition.clone()];
    for follow in &submitted.then {
        match follow {
            FollowUp::Submitted(one) => {
                assert!(one.warnings.is_empty(), "{:?}", one.warnings);
                partitions.push(one.partition.clone());
            }
            FollowUp::Refused { error, .. } => panic!("a deletion refused: {error}"),
        }
    }
    partitions
}

fn refused(answer: Result<Submitted>, kind: RefusalKind) {
    match answer {
        Err(Error::Refused(refusal)) => assert_eq!(refusal.kind, kind, "{refusal:?}"),
        other => panic!("refused as {kind:?}: {other:?}"),
    }
}

#[tokio::test]
async fn state_persisted_reset_tombstones_every_entry_combat_and_token_of_the_instance() {
    let scratch = Scratch::new("reset").await;
    let engine = scratch.serve().await;
    scratch.plant(&engine).await;
    let instance_row = scratch.engine_row(&partition(INSTANCE)).await;
    let before = scratch.tables().await;

    let submitted = reset(&engine, INSTANCE).await.expect("queued");
    assert!(submitted.warnings.is_empty(), "{:?}", submitted.warnings);
    // Entries first, then combats, then tokens: a child before what it is of.
    let tavern: Vec<String> = [
        "ee000000-0000-4000-8000-000000000000".to_owned(),
        entry_id(1),
        entry_id(2),
        entry_id(3),
        ENDED.to_owned(),
        RUNNING.to_owned(),
        bandit_id(1),
        bandit_id(2),
        bandit_id(3),
    ]
    .iter()
    .map(|id| partition(id))
    .collect();
    assert_eq!(landed(&submitted), tavern);
    let settled = engine.run_once().await.expect("the applier ran");
    assert_eq!(settled.len(), tavern.len(), "{settled:?}");
    assert!(
        settled.iter().all(|result| result.status == State::Applied),
        "{settled:?}"
    );
    drop(engine);

    // A soft delete: every partition kept, archived.
    for partition in &tavern {
        let (_, tombstoned) = scratch
            .engine_row(partition)
            .await
            .unwrap_or_else(|| panic!("{partition} kept"));
        assert!(tombstoned, "{partition}");
    }
    // The instance stays as it was, and every deletion carries when the
    // reset was asked, the same for all, into the journal.
    assert_eq!(scratch.engine_row(&partition(INSTANCE)).await, instance_row);
    let journal = scratch.journal().await;
    let mut sorted = tavern.clone();
    sorted.sort();
    assert_eq!(
        journal.iter().map(|(p, _)| p.clone()).collect::<Vec<_>>(),
        sorted
    );
    let stamp = &journal[0].1;
    assert_eq!(stamp["of"], json!(INSTANCE));
    let at = stamp["resetAt"].as_str().expect("a reset_at");
    assert!(
        at.len() == 24 && at.ends_with('Z') && at.as_bytes()[10] == b'T',
        "{at}"
    );
    assert!(
        journal.iter().all(|(_, payload)| payload == stamp),
        "{journal:?}"
    );
    // The tavern's entries, combats and tokens leave the tables, which the
    // aggregate's projection writes; the battlemap, its grid, the instance
    // and the camp's rows are left alone.
    let kept: Vec<String> = before
        .iter()
        .filter(|row| {
            !tavern.iter().any(|partition| {
                row.contains(&format!(
                    "\"id\": \"{}\"",
                    partition.trim_start_matches("BattlemapInstance/")
                ))
            })
        })
        .cloned()
        .collect();
    assert_eq!(before.len() - kept.len(), tavern.len());
    assert_eq!(scratch.tables().await, kept);

    // Reopened: the map has nothing left to reset.
    let engine = scratch.serve().await;
    refused(reset(&engine, INSTANCE).await, RefusalKind::Unprocessable);
    scratch.drop().await;
}

#[tokio::test]
async fn instance_isolated_the_other_instance_of_the_battlemap_is_unchanged() {
    let scratch = Scratch::new("isolated").await;
    let engine = scratch.serve().await;
    scratch.plant(&engine).await;
    let camp: Vec<_> = [CAMP_INSTANCE, CAMP_BANDIT, CAMP_COMBAT, CAMP_ENTRY]
        .iter()
        .map(|id| partition(id))
        .collect();
    let mut kept = Vec::new();
    for partition in &camp {
        kept.push(scratch.engine_row(partition).await);
    }

    let submitted = reset(&engine, INSTANCE).await.expect("queued");
    assert!(landed(&submitted).iter().all(|p| !camp.contains(p)));
    engine.run_once().await.expect("the applier ran");
    for (partition, row) in camp.iter().zip(kept) {
        assert_eq!(&scratch.engine_row(partition).await, &row, "{partition}");
        assert!(!row.expect("planted").1, "{partition}");
    }

    // A deletion of the reset of the tavern, sent on the camp's Bandit, is
    // of no row of the tavern: nothing queued.
    refused(
        submit(
            &engine,
            "table.resetBattlemap@1",
            json!({ "of": INSTANCE, "resetAt": "2026-10-21T11:00:00.000Z" }),
            Some(CAMP_BANDIT),
        )
        .await,
        RefusalKind::NotFound,
    );
    assert!(engine.run_once().await.expect("ran").is_empty());
    assert_eq!(
        scratch
            .engine_row(&partition(CAMP_BANDIT))
            .await
            .map(|r| r.1),
        Some(false)
    );
    scratch.drop().await;
}

#[tokio::test]
async fn rows_only_the_tables_hold_are_tombstoned_and_reset_once() {
    let scratch = Scratch::new("fixtures").await;
    let engine = scratch.serve().await;
    scratch.fixtures().await;

    let submitted = reset(&engine, PONT_INSTANCE).await.expect("queued");
    assert_eq!(
        landed(&submitted),
        [PONT_ENTRY, PONT_COMBAT, PONT_TOKEN].map(partition)
    );
    let settled = engine.run_once().await.expect("the applier ran");
    assert!(
        settled.iter().all(|result| result.status == State::Applied),
        "{settled:?}"
    );
    for id in [PONT_ENTRY, PONT_COMBAT, PONT_TOKEN] {
        assert_eq!(
            scratch.engine_row(&partition(id)).await.map(|r| r.1),
            Some(true),
            "{id}"
        );
    }
    // What the engine tombstoned is gone: nothing left to reset.
    refused(
        reset(&engine, PONT_INSTANCE).await,
        RefusalKind::Unprocessable,
    );
    scratch.drop().await;
}

#[tokio::test]
async fn an_unknown_instance_an_empty_map_and_a_deletion_sent_alone_are_refused() {
    let scratch = Scratch::new("refused").await;
    let engine = scratch.serve().await;
    scratch.plant(&engine).await;
    submit(&engine, "test.openInstance", instance(NOWHERE, PONT), None)
        .await
        .expect("an empty instance");
    engine.run_once().await.expect("ran");

    refused(
        reset(&engine, "b1000000-0000-4000-8000-0000000000ee").await,
        RefusalKind::NotFound,
    );
    refused(reset(&engine, NOWHERE).await, RefusalKind::Unprocessable);
    refused(
        reset(&engine, "pas-un-id").await,
        RefusalKind::Unprocessable,
    );
    // A token is no instance to reset.
    refused(reset(&engine, &bandit_id(1)).await, RefusalKind::NotFound);

    // A deletion sent alone — the running combat, under a stamp no reset
    // carries, or under none — is no reset's: nothing queued.
    for payload in [
        json!({ "of": INSTANCE, "resetAt": "2026-10-21T11:00:00.000Z" }),
        json!({ "of": INSTANCE }),
    ] {
        refused(
            submit(&engine, "table.resetBattlemap@1", payload, Some(RUNNING)).await,
            RefusalKind::Unprocessable,
        );
    }
    assert!(engine.run_once().await.expect("ran").is_empty());
    assert_eq!(
        scratch.engine_row(&partition(RUNNING)).await.map(|r| r.1),
        Some(false)
    );

    // Once the reset applied, one of its deletions sent again under its
    // stamp finds its row gone.
    reset(&engine, INSTANCE).await.expect("queued");
    engine.run_once().await.expect("ran");
    let stamp = scratch.journal().await.remove(0).1;
    refused(
        submit(&engine, "table.resetBattlemap@1", stamp, Some(RUNNING)).await,
        RefusalKind::NotFound,
    );
    scratch.drop().await;
}

#[tokio::test]
async fn a_stamp_authorises_only_its_submitter_in_the_minute_its_reset_stands() {
    let scratch = Scratch::new("stamp").await;
    let engine = scratch.serve().await;
    scratch.plant(&engine).await;
    let lone = "70000000-0000-4000-8000-000000000005";
    submit(&engine, "test.openInstance", instance(NOWHERE, PONT), None)
        .await
        .expect("an empty instance");
    submit(&engine, "test.dropToken", bandit(lone, NOWHERE, 1), None)
        .await
        .expect("a drop queued");
    engine.run_once().await.expect("ran");

    // The one-row reset of the instance stays queued.
    let queued = reset(&engine, NOWHERE).await.expect("queued");
    let payload: String = scratch
        .client
        .query_one(
            "SELECT (entry -> 'payload')::text FROM dataguard_queue WHERE command = $1",
            &[&queued.command_id],
        )
        .await
        .expect("the reset")
        .get(0);
    let stamp: Value = serde_json::from_str(&payload).expect("JSON");
    let at = stamp["resetAt"].as_str().expect("a reset_at").to_owned();
    let now = i64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("after the epoch")
            .as_millis(),
    )
    .expect("milliseconds");
    let under_way = |by: &'static str, at_ms: i64| {
        let client = &scratch.client;
        let at = at.clone();
        async move {
            client
                .query_one(reset_battlemap::LISTING, &[&NOWHERE, &at_ms, &at, &by])
                .await
                .expect("the listing")
                .get::<_, bool>("under_way")
        }
    };
    assert!(under_way("dm", now).await);
    // Someone else, or a minute later, finds no reset of the stamp.
    assert!(!under_way("intruder", now).await);
    assert!(!under_way("dm", now + 120_000).await);
    refused(
        submit_by(
            "intruder",
            &engine,
            "table.resetBattlemap@1",
            stamp.clone(),
            Some(lone),
        )
        .await,
        RefusalKind::Unprocessable,
    );

    // A withdrawn reset authorises nothing.
    engine
        .cancel(&queued.command_id, "dm")
        .await
        .expect("withdrawn");
    assert!(!under_way("dm", now).await);
    refused(
        submit(&engine, "table.resetBattlemap@1", stamp, Some(lone)).await,
        RefusalKind::Unprocessable,
    );
    scratch.drop().await;
}

#[tokio::test]
async fn an_instance_only_the_engine_holds_is_reset_from_the_engines_row() {
    let scratch = Scratch::new("engine_only").await;
    let engine = scratch.serve().await;
    // The engine holds the instance and a Bandit of it; the tables hold neither.
    scratch
        .hold(&partition(INSTANCE), &instance(INSTANCE, TAVERNE))
        .await;
    scratch
        .hold(
            &partition(&bandit_id(1)),
            &bandit(&bandit_id(1), INSTANCE, 1),
        )
        .await;
    assert!(scratch.tables().await.iter().all(|t| !t.contains(INSTANCE)));

    let submitted = reset(&engine, INSTANCE).await.expect("queued");
    assert_eq!(landed(&submitted), [partition(&bandit_id(1))]);

    // A row of the engine naming another instance is none of this one.
    let other = "b1000000-0000-4000-8000-0000000000aa";
    scratch
        .hold(&partition(other), &instance(NOWHERE, TAVERNE))
        .await;
    scratch
        .hold(&partition(&bandit_id(2)), &bandit(&bandit_id(2), other, 2))
        .await;
    refused(reset(&engine, other).await, RefusalKind::NotFound);
    scratch.drop().await;
}

#[tokio::test]
async fn an_instance_the_tables_hold_is_reset_whatever_the_engines_row_says() {
    let scratch = Scratch::new("tables_only").await;
    let engine = scratch.serve().await;
    scratch.fixtures().await;
    // The engine's row of the partition names no instance of that id.
    scratch
        .hold(&partition(PONT_INSTANCE), &instance(NOWHERE, PONT))
        .await;

    let submitted = reset(&engine, PONT_INSTANCE).await.expect("queued");
    assert_eq!(
        landed(&submitted),
        [PONT_ENTRY, PONT_COMBAT, PONT_TOKEN].map(partition)
    );
    scratch.drop().await;
}

#[tokio::test]
async fn a_token_still_queued_is_reset_behind_its_insert() {
    let scratch = Scratch::new("queued").await;
    let engine = scratch.serve().await;
    scratch.plant(&engine).await;
    submit(&engine, "test.openInstance", instance(NOWHERE, PONT), None)
        .await
        .expect("an empty instance");
    engine.run_once().await.expect("ran");

    // A Bandit dropped on each map, the reset clicked before the drops are
    // applied.
    let late = "70000000-0000-4000-8000-000000000004";
    let lone = "70000000-0000-4000-8000-000000000005";
    submit(&engine, "test.dropToken", bandit(late, INSTANCE, 4), None)
        .await
        .expect("a drop queued");
    submit(&engine, "test.dropToken", bandit(lone, NOWHERE, 1), None)
        .await
        .expect("a drop queued");
    let tavern = reset(&engine, INSTANCE).await.expect("queued");
    assert!(landed(&tavern).contains(&partition(late)));
    let empty = reset(&engine, NOWHERE).await.expect("queued");
    assert_eq!(landed(&empty), [partition(lone)]);
    let settled = engine.run_once().await.expect("the applier ran");
    assert!(
        settled.iter().all(|result| result.status == State::Applied),
        "{settled:?}"
    );
    for id in [late, lone] {
        assert_eq!(
            scratch.engine_row(&partition(id)).await.map(|r| r.1),
            Some(true),
            "{id}"
        );
    }
    scratch.drop().await;
}

#[tokio::test]
async fn a_reset_overwrites_a_move_queued_ahead_of_it() {
    let scratch = Scratch::new("overwrite").await;
    let engine = scratch.serve().await;
    scratch.plant(&engine).await;

    // The DM drags Bandit 2, then resets before the move is applied.
    submit(
        &engine,
        "test.move",
        json!({ "tokenId": bandit_id(2), "q": 7, "r": 7 }),
        None,
    )
    .await
    .expect("the move queued");
    let submitted = reset(&engine, INSTANCE).await.expect("queued");
    assert!(landed(&submitted).contains(&partition(&bandit_id(2))));
    let settled = engine.run_once().await.expect("the applier ran");
    assert!(
        settled.iter().all(|result| result.status == State::Applied),
        "{settled:?}"
    );
    assert_eq!(
        scratch
            .engine_row(&partition(&bandit_id(2)))
            .await
            .map(|r| r.1),
        Some(true)
    );
    scratch.drop().await;
}

#[tokio::test]
async fn a_reset_by_a_caller_who_approves_nothing_waits_for_its_review() {
    let scratch = Scratch::new("review").await;
    let engine = scratch.serve().await;
    scratch.plant(&engine).await;

    // A reset deletes critical rows: one with no approver waits, and
    // nothing is tombstoned until a human approves it.
    let body = json!({
        "dataCapability": "table.resetBattlemap@1",
        "target": { "aggregate": "BattlemapInstance", "id": INSTANCE },
        "payload": {},
        "caller": { "by": "joueur", "from": "Battlemap", "roles": [] },
    });
    let submitted = engine
        .submit(serde_json::from_value::<Submission>(body).expect("a submission"))
        .await
        .expect("queued");
    assert_eq!(
        submitted.result.status,
        State::AwaitingReview,
        "{submitted:?}"
    );
    engine.run_once().await.expect("the applier ran");
    for id in [entry_id(1), RUNNING.to_owned(), bandit_id(1)] {
        assert_eq!(
            scratch.engine_row(&partition(&id)).await.map(|r| r.1),
            Some(false),
            "{id}"
        );
    }
    assert!(scratch.journal().await.is_empty());
    scratch.drop().await;
}
