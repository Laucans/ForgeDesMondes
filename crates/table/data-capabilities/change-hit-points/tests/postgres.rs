//! table.changeHitPoints over PostgreSQL (`DATABASE_URL`), as the server
//! plants it beside its target aggregate: an NPC's HP are what the blows
//! left them once the engine is opened again, and change from there
//! (`state-persisted-reset`), on its token alone (`hp-change-one-token`); a
//! PC's change is rejected and its token keeps no HP (`pj-hp-not-tracked`);
//! the original battlemap, its grid and the instance read the same after any
//! change (`instance-isolated`); and a change queued behind its token's
//! removal never brings it back. Each test works in a scratch database of
//! its own, migrated as the server migrates it, dropped when it ends; the
//! instance and its tokens are planted by test-only DataCapabilities.

use std::path::Path;
use std::sync::Arc;

use harness_dataguard::aggregate::Aggregate;
use harness_dataguard::api::{Submission, Submitted};
use harness_dataguard::command::{CommandResult, State};
use harness_dataguard::engine::Engine;
use harness_dataguard::manifest::DataCapability;
use harness_dataguard::memory::MemoryNotifier;
use harness_dataguard::plugins::Plugins;
use harness_dataguard::postgres::{PgStore, ReadOnly, SystemClock};
use harness_dataguard::tokio_postgres::{self, Client, NoTls};
use serde_json::{Value, json};

const VALOMBRE: &str = "c1000000-0000-4000-8000-000000000001";
/// Valombre's battlemap with its grid.
const GUE: &str = "d1000000-0000-4000-8000-00000000000a";
const SEANCE: &str = "5e000000-0000-4000-8000-000000000001";
const ARRIVEE: &str = "5c000000-0000-4000-8000-000000000001";
const INSTANCE: &str = "b1000000-0000-4000-8000-000000000001";
const MORLOX: &str = "e1000000-0000-4000-8000-000000000002";
const MORLOX_1: &str = "70000000-0000-4000-8000-000000000002";
const MORLOX_2: &str = "70000000-0000-4000-8000-000000000003";
const ELOWEN: &str = "e1000000-0000-4000-8000-000000000001";
const JETON_ELOWEN: &str = "70000000-0000-4000-8000-000000000001";

/// The test-only DataCapabilities planting and removing the rows a change
/// lands on.
const OPEN_INSTANCE: &str = r#"{"dataCapability":"test.openInstance","owner":"dataguard","version":1,"effect":"insert","target":{"aggregate":"BattlemapInstance","id":"$battlemapInstanceId"},"mode":"overwrite","callableBy":["*"]}"#;
const DROP_TOKEN: &str = r#"{"dataCapability":"test.dropToken","owner":"dataguard","version":1,"effect":"insert","target":{"aggregate":"BattlemapInstance","id":"$tokenId"},"mode":"overwrite","callableBy":["*"]}"#;
const REMOVE_TOKEN: &str = r#"{"dataCapability":"test.removeToken","owner":"dataguard","version":1,"effect":"delete","target":{"aggregate":"BattlemapInstance","id":"$tokenId"},"mode":"overwrite","callableBy":["*"]}"#;

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
        change_hit_points::plugin(),
    ]
    .into_iter()
    .collect()
}

/// A token of Morlox on the tavern's instance, as `table.placeTokens` drops
/// it: an NPC at 7 of 7.
fn token(id: &str, number: i64, hex: (i64, i64)) -> Value {
    json!({
        "tokenId": id,
        "tokenInstanceId": INSTANCE,
        "characterId": MORLOX,
        "number": number,
        "name": format!("Morlox {number}"),
        "q": hex.0,
        "r": hex.1,
        "visible": true,
        "hpCurrent": 7,
        "hpMax": 7,
        "conditions": [],
    })
}

/// Elowen's token, a PC's: no number, no HP.
fn elowen() -> Value {
    json!({
        "tokenId": JETON_ELOWEN,
        "tokenInstanceId": INSTANCE,
        "characterId": ELOWEN,
        "number": null,
        "name": "Elowen",
        "q": 4,
        "r": 4,
        "visible": true,
        "hpCurrent": null,
        "hpMax": null,
        "conditions": [],
    })
}

/// A database of its own, migrated, holding Valombre and its battlemap with
/// its grid.
struct Scratch {
    name: String,
    url: String,
    client: Client,
}

impl Scratch {
    async fn new(name: &str) -> Scratch {
        let name = format!("change_hit_points_test_{name}_{}", std::process::id());
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
                     ('{GUE}', 30, 12, 8, 'pointe', 'text', 0.4);"
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
        .with_data_capability(manifest(REMOVE_TOKEN))
        .with_plugins(plugins());
        engine.load().await.expect("the engine loads");
        engine
    }

    /// The tavern scene's instance of the ford, Morlox 1 and 2 and Elowen on
    /// it.
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
        for (id, number, hex) in [(MORLOX_1, 1, (5, 5)), (MORLOX_2, 2, (5, 6))] {
            submit(engine, "test.dropToken", token(id, number, hex), None)
                .await
                .expect("a token");
        }
        submit(engine, "test.dropToken", elowen(), None)
            .await
            .expect("Elowen's token");
        let planted = engine.run_once().await.expect("planted");
        assert!(
            planted.iter().all(|result| result.status == State::Applied),
            "{planted:?}"
        );
    }

    /// The token `id`'s row as the engine wrote it, read over a connection
    /// of its own; `None` when it is removed or was never written.
    async fn token(&self, id: &str) -> Option<Value> {
        connect(&self.url)
            .await
            .query_opt(
                "SELECT fields::text FROM dataguard_rows WHERE partition = $1 AND NOT tombstoned",
                &[&format!("BattlemapInstance/{id}")],
            )
            .await
            .expect("the engine's rows")
            .map(|row| serde_json::from_str(row.get::<_, &str>(0)).expect("JSON"))
    }

    /// The battlemaps, their grids and the instances as the tables hold
    /// them, as JSON text.
    async fn tables(&self) -> Vec<String> {
        self.client
            .query(
                "SELECT to_jsonb(m)::text || coalesce(to_jsonb(g)::text, '') FROM media m
                 LEFT JOIN battlemap_grid g ON g.media_id = m.id
                 UNION ALL SELECT to_jsonb(i)::text FROM battlemap_instance i
                 ORDER BY 1",
                &[],
            )
            .await
            .expect("the tables")
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
) -> harness_dataguard::Result<Submitted> {
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

/// `delta` applied to `token` on the tavern's instance, queued.
async fn change(engine: &Engine, token: &str, delta: i64) {
    submit(
        engine,
        "table.changeHitPoints@1",
        json!({ "token": token, "delta": delta }),
        Some(INSTANCE),
    )
    .await
    .expect("queued");
}

/// The one command the applier settled.
async fn settled(engine: &Engine) -> CommandResult {
    let mut settled = engine.run_once().await.expect("the applier ran");
    assert_eq!(settled.len(), 1, "{settled:?}");
    settled.remove(0)
}

#[tokio::test]
async fn state_persisted_reset_an_npc_s_hp_are_what_the_blows_left_once_reopened() {
    let scratch = Scratch::new("reopened").await;
    let engine = scratch.serve().await;
    scratch.plant(&engine).await;

    change(&engine, MORLOX_1, -3).await;
    assert_eq!(settled(&engine).await.status, State::Applied);
    drop(engine);

    // Reopened: a new engine over the same database, and a read of its own.
    let wounded = scratch.token(MORLOX_1).await.expect("Morlox 1");
    let mut expected = token(MORLOX_1, 1, (5, 5));
    expected["hpCurrent"] = json!(4);
    // Everything else of it as it was dropped, and Morlox 2 untouched.
    assert_eq!(wounded, expected);
    assert_eq!(
        scratch.token(MORLOX_2).await.expect("Morlox 2"),
        token(MORLOX_2, 2, (5, 6))
    );
    // The reopened engine steps from where it was left: 4 − 10 stops at 0,
    // and the token is dead by its HP alone.
    let engine = scratch.serve().await;
    change(&engine, MORLOX_1, -10).await;
    assert_eq!(settled(&engine).await.status, State::Applied);
    let dead = scratch.token(MORLOX_1).await.expect("Morlox 1");
    expected["hpCurrent"] = json!(0);
    assert_eq!(dead, expected);
    scratch.drop().await;
}

#[tokio::test]
async fn pj_hp_not_tracked_a_pc_s_change_is_rejected_and_its_token_keeps_no_hp() {
    let scratch = Scratch::new("pc").await;
    let engine = scratch.serve().await;
    scratch.plant(&engine).await;

    change(&engine, JETON_ELOWEN, -5).await;
    let result = settled(&engine).await;
    assert_eq!(result.status, State::Rejected, "{result:?}");
    assert!(
        result.violations.contains(&"pj-hp-not-tracked".to_owned()),
        "{result:?}"
    );
    assert_eq!(scratch.token(JETON_ELOWEN).await, Some(elowen()));
    scratch.drop().await;
}

#[tokio::test]
async fn instance_isolated_the_battlemap_its_grid_and_the_instance_read_the_same_after_changes() {
    let scratch = Scratch::new("isolated").await;
    let engine = scratch.serve().await;
    scratch.plant(&engine).await;
    let before = scratch.tables().await;

    for (token, delta) in [(MORLOX_1, -2), (MORLOX_2, -7), (MORLOX_1, 1)] {
        change(&engine, token, delta).await;
        assert_eq!(settled(&engine).await.status, State::Applied);
    }
    assert_eq!(scratch.tables().await, before);
    scratch.drop().await;
}

#[tokio::test]
async fn a_change_queued_behind_its_token_s_removal_never_brings_it_back() {
    let scratch = Scratch::new("removed").await;
    let engine = scratch.serve().await;
    scratch.plant(&engine).await;

    // Both queued while the token stands: the removal first.
    submit(
        &engine,
        "test.removeToken",
        json!({ "tokenId": MORLOX_1 }),
        None,
    )
    .await
    .expect("the removal queued");
    change(&engine, MORLOX_1, -2).await;
    let results = engine.run_once().await.expect("the applier ran");
    assert_eq!(results.len(), 2, "{results:?}");
    assert_eq!(results[0].status, State::Applied, "{results:?}");
    assert_eq!(results[1].status, State::Rejected, "{results:?}");
    assert_eq!(scratch.token(MORLOX_1).await, None);
    // And once removed, a change is refused before it is queued.
    let refused = submit(
        &engine,
        "table.changeHitPoints@1",
        json!({ "token": MORLOX_1, "delta": 2 }),
        Some(INSTANCE),
    )
    .await;
    assert!(
        matches!(&refused, Err(harness_dataguard::Error::Refused(refusal)) if refusal.message.contains("token")),
        "{refused:?}"
    );
    assert_eq!(scratch.token(MORLOX_1).await, None);
    scratch.drop().await;
}
