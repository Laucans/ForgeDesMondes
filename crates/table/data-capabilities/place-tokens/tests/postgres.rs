//! table.placeTokens over PostgreSQL (`DATABASE_URL`), as the server plants
//! it beside its target aggregate: a monster dropped « × 3 » becomes three
//! tokens numbered 1 to 3, each on its own partition, judged by the
//! BattlemapInstance aggregate and applied; the next drop numbers after them,
//! applied or still queued, and after the tokens the tables held before
//! (`multi-drop-numbering`); an NPC's tokens copy its sheet's HP max, a PC's
//! track none (`pj-hp-not-tracked`); a PC absent from the session, or placed
//! already, is refused (`token-roster`); and what the tables do not hold is
//! refused before anything is queued. Each test works in a scratch database
//! of its own, migrated as the server migrates it, dropped when it ends.

use std::path::Path;
use std::sync::Arc;

use harness_dataguard::aggregate::Aggregate;
use harness_dataguard::api::{FollowUp, Submission, Submitted};
use harness_dataguard::command::State;
use harness_dataguard::engine::Engine;
use harness_dataguard::manifest::DataCapability;
use harness_dataguard::memory::{MemoryNotifier, MemoryStore};
use harness_dataguard::plugins::{Plugins, RefusalKind};
use harness_dataguard::postgres::{PgStore, ReadOnly, SystemClock};
use harness_dataguard::tokio_postgres::{self, Client, NoTls};
use harness_dataguard::{Error, Result};
use place_tokens::token_id;
use serde_json::{Value, json};

const VALOMBRE: &str = "c1000000-0000-4000-8000-000000000001";
const BRUME: &str = "c1000000-0000-4000-8000-000000000002";
const SEANCE: &str = "5e000000-0000-4000-8000-000000000001";
const SCENE: &str = "5c000000-0000-4000-8000-000000000001";
const AUTRE_SCENE: &str = "5c000000-0000-4000-8000-000000000002";
const GUE: &str = "d1000000-0000-4000-8000-00000000000a";
/// The instance the tables hold, and one only the engine wrote.
const INSTANCE: &str = "b1000000-0000-4000-8000-000000000001";
const ENGINE_INSTANCE: &str = "b1000000-0000-4000-8000-000000000002";
/// Two PCs — Elowen present at the session, Kael not — two NPCs of
/// Valombre, one with its tokens in the tables already, and an NPC of Brume.
const ELOWEN: &str = "e1000000-0000-4000-8000-000000000001";
const KAEL: &str = "e1000000-0000-4000-8000-000000000002";
const BANDIT: &str = "e1000000-0000-4000-8000-000000000003";
const LOUP: &str = "e1000000-0000-4000-8000-000000000004";
const NOYEE: &str = "e2000000-0000-4000-8000-000000000001";
/// A place of Valombre: no character.
const GUE_LIEU: &str = "e1000000-0000-4000-8000-000000000009";
const NOWHERE: &str = "e1000000-0000-4000-8000-0000000000ff";

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

fn manifest() -> DataCapability {
    DataCapability::parse(include_str!("../data-capability.json")).expect("contract F")
}

fn plugins() -> Plugins {
    [
        aggregate_battlemap_instance::plugin(),
        place_tokens::plugin(),
    ]
    .into_iter()
    .collect()
}

/// A database of its own, migrated, holding Valombre — a session with its
/// scene's instance, Elowen present at it, Kael not, the bandit and the
/// wolf, the wolf with two tokens on the instance already, and a place — and
/// Brume with its drowned woman; and an instance the engine alone wrote.
struct Scratch {
    name: String,
    url: String,
    client: Client,
}

impl Scratch {
    async fn new(name: &str) -> Scratch {
        let name = format!("place_tokens_test_{name}_{}", std::process::id());
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
        // The engine's own tables, which its store makes when it opens.
        PgStore::open(connect(&url).await)
            .await
            .expect("the engine's tables");
        client
            .batch_execute(&format!(
                "INSERT INTO campaign (id, name, kind) VALUES
                     ('{VALOMBRE}', 'Les Cendres de Valombre', 'campagne'),
                     ('{BRUME}', 'Les Brumes', 'campagne');
                 INSERT INTO session (id, campaign_id, position, title, status, microphone_on) VALUES
                     ('{SEANCE}', '{VALOMBRE}', 1, 'L''embuscade du Gué', 'prep', false);
                 INSERT INTO scene (id, session_id, position, title) VALUES
                     ('{SCENE}', '{SEANCE}', 1, 'Le gué'),
                     ('{AUTRE_SCENE}', '{SEANCE}', 2, 'La forêt');
                 INSERT INTO media (id, campaign_id, kind, title, file_path, width_px, height_px) VALUES
                     ('{GUE}', '{VALOMBRE}', 'battlemap', 'Le Gué des Corbeaux', 'media/gue.png', 960, 640);
                 INSERT INTO battlemap_grid (media_id, hex_size_px, offset_x, offset_y, orientation, color, opacity) VALUES
                     ('{GUE}', 30, 12, 8, 'pointe', 'text', 0.4);
                 INSERT INTO entity (id, campaign_id, type, name) VALUES
                     ('{ELOWEN}', '{VALOMBRE}', 'pj', 'Elowen Ventargent'),
                     ('{KAEL}', '{VALOMBRE}', 'pj', 'Kael Durand'),
                     ('{BANDIT}', '{VALOMBRE}', 'pnj', 'Bandit'),
                     ('{LOUP}', '{VALOMBRE}', 'pnj', ' Loup '),
                     ('{GUE_LIEU}', '{VALOMBRE}', 'lieu', 'Le Gué'),
                     ('{NOYEE}', '{BRUME}', 'pnj', 'La Noyée');
                 INSERT INTO character (entity_id, hp_max, token_color) VALUES
                     ('{ELOWEN}', 44, 'success'),
                     ('{KAEL}', 49, 'info'),
                     ('{BANDIT}', 11, 'danger'),
                     ('{LOUP}', 7, 'danger'),
                     ('{NOYEE}', 20, 'danger');
                 INSERT INTO entity_version (entity_id, version_number, source, snapshot, diff)
                     SELECT id, 1, 'dm', '{{}}', '[]' FROM entity;
                 INSERT INTO session_present_character (session_id, character_id) VALUES
                     ('{SEANCE}', '{ELOWEN}');
                 INSERT INTO battlemap_instance (id, battlemap_media_id, session_id, scene_id) VALUES
                     ('{INSTANCE}', '{GUE}', '{SEANCE}', '{SCENE}');
                 INSERT INTO token (id, instance_id, character_id, number, name, q, r, visible,
                         hp_current, hp_max, conditions) VALUES
                     (gen_random_uuid(), '{INSTANCE}', '{LOUP}', 1, 'Loup 1', 0, 0, true, 7, 7, '{{}}'),
                     (gen_random_uuid(), '{INSTANCE}', '{LOUP}', 2, 'Loup 2', 1, 0, true, 3, 7, '{{}}');
                 INSERT INTO dataguard_rows (partition, fields) VALUES
                     ('BattlemapInstance/{ENGINE_INSTANCE}', '{{
                         \"battlemapInstanceId\": \"{ENGINE_INSTANCE}\",
                         \"battlemapMediaId\": \"{GUE}\",
                         \"sessionId\": \"{SEANCE}\",
                         \"sceneId\": \"{AUTRE_SCENE}\",
                         \"createdAt\": \"2026-10-21T10:00:00Z\"}}');"
            ))
            .await
            .expect("Valombre and Brume");
        Scratch { name, url, client }
    }

    /// The engine as the server plants this unit and its target aggregate
    /// over this database, the tables lent to the Prepare.
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
        .with_data_capability(manifest())
        .with_plugins(plugins());
        engine.load().await.expect("the engine loads");
        engine
    }

    /// The tokens of `character` on `instance` the engine holds, each its
    /// row's fields, by number.
    async fn tokens(&self, instance: &str, character: &str) -> Vec<Value> {
        self.client
            .query(
                "SELECT fields::text FROM dataguard_rows
                 WHERE partition LIKE 'BattlemapInstance/%' AND NOT tombstoned
                     AND fields ->> 'tokenInstanceId' = $1 AND fields ->> 'characterId' = $2
                 ORDER BY (fields ->> 'number')::int NULLS FIRST",
                &[&instance, &character],
            )
            .await
            .expect("the engine's rows")
            .iter()
            .map(|row| serde_json::from_str(row.get::<_, &str>(0)).expect("JSON"))
            .collect()
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

/// A drop as the Battlemap sends it — by the DM, approver of their own
/// campaign: a token's ids, hex and HP are critical fields — on `instance`.
async fn post(engine: &Engine, instance: &str, payload: Value) -> Result<Submitted> {
    let body = json!({
        "dataCapability": "table.placeTokens@1",
        "target": { "aggregate": "BattlemapInstance", "id": instance },
        "payload": payload,
        "caller": { "by": "dm", "from": "Battlemap", "roles": ["role:approver"] },
    });
    engine
        .submit(serde_json::from_value::<Submission>(body).expect("a submission"))
        .await
}

/// Every command queued run by the applier, each applied.
async fn settle(engine: &Engine) {
    loop {
        let settled = engine.run_once().await.expect("the applier ran");
        if settled.is_empty() {
            return;
        }
        for result in settled {
            assert_eq!(result.status, State::Applied, "{result:?}");
        }
    }
}

/// A drop queued, with every follow-up it asked for, then applied.
async fn place(engine: &Engine, instance: &str, payload: Value) {
    let submitted = post(engine, instance, payload).await.expect("queued");
    for follow in &submitted.then {
        assert!(matches!(follow, FollowUp::Submitted(_)), "{follow:?}");
    }
    settle(engine).await;
}

/// The refusal `answer` is, of `kind`: its message.
fn refused(answer: Result<Submitted>, kind: RefusalKind) -> String {
    match answer {
        Err(Error::Refused(refusal)) if refusal.kind == kind => refusal.message,
        other => panic!("a {kind:?} refusal: {other:?}"),
    }
}

fn numbers(tokens: &[Value]) -> Vec<Value> {
    tokens.iter().map(|token| token["number"].clone()).collect()
}

fn dropped(character: &str, count: u16) -> Value {
    json!({ "character": character, "q": 4, "r": -1, "count": count })
}

#[tokio::test]
async fn multi_drop_numbering_a_monster_dropped_three_times_then_twice_is_numbered_1_to_5() {
    let scratch = Scratch::new("numbered").await;
    let engine = scratch.serve().await;

    place(&engine, INSTANCE, dropped(BANDIT, 3)).await;
    let tokens = scratch.tokens(INSTANCE, BANDIT).await;
    assert_eq!(numbers(&tokens), [json!(1), json!(2), json!(3)]);
    for (token, number) in tokens.iter().zip(1..) {
        assert_eq!(token["tokenId"], token_id(INSTANCE, BANDIT, Some(number)));
        assert_eq!(token["name"], format!("Bandit {number}"));
        assert_eq!((&token["q"], &token["r"]), (&json!(4), &json!(-1)));
        assert_eq!(token["visible"], true);
        assert_eq!(token["conditions"], json!([]));
    }

    place(&engine, INSTANCE, dropped(BANDIT, 2)).await;
    let tokens = scratch.tokens(INSTANCE, BANDIT).await;
    assert_eq!(
        numbers(&tokens),
        [json!(1), json!(2), json!(3), json!(4), json!(5)]
    );
    assert_eq!(tokens[4]["name"], "Bandit 5");
    // Another instance numbers its own tokens from 1.
    place(&engine, ENGINE_INSTANCE, dropped(BANDIT, 1)).await;
    assert_eq!(
        numbers(&scratch.tokens(ENGINE_INSTANCE, BANDIT).await),
        [json!(1)]
    );
    scratch.drop().await;
}

#[tokio::test]
async fn multi_drop_numbering_a_drop_numbers_after_the_ones_still_queued() {
    let scratch = Scratch::new("queued").await;
    let engine = scratch.serve().await;

    let first = post(&engine, INSTANCE, dropped(BANDIT, 3))
        .await
        .expect("queued");
    assert_eq!(first.then.len(), 2);
    let second = post(&engine, INSTANCE, dropped(BANDIT, 2))
        .await
        .expect("queued");
    assert_eq!(
        second.partition,
        format!("BattlemapInstance/{}", token_id(INSTANCE, BANDIT, Some(4)))
    );
    settle(&engine).await;
    assert_eq!(
        numbers(&scratch.tokens(INSTANCE, BANDIT).await),
        [json!(1), json!(2), json!(3), json!(4), json!(5)]
    );
    scratch.drop().await;
}

#[tokio::test]
async fn multi_drop_numbering_a_drop_numbers_after_the_tokens_the_tables_hold() {
    let scratch = Scratch::new("tables").await;
    let engine = scratch.serve().await;

    place(&engine, INSTANCE, dropped(LOUP, 2)).await;
    let tokens = scratch.tokens(INSTANCE, LOUP).await;
    assert_eq!(numbers(&tokens), [json!(3), json!(4)]);
    // The sheet's name, trimmed.
    assert_eq!(tokens[0]["name"], "Loup 3");
    scratch.drop().await;
}

// AC1: a token the engine tombstoned and the engine restarted, a drop that reuses
// its number is Applied and the tombstoned partition stays tombstoned.
// AC2, AC3: the tables hold no stale row of a tombstoned token, so the drop holds
// whether or not they carry UNIQUE (instance_id, character_id, number).
#[tokio::test]
async fn multi_drop_numbering_a_token_removed_leaves_its_number_free_and_its_tombstone_kept() {
    let scratch = Scratch::new("removed").await;
    let engine = scratch.serve().await;
    let tombstone = |partition: String| {
        let client = &scratch.client;
        async move {
            client
                .execute(
                    "UPDATE dataguard_rows SET tombstoned = true WHERE partition = $1",
                    &[&partition],
                )
                .await
                .expect("a tombstone");
        }
    };
    let removed = format!("BattlemapInstance/{}", token_id(INSTANCE, BANDIT, Some(2)));
    let elowen = format!("BattlemapInstance/{}", token_id(INSTANCE, ELOWEN, None));

    place(&engine, INSTANCE, dropped(BANDIT, 2)).await;
    place(&engine, INSTANCE, dropped(ELOWEN, 1)).await;
    tombstone(removed.clone()).await;
    tombstone(elowen.clone()).await;
    // The next start — the projection's warm — removes the tombstoned tokens
    // from the tables, which free their numbers there too.
    drop(engine);
    let engine = scratch.serve().await;
    place(&engine, INSTANCE, dropped(BANDIT, 1)).await;
    place(&engine, INSTANCE, dropped(ELOWEN, 1)).await;

    let tokens = scratch.tokens(INSTANCE, BANDIT).await;
    assert_eq!(numbers(&tokens), [json!(1), json!(2)]);
    assert_eq!(tokens[1]["hpCurrent"], 11);
    assert_eq!(scratch.tokens(INSTANCE, ELOWEN).await.len(), 1);
    // The new tokens have ids of their own: what was removed stays so.
    for partition in [removed, elowen] {
        let tombstoned: bool = scratch
            .client
            .query_one(
                "SELECT tombstoned FROM dataguard_rows WHERE partition = $1",
                &[&partition],
            )
            .await
            .expect("the removed token")
            .get(0);
        assert!(tombstoned, "{partition}");
    }
    scratch.drop().await;
}

#[tokio::test]
async fn pj_hp_not_tracked_an_npc_copies_its_sheet_s_hp_max_and_a_pc_tracks_none() {
    let scratch = Scratch::new("hp").await;
    let engine = scratch.serve().await;

    place(&engine, INSTANCE, dropped(BANDIT, 2)).await;
    for token in scratch.tokens(INSTANCE, BANDIT).await {
        assert_eq!(
            (&token["hpMax"], &token["hpCurrent"]),
            (&json!(11), &json!(11))
        );
    }
    place(&engine, INSTANCE, dropped(ELOWEN, 1)).await;
    let elowen = scratch.tokens(INSTANCE, ELOWEN).await;
    assert_eq!(elowen.len(), 1, "{elowen:?}");
    assert_eq!(elowen[0]["tokenId"], token_id(INSTANCE, ELOWEN, None));
    assert_eq!(elowen[0]["name"], "Elowen Ventargent");
    for field in ["hpMax", "hpCurrent", "number"] {
        assert_eq!(elowen[0][field], Value::Null, "{field}");
    }
    // The sheet itself is never written.
    let hp: i16 = scratch
        .client
        .query_one(
            "SELECT hp_max FROM character WHERE entity_id = $1::text::uuid",
            &[&BANDIT],
        )
        .await
        .expect("the sheet")
        .get(0);
    assert_eq!(hp, 11);
    scratch.drop().await;
}

#[tokio::test]
async fn token_roster_a_pc_absent_from_the_session_or_placed_already_is_refused() {
    let scratch = Scratch::new("roster").await;
    let engine = scratch.serve().await;

    let message = refused(
        post(&engine, INSTANCE, dropped(KAEL, 1)).await,
        RefusalKind::Unprocessable,
    );
    assert!(message.contains("token-roster"), "{message}");
    let message = refused(
        post(&engine, INSTANCE, dropped(ELOWEN, 2)).await,
        RefusalKind::Unprocessable,
    );
    assert!(message.contains("token-roster"), "{message}");
    // Queued and not yet applied.
    post(&engine, INSTANCE, dropped(ELOWEN, 1))
        .await
        .expect("queued");
    let message = refused(
        post(&engine, INSTANCE, dropped(ELOWEN, 1)).await,
        RefusalKind::Unprocessable,
    );
    assert!(message.contains("token-roster"), "{message}");
    settle(&engine).await;
    // Applied.
    let message = refused(
        post(&engine, INSTANCE, dropped(ELOWEN, 1)).await,
        RefusalKind::Unprocessable,
    );
    assert!(message.contains("token-roster"), "{message}");
    assert!(scratch.tokens(INSTANCE, KAEL).await.is_empty());
    assert_eq!(scratch.tokens(INSTANCE, ELOWEN).await.len(), 1);
    scratch.drop().await;
}

#[tokio::test]
async fn token_roster_a_pc_the_tables_hold_on_the_instance_is_refused() {
    let scratch = Scratch::new("tabled").await;
    scratch
        .client
        .batch_execute(&format!(
            "INSERT INTO token (id, instance_id, character_id, number, name, q, r, visible,
                 hp_current, hp_max, conditions)
             VALUES (gen_random_uuid(), '{INSTANCE}', '{ELOWEN}', NULL, 'Elowen', 0, 6, true,
                 NULL, NULL, '{{}}')"
        ))
        .await
        .expect("Elowen on the map");
    let engine = scratch.serve().await;

    let message = refused(
        post(&engine, INSTANCE, dropped(ELOWEN, 1)).await,
        RefusalKind::Unprocessable,
    );
    assert!(message.contains("token-roster"), "{message}");
    // On another instance of the session she is not placed yet.
    place(&engine, ENGINE_INSTANCE, dropped(ELOWEN, 1)).await;
    assert_eq!(scratch.tokens(ENGINE_INSTANCE, ELOWEN).await.len(), 1);
    scratch.drop().await;
}

#[tokio::test]
async fn token_roster_a_pc_made_present_by_the_engine_is_placed() {
    let scratch = Scratch::new("present").await;
    scratch
        .client
        .execute(
            "INSERT INTO dataguard_rows (partition, fields) VALUES ($1, $2::text::jsonb)",
            &[
                &format!("Session/{KAEL}"),
                &json!({
                    "sessionPresentCharacterSessionId": SEANCE.to_uppercase(),
                    "sessionPresentCharacterCharacterId": KAEL,
                })
                .to_string(),
            ],
        )
        .await
        .expect("Kael present");
    let engine = scratch.serve().await;

    place(&engine, INSTANCE, dropped(KAEL, 1)).await;
    assert_eq!(scratch.tokens(INSTANCE, KAEL).await.len(), 1);
    scratch.drop().await;
}

#[tokio::test]
async fn what_the_tables_do_not_hold_is_refused_before_it_is_queued() {
    let scratch = Scratch::new("refused").await;
    let engine = scratch.serve().await;

    for (instance, payload, kind, words) in [
        (
            INSTANCE,
            dropped(NOYEE, 1),
            RefusalKind::Unprocessable,
            "campagne",
        ),
        (
            INSTANCE,
            dropped(NOWHERE, 1),
            RefusalKind::NotFound,
            "personnage",
        ),
        (
            INSTANCE,
            dropped(GUE_LIEU, 1),
            RefusalKind::NotFound,
            "personnage",
        ),
        (
            NOWHERE,
            dropped(BANDIT, 1),
            RefusalKind::NotFound,
            "instance",
        ),
        (
            INSTANCE,
            dropped(BANDIT, 13),
            RefusalKind::Unprocessable,
            "12",
        ),
        (
            INSTANCE,
            json!({ "character": BANDIT, "q": "4", "r": 0 }),
            RefusalKind::Unprocessable,
            "q",
        ),
        (
            "bmi-gue",
            dropped(BANDIT, 1),
            RefusalKind::Unprocessable,
            "instance",
        ),
    ] {
        let message = refused(post(&engine, instance, payload.clone()).await, kind);
        assert!(message.contains(words), "{payload}: {message}");
    }
    assert!(scratch.tokens(INSTANCE, NOYEE).await.is_empty());
    assert!(scratch.tokens(INSTANCE, BANDIT).await.is_empty());
    scratch.drop().await;
}

#[tokio::test]
async fn what_the_engine_deleted_is_not_there_any_more() {
    let scratch = Scratch::new("tombstoned").await;
    let engine = scratch.serve().await;
    for partition in [
        format!("BattlemapInstance/{INSTANCE}"),
        format!("BattlemapInstance/{ENGINE_INSTANCE}"),
        format!("Entity/{BANDIT}"),
    ] {
        scratch
            .client
            .execute(
                "INSERT INTO dataguard_rows (partition, tombstoned) VALUES ($1, true)
                 ON CONFLICT (partition) DO UPDATE SET tombstoned = true",
                &[&partition],
            )
            .await
            .expect("a tombstone");
    }

    for (instance, character, words) in [
        (INSTANCE, LOUP, "instance"),
        (ENGINE_INSTANCE, LOUP, "instance"),
    ] {
        let message = refused(
            post(&engine, instance, dropped(character, 1)).await,
            RefusalKind::NotFound,
        );
        assert!(message.contains(words), "{instance}: {message}");
    }
    scratch.drop().await;

    let scratch = Scratch::new("entity_gone").await;
    scratch
        .client
        .execute(
            "INSERT INTO dataguard_rows (partition, tombstoned) VALUES ($1, true)",
            &[&format!("Entity/{BANDIT}")],
        )
        .await
        .expect("a tombstone");
    let engine = scratch.serve().await;
    let message = refused(
        post(&engine, INSTANCE, dropped(BANDIT, 1)).await,
        RefusalKind::NotFound,
    );
    assert!(message.contains("personnage"), "{message}");
    scratch.drop().await;
}

#[tokio::test]
async fn over_memory_no_table_holds_the_instance() {
    let engine = Engine::new(
        Arc::new(MemoryStore::default()),
        Arc::new(MemoryNotifier::default()),
        Arc::new(SystemClock),
    )
    .with_aggregate(aggregate())
    .with_data_capability(manifest())
    .with_plugins(plugins());
    engine.load().await.expect("the engine loads");

    let message = refused(
        post(&engine, INSTANCE, dropped(BANDIT, 1)).await,
        RefusalKind::NotFound,
    );
    assert!(message.contains("instance"), "{message}");
}
