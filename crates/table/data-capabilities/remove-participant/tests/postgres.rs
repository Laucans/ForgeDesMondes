//! table.removeParticipant over PostgreSQL (`DATABASE_URL`), as the server
//! plants it beside its target aggregate: four Bandits in the tavern's
//! combat, ranked 1 to 4, Bandit 1's turn. A participant named by its token
//! is its entry in the instance's open combat; taken out, its entry is a
//! tombstone — its row kept, archived, a soft delete — still out once the
//! engine is opened again, and none to take out twice; its token stays, and
//! the tables read the same — the command writes none. A token whose combat
//! ended is in no running order. What the removal leaves to the tables is
//! held as a fixture: once an entry's row leaves `initiative_entry` (what its
//! tombstone is to the read side, which the aggregate's projection — none
//! yet — writes), its token stays and the order the carousel shows closes up
//! — 1, 2, 3 in the previous order — with no rank written. Each test works
//! in a scratch database of its own, migrated as the server migrates it,
//! dropped when it ends.

use std::path::Path;
use std::sync::Arc;

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
use remove_participant::partition;
use serde_json::{Value, json};

const VALOMBRE: &str = "c1000000-0000-4000-8000-000000000001";
/// Valombre's battlemap with its grid.
const GUE: &str = "d1000000-0000-4000-8000-00000000000a";
const SEANCE: &str = "5e000000-0000-4000-8000-000000000001";
const ARRIVEE: &str = "5c000000-0000-4000-8000-000000000001";
const INSTANCE: &str = "b1000000-0000-4000-8000-000000000001";
const BANDIT: &str = "e1000000-0000-4000-8000-0000000000b1";
const COMBAT: &str = "c0000000-0000-4000-8000-000000000001";

/// The test-only DataCapabilities planting the rows a removal lands on, and
/// the combat's end.
const OPEN_INSTANCE: &str = r#"{"dataCapability":"test.openInstance","owner":"dataguard","version":1,"effect":"insert","target":{"aggregate":"BattlemapInstance","id":"$battlemapInstanceId"},"mode":"overwrite","callableBy":["*"]}"#;
const DROP_TOKEN: &str = r#"{"dataCapability":"test.dropToken","owner":"dataguard","version":1,"effect":"insert","target":{"aggregate":"BattlemapInstance","id":"$tokenId"},"mode":"overwrite","callableBy":["*"]}"#;
const START_COMBAT: &str = r#"{"dataCapability":"test.startCombat","owner":"dataguard","version":1,"effect":"insert","target":{"aggregate":"BattlemapInstance","id":"$combatId"},"mode":"overwrite","callableBy":["*"]}"#;
const ENTER: &str = r#"{"dataCapability":"test.enter","owner":"dataguard","version":1,"effect":"insert","target":{"aggregate":"BattlemapInstance","id":"$initiativeEntryId"},"mode":"overwrite","callableBy":["*"]}"#;
const END_COMBAT: &str = r#"{"dataCapability":"test.endCombat","owner":"dataguard","version":1,"effect":"update","target":{"aggregate":"BattlemapInstance","id":"$combat"},"touches":["BattlemapInstance.endedAt","BattlemapInstance.currentEntryId"],"mode":"overwrite","callableBy":["*"]}"#;

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
    [
        aggregate_battlemap_instance::plugin(),
        remove_participant::plugin(),
    ]
    .into_iter()
    .collect()
}

/// Bandit `number` on the tavern's instance, as `table.placeTokens` drops
/// it.
fn bandit(number: u8) -> Value {
    json!({
        "tokenId": bandit_id(number),
        "tokenInstanceId": INSTANCE,
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

/// Bandit `number`'s place in the combat, as `table.startCombat` seats it:
/// rank `number`.
fn entry(number: u8) -> Value {
    json!({
        "initiativeEntryId": entry_id(number),
        "initiativeEntryCombatId": COMBAT,
        "initiativeEntryTokenId": bandit_id(number),
        "roll": 15 - i64::from(number),
        "initiative": 15 - i64::from(number),
        "dexScore": 10,
        "tiebreak": f64::from(number) / 10.0,
        "rank": number,
    })
}

/// A database of its own, migrated, holding Valombre, its battlemap with
/// its grid, the session and its scene, and the Bandit, whom the tokens
/// stand for: what the tables' foreign keys ask of the rows the aggregate's
/// projection writes.
struct Scratch {
    name: String,
    url: String,
    client: Client,
}

impl Scratch {
    async fn new(name: &str) -> Scratch {
        let name = format!("remove_participant_test_{name}_{}", std::process::id());
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
                     ('{BANDIT}', '{VALOMBRE}', 'pnj', 'Bandit');
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
        .with_data_capability(manifest(END_COMBAT))
        .with_plugins(plugins());
        engine.load().await.expect("the engine loads");
        engine
    }

    /// The tavern scene's instance of the ford, Bandit 1 to 4 on it and
    /// their combat, ranked 1 to 4, Bandit 1's turn, as the engine holds
    /// them.
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
        for number in 1..=4 {
            submit(engine, "test.dropToken", bandit(number), None)
                .await
                .expect("a token");
        }
        submit(
            engine,
            "test.startCombat",
            json!({
                "combatId": COMBAT,
                "combatInstanceId": INSTANCE,
                "round": 1,
                "currentEntryId": entry_id(1),
                "startedAt": "2026-10-21T10:05:00Z",
            }),
            None,
        )
        .await
        .expect("the combat");
        for number in 1..=4 {
            submit(engine, "test.enter", entry(number), None)
                .await
                .expect("an entry");
        }
        let planted = engine.run_once().await.expect("planted");
        assert!(
            planted.iter().all(|result| result.status == State::Applied),
            "{planted:?}"
        );
    }

    /// The row the engine holds on `partition`, and whether it is archived;
    /// `None` when it was never written.
    async fn engine_row(&self, partition: &str) -> Option<(Value, bool)> {
        connect(&self.url)
            .await
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

    /// The product's tables a removal could reach, as JSON text.
    async fn tables(&self) -> Vec<String> {
        self.texts(
            "SELECT to_jsonb(m)::text FROM media m
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

/// `participant` taken out of the tavern's combat.
async fn remove(engine: &Engine, participant: &str) -> Result<Submitted> {
    submit(
        engine,
        "table.removeParticipant@1",
        json!({ "participant": participant }),
        Some(INSTANCE),
    )
    .await
}

fn not_found(answer: Result<Submitted>) {
    match answer {
        Err(Error::Refused(refusal)) => assert_eq!(refusal.kind, RefusalKind::NotFound),
        other => panic!("a participant that is not there: {other:?}"),
    }
}

#[tokio::test]
async fn a_participant_named_by_its_token_is_its_entry_tombstoned_still_out_once_reopened() {
    let scratch = Scratch::new("tombstone").await;
    let engine = scratch.serve().await;
    scratch.plant(&engine).await;
    let before = scratch.tables().await;

    let submitted = remove(&engine, &bandit_id(2)).await.expect("queued");
    // The token named is its entry's partition the command lands on.
    assert_eq!(submitted.partition, partition(&entry_id(2)));
    assert!(submitted.warnings.is_empty(), "{:?}", submitted.warnings);
    let settled = engine.run_once().await.expect("the applier ran");
    assert_eq!(settled.len(), 1, "{settled:?}");
    assert_eq!(settled[0].status, State::Applied, "{settled:?}");
    drop(engine);

    // A soft delete: the engine keeps the entry's partition, archived; the
    // token stays, every other entry with its rank.
    let (_, tombstoned) = scratch
        .engine_row(&partition(&entry_id(2)))
        .await
        .expect("Bandit 2's entry kept");
    assert!(tombstoned);
    for number in 1..=4 {
        assert_eq!(
            scratch.engine_row(&partition(&bandit_id(number))).await,
            Some((bandit(number), false))
        );
    }
    for number in [1, 3, 4] {
        assert_eq!(
            scratch.engine_row(&partition(&entry_id(number))).await,
            Some((entry(number), false))
        );
    }
    // The turn stays Bandit 1's.
    let (combat, _) = scratch
        .engine_row(&partition(COMBAT))
        .await
        .expect("the combat");
    assert_eq!(combat["currentEntryId"], json!(entry_id(1)));
    // The tables follow the soft delete: its entry's row is out of
    // `initiative_entry`, the token and every other row stay.
    let entry_row = format!("\"id\": \"{}\"", entry_id(2));
    let kept: Vec<String> = before
        .iter()
        .filter(|row| !row.contains(&entry_row))
        .cloned()
        .collect();
    assert_eq!(kept.len() + 1, before.len(), "{before:?}");
    assert_eq!(scratch.tables().await, kept);

    // Reopened: neither its entry nor its token is a participant to take
    // out again, and nothing is queued.
    let engine = scratch.serve().await;
    not_found(remove(&engine, &entry_id(2)).await);
    not_found(remove(&engine, &bandit_id(2)).await);
    assert!(engine.run_once().await.expect("ran").is_empty());
    let (_, tombstoned) = scratch
        .engine_row(&partition(&entry_id(2)))
        .await
        .expect("Bandit 2's entry kept");
    assert!(tombstoned);
    scratch.drop().await;
}

#[tokio::test]
async fn a_token_whose_combat_ended_is_in_no_running_order() {
    let scratch = Scratch::new("ended").await;
    let engine = scratch.serve().await;
    scratch.plant(&engine).await;
    submit(
        &engine,
        "test.endCombat",
        json!({ "endedAt": "2026-10-21T11:00:00Z", "currentEntryId": null }),
        Some(COMBAT),
    )
    .await
    .expect("the end");
    let ended = engine.run_once().await.expect("ended");
    assert!(
        ended.iter().all(|r| r.status == State::Applied),
        "{ended:?}"
    );

    not_found(remove(&engine, &bandit_id(3)).await);
    // Named by its entry, the combat it is of has ended.
    match remove(&engine, &entry_id(3)).await {
        Err(Error::Refused(refusal)) => {
            assert_eq!(refusal.kind, RefusalKind::Unprocessable);
            assert_eq!(refusal.message, "ce combat est terminé");
        }
        other => panic!("an ended combat: {other:?}"),
    }
    assert!(engine.run_once().await.expect("ran").is_empty());
    assert_eq!(
        scratch.engine_row(&partition(&entry_id(3))).await,
        Some((entry(3), false))
    );
    scratch.drop().await;
}

#[tokio::test]
async fn a_token_s_entry_holding_the_turn_is_refused() {
    let scratch = Scratch::new("turn").await;
    let engine = scratch.serve().await;
    scratch.plant(&engine).await;
    match remove(&engine, &bandit_id(1)).await {
        Err(Error::Refused(refusal)) => {
            assert_eq!(refusal.kind, RefusalKind::Unprocessable);
            assert!(refusal.message.starts_with("next-turn:"), "{refusal:?}");
        }
        other => panic!("the turn's holder: {other:?}"),
    }
    assert!(engine.run_once().await.expect("ran").is_empty());
    scratch.drop().await;
}

#[tokio::test]
async fn an_entry_at_rank_2_of_4_gone_from_the_tables_leaves_its_token_and_the_order_1_2_3() {
    let scratch = Scratch::new("relations").await;
    let tokens: Vec<String> = (1..=4)
        .map(|n| {
            format!(
                "('{}', '{INSTANCE}', '{BANDIT}', {n}, 'Bandit {n}', 5, {n}, true, 11, 11, '{{}}')",
                bandit_id(n)
            )
        })
        .collect();
    let entries: Vec<String> = (1..=4)
        .map(|n| {
            format!(
                "('{}', '{COMBAT}', '{}', 10, {}, 12, 0.5, {n})",
                entry_id(n),
                bandit_id(n),
                20 - n
            )
        })
        .collect();
    scratch
        .client
        .batch_execute(&format!(
            "INSERT INTO battlemap_instance (id, battlemap_media_id, session_id, scene_id) VALUES
                 ('{INSTANCE}', '{GUE}', '{SEANCE}', '{ARRIVEE}');
             INSERT INTO token (id, instance_id, character_id, number, name, q, r, visible, hp_current, hp_max, conditions)
                 VALUES {};
             INSERT INTO combat (id, instance_id, round, started_at) VALUES
                 ('{COMBAT}', '{INSTANCE}', 1, '2026-10-21T10:05:00Z');
             INSERT INTO initiative_entry (id, combat_id, token_id, roll, initiative, dex_score, tiebreak, rank)
                 VALUES {};
             UPDATE combat SET current_entry_id = '{}' WHERE id = '{COMBAT}';",
            tokens.join(", "),
            entries.join(", "),
            entry_id(1),
        ))
        .await
        .expect("the tavern's combat");

    // Bandit 2's entry's tombstone, as the read side holds it: its row
    // gone. No projection of BattlemapInstance writes the tables yet; this
    // is what one does with a tombstone.
    scratch
        .client
        .batch_execute(&format!(
            "DELETE FROM initiative_entry WHERE id = '{}'",
            entry_id(2)
        ))
        .await
        .expect("taken out");

    // The order closes up in the previous order; the ranks stored are
    // untouched.
    let order = scratch
        .texts(
            "SELECT t.name || ' ' || v.order_index || ' ' || e.rank
             FROM initiative_order_view v
             JOIN initiative_entry e ON e.id = v.entry_id
             JOIN token t ON t.id = e.token_id
             ORDER BY v.order_index",
        )
        .await;
    assert_eq!(order, ["Bandit 1 1 1", "Bandit 3 2 3", "Bandit 4 3 4"]);
    // Its token stays on the map, with the others.
    let tokens = scratch
        .texts("SELECT name FROM token ORDER BY number")
        .await;
    assert_eq!(tokens, ["Bandit 1", "Bandit 2", "Bandit 3", "Bandit 4"]);
    // The turn stays where it was.
    let current = scratch
        .texts("SELECT coalesce(current_entry_id::text, 'null') FROM combat")
        .await;
    assert_eq!(current, [entry_id(1)]);
    scratch.drop().await;
}

#[tokio::test]
async fn a_token_targeted_itself_is_its_entry_and_a_token_of_another_instance_is_none() {
    let scratch = Scratch::new("targets").await;
    let engine = scratch.serve().await;
    scratch.plant(&engine).await;

    // Named against another instance: its combat is not that instance's.
    not_found(
        submit(
            &engine,
            "table.removeParticipant@1",
            json!({ "participant": bandit_id(3) }),
            Some("b1000000-0000-4000-8000-000000000002"),
        )
        .await,
    );
    assert!(engine.run_once().await.expect("ran").is_empty());

    // The token as the target, in no instance named: its entry is taken out.
    let submitted = submit(
        &engine,
        "table.removeParticipant@1",
        json!({}),
        Some(&bandit_id(3)),
    )
    .await
    .expect("queued");
    assert_eq!(submitted.partition, partition(&entry_id(3)));
    let settled = engine.run_once().await.expect("the applier ran");
    assert!(
        settled.iter().all(|r| r.status == State::Applied),
        "{settled:?}"
    );
    let (_, tombstoned) = scratch
        .engine_row(&partition(&entry_id(3)))
        .await
        .expect("Bandit 3's entry kept");
    assert!(tombstoned);
    assert_eq!(
        scratch.engine_row(&partition(&bandit_id(3))).await,
        Some((bandit(3), false))
    );
    scratch.drop().await;
}
