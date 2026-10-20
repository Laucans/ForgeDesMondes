//! table.startCombat over PostgreSQL (`DATABASE_URL`): a start on a
//! battlemap instance the tables hold is judged against its tokens, their
//! sheets and the combats of the instance before it is queued, and the
//! combat it makes, once applied, is a row of `combat` with one row of
//! `initiative_entry` per token — an NPC's d20 plus its DEX modifier, a PC's
//! initiative the DM's (`npc-initiative-bounds`), ranked by initiative then
//! DEX (`initiative-tiebreak`), a hidden token like any other
//! (`hidden-keeps-rank`), at round 1 on rank 1, and never beside another
//! open combat (`one-active-combat`). A start refused queues nothing. Each
//! test works in a scratch database of its own, migrated by the Data layer,
//! dropped when it ends — but the one over the engine's memory.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use dataguard_server::{Engine, MEMORY, Planted, planted, router};
use harness_dataguard::command::State;
use harness_dataguard::manifest::DataCapability;
use serde_json::{Value, json};
use tokio_postgres::error::SqlState;
use tokio_postgres::{Client, NoTls};
use tower::ServiceExt;

const VALOMBRE: &str = "c1000000-0000-4000-8000-000000000001";
const ELOWEN: &str = "e1000000-0000-4000-8000-000000000001";
const KAEL: &str = "e1000000-0000-4000-8000-000000000002";
const BANDIT: &str = "e1000000-0000-4000-8000-000000000003";
const VESNA: &str = "e1000000-0000-4000-8000-000000000004";
const TAVERNE: &str = "e1000000-0000-4000-8000-000000000005";
const CARTE: &str = "6d000000-0000-4000-8000-000000000001";
const SEANCE: &str = "5e000000-0000-4000-8000-000000000001";
const ARRIVEE: &str = "5c000000-0000-4000-8000-000000000001";
const INSTANCE: &str = "b1a00000-0000-4000-8000-000000000001";
const NOWHERE: &str = "b1a00000-0000-4000-8000-0000000000ff";
const T_ELOWEN: &str = "70000000-0000-4000-8000-000000000001";
const T_BANDIT: &str = "70000000-0000-4000-8000-000000000003";
const T_VESNA: &str = "70000000-0000-4000-8000-000000000004";

fn url() -> String {
    std::env::var("DATABASE_URL").expect("DATABASE_URL names the harness's PostgreSQL")
}

fn migrations() -> PathBuf {
    Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../data/migrations"
    ))
    .to_path_buf()
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

async fn connect(config: &tokio_postgres::Config) -> Client {
    let (client, connection) = config.connect(NoTls).await.expect("PostgreSQL");
    tokio::spawn(connection);
    client
}

/// The SQL adding element `id` of the campaign, with the version every
/// element is written with, and its sheet for a PC or an NPC.
fn element(id: &str, kind: &str, name: &str, dex: Option<i16>) -> String {
    let sheet = if kind == "lieu" {
        String::new()
    } else {
        let dex = dex.map_or("NULL".to_string(), |dex| dex.to_string());
        format!(
            "INSERT INTO character (entity_id, token_color, hp_max, dexterity) \
             VALUES ('{id}', '#888', 20, {dex});"
        )
    };
    format!(
        "INSERT INTO entity (id, campaign_id, type, name) VALUES ('{id}', '{VALOMBRE}', '{kind}', '{name}'); \
         {sheet} \
         INSERT INTO entity_version (entity_id, version_number, source, snapshot, diff) \
         VALUES ('{id}', 1, 'dm', '{{}}', '[]');"
    )
}

/// The SQL adding a token of `character` on the instance.
fn token(id: &str, character: &str, name: &str, visible: bool, hp: Option<i16>) -> String {
    let hp = hp.map_or("NULL".to_string(), |hp| hp.to_string());
    format!(
        "INSERT INTO token (id, instance_id, character_id, name, q, r, visible, hp_current, hp_max, conditions) \
         VALUES ('{id}', '{INSTANCE}', '{character}', '{name}', 0, 0, {visible}, {hp}, {hp}, '{{}}');"
    )
}

/// A database of its own.
struct Scratch {
    client: Client,
    name: String,
    url: String,
}

impl Scratch {
    /// Migrated by the Data layer, holding the tavern's instance and its
    /// tokens as the tables hold them: Elowen, a PC of DEX 14, present; the
    /// Bandit, an NPC of DEX 12; Vesna, an NPC of DEX 16, hidden. Kael, a PC
    /// of no DEX, present, has no token yet.
    async fn new(name: &str) -> Scratch {
        let name = format!("start_combat_test_{name}_{}", std::process::id());
        let config: tokio_postgres::Config = url().parse().expect("a PostgreSQL URL");
        let admin = connect(&config).await;
        for sql in [
            format!("DROP DATABASE IF EXISTS {name} WITH (FORCE)"),
            format!("CREATE DATABASE {name}"),
        ] {
            admin.batch_execute(&sql).await.expect("a scratch database");
        }
        let mut client = connect(config.clone().dbname(&name)).await;
        data::migrate(&mut client, &migrations())
            .await
            .expect("migrated by the Data layer");
        client
            .batch_execute(&format!(
                "INSERT INTO campaign (id, name, kind) VALUES ('{VALOMBRE}', 'Les Cendres de Valombre', 'campagne');
                 {}{}{}{}{}
                 INSERT INTO media (id, campaign_id, kind, title, file_path, width_px, height_px, place_id)
                 VALUES ('{CARTE}', '{VALOMBRE}', 'battlemap', 'Taverne', 'carte.png', 2048, 1536, '{TAVERNE}');
                 INSERT INTO battlemap_grid (media_id, hex_size_px, offset_x, offset_y, orientation, color, opacity)
                 VALUES ('{CARTE}', 64, 0, 0, 'pointe', '#202020', 0.4);
                 INSERT INTO session (id, campaign_id, position, title, status, microphone_on)
                 VALUES ('{SEANCE}', '{VALOMBRE}', 1, 'Le Sanglier', 'live', true);
                 INSERT INTO session_present_character (session_id, character_id)
                 VALUES ('{SEANCE}', '{ELOWEN}'), ('{SEANCE}', '{KAEL}');
                 INSERT INTO scene (id, session_id, position, title) VALUES ('{ARRIVEE}', '{SEANCE}', 1, 'Arrivée');
                 INSERT INTO battlemap_instance (id, battlemap_media_id, session_id, scene_id)
                 VALUES ('{INSTANCE}', '{CARTE}', '{SEANCE}', '{ARRIVEE}');
                 {}{}{}",
                element(ELOWEN, "pj", "Elowen", Some(14)),
                element(KAEL, "pj", "Kael", None),
                element(BANDIT, "pnj", "Bandit", Some(12)),
                element(VESNA, "pnj", "Vesna", Some(16)),
                element(TAVERNE, "lieu", "La Taverne du Sanglier", None),
                token(T_ELOWEN, ELOWEN, "Elowen", true, None),
                token(T_BANDIT, BANDIT, "Bandit 1", true, Some(11)),
                token(T_VESNA, VESNA, "Vesna", false, Some(30)),
            ))
            .await
            .expect("the tavern's instance");
        let url = naming(&name);
        Scratch { client, name, url }
    }

    /// The engine as the server plants it over this database, with the
    /// test-only DataCapability that ends a combat — table.endCombat's
    /// stand-in —, and its wire protocol.
    async fn serve(&self) -> (Arc<Engine>, Router) {
        let Planted { engine, rows } = planted(&self.url)
            .await
            .expect("the engine over PostgreSQL");
        let manifest = json!({
            "dataCapability": "test.endCombat",
            "owner": "dataguard",
            "version": 1,
            "effect": "update",
            "target": { "aggregate": "BattlemapInstance", "id": "$combatId" },
            "mode": "overwrite",
            "callableBy": ["*"],
        });
        let engine = Arc::new(engine.with_data_capability(
            DataCapability::parse(&manifest.to_string()).expect("contract F"),
        ));
        let app = router(
            engine.clone(),
            rows,
            Arc::new(ports_llm::fake::FakeLlm::new()),
        );
        (engine, app)
    }

    /// The combats of the instance as `combat` holds them.
    async fn combats(&self) -> Vec<Value> {
        self.json(
            "SELECT jsonb_build_object('id', id, 'round', round, 'current', current_entry_id,
                 'started', started_at IS NOT NULL, 'ended', ended_at IS NOT NULL)::text
             FROM combat WHERE instance_id = $1::text::uuid ORDER BY started_at, id",
        )
        .await
    }

    /// The entries of the instance's combats, by rank.
    async fn entries(&self) -> Vec<Value> {
        self.json(
            "SELECT jsonb_build_object('id', e.id, 'combat', e.combat_id, 'token', e.token_id,
                 'roll', e.roll, 'initiative', e.initiative, 'dex', e.dex_score,
                 'tiebreak', e.tiebreak, 'rank', e.rank)::text
             FROM initiative_entry e JOIN combat c ON c.id = e.combat_id
             WHERE c.instance_id = $1::text::uuid ORDER BY c.started_at, e.rank",
        )
        .await
    }

    async fn json(&self, sql: &str) -> Vec<Value> {
        self.client
            .query(sql, &[&INSTANCE])
            .await
            .expect("the combat")
            .iter()
            .map(|row| serde_json::from_str(row.get::<_, &str>(0)).expect("JSON"))
            .collect()
    }

    async fn drop(self) {
        drop(self.client);
        let config: tokio_postgres::Config = url().parse().expect("a PostgreSQL URL");
        let admin = connect(&config).await;
        let sql = format!("DROP DATABASE {} WITH (FORCE)", self.name);
        for _ in 0..20 {
            match admin.batch_execute(&sql).await {
                Ok(()) => return,
                Err(e) if e.code() == Some(&SqlState::INSUFFICIENT_PRIVILEGE) => {
                    tokio::time::sleep(std::time::Duration::from_millis(250)).await;
                }
                Err(e) => panic!("the scratch database dropped: {e:?}"),
            }
        }
        panic!("the scratch database {} is still in use", self.name);
    }
}

/// `POST /commands` of `data_capability` on `target`: its status and answer.
async fn send(
    app: &Router,
    data_capability: &str,
    target: &str,
    payload: Value,
    key: Option<&str>,
) -> (StatusCode, Value) {
    let mut body = json!({
        "dataCapability": data_capability,
        "payload": payload,
        "target": { "id": target },
        "caller": { "by": "dm", "from": "table.Battlemap", "roles": ["role:approver"] },
    });
    if let Some(key) = key {
        body["idempotencyKey"] = json!(key);
    }
    let request = Request::post("/commands")
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    let response = app.clone().oneshot(request).await.expect("an answer");
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("a body");
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

/// table.startCombat on `instance`.
async fn start(app: &Router, instance: &str, payload: Value) -> (StatusCode, Value) {
    send(app, "table.startCombat@1", instance, payload, None).await
}

/// Runs the applier once, every result applied: how many commands it ran.
async fn applied(engine: &Engine) -> usize {
    let results = engine.run_once().await.expect("the applier ran");
    for result in &results {
        assert_eq!(result.status, State::Applied, "{result:?}");
    }
    results.len()
}

/// The entries' tokens, rank 1 first.
fn tokens_of(entries: &[Value]) -> Vec<&str> {
    entries
        .iter()
        .map(|e| e["token"].as_str().expect("a token"))
        .collect()
}

fn entry_of<'a>(entries: &'a [Value], token: &str) -> &'a Value {
    entries
        .iter()
        .find(|e| e["token"] == token)
        .expect("an entry")
}

#[tokio::test]
async fn initiative_tiebreak_the_pc_at_18_first_then_dex_16_before_dex_12() {
    let s = Scratch::new("tiebreak").await;
    let (engine, app) = s.serve().await;
    let (status, body) = start(
        &app,
        INSTANCE,
        json!({ "initiative": { ELOWEN: 18 }, "rolls": { T_BANDIT: 13, T_VESNA: 11 } }),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    // The combat, then one entry per token.
    assert_eq!(applied(&engine).await, 4);
    let entries = s.entries().await;
    assert_eq!(tokens_of(&entries), [T_ELOWEN, T_VESNA, T_BANDIT]);
    let ranks: Vec<&Value> = entries.iter().map(|e| &e["rank"]).collect();
    assert_eq!(ranks, [&json!(1), &json!(2), &json!(3)]);
    // npc-initiative-bounds: 13 + 1 and 11 + 3; the PC rolls nothing.
    let bandit = entry_of(&entries, T_BANDIT);
    assert_eq!(
        (&bandit["roll"], &bandit["initiative"], &bandit["dex"]),
        (&json!(13), &json!(14), &json!(12))
    );
    let vesna = entry_of(&entries, T_VESNA);
    assert_eq!(
        (&vesna["roll"], &vesna["initiative"], &vesna["dex"]),
        (&json!(11), &json!(14), &json!(16))
    );
    let elowen = entry_of(&entries, T_ELOWEN);
    assert_eq!(
        (&elowen["roll"], &elowen["initiative"], &elowen["dex"]),
        (&Value::Null, &json!(18), &json!(14))
    );
    // The combat opens at round 1 on rank 1.
    let combats = s.combats().await;
    assert_eq!(combats.len(), 1);
    assert_eq!(combats[0]["round"], 1);
    assert_eq!(combats[0]["current"], entries[0]["id"]);
    assert_eq!(
        (&combats[0]["started"], &combats[0]["ended"]),
        (&json!(true), &json!(false))
    );
    assert!(entries.iter().all(|e| e["combat"] == combats[0]["id"]));
    s.drop().await;
}

#[tokio::test]
async fn npc_initiative_bounds_an_npc_with_no_roll_sent_throws_a_d20() {
    let s = Scratch::new("bounds").await;
    let (engine, app) = s.serve().await;
    let (status, body) = start(&app, INSTANCE, json!({ "initiative": { ELOWEN: 18 } })).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    assert_eq!(applied(&engine).await, 4);
    let entries = s.entries().await;
    for (token, modifier) in [(T_BANDIT, 1), (T_VESNA, 3)] {
        let entry = entry_of(&entries, token);
        let roll = entry["roll"].as_i64().expect("an NPC rolls");
        assert!((1..=20).contains(&roll), "{entry}");
        assert_eq!(entry["initiative"], roll + modifier, "{entry}");
        assert!(entry["tiebreak"].is_number(), "{entry}");
    }
    assert_eq!(entry_of(&entries, T_ELOWEN)["roll"], Value::Null);
    s.drop().await;
}

#[tokio::test]
async fn hidden_keeps_rank_a_hidden_token_has_a_rank_like_any_other() {
    let s = Scratch::new("hidden").await;
    let (engine, app) = s.serve().await;
    let (status, body) = start(
        &app,
        INSTANCE,
        json!({ "initiative": { ELOWEN: 2 }, "rolls": { T_BANDIT: 5, T_VESNA: 20 } }),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    assert_eq!(applied(&engine).await, 4);
    let entries = s.entries().await;
    // Vesna, hidden, first at 23; the start left her hidden.
    assert_eq!(tokens_of(&entries), [T_VESNA, T_BANDIT, T_ELOWEN]);
    let hidden: bool = s
        .client
        .query_one(
            "SELECT visible FROM token WHERE id = $1::text::uuid",
            &[&T_VESNA],
        )
        .await
        .expect("Vesna's token")
        .get(0);
    assert!(!hidden);
    s.drop().await;
}

#[tokio::test]
async fn one_active_combat_a_second_start_is_refused_queued_or_applied() {
    let s = Scratch::new("one_active").await;
    let (engine, app) = s.serve().await;
    let payload = json!({ "initiative": { ELOWEN: 18 } });
    let (status, body) = start(&app, INSTANCE, payload.clone()).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    // Still queued: it is the instance's open combat already.
    let (status, body) = start(&app, INSTANCE, payload.clone()).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(applied(&engine).await, 4);
    let (status, body) = start(&app, INSTANCE, payload.clone()).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(applied(&engine).await, 0, "a start refused queues nothing");
    assert_eq!(s.combats().await.len(), 1);
    // Once it ended, the instance may fight again: the engine's row tells.
    let ended = s.combats().await[0]["id"]
        .as_str()
        .expect("an id")
        .to_string();
    let (status, body) = send(
        &app,
        "test.endCombat",
        &ended,
        json!({ "combatId": ended, "endedAt": "2026-10-20T21:00:00Z" }),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    assert_eq!(applied(&engine).await, 1);
    let (status, body) = start(&app, INSTANCE, payload).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    assert_eq!(applied(&engine).await, 4);
    assert_eq!(s.combats().await.len(), 2);
    s.drop().await;
}

#[tokio::test]
async fn a_token_still_queued_joins_the_combat() {
    let s = Scratch::new("queued").await;
    let (engine, app) = s.serve().await;
    let (status, body) = send(
        &app,
        "table.placeTokens@1",
        INSTANCE,
        json!({ "character": KAEL, "q": 2, "r": 1 }),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    let (status, body) = start(
        &app,
        INSTANCE,
        json!({ "initiative": { ELOWEN: 18, KAEL: 25 } }),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    // The token, four entries and the combat, applied in the order of their
    // partitions' names: whichever lands last, the table holds them all.
    assert_eq!(applied(&engine).await, 6);
    let entries = s.entries().await;
    assert_eq!(entries.len(), 4);
    // Kael has no DEX: his 25 ranks him first all the same.
    assert_eq!(entries[0]["initiative"], 25);
    assert_eq!(entries[0]["dex"], Value::Null);
    assert_eq!(s.combats().await[0]["current"], entries[0]["id"]);
    s.drop().await;
}

#[tokio::test]
async fn a_start_the_instance_cannot_hold_queues_nothing() {
    let s = Scratch::new("refused").await;
    let (engine, app) = s.serve().await;
    for (instance, payload, expected) in [
        // Elowen's initiative is not entered.
        (
            INSTANCE,
            json!({ "rolls": { T_BANDIT: 3 } }),
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
        // A PC rolls nothing.
        (
            INSTANCE,
            json!({ "initiative": { ELOWEN: 18 }, "rolls": { T_ELOWEN: 3 } }),
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
        (
            INSTANCE,
            json!({ "initiative": { ELOWEN: 18 }, "rolls": { T_BANDIT: 21 } }),
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
        (
            NOWHERE,
            json!({ "initiative": { ELOWEN: 18 } }),
            StatusCode::NOT_FOUND,
        ),
        (
            "la-taverne",
            json!({ "initiative": { ELOWEN: 18 } }),
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
    ] {
        let (status, body) = start(&app, instance, payload.clone()).await;
        assert_eq!(status, expected, "{payload}: {body}");
        assert!(body["error"].is_string(), "{body}");
    }
    assert_eq!(applied(&engine).await, 0);
    assert!(s.combats().await.is_empty());
    s.drop().await;
}

#[tokio::test]
async fn a_start_sent_again_under_its_key_is_replayed() {
    let s = Scratch::new("replay").await;
    let (engine, app) = s.serve().await;
    let payload = json!({ "initiative": { ELOWEN: 18 } });
    let (status, first) = send(
        &app,
        "table.startCombat@1",
        INSTANCE,
        payload.clone(),
        Some("k"),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{first}");
    assert_eq!(applied(&engine).await, 4);
    let (status, again) = send(&app, "table.startCombat@1", INSTANCE, payload, Some("k")).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{again}");
    assert_eq!(again["commandId"], first["commandId"]);
    assert_eq!(applied(&engine).await, 0);
    assert_eq!(s.combats().await.len(), 1);
    s.drop().await;
}

#[tokio::test]
async fn state_persisted_reset_a_combat_the_tables_lost_is_projected_again_at_the_next_start() {
    let s = Scratch::new("repair").await;
    {
        let (engine, app) = s.serve().await;
        let (status, body) = start(
            &app,
            INSTANCE,
            json!({ "initiative": { ELOWEN: 18 }, "rolls": { T_BANDIT: 13, T_VESNA: 11 } }),
        )
        .await;
        assert_eq!(status, StatusCode::ACCEPTED, "{body}");
        assert_eq!(applied(&engine).await, 4);
    }
    let (combats, entries) = (s.combats().await, s.entries().await);
    s.client
        .batch_execute("DELETE FROM combat")
        .await
        .expect("the combat lost, and its entries with it");
    assert!(s.entries().await.is_empty());
    let _ = s.serve().await;
    assert_eq!(s.combats().await, combats);
    assert_eq!(s.entries().await, entries);
    s.drop().await;
}

#[tokio::test]
async fn over_the_engine_s_memory_no_instance_is_known_and_nothing_is_queued() {
    let planted = planted(MEMORY).await.expect("the in-memory engine");
    let engine = Arc::new(planted.engine);
    let app = router(
        engine.clone(),
        planted.rows,
        Arc::new(ports_llm::fake::FakeLlm::new()),
    );
    let (status, body) = start(&app, INSTANCE, json!({ "initiative": { ELOWEN: 18 } })).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    let (status, body) = start(&app, INSTANCE, json!({ "rolls": { T_BANDIT: 0 } })).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(engine.run_once().await.expect("the applier ran").is_empty());
}

/// Dice that always throw `d20` and draw by token, in the lineup's order.
struct Loaded(i64, std::vec::IntoIter<i64>);

impl start_combat::Dice for Loaded {
    fn d20(&mut self) -> i64 {
        self.0
    }

    fn draw(&mut self) -> i64 {
        self.1.next().unwrap_or(0)
    }
}

#[tokio::test]
async fn the_rows_a_start_prepares_are_applied_by_the_dataguard_s_invariants() {
    use harness_dataguard::api::Submission;
    use harness_dataguard::guard::Caller;
    use start_combat::{Fresh, Lineup, Participant, prepare, request};
    let planted = planted(MEMORY).await.expect("the in-memory engine");
    let participant = |token: &str, character: &str, kind: &str, dex: Option<i64>| Participant {
        token: token.to_string(),
        character: character.to_string(),
        kind: Some(kind.to_string()),
        dex,
    };
    let lineup = Lineup {
        instance: true,
        tokens: vec![
            participant(T_ELOWEN, ELOWEN, "pj", None),
            participant(T_BANDIT, BANDIT, "pnj", Some(1)),
            participant(T_VESNA, VESNA, "pnj", None),
        ],
        open: false,
    };
    let wire = json!({ "initiative": { ELOWEN: -3 }, "rolls": { T_BANDIT: 1 } });
    let Value::Object(wire) = wire else {
        unreachable!()
    };
    let request = request(&wire.into_iter().collect(), Some(INSTANCE)).expect("a request");
    let entries: Vec<String> = (1..=3)
        .map(|i| format!("ee000000-0000-4000-8000-{i:012}"))
        .collect();
    let fresh = Fresh {
        combat: "c0000000-0000-4000-8000-000000000001",
        entries: &entries,
        now: "2026-10-20T20:05:00.000Z",
    };
    // DEX 1 rolling 1 is -4; DEX 10 by default rolling 20 is 20; the PC -3.
    let mut dice = Loaded(20, vec![5, 6, 2_147_483_647].into_iter());
    let prepared = prepare(&request, &lineup, fresh, &mut dice).expect("prepared");
    for (id, row) in std::iter::once(prepared.combat).chain(prepared.entries) {
        planted
            .engine
            .submit(Submission {
                data_capability: "table.startCombat@1".to_string(),
                payload: row,
                target_id: Some(id),
                based_on: None,
                idempotency_key: None,
                caller: Caller {
                    by: "dm".to_string(),
                    from: "table.Battlemap".to_string(),
                    permissions: Vec::new(),
                    roles: vec!["role:approver".to_string()],
                },
            })
            .await
            .expect("queued");
    }
    assert_eq!(applied(&planted.engine).await, 4);
    let ranks: Vec<(Value, Value)> = {
        let mut ranks = Vec::new();
        for id in &entries {
            let row = planted
                .rows
                .projected(&format!("BattlemapInstance/{id}"))
                .await
                .expect("the entry");
            ranks.push((
                row["initiativeEntryTokenId"].clone(),
                row["initiative"].clone(),
            ));
        }
        ranks
    };
    assert_eq!(
        ranks,
        [
            (json!(T_VESNA), json!(20)),
            (json!(T_ELOWEN), json!(-3)),
            (json!(T_BANDIT), json!(-4)),
        ]
    );
}
