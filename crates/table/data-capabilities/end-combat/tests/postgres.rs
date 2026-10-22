//! table.endCombat over PostgreSQL (`DATABASE_URL`), as the server plants it
//! beside its target aggregate and `table.startCombat`: Elowen, a wounded
//! Bandit and a poisoned Vesna in the tavern's combat, started by the real
//! start. Ended, the combat is stamped and no turn is current, its tokens,
//! their HP and their conditions, and its entries, stay as they were, and
//! the tables read the same; a new combat starts once it ended
//! (`one-active-combat`), and the next end closes that one. An instance with
//! no combat open — none started, one ended, or one whose end is queued —
//! is refused before anything is queued. Each test works in a scratch
//! database of its own, migrated as the server migrates it, dropped when it
//! ends.

use std::path::Path;
use std::sync::Arc;

use end_combat::partition;
use harness_dataguard::aggregate::Aggregate;
use harness_dataguard::api::{Submission, Submitted};
use harness_dataguard::command::State;
use harness_dataguard::engine::Engine;
use harness_dataguard::manifest::DataCapability;
use harness_dataguard::memory::MemoryNotifier;
use harness_dataguard::plugins::{Plugins, RefusalKind};
use harness_dataguard::postgres::{PgStore, ReadOnly, SystemClock};
use harness_dataguard::tokio_postgres::{self, Client, NoTls};
use harness_dataguard::{Error, Result};
use serde_json::{Value, json};
use start_combat::combat_id;

const VALOMBRE: &str = "c1000000-0000-4000-8000-000000000001";
/// Valombre's battlemap with its grid.
const GUE: &str = "d1000000-0000-4000-8000-00000000000a";
const SEANCE: &str = "5e000000-0000-4000-8000-000000000001";
const ARRIVEE: &str = "5c000000-0000-4000-8000-000000000001";
const INSTANCE: &str = "b1000000-0000-4000-8000-000000000001";
/// An instance no table holds, and no combat.
const NOWHERE: &str = "b1000000-0000-4000-8000-0000000000ff";
const ELOWEN: &str = "e1000000-0000-4000-8000-000000000001";
const BANDIT: &str = "e1000000-0000-4000-8000-0000000000b1";
const VESNA: &str = "e1000000-0000-4000-8000-0000000000b2";
const T_ELOWEN: &str = "70000000-0000-4000-8000-000000000001";
const T_BANDIT: &str = "70000000-0000-4000-8000-000000000002";
const T_VESNA: &str = "70000000-0000-4000-8000-000000000003";

/// The test-only DataCapabilities planting the rows a combat runs among,
/// and one writing a combat row as it is, beside the start.
const OPEN_INSTANCE: &str = r#"{"dataCapability":"test.openInstance","owner":"dataguard","version":1,"effect":"insert","target":{"aggregate":"BattlemapInstance","id":"$battlemapInstanceId"},"mode":"overwrite","callableBy":["*"]}"#;
const DROP_TOKEN: &str = r#"{"dataCapability":"test.dropToken","owner":"dataguard","version":1,"effect":"insert","target":{"aggregate":"BattlemapInstance","id":"$tokenId"},"mode":"overwrite","callableBy":["*"]}"#;
const WRITE_COMBAT: &str = r#"{"dataCapability":"test.writeCombat","owner":"dataguard","version":1,"effect":"insert","target":{"aggregate":"BattlemapInstance","id":"$combatId"},"mode":"overwrite","callableBy":["*"]}"#;

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
        end_combat::plugin(),
        start_combat::plugin(),
    ]
    .into_iter()
    .collect()
}

/// A token of `character` on the tavern's instance, as `table.placeTokens`
/// drops it; an NPC's tracks its HP, `hp` of 11.
fn token(id: &str, character: &str, name: &str, hp: i64, conditions: Value) -> Value {
    let npc = character != ELOWEN;
    json!({
        "tokenId": id,
        "tokenInstanceId": INSTANCE,
        "characterId": character,
        "number": if npc { json!(1) } else { Value::Null },
        "name": name,
        "q": 5,
        "r": 5,
        "visible": true,
        "hpCurrent": if npc { json!(hp) } else { Value::Null },
        "hpMax": if npc { json!(11) } else { Value::Null },
        "conditions": conditions,
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
        let name = format!("end_combat_test_{name}_{}", std::process::id());
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

    /// The engine as the server plants this unit, its target aggregate and
    /// `table.startCombat` over this database, with the test-only
    /// DataCapabilities.
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
        .with_data_capability(manifest(include_str!(
            "../../start-combat/data-capability.json"
        )))
        .with_data_capability(manifest(OPEN_INSTANCE))
        .with_data_capability(manifest(DROP_TOKEN))
        .with_data_capability(manifest(WRITE_COMBAT))
        .with_plugins(plugins());
        engine.load().await.expect("the engine loads");
        engine
    }

    /// The tavern scene's instance of the ford, and on it Elowen, the
    /// Bandit at 4 HP of 11 and Vesna, poisoned, as the engine holds them.
    async fn plant(&self, engine: &Engine) {
        submit(
            engine,
            "test.openInstance",
            json!({
                "battlemapInstanceId": INSTANCE,
                "battlemapMediaId": GUE,
                "sessionId": SEANCE,
                "sceneId": ARRIVEE,
                "createdAt": "2026-10-22T10:00:00Z",
            }),
            None,
        )
        .await
        .expect("the instance");
        for token in [
            token(T_ELOWEN, ELOWEN, "Elowen", 0, json!([])),
            token(T_BANDIT, BANDIT, "Bandit 1", 4, json!([])),
            token(T_VESNA, VESNA, "Chef bandit 1", 11, json!(["empoisonné"])),
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

    /// Every row the engine holds but the combats', and every row of the
    /// product's tables an end could reach, as JSON text.
    async fn untouched(&self) -> Vec<String> {
        self.texts(
            "SELECT partition || ' ' || fields::text || ' ' || tombstoned::text FROM dataguard_rows
                 WHERE fields ->> 'combatId' IS NULL
             UNION ALL SELECT to_jsonb(i)::text FROM battlemap_instance i
             UNION ALL SELECT to_jsonb(t)::text FROM token t
             UNION ALL SELECT to_jsonb(c)::text FROM combat c
             UNION ALL SELECT to_jsonb(e)::text FROM initiative_entry e
             ORDER BY 1",
        )
        .await
    }

    async fn texts(&self, sql: &str) -> Vec<String> {
        self.client
            .query(sql, &[])
            .await
            .unwrap_or_else(|e| panic!("{sql}: {e:?}"))
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

/// A start of the tavern's combat, as the Battlemap sends it: Elowen
/// entered at 18, the Bandit rolling 13, Vesna 11.
async fn start(engine: &Engine) -> Submitted {
    let started = submit(
        engine,
        "table.startCombat@1",
        json!({ "initiative": { ELOWEN: 18 }, "rolls": { T_BANDIT: 13, T_VESNA: 11 } }),
        Some(INSTANCE),
    )
    .await
    .expect("started");
    applied(engine).await;
    started
}

/// The end of `instance`'s combat, as the Battlemap sends it.
async fn end(engine: &Engine, instance: &str) -> Result<Submitted> {
    submit(engine, "table.endCombat@1", json!({}), Some(instance)).await
}

/// That `answer` is the refusal of an instance with no combat open, 422.
fn no_open_combat(answer: Result<Submitted>) {
    match answer {
        Err(Error::Refused(refusal)) => {
            assert_eq!(refusal.kind, RefusalKind::Unprocessable);
            assert!(
                refusal.message.starts_with("one-active-combat"),
                "{}",
                refusal.message
            );
        }
        other => panic!("no combat open: {other:?}"),
    }
}

/// That `submitted` was queued with no invariant at risk.
fn safe(submitted: &Submitted) {
    assert!(
        !submitted
            .warnings
            .iter()
            .any(|warning| warning.starts_with("invariant(s) at risk")),
        "{:?}",
        submitted.warnings
    );
}

#[tokio::test(flavor = "current_thread")]
async fn an_open_combat_ends_now_with_no_turn_its_tokens_hp_conditions_and_entries_as_they_were() {
    let scratch = Scratch::new("ends").await;
    let engine = scratch.serve().await;
    scratch.plant(&engine).await;
    start(&engine).await;
    let combat = combat_id(INSTANCE, 0);
    let started = scratch.row(&partition(&combat)).await.expect("the combat");
    assert!(started["currentEntryId"].as_str().is_some(), "{started}");
    let before = scratch.untouched().await;

    let ended = end(&engine, INSTANCE).await.expect("an end");
    assert_eq!(ended.partition, partition(&combat));
    safe(&ended);
    applied(&engine).await;

    let row = scratch.row(&partition(&combat)).await.expect("the combat");
    let ended_at = row["endedAt"].as_str().expect("an end stamped");
    let started_at = row["startedAt"].as_str().expect("its start");
    assert!(ended_at >= started_at, "{ended_at} before {started_at}");
    assert_eq!(row["currentEntryId"], Value::Null);
    // The rest of the combat stays as it started: its instance, its round.
    for field in ["combatId", "combatInstanceId", "round", "startedAt"] {
        assert_eq!(row[field], started[field], "{field}");
    }
    // The tokens, their HP and their conditions, the entries and the tables
    // read the same: the end writes the combat's row alone.
    assert_eq!(scratch.untouched().await, before);
    let bandit = scratch.row(&partition(T_BANDIT)).await.expect("the Bandit");
    assert_eq!(bandit["hpCurrent"], 4);
    let vesna = scratch.row(&partition(T_VESNA)).await.expect("Vesna");
    assert_eq!(vesna["conditions"], json!(["empoisonné"]));
    scratch.drop().await;
}

#[tokio::test(flavor = "current_thread")]
async fn one_active_combat_once_ended_a_new_combat_starts_and_the_next_end_closes_that_one() {
    let scratch = Scratch::new("restart").await;
    let engine = scratch.serve().await;
    scratch.plant(&engine).await;
    start(&engine).await;
    // A second start while the first runs is refused.
    let refused = submit(
        &engine,
        "table.startCombat@1",
        json!({ "initiative": { ELOWEN: 18 } }),
        Some(INSTANCE),
    )
    .await;
    assert!(matches!(refused, Err(Error::Refused(_))), "{refused:?}");
    end(&engine, INSTANCE).await.expect("an end");
    applied(&engine).await;
    let first = combat_id(INSTANCE, 0);
    let first_ended = scratch.row(&partition(&first)).await.expect("the first")["endedAt"].clone();

    let restarted = start(&engine).await;
    let second = combat_id(INSTANCE, 1);
    assert_eq!(restarted.partition, partition(&second));
    let row = scratch.row(&partition(&second)).await.expect("the second");
    assert_eq!(row["endedAt"], Value::Null);
    assert!(row["currentEntryId"].as_str().is_some(), "{row}");

    // Sent with the instance in its payload, the next end closes the new
    // combat; the first stays ended as it was.
    let ended = submit(
        &engine,
        "table.endCombat@1",
        json!({ "instance": INSTANCE }),
        None,
    )
    .await
    .expect("an end");
    assert_eq!(ended.partition, partition(&second));
    safe(&ended);
    applied(&engine).await;
    let row = scratch.row(&partition(&second)).await.expect("the second");
    assert!(row["endedAt"].as_str().is_some(), "{row}");
    assert_eq!(row["currentEntryId"], Value::Null);
    let first = scratch.row(&partition(&first)).await.expect("the first");
    assert_eq!(first["endedAt"], first_ended);
    scratch.drop().await;
}

#[tokio::test(flavor = "current_thread")]
async fn an_instance_with_no_combat_open_ends_none_and_nothing_is_queued() {
    let scratch = Scratch::new("none").await;
    let engine = scratch.serve().await;
    scratch.plant(&engine).await;
    let before = scratch.untouched().await;
    // None started yet.
    no_open_combat(end(&engine, INSTANCE).await);
    // Another instance's combat is not this one's, and an instance named by
    // no uuid ends none.
    start(&engine).await;
    no_open_combat(end(&engine, NOWHERE).await);
    let not_an_id = end(&engine, "la-taverne").await;
    assert!(
        matches!(&not_an_id, Err(Error::Refused(refusal)) if refusal.kind == RefusalKind::Unprocessable),
        "{not_an_id:?}"
    );
    // An end queued and not yet applied leaves no combat open to end twice.
    end(&engine, INSTANCE).await.expect("an end");
    no_open_combat(end(&engine, INSTANCE).await);
    let run = engine.run_once().await.expect("a run");
    assert_eq!(run.len(), 1, "{run:?}");
    assert_eq!(run[0].status, State::Applied);
    // Applied, it leaves none either.
    no_open_combat(end(&engine, INSTANCE).await);
    assert!(engine.run_once().await.expect("a run").is_empty());
    // The refusals wrote nothing: the tokens are as they were planted.
    let after = scratch.untouched().await;
    assert!(
        before.iter().all(|row| after.contains(row)),
        "{before:?}\n{after:?}"
    );
    scratch.drop().await;
}

#[tokio::test(flavor = "current_thread")]
async fn of_two_combats_with_no_end_the_latest_started_is_the_one_ended() {
    let scratch = Scratch::new("latest").await;
    let engine = scratch.serve().await;
    scratch.plant(&engine).await;
    // Two combats left open by a write that came before the start's rule,
    // the older one first: the end closes the one started last.
    let older = "c0000000-0000-4000-8000-0000000000a1";
    let latest = "c0000000-0000-4000-8000-0000000000a2";
    for (combat, started) in [
        (older, "2026-10-22T10:05:00+00:00"),
        (latest, "2026-10-22T10:30:00+00:00"),
    ] {
        submit(
            &engine,
            "test.writeCombat",
            json!({
                "combatId": combat,
                "combatInstanceId": INSTANCE,
                "round": 1,
                "currentEntryId": null,
                "startedAt": started,
            }),
            None,
        )
        .await
        .expect("a combat");
    }
    applied(&engine).await;

    let ended = end(&engine, INSTANCE).await.expect("an end");
    assert_eq!(ended.partition, partition(latest));
    // With the latest one's end queued, the next end closes the older one.
    let ended = end(&engine, INSTANCE).await.expect("an end");
    assert_eq!(ended.partition, partition(older));
    applied(&engine).await;
    for combat in [older, latest] {
        let row = scratch.row(&partition(combat)).await.expect("a combat");
        assert!(row["endedAt"].as_str().is_some(), "{combat}: {row}");
    }
    no_open_combat(end(&engine, INSTANCE).await);
    scratch.drop().await;
}
