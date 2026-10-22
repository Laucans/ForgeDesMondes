//! table.reorderInitiative over PostgreSQL (`DATABASE_URL`), as the server
//! plants it beside its target aggregate. The tavern's combat of four —
//! Elowen, the Bandit, Vesna hidden, the Wolf — is reordered with the Wolf
//! first: the ranks are 1 to 4 in the new order, without gap or duplicate,
//! each written on its entry's partition and judged by the aggregate; the
//! hidden one takes the rank it is given (`hidden-keeps-rank`); the turn,
//! the round and the tokens stay. An order that omits, repeats or adds a
//! participant is refused, as one on an instance with no open combat, and
//! nothing is queued. Each test works in a scratch database of its own,
//! migrated as the server migrates it, dropped when it ends.

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
use reorder_initiative::partition;
use serde_json::{Value, json};

const VALOMBRE: &str = "c1000000-0000-4000-8000-000000000001";
const GUE: &str = "d1000000-0000-4000-8000-00000000000a";
const SEANCE: &str = "5e000000-0000-4000-8000-000000000001";
const ARRIVEE: &str = "5c000000-0000-4000-8000-000000000001";
const EMBUSCADE: &str = "5c000000-0000-4000-8000-000000000002";
/// The tavern's instance, the engine's; the ambush's, the tables' alone.
const INSTANCE: &str = "b1000000-0000-4000-8000-000000000001";
const TABLES_ONLY: &str = "b1000000-0000-4000-8000-000000000002";
const NOWHERE: &str = "b1000000-0000-4000-8000-0000000000ff";
const ELOWEN: &str = "e1000000-0000-4000-8000-000000000001";
const BANDIT: &str = "e1000000-0000-4000-8000-0000000000b1";
const VESNA: &str = "e1000000-0000-4000-8000-0000000000b2";
const LOUP: &str = "e1000000-0000-4000-8000-0000000000b3";
const T_ELOWEN: &str = "70000000-0000-4000-8000-000000000001";
const T_BANDIT: &str = "70000000-0000-4000-8000-000000000002";
const T_VESNA: &str = "70000000-0000-4000-8000-000000000003";
const T_LOUP: &str = "70000000-0000-4000-8000-000000000004";
/// The tavern's combat and its four entries, ranked 1 to 4 in this order.
const COMBAT: &str = "c0000000-0000-4000-8000-000000000001";
const E_ELOWEN: &str = "ee000000-0000-4000-8000-000000000001";
const E_BANDIT: &str = "ee000000-0000-4000-8000-000000000002";
const E_VESNA: &str = "ee000000-0000-4000-8000-000000000003";
const E_LOUP: &str = "ee000000-0000-4000-8000-000000000004";

/// The test-only DataCapabilities planting the rows a reorder lands among,
/// ending a combat and removing a token.
const OPEN_INSTANCE: &str = r#"{"dataCapability":"test.openInstance","owner":"dataguard","version":1,"effect":"insert","target":{"aggregate":"BattlemapInstance","id":"$battlemapInstanceId"},"mode":"overwrite","callableBy":["*"]}"#;
const DROP_TOKEN: &str = r#"{"dataCapability":"test.dropToken","owner":"dataguard","version":1,"effect":"insert","target":{"aggregate":"BattlemapInstance","id":"$tokenId"},"mode":"overwrite","callableBy":["*"]}"#;
const REMOVE_TOKEN: &str = r#"{"dataCapability":"test.removeToken","owner":"dataguard","version":1,"effect":"delete","target":{"aggregate":"BattlemapInstance","id":"$token"},"mode":"overwrite","callableBy":["*"]}"#;
const START_COMBAT: &str = r#"{"dataCapability":"test.startCombat","owner":"dataguard","version":1,"effect":"insert","target":{"aggregate":"BattlemapInstance","id":"$combatId"},"mode":"overwrite","callableBy":["*"]}"#;
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

/// A token of `character` on the tavern's instance; an NPC's tracks 11 HP.
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

/// The entry `id` of `token` in the tavern's combat, at `rank`: a PC's when
/// `roll` is none.
fn entry(id: &str, token: &str, roll: Option<i64>, initiative: i64, rank: i64) -> Value {
    json!({
        "initiativeEntryId": id,
        "initiativeEntryCombatId": COMBAT,
        "initiativeEntryTokenId": token,
        "roll": roll,
        "initiative": initiative,
        "dexScore": 10,
        "tiebreak": rank * 7,
        "rank": rank,
    })
}

/// A database of its own, migrated, holding Valombre, its battlemap, the
/// grid, the session and its two scenes, the ambush's instance in the tables alone.
struct Scratch {
    name: String,
    url: String,
    client: Client,
}

impl Scratch {
    async fn new(name: &str) -> Scratch {
        let name = format!("reorder_initiative_test_{name}_{}", std::process::id());
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
                     ('{ARRIVEE}', '{SEANCE}', 1, 'Arrivée'),
                     ('{EMBUSCADE}', '{SEANCE}', 2, 'Embuscade');
                 INSERT INTO battlemap_instance (id, battlemap_media_id, session_id, scene_id) VALUES
                     ('{TABLES_ONLY}', '{GUE}', '{SEANCE}', '{EMBUSCADE}');"
            ))
            .await
            .expect("Valombre");
        Scratch { name, url, client }
    }

    /// The engine as the server plants this unit and its target aggregate
    /// over this database, with the test-only DataCapabilities; and the
    /// tavern's instance, its four tokens — Vesna hidden — and its combat,
    /// Elowen's turn of round 1, ranked Elowen, the Bandit, Vesna, the Wolf.
    async fn serve(&self) -> Engine {
        let plugins: Plugins = [
            aggregate_battlemap_instance::plugin(),
            reorder_initiative::plugin(),
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
        .with_data_capability(manifest(OPEN_INSTANCE))
        .with_data_capability(manifest(DROP_TOKEN))
        .with_data_capability(manifest(REMOVE_TOKEN))
        .with_data_capability(manifest(START_COMBAT))
        .with_data_capability(manifest(ENTER))
        .with_data_capability(manifest(END_COMBAT))
        .with_plugins(plugins);
        engine.load().await.expect("the engine loads");
        submit(
            &engine,
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
            token(T_LOUP, LOUP, "Loup 1", true),
        ] {
            submit(&engine, "test.dropToken", token, None)
                .await
                .expect("a token");
        }
        submit(
            &engine,
            "test.startCombat",
            json!({
                "combatId": COMBAT,
                "combatInstanceId": INSTANCE,
                "round": 1,
                "currentEntryId": E_ELOWEN,
                "startedAt": "2026-10-21T10:05:00Z",
            }),
            None,
        )
        .await
        .expect("the combat");
        for entry in [
            entry(E_ELOWEN, T_ELOWEN, None, 18, 1),
            entry(E_BANDIT, T_BANDIT, Some(13), 13, 2),
            entry(E_VESNA, T_VESNA, Some(11), 11, 3),
            entry(E_LOUP, T_LOUP, Some(4), 4, 4),
        ] {
            submit(&engine, "test.enter", entry, None)
                .await
                .expect("an entry");
        }
        applied(&engine).await;
        engine
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

    /// Each entry's rank as the engine holds it: Elowen's, the Bandit's,
    /// Vesna's, the Wolf's.
    async fn ranks(&self) -> Vec<Value> {
        let mut ranks = Vec::new();
        for entry in [E_ELOWEN, E_BANDIT, E_VESNA, E_LOUP] {
            let row = self.row(&partition(entry)).await.expect("an entry");
            ranks.push(row["rank"].clone());
        }
        ranks
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

/// A reorder of `instance`, as the Battlemap sends it.
async fn reorder(engine: &Engine, instance: &str, order: &[&str]) -> Result<Submitted> {
    submit(
        engine,
        "table.reorderInitiative@1",
        json!({ "order": order }),
        Some(instance),
    )
    .await
}

/// The refusal `result` is, by its kind and its message.
fn refusal(result: Result<Submitted>) -> (RefusalKind, String) {
    match result {
        Err(Error::Refused(refusal)) => (refusal.kind, refusal.message),
        other => panic!("refused, not {other:?}"),
    }
}

/// The ranks the tavern's combat starts with.
fn planted() -> Vec<Value> {
    vec![json!(1), json!(2), json!(3), json!(4)]
}

#[tokio::test(flavor = "current_thread")]
async fn initiative_tiebreak_the_fourth_moved_first_ranks_1_to_4_in_the_new_order() {
    let scratch = Scratch::new("fourth_first").await;
    let engine = scratch.serve().await;

    let reordered = reorder(&engine, INSTANCE, &[T_LOUP, T_ELOWEN, T_BANDIT, T_VESNA])
        .await
        .expect("queued");
    // The command ranks the Wolf; each other entry is a follow-up of its own.
    assert_eq!(reordered.partition, partition(E_LOUP));
    assert!(reordered.warnings.is_empty(), "{:?}", reordered.warnings);
    let partitions: Vec<&str> = reordered
        .then
        .iter()
        .map(|follow_up| {
            let FollowUp::Submitted(rank) = follow_up else {
                panic!("a rank refused: {follow_up:?}");
            };
            assert!(rank.warnings.is_empty(), "{:?}", rank.warnings);
            rank.partition.as_str()
        })
        .collect();
    assert_eq!(
        partitions,
        [partition(E_ELOWEN), partition(E_BANDIT), partition(E_VESNA)]
    );
    applied(&engine).await;

    // Elowen 2nd, the Bandit 3rd, Vesna 4th, the Wolf 1st: no gap, no
    // duplicate.
    assert_eq!(
        scratch.ranks().await,
        [json!(2), json!(3), json!(4), json!(1)]
    );
    // The rank alone: the initiative, the draw and the links stay.
    let loup = scratch.row(&partition(E_LOUP)).await.expect("the Wolf");
    assert_eq!(
        loup,
        json!({
            "initiativeEntryId": E_LOUP,
            "initiativeEntryCombatId": COMBAT,
            "initiativeEntryTokenId": T_LOUP,
            "roll": 4,
            "initiative": 4,
            "dexScore": 10,
            "tiebreak": 28,
            "rank": 1,
        })
    );
    // The turn and the round stay with Elowen, in round 1.
    let combat = scratch.row(&partition(COMBAT)).await.expect("the combat");
    assert_eq!(combat["round"], 1);
    assert_eq!(combat["currentEntryId"], E_ELOWEN);
    scratch.drop().await;
}

#[tokio::test(flavor = "current_thread")]
async fn hidden_keeps_rank_the_hidden_participant_takes_the_rank_it_is_given_and_stays_hidden() {
    let scratch = Scratch::new("hidden").await;
    let engine = scratch.serve().await;

    // Vesna, hidden, first — named by her entry, the others by their token.
    reorder(&engine, INSTANCE, &[E_VESNA, T_BANDIT, T_ELOWEN, E_LOUP])
        .await
        .expect("queued");
    applied(&engine).await;

    assert_eq!(
        scratch.ranks().await,
        [json!(3), json!(2), json!(1), json!(4)]
    );
    let vesna = scratch.row(&partition(T_VESNA)).await.expect("Vesna");
    assert_eq!(vesna["visible"], false);
    scratch.drop().await;
}

#[tokio::test(flavor = "current_thread")]
async fn an_order_that_omits_or_repeats_a_participant_is_rejected_and_nothing_queued() {
    let scratch = Scratch::new("not_them").await;
    let engine = scratch.serve().await;

    for order in [
        // The Wolf omitted.
        &[T_ELOWEN, T_BANDIT, T_VESNA][..],
        // Elowen twice, the Wolf omitted.
        &[T_ELOWEN, T_BANDIT, T_VESNA, T_ELOWEN][..],
        // Elowen by her token, then by her entry.
        &[T_ELOWEN, T_BANDIT, T_VESNA, E_ELOWEN][..],
        // A stranger in the Wolf's place.
        &[T_ELOWEN, T_BANDIT, T_VESNA, NOWHERE][..],
    ] {
        let (kind, message) = refusal(reorder(&engine, INSTANCE, order).await);
        assert_eq!(kind, RefusalKind::Unprocessable, "{order:?}");
        assert!(message.contains("chaque participant"), "{message}");
    }
    assert!(engine.run_once().await.expect("a run").is_empty());
    assert_eq!(scratch.ranks().await, planted());
    scratch.drop().await;
}

#[tokio::test(flavor = "current_thread")]
async fn an_unknown_instance_is_not_found_and_one_with_no_open_combat_is_refused() {
    let scratch = Scratch::new("no_combat").await;
    let engine = scratch.serve().await;

    let (kind, _) = refusal(reorder(&engine, NOWHERE, &[T_ELOWEN]).await);
    assert_eq!(kind, RefusalKind::NotFound);
    // An instance the tables hold, with no combat.
    let (kind, message) = refusal(reorder(&engine, TABLES_ONLY, &[T_ELOWEN]).await);
    assert_eq!(kind, RefusalKind::Unprocessable);
    assert!(message.contains("pas de combat en cours"), "{message}");

    // An end queued, not yet applied, closes the combat already.
    submit(
        &engine,
        "test.endCombat",
        json!({ "endedAt": "2026-10-21T11:00:00Z", "currentEntryId": null }),
        Some(COMBAT),
    )
    .await
    .expect("the end");
    let all = [T_ELOWEN, T_BANDIT, T_VESNA, T_LOUP];
    let (kind, message) = refusal(reorder(&engine, INSTANCE, &all).await);
    assert_eq!(kind, RefusalKind::Unprocessable);
    assert!(message.contains("pas de combat en cours"), "{message}");
    // Applied, it is ended for good.
    applied(&engine).await;
    let (_, message) = refusal(reorder(&engine, INSTANCE, &all).await);
    assert!(message.contains("pas de combat en cours"), "{message}");
    assert_eq!(scratch.ranks().await, planted());
    scratch.drop().await;
}

#[tokio::test(flavor = "current_thread")]
async fn a_token_taken_off_the_map_is_no_participant_to_order() {
    let scratch = Scratch::new("removed").await;
    let engine = scratch.serve().await;
    submit(&engine, "test.removeToken", json!({}), Some(T_LOUP))
        .await
        .expect("removed");
    applied(&engine).await;

    let (_, message) =
        refusal(reorder(&engine, INSTANCE, &[T_LOUP, T_ELOWEN, T_BANDIT, T_VESNA]).await);
    assert!(message.contains("chaque participant"), "{message}");
    reorder(&engine, INSTANCE, &[T_VESNA, T_ELOWEN, T_BANDIT])
        .await
        .expect("queued");
    applied(&engine).await;

    let ranks: Vec<Value> = scratch.ranks().await.into_iter().take(3).collect();
    assert_eq!(ranks, [json!(2), json!(3), json!(1)]);
    scratch.drop().await;
}

/// A follow-up of the order `order` ranking `entry`, sent as the engine
/// sends one.
async fn follow_up(engine: &Engine, order: &[&str], entry: &str) -> Result<Submitted> {
    submit(
        engine,
        "table.reorderInitiative@1",
        json!({ "order": order, "entry": entry }),
        Some(INSTANCE),
    )
    .await
}

#[tokio::test(flavor = "current_thread")]
async fn a_follow_up_sent_alone_off_a_reorder_is_refused_and_one_in_sequence_ranks_its_entry() {
    let scratch = Scratch::new("follow_up").await;
    let engine = scratch.serve().await;
    let order = [E_LOUP, E_ELOWEN, E_BANDIT, E_VESNA];

    let (kind, message) = refusal(follow_up(&engine, &order, COMBAT).await);
    assert_eq!(kind, RefusalKind::Unprocessable);
    assert!(message.contains("chaque participant"), "{message}");
    // The Wolf still ranks 4th, not 1st: the Bandit's rank would follow no
    // reorder. The first entry is the command's, never a follow-up's.
    for entry in [E_BANDIT, E_LOUP] {
        let (kind, message) = refusal(follow_up(&engine, &order, entry).await);
        assert_eq!(kind, RefusalKind::Unprocessable);
        assert!(message.contains("précèdent"), "{message}");
    }
    assert!(engine.run_once().await.expect("a run").is_empty());

    // Elowen 1st and the Bandit 2nd already: in the order that puts the
    // Wolf before Vesna, the Wolf's follow-up is in sequence, then Vesna's,
    // each ranking its entry alone.
    let order = [E_ELOWEN, E_BANDIT, E_LOUP, E_VESNA];
    let (_, message) = refusal(follow_up(&engine, &order, E_VESNA).await);
    assert!(message.contains("précèdent"), "{message}");
    for entry in [E_LOUP, E_VESNA] {
        let ranked = follow_up(&engine, &order, entry).await.expect("queued");
        assert_eq!(ranked.partition, partition(entry));
        assert!(ranked.then.is_empty(), "{:?}", ranked.then);
    }
    applied(&engine).await;
    assert_eq!(
        scratch.ranks().await,
        [json!(1), json!(2), json!(4), json!(3)]
    );
    scratch.drop().await;
}

#[tokio::test(flavor = "current_thread")]
async fn a_reorder_queued_behind_another_asks_its_author_to_confirm_the_overwrite() {
    let scratch = Scratch::new("stale").await;
    let engine = scratch.serve().await;

    reorder(&engine, INSTANCE, &[T_LOUP, T_ELOWEN, T_BANDIT, T_VESNA])
        .await
        .expect("queued");
    let second = reorder(&engine, INSTANCE, &[T_BANDIT, T_LOUP, T_ELOWEN, T_VESNA])
        .await
        .expect("parked");
    assert_eq!(second.result.status, State::AwaitingConfirmation);
    // Its follow-ups follow its own ranks, which the queue holds while their
    // author decides.
    assert_eq!(second.then.len(), 3, "{:?}", second.then);
    for follow_up in &second.then {
        let FollowUp::Submitted(rank) = follow_up else {
            panic!("a rank refused: {follow_up:?}");
        };
        assert_eq!(rank.result.status, State::AwaitingConfirmation);
    }
    scratch.drop().await;
}
