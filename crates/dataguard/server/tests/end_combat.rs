//! table.endCombat over PostgreSQL (`DATABASE_URL`): an end on a battlemap
//! instance is judged against the instance's combat still open — applied or
//! queued — before it is queued, and once applied the combat's row of
//! `combat` holds its end and no current entry, while the instance's tokens,
//! their HP, their conditions and the combat's entries stay as they were. An
//! instance with no combat open ends none, and once ended a new combat may
//! start (`one-active-combat`). An end refused queues nothing. Each test
//! works in a scratch database of its own, migrated by the Data layer,
//! dropped when it ends — but the one over the engine's memory.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use dataguard_server::{Engine, MEMORY, Planted, planted, router};
use harness_dataguard::command::State;
use serde_json::{Value, json};
use tokio_postgres::error::SqlState;
use tokio_postgres::{Client, NoTls};
use tower::ServiceExt;

const VALOMBRE: &str = "c1000000-0000-4000-8000-000000000001";
const ELOWEN: &str = "e1000000-0000-4000-8000-000000000001";
const BANDIT: &str = "e1000000-0000-4000-8000-000000000003";
const TAVERNE: &str = "e1000000-0000-4000-8000-000000000005";
const CARTE: &str = "6d000000-0000-4000-8000-000000000001";
const SEANCE: &str = "5e000000-0000-4000-8000-000000000001";
const ARRIVEE: &str = "5c000000-0000-4000-8000-000000000001";
const INSTANCE: &str = "b1a00000-0000-4000-8000-000000000001";
const T_ELOWEN: &str = "70000000-0000-4000-8000-000000000001";
const T_BANDIT_1: &str = "70000000-0000-4000-8000-000000000003";
const T_BANDIT_2: &str = "70000000-0000-4000-8000-000000000004";

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
fn element(id: &str, kind: &str, name: &str) -> String {
    let sheet = if kind == "lieu" {
        String::new()
    } else {
        format!(
            "INSERT INTO character (entity_id, token_color, hp_max, dexterity) \
             VALUES ('{id}', '#888', 11, 12);"
        )
    };
    format!(
        "INSERT INTO entity (id, campaign_id, type, name) VALUES ('{id}', '{VALOMBRE}', '{kind}', '{name}'); \
         {sheet} \
         INSERT INTO entity_version (entity_id, version_number, source, snapshot, diff) \
         VALUES ('{id}', 1, 'dm', '{{}}', '[]');"
    )
}

/// The SQL adding a token of `character` on the instance, wounded and
/// marked as the fight left it.
fn token(id: &str, character: &str, name: &str, hp: Option<i16>, conditions: &str) -> String {
    let hp_max = hp.map_or("NULL".to_string(), |_| "11".to_string());
    let hp = hp.map_or("NULL".to_string(), |hp| hp.to_string());
    format!(
        "INSERT INTO token (id, instance_id, character_id, name, q, r, visible, hp_current, hp_max, conditions) \
         VALUES ('{id}', '{INSTANCE}', '{character}', '{name}', 3, 2, true, {hp}, {hp_max}, '{conditions}');"
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
    /// tokens as the tables hold them: Elowen, a PC, present; Bandit 1, an
    /// NPC at 4 HP of 11, poisoned; Bandit 2, dead.
    async fn new(name: &str) -> Scratch {
        let name = format!("end_combat_test_{name}_{}", std::process::id());
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
                 {}{}{}
                 INSERT INTO media (id, campaign_id, kind, title, file_path, width_px, height_px, place_id)
                 VALUES ('{CARTE}', '{VALOMBRE}', 'battlemap', 'Taverne', 'carte.png', 2048, 1536, '{TAVERNE}');
                 INSERT INTO battlemap_grid (media_id, hex_size_px, offset_x, offset_y, orientation, color, opacity)
                 VALUES ('{CARTE}', 64, 0, 0, 'pointe', '#202020', 0.4);
                 INSERT INTO session (id, campaign_id, position, title, status, microphone_on)
                 VALUES ('{SEANCE}', '{VALOMBRE}', 1, 'Le Sanglier', 'live', true);
                 INSERT INTO session_present_character (session_id, character_id)
                 VALUES ('{SEANCE}', '{ELOWEN}');
                 INSERT INTO scene (id, session_id, position, title) VALUES ('{ARRIVEE}', '{SEANCE}', 1, 'Arrivée');
                 INSERT INTO battlemap_instance (id, battlemap_media_id, session_id, scene_id)
                 VALUES ('{INSTANCE}', '{CARTE}', '{SEANCE}', '{ARRIVEE}');
                 {}{}{}",
                element(ELOWEN, "pj", "Elowen"),
                element(BANDIT, "pnj", "Bandit"),
                element(TAVERNE, "lieu", "La Taverne du Sanglier"),
                token(T_ELOWEN, ELOWEN, "Elowen", None, "{béni}"),
                token(T_BANDIT_1, BANDIT, "Bandit 1", Some(4), "{empoisonné}"),
                token(T_BANDIT_2, BANDIT, "Bandit 2", Some(0), "{}"),
            ))
            .await
            .expect("the tavern's instance");
        let url = naming(&name);
        Scratch { client, name, url }
    }

    /// The engine as the server plants it over this database — repairing
    /// what the tables lost — and its wire protocol.
    async fn serve(&self) -> (Arc<Engine>, Router) {
        let Planted { engine, rows } = planted(&self.url)
            .await
            .expect("the engine over PostgreSQL");
        let engine = Arc::new(engine);
        let app = router(
            engine.clone(),
            rows,
            Arc::new(ports_llm::fake::FakeLlm::new()),
        );
        (engine, app)
    }

    /// The combats of the instance as `combat` holds them, oldest first.
    async fn combats(&self) -> Vec<Value> {
        self.json(
            "SELECT jsonb_build_object('id', id, 'round', round, 'current', current_entry_id,
                 'started', started_at, 'ended', ended_at)::text
             FROM combat WHERE instance_id = $1::text::uuid ORDER BY started_at, id",
        )
        .await
    }

    /// The instance's tokens, every column, by id.
    async fn tokens(&self) -> Vec<Value> {
        self.json(
            "SELECT to_jsonb(t)::text FROM token t WHERE t.instance_id = $1::text::uuid ORDER BY t.id",
        )
        .await
    }

    /// The entries of the instance's combats, every column, by combat and rank.
    async fn entries(&self) -> Vec<Value> {
        self.json(
            "SELECT to_jsonb(e)::text FROM initiative_entry e JOIN combat c ON c.id = e.combat_id
             WHERE c.instance_id = $1::text::uuid ORDER BY c.started_at, e.rank",
        )
        .await
    }

    async fn json(&self, sql: &str) -> Vec<Value> {
        self.client
            .query(sql, &[&INSTANCE])
            .await
            .expect("the instance")
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
    target: Option<&str>,
    payload: Value,
    key: Option<&str>,
) -> (StatusCode, Value) {
    let mut body = json!({
        "dataCapability": data_capability,
        "payload": payload,
        "caller": { "by": "dm", "from": "table.Battlemap", "roles": ["role:approver"] },
    });
    if let Some(target) = target {
        body["target"] = json!({ "id": target });
    }
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

/// table.startCombat on the instance, the NPCs' d20s rolled at the table.
async fn start(app: &Router) -> (StatusCode, Value) {
    let payload = json!({
        "initiative": { ELOWEN: 15 },
        "rolls": { T_BANDIT_1: 12, T_BANDIT_2: 3 },
    });
    send(app, "table.startCombat@1", Some(INSTANCE), payload, None).await
}

/// table.endCombat on the instance, as the Battlemap sends it: no target,
/// the instance in its payload.
async fn end(app: &Router, key: Option<&str>) -> (StatusCode, Value) {
    send(
        app,
        "table.endCombat@1",
        None,
        json!({ "instance": INSTANCE }),
        key,
    )
    .await
}

/// Runs the applier once, every result applied: how many commands it ran.
async fn applied(engine: &Engine) -> usize {
    let results = engine.run_once().await.expect("the applier ran");
    for result in &results {
        assert_eq!(result.status, State::Applied, "{result:?}");
    }
    results.len()
}

/// A combat started and applied: its combat and three entries.
async fn fighting(engine: &Engine, app: &Router) {
    let (status, body) = start(app).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    assert_eq!(applied(engine).await, 4);
}

#[tokio::test]
async fn an_open_combat_ends_with_no_turn_current_and_its_tokens_as_they_were() {
    let s = Scratch::new("ends").await;
    let (engine, app) = s.serve().await;
    fighting(&engine, &app).await;
    let combat = &s.combats().await[0];
    assert!(combat["ended"].is_null());
    assert!(!combat["current"].is_null(), "a turn runs: {combat}");
    let (tokens, entries) = (s.tokens().await, s.entries().await);
    let (status, body) = end(&app, None).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    assert_eq!(applied(&engine).await, 1);
    let ended = &s.combats().await[0];
    assert!(!ended["ended"].is_null(), "ended_at is set: {ended}");
    assert!(ended["current"].is_null(), "no turn is current: {ended}");
    assert_eq!(ended["round"], combat["round"]);
    assert_eq!(ended["started"], combat["started"]);
    // The map keeps its tokens, their wounds and their badges; the order
    // stays as it stood.
    assert_eq!(s.tokens().await, tokens);
    assert_eq!(s.entries().await, entries);
    s.drop().await;
}

#[tokio::test]
async fn one_active_combat_an_ended_combat_lets_a_new_one_start() {
    let s = Scratch::new("restart").await;
    let (engine, app) = s.serve().await;
    fighting(&engine, &app).await;
    let (status, body) = start(&app).await;
    assert_eq!(status, StatusCode::CONFLICT, "still open: {body}");
    let (status, body) = end(&app, None).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    // Queued, not applied: the next start reads the end already.
    let (status, body) = start(&app).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    assert_eq!(applied(&engine).await, 5, "the end, then the new combat");
    let combats = s.combats().await;
    assert_eq!(combats.len(), 2);
    assert!(!combats[0]["ended"].is_null());
    assert!(combats[1]["ended"].is_null());
    assert!(!combats[1]["current"].is_null());
    s.drop().await;
}

#[tokio::test]
async fn no_open_combat_an_end_is_refused_and_queues_nothing() {
    let s = Scratch::new("none_open").await;
    let (engine, app) = s.serve().await;
    let (status, body) = end(&app, None).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert!(body["error"].is_string(), "{body}");
    fighting(&engine, &app).await;
    let (status, body) = end(&app, None).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    // Its end queued, the combat is open no more.
    let (status, body) = end(&app, None).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(applied(&engine).await, 1, "one end queued");
    let (status, body) = end(&app, None).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(applied(&engine).await, 0, "an end refused queues nothing");
    s.drop().await;
}

#[tokio::test]
async fn an_end_naming_no_instance_is_refused() {
    let s = Scratch::new("no_id").await;
    let (engine, app) = s.serve().await;
    fighting(&engine, &app).await;
    for payload in [json!({}), json!({ "instance": "la-taverne" })] {
        let (status, body) = send(&app, "table.endCombat@1", None, payload, None).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    }
    assert_eq!(applied(&engine).await, 0);
    assert!(s.combats().await[0]["ended"].is_null());
    s.drop().await;
}

#[tokio::test]
async fn the_instance_as_the_target_names_the_combat_ended() {
    let s = Scratch::new("target").await;
    let (engine, app) = s.serve().await;
    fighting(&engine, &app).await;
    let (status, body) = send(
        &app,
        "table.endCombat@1",
        Some(&INSTANCE.to_uppercase()),
        json!({}),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    assert_eq!(applied(&engine).await, 1);
    assert!(!s.combats().await[0]["ended"].is_null());
    s.drop().await;
}

#[tokio::test]
async fn an_end_sent_again_under_its_key_is_replayed() {
    let s = Scratch::new("replay").await;
    let (engine, app) = s.serve().await;
    fighting(&engine, &app).await;
    let (status, first) = end(&app, Some("k")).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{first}");
    assert_eq!(applied(&engine).await, 1);
    let (status, again) = end(&app, Some("k")).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{again}");
    assert_eq!(again["commandId"], first["commandId"]);
    assert_eq!(applied(&engine).await, 0);
    // Under another key, there is no combat left to end.
    let (status, body) = end(&app, Some("other")).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    s.drop().await;
}

#[tokio::test]
async fn an_end_the_table_lost_is_written_again_at_the_next_start() {
    let s = Scratch::new("repair").await;
    {
        let (engine, app) = s.serve().await;
        fighting(&engine, &app).await;
        let (status, body) = end(&app, None).await;
        assert_eq!(status, StatusCode::ACCEPTED, "{body}");
        assert_eq!(applied(&engine).await, 1);
    }
    let before = s.combats().await;
    let entry = s.entries().await[0]["id"].clone();
    s.client
        .execute(
            "UPDATE combat SET ended_at = NULL, current_entry_id = $1::text::uuid",
            &[&entry.as_str()],
        )
        .await
        .expect("an end lost");
    let _ = s.serve().await;
    assert_eq!(s.combats().await, before);
    s.drop().await;
}

#[tokio::test]
async fn over_the_engine_s_memory_no_combat_is_open() {
    let Planted { engine, rows } = planted(MEMORY).await.expect("the in-memory engine");
    let app = router(
        Arc::new(engine),
        rows,
        Arc::new(ports_llm::fake::FakeLlm::new()),
    );
    let (status, body) = end(&app, None).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
}
