//! The BattlemapInstance aggregate's tables over PostgreSQL
//! (`DATABASE_URL`), as the server's store runs its projection: what the
//! engine applies on `BattlemapInstance/<id>` reaches `battlemap_instance`,
//! `token`, `combat` and `initiative_entry` in the apply transaction,
//! whatever order the partitions of a run land in; a tombstone takes its row
//! out; what the tables refuse rejects the command whole; a start writes
//! again what the tables lost; a row only the tables hold is the engine's
//! row. The DataCapabilities here are test-only: each write is the row as
//! given. Each test works in a scratch database of its own, migrated as the
//! server migrates it, dropped when it ends.

use std::path::Path;
use std::sync::Arc;

use harness_dataguard::aggregate::Aggregate;
use harness_dataguard::api::Submission;
use harness_dataguard::command::{CommandResult, State};
use harness_dataguard::engine::Engine;
use harness_dataguard::manifest::DataCapability;
use harness_dataguard::memory::MemoryNotifier;
use harness_dataguard::plugins::Plugins;
use harness_dataguard::postgres::{PgStore, SystemClock};
use harness_dataguard::store::Store;
use harness_dataguard::tokio_postgres::{self, Client, NoTls};
use serde_json::{Value, json};

/// The test-only DataCapabilities: a row written whole, fields of a row
/// changed, a step of a combat's round, a row deleted.
const MANIFESTS: [(&str, &str, &str); 4] = [
    ("test.insert", "insert", "overwrite"),
    ("test.update", "update", "overwrite"),
    ("test.step", "update", "relative"),
    ("test.delete", "delete", "overwrite"),
];

const VALOMBRE: &str = "c1000000-0000-4000-8000-000000000001";
const GUE: &str = "d1000000-0000-4000-8000-00000000000a";
const SEANCE: &str = "5e000000-0000-4000-8000-000000000001";
const SCENE: &str = "5c000000-0000-4000-8000-000000000001";
/// A scene the tables do not hold.
const NO_SCENE: &str = "5c000000-0000-4000-8000-0000000000ff";
/// Valombre's PC and its NPC.
const ELOWEN: &str = "e1000000-0000-4000-8000-000000000001";
const BANDIT: &str = "e1000000-0000-4000-8000-000000000003";
/// A character the tables do not hold.
const NOBODY: &str = "e1000000-0000-4000-8000-0000000000ff";
const INSTANCE: &str = "b1000000-0000-4000-8000-000000000001";
/// An instance only the engine holds, on a scene the tables do not.
const ELSEWHERE: &str = "b1000000-0000-4000-8000-000000000002";
/// An instance only the tables hold.
const FIXTURE: &str = "b1000000-0000-4000-8000-0000000000aa";
/// Tokens: their partitions sort before the instance's, so a run applies
/// them first.
const T_ELOWEN: &str = "0a000000-0000-4000-8000-000000000001";
const T_BANDIT_1: &str = "0a000000-0000-4000-8000-000000000002";
const T_BANDIT_2: &str = "0a000000-0000-4000-8000-000000000003";
const COMBAT: &str = "cb000000-0000-4000-8000-000000000001";
/// Entries: their partitions sort before the combat's, so a run applies
/// them first.
const E_ELOWEN: &str = "1e000000-0000-4000-8000-000000000001";
const E_BANDIT_1: &str = "1e000000-0000-4000-8000-000000000002";
const E_BANDIT_2: &str = "1e000000-0000-4000-8000-000000000003";

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

fn plugins() -> Plugins {
    std::iter::once(aggregate_battlemap_instance::plugin()).collect()
}

/// A database of its own, migrated as the server migrates it, holding
/// Valombre, its session and scene, its battlemap of the ford with its grid,
/// Elowen (a PC)
/// and the Bandit (an NPC).
struct Scratch {
    name: String,
    url: String,
    client: Client,
}

impl Scratch {
    async fn new(name: &str) -> Scratch {
        let name = format!(
            "battlemap_instance_tables_test_{name}_{}",
            std::process::id()
        );
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
                 INSERT INTO session (id, campaign_id, position, title, status, microphone_on) VALUES
                     ('{SEANCE}', '{VALOMBRE}', 1, 'L''embuscade du Gué', 'live', false);
                 INSERT INTO scene (id, session_id, position, title) VALUES
                     ('{SCENE}', '{SEANCE}', 1, 'Le gué');
                 INSERT INTO media (id, campaign_id, kind, title, file_path, width_px, height_px) VALUES
                     ('{GUE}', '{VALOMBRE}', 'battlemap', 'Le Gué des Corbeaux', 'media/gue.png', 960, 640);
                 INSERT INTO battlemap_grid (media_id, hex_size_px, offset_x, offset_y, orientation, color, opacity) VALUES
                     ('{GUE}', 30, 12, 8, 'pointe', 'text', 0.4);
                 INSERT INTO entity (id, campaign_id, type, name) VALUES
                     ('{ELOWEN}', '{VALOMBRE}', 'pj', 'Elowen'),
                     ('{BANDIT}', '{VALOMBRE}', 'pnj', 'Bandit');
                 INSERT INTO character (entity_id, hp_max, token_color) VALUES
                     ('{ELOWEN}', 44, 'success'),
                     ('{BANDIT}', 11, 'danger');
                 INSERT INTO entity_version (entity_id, version_number, source, snapshot, diff)
                     SELECT id, 1, 'dm', '{{}}', '[]' FROM entity;"
            ))
            .await
            .expect("Valombre");
        Scratch { name, url, client }
    }

    /// The store as the server opens it over this database, the aggregate's
    /// projection warmed.
    async fn store(&self) -> Arc<PgStore> {
        Arc::new(
            PgStore::open(connect(&self.url).await)
                .await
                .expect("the store")
                .with_projections(&plugins())
                .await
                .expect("warmed"),
        )
    }

    /// The engine over [`Self::store`], with the aggregate and the
    /// test-only DataCapabilities.
    async fn serve(&self) -> Engine {
        let mut engine = Engine::new(
            self.store().await,
            Arc::new(MemoryNotifier::default()),
            Arc::new(SystemClock),
        )
        .with_aggregate(Aggregate::parse(include_str!("../aggregate.json")).expect("contract I"))
        .with_plugins(plugins());
        for (id, effect, mode) in MANIFESTS {
            let manifest = json!({
                "dataCapability": id, "owner": "dataguard", "version": 1, "effect": effect,
                "target": { "aggregate": "BattlemapInstance", "id": "$id" },
                "mode": mode, "callableBy": ["*"],
            });
            engine = engine.with_data_capability(
                DataCapability::parse(&manifest.to_string()).expect("contract F"),
            );
        }
        engine.load().await.expect("the engine loads");
        engine
    }

    /// The rows `sql` selects, each as JSON.
    async fn rows(&self, sql: &str) -> Vec<Value> {
        self.client
            .query(
                &format!("SELECT to_jsonb(selected)::text FROM ({sql}) selected"),
                &[],
            )
            .await
            .expect("the tables")
            .iter()
            .map(|row| serde_json::from_str(row.get::<_, &str>(0)).expect("JSON"))
            .collect()
    }

    /// The instances the tables hold.
    async fn instances(&self) -> Vec<Value> {
        self.rows(
            "SELECT id::text, battlemap_media_id::text AS media, session_id::text AS session, \
             scene_id::text AS scene, created_at = '2026-10-21T10:00:00Z' AS created \
             FROM battlemap_instance ORDER BY id",
        )
        .await
    }

    /// The tokens the tables hold, by id.
    async fn tokens(&self) -> Vec<Value> {
        self.rows(
            "SELECT id::text, instance_id::text AS instance, character_id::text AS character, \
             number, name, q, r, visible, hp_current, hp_max, conditions \
             FROM token ORDER BY id",
        )
        .await
    }

    /// The combats the tables hold.
    async fn combats(&self) -> Vec<Value> {
        self.rows(
            "SELECT id::text, instance_id::text AS instance, round, \
             current_entry_id::text AS current, started_at = '2026-10-21T10:05:00Z' AS started, \
             ended_at IS NOT NULL AS ended FROM combat ORDER BY id",
        )
        .await
    }

    /// The entries the tables hold, by combat and rank.
    async fn entries(&self) -> Vec<Value> {
        self.rows(
            "SELECT id::text, combat_id::text AS combat, token_id::text AS token, roll, \
             initiative, dex_score, tiebreak, rank FROM initiative_entry ORDER BY combat_id, rank",
        )
        .await
    }

    /// The engine's row on `id`'s partition, as JSON.
    async fn engine_row(&self, id: &str) -> Value {
        let fields: String = self
            .client
            .query_one(
                "SELECT fields::text FROM dataguard_rows WHERE partition = $1",
                &[&format!("BattlemapInstance/{id}")],
            )
            .await
            .expect("the engine's row")
            .get(0);
        serde_json::from_str(&fields).expect("JSON")
    }

    /// Whether the engine holds a live row on `id`'s partition.
    async fn engine_holds(&self, id: &str) -> bool {
        self.client
            .query_one(
                "SELECT EXISTS (SELECT 1 FROM dataguard_rows \
                 WHERE partition = $1 AND NOT tombstoned)",
                &[&format!("BattlemapInstance/{id}")],
            )
            .await
            .expect("the engine's rows")
            .get(0)
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

/// A command of `data_capability` writing `payload` on the row `id`,
/// queued.
async fn submit(engine: &Engine, data_capability: &str, id: &str, payload: Value) {
    let submission: Submission = serde_json::from_value(json!({
        "dataCapability": data_capability,
        "targetId": id,
        "payload": payload,
        "caller": { "by": "dm", "from": "test", "roles": ["role:approver"] },
    }))
    .expect("a submission");
    engine.submit(submission).await.expect("queued");
}

/// The commands queued, applied in one run: each one's result.
async fn run(engine: &Engine) -> Vec<CommandResult> {
    engine.run_once().await.expect("the applier ran")
}

/// Every command of the run applied.
fn applied(results: &[CommandResult]) {
    assert!(
        results.iter().all(|result| result.status == State::Applied),
        "{results:?}"
    );
}

/// The one command of the run rejected.
fn rejected(results: &[CommandResult]) {
    assert_eq!(results.len(), 1, "{results:?}");
    assert_eq!(results[0].status, State::Rejected, "{results:?}");
}

/// The tavern scene's instance of the ford.
fn instance() -> Value {
    json!({
        "battlemapInstanceId": INSTANCE, "battlemapMediaId": GUE, "sessionId": SEANCE,
        "sceneId": SCENE, "createdAt": "2026-10-21T10:00:00Z",
    })
}

/// A token of `character` on the instance; an NPC's tracks its HP.
fn token(id: &str, character: &str, number: Option<i64>, name: &str, hp: Option<i64>) -> Value {
    json!({
        "tokenId": id, "tokenInstanceId": INSTANCE, "characterId": character,
        "number": number, "name": name, "q": 1, "r": 2, "visible": true,
        "hpCurrent": hp, "hpMax": hp, "conditions": [],
    })
}

/// The combat on the instance, at round 1 on `current`.
fn combat(current: &str) -> Value {
    json!({
        "combatId": COMBAT, "combatInstanceId": INSTANCE, "round": 1,
        "currentEntryId": current, "startedAt": "2026-10-21T10:05:00Z",
    })
}

/// The entry of `token` in the combat, at `rank`; an NPC rolls.
fn entry(id: &str, token: &str, roll: Option<i64>, initiative: i64, rank: i64) -> Value {
    json!({
        "initiativeEntryId": id, "initiativeEntryCombatId": COMBAT,
        "initiativeEntryTokenId": token, "roll": roll, "initiative": initiative,
        "dexScore": 12, "tiebreak": 0.25, "rank": rank,
    })
}

/// A token's row as [`Scratch::tokens`] selects it.
fn tabled_token(
    id: &str,
    character: &str,
    (number, name): (Option<i64>, &str),
    (q, r): (i64, i64),
    visible: bool,
    hp: (Option<i64>, Option<i64>),
    conditions: &[&str],
) -> Value {
    json!({
        "id": id, "instance": INSTANCE, "character": character, "number": number,
        "name": name, "q": q, "r": r, "visible": visible, "hp_current": hp.0,
        "hp_max": hp.1, "conditions": conditions,
    })
}

/// An entry's row as [`Scratch::entries`] selects it.
fn tabled_entry(id: &str, token: &str, roll: Option<i64>, initiative: i64, rank: i64) -> Value {
    json!({
        "id": id, "combat": COMBAT, "token": token, "roll": roll, "initiative": initiative,
        "dex_score": 12, "tiebreak": 0.25, "rank": rank,
    })
}

/// The instance and its three tokens, in one run: the tokens' partitions
/// land first, before the table holds their instance.
async fn plant_tokens(engine: &Engine) {
    submit(engine, "test.insert", INSTANCE, instance()).await;
    for (id, character, number, name, hp) in [
        (T_ELOWEN, ELOWEN, None, "Elowen", None),
        (T_BANDIT_1, BANDIT, Some(1), "Bandit 1", Some(11)),
        (T_BANDIT_2, BANDIT, Some(2), "Bandit 2", Some(11)),
    ] {
        submit(
            engine,
            "test.insert",
            id,
            token(id, character, number, name, hp),
        )
        .await;
    }
    applied(&run(engine).await);
}

/// The combat with the Bandit 1 and Elowen seated, in one run: the entries'
/// partitions land first, before the table holds their combat, and the
/// combat names its current entry before the table holds it.
async fn plant_combat(engine: &Engine) {
    submit(engine, "test.insert", COMBAT, combat(E_BANDIT_1)).await;
    submit(
        engine,
        "test.insert",
        E_BANDIT_1,
        entry(E_BANDIT_1, T_BANDIT_1, Some(15), 16, 1),
    )
    .await;
    submit(
        engine,
        "test.insert",
        E_ELOWEN,
        entry(E_ELOWEN, T_ELOWEN, None, 14, 2),
    )
    .await;
    applied(&run(engine).await);
}

#[tokio::test]
async fn an_instance_created_is_in_battlemap_instance_once_applied() {
    let s = Scratch::new("instance").await;
    let engine = s.serve().await;
    submit(&engine, "test.insert", INSTANCE, instance()).await;
    applied(&run(&engine).await);
    assert_eq!(
        s.instances().await,
        [json!({
            "id": INSTANCE, "media": GUE, "session": SEANCE, "scene": SCENE, "created": true,
        })]
    );
    s.drop().await;
}

#[tokio::test]
async fn tokens_placed_moved_renamed_changed_and_removed_leave_the_instance_s_live_tokens() {
    let s = Scratch::new("tokens").await;
    let engine = s.serve().await;
    plant_tokens(&engine).await;
    assert_eq!(
        s.tokens().await,
        [
            tabled_token(
                T_ELOWEN,
                ELOWEN,
                (None, "Elowen"),
                (1, 2),
                true,
                (None, None),
                &[]
            ),
            tabled_token(
                T_BANDIT_1,
                BANDIT,
                (Some(1), "Bandit 1"),
                (1, 2),
                true,
                (Some(11), Some(11)),
                &[]
            ),
            tabled_token(
                T_BANDIT_2,
                BANDIT,
                (Some(2), "Bandit 2"),
                (1, 2),
                true,
                (Some(11), Some(11)),
                &[]
            ),
        ]
    );
    // Moved, renamed, hidden, wounded and poisoned; then the second bandit
    // off the map.
    for (id, payload) in [
        (T_ELOWEN, json!({ "q": 4, "r": -3 })),
        (T_BANDIT_1, json!({ "name": "Chef bandit" })),
        (T_BANDIT_1, json!({ "visible": false })),
        (T_BANDIT_1, json!({ "hpCurrent": 4 })),
        (
            T_BANDIT_1,
            json!({ "conditions": ["empoisonné", "à terre"] }),
        ),
    ] {
        submit(&engine, "test.update", id, payload).await;
        applied(&run(&engine).await);
    }
    submit(&engine, "test.delete", T_BANDIT_2, json!({})).await;
    applied(&run(&engine).await);
    assert_eq!(
        s.tokens().await,
        [
            tabled_token(
                T_ELOWEN,
                ELOWEN,
                (None, "Elowen"),
                (4, -3),
                true,
                (None, None),
                &[]
            ),
            tabled_token(
                T_BANDIT_1,
                BANDIT,
                (Some(1), "Chef bandit"),
                (1, 2),
                false,
                (Some(4), Some(11)),
                &["empoisonné", "à terre"]
            ),
        ]
    );
    // The table derives what the players see.
    assert_eq!(
        s.rows("SELECT dead, health_band, label_initial FROM token ORDER BY id")
            .await,
        [
            json!({ "dead": false, "health_band": null, "label_initial": "E" }),
            json!({ "dead": false, "health_band": "en sang", "label_initial": "C" }),
        ]
    );
    s.drop().await;
}

#[tokio::test]
async fn a_combat_started_joined_left_turned_and_ended_holds_its_order_by_rank() {
    let s = Scratch::new("combat").await;
    let engine = s.serve().await;
    plant_tokens(&engine).await;
    plant_combat(&engine).await;
    assert_eq!(
        s.combats().await,
        [json!({
            "id": COMBAT, "instance": INSTANCE, "round": 1, "current": E_BANDIT_1,
            "started": true, "ended": false,
        })]
    );
    assert_eq!(
        s.entries().await,
        [
            tabled_entry(E_BANDIT_1, T_BANDIT_1, Some(15), 16, 1),
            tabled_entry(E_ELOWEN, T_ELOWEN, None, 14, 2),
        ]
    );
    // The second bandit joins between them, Elowen moves down a rank.
    submit(
        &engine,
        "test.insert",
        E_BANDIT_2,
        entry(E_BANDIT_2, T_BANDIT_2, Some(14), 15, 2),
    )
    .await;
    submit(&engine, "test.update", E_ELOWEN, json!({ "rank": 3 })).await;
    applied(&run(&engine).await);
    // « Suivant », then again past the last: round 2.
    for (round, current) in [(0, E_BANDIT_2), (0, E_ELOWEN), (1, E_BANDIT_1)] {
        submit(
            &engine,
            "test.step",
            COMBAT,
            json!({ "round": round, "currentEntryId": current }),
        )
        .await;
        applied(&run(&engine).await);
    }
    assert_eq!(
        s.combats().await,
        [json!({
            "id": COMBAT, "instance": INSTANCE, "round": 2, "current": E_BANDIT_1,
            "started": true, "ended": false,
        })]
    );
    assert_eq!(
        s.entries().await,
        [
            tabled_entry(E_BANDIT_1, T_BANDIT_1, Some(15), 16, 1),
            tabled_entry(E_BANDIT_2, T_BANDIT_2, Some(14), 15, 2),
            tabled_entry(E_ELOWEN, T_ELOWEN, None, 14, 3),
        ]
    );
    // The bandit whose turn it is leaves the order, his token stays; the
    // order view closes up.
    submit(&engine, "test.delete", E_BANDIT_1, json!({})).await;
    applied(&run(&engine).await);
    assert_eq!(
        s.entries().await,
        [
            tabled_entry(E_BANDIT_2, T_BANDIT_2, Some(14), 15, 2),
            tabled_entry(E_ELOWEN, T_ELOWEN, None, 14, 3),
        ]
    );
    assert_eq!(s.tokens().await.len(), 3);
    assert_eq!(
        s.rows(
            "SELECT entry_id::text, order_index FROM initiative_order_view ORDER BY order_index"
        )
        .await,
        [
            json!({ "entry_id": E_BANDIT_2, "order_index": 1 }),
            json!({ "entry_id": E_ELOWEN, "order_index": 2 }),
        ]
    );
    // The turn moves on to the next one.
    submit(
        &engine,
        "test.update",
        COMBAT,
        json!({ "currentEntryId": E_BANDIT_2 }),
    )
    .await;
    applied(&run(&engine).await);
    assert_eq!(s.combats().await[0]["current"], json!(E_BANDIT_2));
    // A token off the map takes its entry out of the order.
    submit(&engine, "test.delete", T_BANDIT_2, json!({})).await;
    applied(&run(&engine).await);
    assert_eq!(
        s.entries().await,
        [tabled_entry(E_ELOWEN, T_ELOWEN, None, 14, 3)]
    );
    assert_eq!(s.combats().await[0]["current"], Value::Null);
    // The end: no turn is current, the tokens stay.
    submit(
        &engine,
        "test.update",
        COMBAT,
        json!({ "endedAt": "2026-10-21T11:00:00Z", "currentEntryId": null }),
    )
    .await;
    applied(&run(&engine).await);
    assert_eq!(
        s.combats().await,
        [json!({
            "id": COMBAT, "instance": INSTANCE, "round": 2, "current": null,
            "started": true, "ended": true,
        })]
    );
    // A reset: entries, combat and tokens out; the instance stays.
    for id in [E_ELOWEN, COMBAT, T_ELOWEN, T_BANDIT_1] {
        submit(&engine, "test.delete", id, json!({})).await;
    }
    applied(&run(&engine).await);
    assert_eq!(s.entries().await, Vec::<Value>::new());
    assert_eq!(s.combats().await, Vec::<Value>::new());
    assert_eq!(s.tokens().await, Vec::<Value>::new());
    assert_eq!(s.instances().await.len(), 1);
    s.drop().await;
}

#[tokio::test]
async fn a_write_the_tables_refuse_is_rejected_and_neither_the_engine_nor_the_tables_change() {
    let s = Scratch::new("refused").await;
    let engine = s.serve().await;
    // A scene the tables do not hold: a foreign key (class 23).
    let mut elsewhere = instance();
    elsewhere["sceneId"] = json!(NO_SCENE);
    submit(&engine, "test.insert", INSTANCE, elsewhere).await;
    rejected(&run(&engine).await);
    assert!(!s.engine_holds(INSTANCE).await);
    assert_eq!(s.instances().await, Vec::<Value>::new());
    // A creation no time is: a value no column takes (class 22).
    let mut untimed = instance();
    untimed["createdAt"] = json!("mardi soir");
    submit(&engine, "test.insert", INSTANCE, untimed).await;
    rejected(&run(&engine).await);
    assert!(!s.engine_holds(INSTANCE).await);
    assert_eq!(s.instances().await, Vec::<Value>::new());
    // A character the tables do not hold: a deferred foreign key, judged at
    // once.
    submit(&engine, "test.insert", INSTANCE, instance()).await;
    applied(&run(&engine).await);
    submit(
        &engine,
        "test.insert",
        T_BANDIT_1,
        token(T_BANDIT_1, NOBODY, Some(1), "Bandit 1", Some(11)),
    )
    .await;
    rejected(&run(&engine).await);
    assert!(!s.engine_holds(T_BANDIT_1).await);
    assert_eq!(s.tokens().await, Vec::<Value>::new());
    // A change the tables refuse leaves the row as it was, in both: an end
    // no time is, which the judge lets through.
    submit(&engine, "test.insert", COMBAT, combat(E_BANDIT_1)).await;
    applied(&run(&engine).await);
    submit(
        &engine,
        "test.update",
        COMBAT,
        json!({ "endedAt": "jamais" }),
    )
    .await;
    rejected(&run(&engine).await);
    assert_eq!(s.engine_row(COMBAT).await.get("endedAt"), None);
    assert_eq!(s.combats().await[0]["ended"], json!(false));
    s.drop().await;
}

#[tokio::test]
async fn a_start_projects_again_what_the_tables_lost() {
    let s = Scratch::new("warm").await;
    let engine = s.serve().await;
    plant_tokens(&engine).await;
    plant_combat(&engine).await;
    submit(&engine, "test.delete", T_BANDIT_2, json!({})).await;
    applied(&run(&engine).await);
    let (tokens, combats, entries) = (s.tokens().await, s.combats().await, s.entries().await);
    drop(engine);
    // The tables lose the instance and all that cascades from it; they keep
    // the token the engine took off the map, on an instance only they hold;
    // the engine holds an instance on a scene the tables do not, which they
    // refuse.
    s.client
        .batch_execute(&format!(
            "DELETE FROM battlemap_instance;
             INSERT INTO battlemap_instance (id, battlemap_media_id, session_id, scene_id)
                 VALUES ('{FIXTURE}', '{GUE}', '{SEANCE}', '{SCENE}');
             INSERT INTO token (id, instance_id, character_id, name, q, r, visible, conditions)
                 VALUES ('{T_BANDIT_2}', '{FIXTURE}', '{BANDIT}', 'Bandit 2', 0, 0, true, '{{}}');
             INSERT INTO dataguard_rows (partition, fields) VALUES
                 ('BattlemapInstance/{ELSEWHERE}', '{{\"battlemapInstanceId\": \"{ELSEWHERE}\",
                    \"battlemapMediaId\": \"{GUE}\", \"sessionId\": \"{SEANCE}\",
                    \"sceneId\": \"{NO_SCENE}\"}}');"
        ))
        .await
        .expect("the tables lost");
    let _warmed = s.serve().await;
    assert_eq!(s.tokens().await, tokens);
    assert_eq!(s.combats().await, combats);
    assert_eq!(s.entries().await, entries);
    assert_eq!(
        s.rows("SELECT id::text FROM battlemap_instance ORDER BY id")
            .await,
        [json!({ "id": INSTANCE }), json!({ "id": FIXTURE })]
    );
    s.drop().await;
}

#[tokio::test]
async fn a_start_writes_again_what_the_tables_hold_otherwise() {
    let s = Scratch::new("stale").await;
    let engine = s.serve().await;
    plant_tokens(&engine).await;
    plant_combat(&engine).await;
    submit(
        &engine,
        "test.update",
        COMBAT,
        json!({ "endedAt": "2026-10-21T11:00:00Z", "currentEntryId": null }),
    )
    .await;
    applied(&run(&engine).await);
    let (tokens, combats, entries) = (s.tokens().await, s.combats().await, s.entries().await);
    drop(engine);
    // The tables hold a token elsewhere and hurt, the combat running again
    // at another round, an entry at another rank.
    s.client
        .batch_execute(&format!(
            "UPDATE token SET q = 9, hp_current = 2 WHERE id = '{T_BANDIT_1}';
             UPDATE combat SET round = 5, ended_at = NULL, current_entry_id = '{E_ELOWEN}';
             UPDATE initiative_entry SET rank = 7 WHERE id = '{E_BANDIT_1}';"
        ))
        .await
        .expect("the tables changed");
    let _warmed = s.serve().await;
    assert_eq!(s.tokens().await, tokens);
    assert_eq!(s.combats().await, combats);
    assert_eq!(s.entries().await, entries);
    s.drop().await;
}

#[tokio::test]
async fn a_row_waiting_on_its_parent_that_the_tables_refuse_leaves_the_parent_written() {
    let s = Scratch::new("waiting").await;
    let engine = s.serve().await;
    // In one run, two tokens land before their instance: one of a character
    // the tables do not hold.
    submit(&engine, "test.insert", INSTANCE, instance()).await;
    submit(
        &engine,
        "test.insert",
        T_BANDIT_1,
        token(T_BANDIT_1, NOBODY, Some(1), "Inconnu 1", Some(11)),
    )
    .await;
    submit(
        &engine,
        "test.insert",
        T_BANDIT_2,
        token(T_BANDIT_2, BANDIT, Some(2), "Bandit 2", Some(11)),
    )
    .await;
    applied(&run(&engine).await);
    assert_eq!(s.instances().await.len(), 1);
    assert_eq!(
        s.rows("SELECT id::text FROM token").await,
        [json!({ "id": T_BANDIT_2 })]
    );
    s.drop().await;
}

#[tokio::test]
async fn a_start_over_tables_not_migrated_yet_projects_nothing() {
    let s = Scratch::new("bare").await;
    s.client
        .batch_execute("DROP VIEW initiative_order_view; DROP TABLE initiative_entry CASCADE")
        .await
        .expect("the tables unmigrated");
    let _warmed = s.serve().await;
    s.drop().await;
}

#[tokio::test]
async fn a_row_only_the_tables_hold_is_the_engine_s_row_until_a_command_writes_it() {
    let s = Scratch::new("hydrate").await;
    s.client
        .batch_execute(&format!(
            "INSERT INTO battlemap_instance (id, battlemap_media_id, session_id, scene_id, created_at)
                 VALUES ('{INSTANCE}', '{GUE}', '{SEANCE}', '{SCENE}', '2026-10-21T10:00:00Z');
             INSERT INTO token (id, instance_id, character_id, number, name, q, r, visible,
                     hp_current, hp_max, conditions)
                 VALUES ('{T_BANDIT_1}', '{INSTANCE}', '{BANDIT}', 1, 'Bandit 1', 3, 4, true, 6, 11,
                     '{{à terre}}');
             INSERT INTO combat (id, instance_id, round, started_at)
                 VALUES ('{COMBAT}', '{INSTANCE}', 2, '2026-10-21T10:05:00Z');
             INSERT INTO initiative_entry (id, combat_id, token_id, roll, initiative, dex_score,
                     tiebreak, rank)
                 VALUES ('{E_BANDIT_1}', '{COMBAT}', '{T_BANDIT_1}', 15, 16, 12, 0.25, 1);
             UPDATE combat SET current_entry_id = '{E_BANDIT_1}';"
        ))
        .await
        .expect("the fixtures");
    let store = s.store().await;
    let row = |id: &str| {
        let store = store.clone();
        let partition = format!("BattlemapInstance/{}", id.to_uppercase());
        async move {
            Value::Object(
                store
                    .row(&partition)
                    .await
                    .expect("the row")
                    .into_iter()
                    .collect(),
            )
        }
    };
    let instance = row(INSTANCE).await;
    assert_eq!(instance["battlemapInstanceId"], json!(INSTANCE));
    assert_eq!(instance["sceneId"], json!(SCENE));
    assert!(instance.get("resetAt").is_none(), "{instance}");
    assert_eq!(
        row(T_BANDIT_1).await,
        json!({
            "tokenId": T_BANDIT_1, "tokenInstanceId": INSTANCE, "characterId": BANDIT,
            "number": 1, "name": "Bandit 1", "q": 3, "r": 4, "visible": true,
            "hpCurrent": 6, "hpMax": 11, "conditions": ["à terre"],
        })
    );
    let combat = row(COMBAT).await;
    assert_eq!(combat["round"], json!(2));
    assert_eq!(combat["currentEntryId"], json!(E_BANDIT_1));
    assert_eq!(
        row(E_BANDIT_1).await,
        json!({
            "initiativeEntryId": E_BANDIT_1, "initiativeEntryCombatId": COMBAT,
            "initiativeEntryTokenId": T_BANDIT_1, "roll": 15, "initiative": 16,
            "dexScore": 12, "tiebreak": 0.25, "rank": 1,
        })
    );
    assert_eq!(row(T_ELOWEN).await, json!({}));
    // A command on the fixture's token writes the whole row.
    let engine = s.serve().await;
    submit(&engine, "test.update", T_BANDIT_1, json!({ "q": 7 })).await;
    applied(&run(&engine).await);
    assert_eq!(
        s.tokens().await,
        [tabled_token(
            T_BANDIT_1,
            BANDIT,
            (Some(1), "Bandit 1"),
            (7, 4),
            true,
            (Some(6), Some(11)),
            &["à terre"]
        )]
    );
    s.drop().await;
}
