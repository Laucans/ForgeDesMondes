//! table.removeToken over PostgreSQL (`DATABASE_URL`), as the server plants
//! it beside its target aggregate: a removed token is a tombstone — its row
//! kept, archived, a soft delete — still removed once the engine is opened
//! again, and none to remove twice; the battlemap, its grid and the
//! instance read the same (`instance-isolated`) — the command writes no
//! table. What the removal leaves to the tables is held as a fixture: once a
//! token's row leaves `token` (what its tombstone is to the read side, which
//! the aggregate's projection — none yet — writes), they follow the
//! aggregate's relations: its entry goes (cascade), a turn still on it is no
//! one's (nullify), and the order the carousel shows closes up — 1, 2, 3 in
//! the previous order — with no rank written. Each test works in a scratch
//! database of its own, migrated as the server migrates it, dropped when it
//! ends.

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
use serde_json::{Value, json};

const VALOMBRE: &str = "c1000000-0000-4000-8000-000000000001";
/// Valombre's battlemap with its grid.
const GUE: &str = "d1000000-0000-4000-8000-00000000000a";
const SEANCE: &str = "5e000000-0000-4000-8000-000000000001";
const ARRIVEE: &str = "5c000000-0000-4000-8000-000000000001";
const INSTANCE: &str = "b1000000-0000-4000-8000-000000000001";
const BANDIT: &str = "e1000000-0000-4000-8000-0000000000b1";
const COMBAT: &str = "c0000000-0000-4000-8000-000000000001";

/// The test-only DataCapabilities planting the rows a removal lands on.
const OPEN_INSTANCE: &str = r#"{"dataCapability":"test.openInstance","owner":"dataguard","version":1,"effect":"insert","target":{"aggregate":"BattlemapInstance","id":"$battlemapInstanceId"},"mode":"overwrite","callableBy":["*"]}"#;
const DROP_TOKEN: &str = r#"{"dataCapability":"test.dropToken","owner":"dataguard","version":1,"effect":"insert","target":{"aggregate":"BattlemapInstance","id":"$tokenId"},"mode":"overwrite","callableBy":["*"]}"#;

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
        remove_token::plugin(),
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

/// A database of its own, migrated, holding Valombre, its battlemap with
/// its grid, the session and its scene.
struct Scratch {
    name: String,
    url: String,
    client: Client,
}

impl Scratch {
    async fn new(name: &str) -> Scratch {
        let name = format!("remove_token_test_{name}_{}", std::process::id());
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
                     ('{ARRIVEE}', '{SEANCE}', 1, 'Arrivée');"
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
        .with_plugins(plugins());
        engine.load().await.expect("the engine loads");
        engine
    }

    /// The tavern scene's instance of the ford, and Bandit 1 and 2 on it,
    /// as the engine holds them.
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
        for number in [1, 2] {
            submit(engine, "test.dropToken", bandit(number), None)
                .await
                .expect("a token");
        }
        let planted = engine.run_once().await.expect("planted");
        assert!(
            planted.iter().all(|result| result.status == State::Applied),
            "{planted:?}"
        );
    }

    /// The token `number`'s row as the engine holds it, and whether it is
    /// archived; `None` when it was never written.
    async fn engine_row(&self, number: u8) -> Option<(Value, bool)> {
        connect(&self.url)
            .await
            .query_opt(
                "SELECT fields::text, tombstoned FROM dataguard_rows WHERE partition = $1",
                &[&format!("BattlemapInstance/{}", bandit_id(number))],
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

    /// The battlemaps, their grids and the instances as the tables hold
    /// them, as JSON text.
    async fn tables(&self) -> Vec<String> {
        self.texts(
            "SELECT to_jsonb(m)::text || coalesce(to_jsonb(g)::text, '') FROM media m
             LEFT JOIN battlemap_grid g ON g.media_id = m.id
             UNION ALL SELECT to_jsonb(i)::text FROM battlemap_instance i
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

    async fn drop(self) {
        drop(self.client);
        connect(&url())
            .await
            .batch_execute(&format!("DROP DATABASE {} WITH (FORCE)", self.name))
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

/// Bandit `number` removed from the tavern's instance.
async fn remove(engine: &Engine, number: u8) -> Result<Submitted> {
    submit(
        engine,
        "table.removeToken@1",
        json!({ "token": bandit_id(number) }),
        Some(INSTANCE),
    )
    .await
}

#[tokio::test]
async fn a_removed_token_is_a_tombstone_still_removed_once_reopened() {
    let scratch = Scratch::new("tombstone").await;
    let engine = scratch.serve().await;
    scratch.plant(&engine).await;
    let before = scratch.tables().await;

    remove(&engine, 2).await.expect("queued");
    let settled = engine.run_once().await.expect("the applier ran");
    assert_eq!(settled.len(), 1, "{settled:?}");
    assert_eq!(settled[0].status, State::Applied, "{settled:?}");
    drop(engine);

    // A soft delete: the engine keeps the partition, archived.
    let (_, tombstoned) = scratch.engine_row(2).await.expect("Bandit 2's row kept");
    assert!(tombstoned);
    assert_eq!(scratch.engine_row(1).await, Some((bandit(1), false)));
    assert_eq!(scratch.tables().await, before);

    // Reopened: the token is none to remove again, and nothing is queued.
    let engine = scratch.serve().await;
    match remove(&engine, 2).await {
        Err(Error::Refused(refusal)) => assert_eq!(refusal.kind, RefusalKind::NotFound),
        other => panic!("a removed token is not found: {other:?}"),
    }
    assert!(engine.run_once().await.expect("ran").is_empty());
    let (_, tombstoned) = scratch.engine_row(2).await.expect("Bandit 2's row kept");
    assert!(tombstoned);
    scratch.drop().await;
}

#[tokio::test]
async fn a_token_at_rank_2_of_4_gone_from_the_tables_leaves_the_order_1_2_3_and_no_turn_on_it() {
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
            "INSERT INTO entity (id, campaign_id, type, name) VALUES ('{BANDIT}', '{VALOMBRE}', 'pnj', 'Bandit');
             INSERT INTO character (entity_id, token_color) VALUES ('{BANDIT}', '#888');
             INSERT INTO entity_version (entity_id, version_number, source, snapshot, diff)
                 VALUES ('{BANDIT}', 1, 'dm', '{{}}', '[]');
             INSERT INTO battlemap_instance (id, battlemap_media_id, session_id, scene_id) VALUES
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
            entry_id(2),
        ))
        .await
        .expect("the tavern's combat");

    // Bandit 2's tombstone, as the read side holds it: its row gone. No
    // projection of BattlemapInstance writes the tables yet; this is what
    // one does with a tombstone.
    scratch
        .client
        .batch_execute(&format!("DELETE FROM token WHERE id = '{}'", bandit_id(2)))
        .await
        .expect("removed");

    // Its entry went with it, and the order closes up in the previous order;
    // the ranks stored are untouched.
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
    let entries = scratch
        .texts(&format!(
            "SELECT count(*)::text FROM initiative_entry WHERE id = '{}'",
            entry_id(2)
        ))
        .await;
    assert_eq!(entries, ["0"]);
    // The turn held by the token is no one's: table.nextTurn, sent first,
    // gives it to the next living participant.
    let current = scratch
        .texts("SELECT coalesce(current_entry_id::text, 'null') FROM combat")
        .await;
    assert_eq!(current, ["null"]);
    scratch.drop().await;
}
