//! table.nextTurn over PostgreSQL (`DATABASE_URL`): « Suivant » passes the
//! turn of the instance's open combat to the next living participant in rank
//! order and adds a round when it wraps to the first (`next-turn`); a dead
//! NPC is skipped (`dead-derived`), a hidden one takes its turn at its rank
//! (`hidden-keeps-rank`). Once applied, the turn is `combat.current_entry_id`
//! and `combat.round`, and a turn the table lost is written again at the
//! next start. Each test works in a scratch database of its own, migrated by
//! the Data layer, dropped when it ends.
//!
//! The tokens are dropped by `table.placeTokens`; the combat and its entries
//! are written by a test-only DataCapability, and laid in the tables as a
//! start's projection lays them: a start (`table.startCombat`) is another
//! task's.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use dataguard_server::{Engine, Planted, planted, router};
use harness_dataguard::api::Submission;
use harness_dataguard::command::State;
use harness_dataguard::guard::Caller;
use harness_dataguard::manifest::DataCapability;
use serde_json::{Value, json};
use tokio_postgres::error::SqlState;
use tokio_postgres::{Client, NoTls};
use tower::ServiceExt;

const VALOMBRE: &str = "c1000000-0000-4000-8000-000000000001";
const BANDIT: &str = "e1000000-0000-4000-8000-000000000003";
const VESNA: &str = "e1000000-0000-4000-8000-000000000004";
const TAVERNE: &str = "e1000000-0000-4000-8000-000000000005";
const CARTE: &str = "6d000000-0000-4000-8000-000000000001";
const SEANCE: &str = "5e000000-0000-4000-8000-000000000001";
const ARRIVEE: &str = "5c000000-0000-4000-8000-000000000001";
const INSTANCE: &str = "b1a00000-0000-4000-8000-000000000001";
const COMBAT: &str = "c0000000-0000-4000-8000-000000000001";
/// The entries of the three participants, ranked 1 to 3.
const ENTRIES: [&str; 3] = [
    "1e000000-0000-4000-8000-000000000001",
    "1e000000-0000-4000-8000-000000000002",
    "1e000000-0000-4000-8000-000000000003",
];
/// The three participants, in rank order.
const PARTICIPANTS: [&str; 3] = ["Chef bandit 1", "Bandit 1", "Bandit 2"];

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

/// The SQL adding element `id` of Valombre, with the version every element
/// is written with, and its sheet for an NPC.
fn element(id: &str, kind: &str, name: &str, hp_max: Option<i16>) -> String {
    let sheet = if kind == "lieu" {
        String::new()
    } else {
        let hp = hp_max.map_or("NULL".to_string(), |hp| hp.to_string());
        format!(
            "INSERT INTO character (entity_id, token_color, hp_max) VALUES ('{id}', '#888', {hp});"
        )
    };
    format!(
        "INSERT INTO entity (id, campaign_id, type, name) VALUES ('{id}', '{VALOMBRE}', '{kind}', '{name}'); \
         {sheet} \
         INSERT INTO entity_version (entity_id, version_number, source, snapshot, diff) \
         VALUES ('{id}', 1, 'dm', '{{}}', '[]');"
    )
}

/// A database of its own.
struct Scratch {
    client: Client,
    name: String,
    url: String,
}

impl Scratch {
    /// Migrated by the Data layer, holding the tavern's instance, the
    /// Bandit, an NPC of 11 HP, and Vesna, the bandits' chief, of 22.
    async fn new(name: &str) -> Scratch {
        let name = format!("next_turn_test_{name}_{}", std::process::id());
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
                 INSERT INTO scene (id, session_id, position, title) VALUES ('{ARRIVEE}', '{SEANCE}', 1, 'Arrivée');
                 INSERT INTO battlemap_instance (id, battlemap_media_id, session_id, scene_id)
                 VALUES ('{INSTANCE}', '{CARTE}', '{SEANCE}', '{ARRIVEE}');",
                element(BANDIT, "pnj", "Bandit", Some(11)),
                element(VESNA, "pnj", "Chef bandit", Some(22)),
                element(TAVERNE, "lieu", "La Taverne du Sanglier", None),
            ))
            .await
            .expect("the tavern's instance");
        let url = naming(&name);
        Scratch { client, name, url }
    }

    /// The engine as the server plants it over this database — repairing
    /// what the tables lost — with the test-only DataCapability that writes
    /// a combat or an entry, and its wire protocol.
    async fn serve(&self) -> (Arc<Engine>, Router) {
        let Planted { engine, rows } = planted(&self.url)
            .await
            .expect("the engine over PostgreSQL");
        let manifest = json!({
            "dataCapability": "test.seedCombat",
            "owner": "dataguard",
            "version": 1,
            "effect": "insert",
            "target": { "aggregate": "BattlemapInstance", "id": "$id" },
            "mode": "overwrite",
            "callableBy": ["*"],
        });
        let removal = json!({
            "dataCapability": "test.removeToken",
            "owner": "dataguard",
            "version": 1,
            "effect": "delete",
            "target": { "aggregate": "BattlemapInstance", "id": "$id" },
            "mode": "overwrite",
            "callableBy": ["*"],
        });
        let engine = Arc::new(
            engine
                .with_data_capability(
                    DataCapability::parse(&manifest.to_string()).expect("contract F"),
                )
                .with_data_capability(
                    DataCapability::parse(&removal.to_string()).expect("contract F"),
                ),
        );
        let app = router(
            engine.clone(),
            rows,
            Arc::new(ports_llm::fake::FakeLlm::new()),
        );
        (engine, app)
    }

    /// The token named `name`'s id.
    async fn token(&self, name: &str) -> String {
        self.client
            .query_one("SELECT id::text FROM token WHERE name = $1", &[&name])
            .await
            .expect("the token")
            .get(0)
    }

    /// The combat as `combat` holds it: its round, and the rank of the entry
    /// whose turn it is (1 first), none when no turn is current.
    async fn turn(&self) -> (i32, Option<usize>) {
        let row = self
            .client
            .query_one(
                "SELECT round, current_entry_id::text FROM combat WHERE id = $1::text::uuid",
                &[&COMBAT],
            )
            .await
            .expect("the combat");
        let current: Option<String> = row.get(1);
        let rank = current.map(|current| {
            1 + ENTRIES
                .iter()
                .position(|entry| *entry == current)
                .expect("one of the entries")
        });
        (row.get(0), rank)
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

/// `POST /commands` of `data_capability` on the instance, by the DM: its
/// status and answer.
async fn command(app: &Router, data_capability: &str, payload: Value) -> (StatusCode, Value) {
    let body = json!({
        "dataCapability": data_capability,
        "payload": payload,
        "target": { "id": INSTANCE },
        "caller": { "by": "dm", "from": "table.Battlemap", "roles": ["role:approver"] },
    });
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

/// Runs the applier once, every result applied: how many commands it ran.
async fn applied(engine: &Engine) -> usize {
    let results = engine.run_once().await.expect("the applier ran");
    for result in &results {
        assert_eq!(result.status, State::Applied, "{result:?}");
    }
    results.len()
}

/// `payload` written on `BattlemapInstance/<id>` by the test-only
/// DataCapability, queued.
async fn seed(engine: &Engine, id: &str, payload: Value) {
    submit(engine, "test.seedCombat", id, payload).await;
}

/// The token `id` taken off the map by the test-only DataCapability — the
/// engine's tombstone, as a removal of a token writes it — queued.
async fn remove(engine: &Engine, id: &str) {
    submit(engine, "test.removeToken", id, json!({})).await;
}

/// `payload` sent by `data_capability` on `BattlemapInstance/<id>`, queued.
async fn submit(engine: &Engine, data_capability: &str, id: &str, payload: Value) {
    let Value::Object(payload) = payload else {
        unreachable!("a payload is an object");
    };
    engine
        .submit(Submission {
            data_capability: data_capability.to_string(),
            payload: payload.into_iter().collect(),
            target_id: Some(id.to_string()),
            based_on: None,
            idempotency_key: None,
            caller: Caller {
                by: "dm".to_string(),
                from: "table.battlemap".to_string(),
                permissions: Vec::new(),
                roles: vec!["role:approver".to_string()],
            },
        })
        .await
        .expect("queued");
}

/// Chef bandit 1, Bandit 1 and Bandit 2 dropped on the instance, in combat
/// in that order at round 1, on the first — ended at `ended` when one is
/// given —, the engine's rows laid in the tables as a start's projection
/// lays them.
async fn fight(s: &Scratch, engine: &Engine, app: &Router, ended: Option<&str>) {
    for (character, count) in [(VESNA, 1), (BANDIT, 2)] {
        let (status, body) = command(
            app,
            "table.placeTokens@1",
            json!({ "character": character, "q": 5, "r": 5, "count": count }),
        )
        .await;
        assert_eq!(status, StatusCode::ACCEPTED, "{body}");
        assert_eq!(applied(engine).await, count);
    }
    seed(
        engine,
        COMBAT,
        json!({
            "combatId": COMBAT,
            "combatInstanceId": INSTANCE,
            "round": 1,
            "currentEntryId": ENTRIES[0],
            "startedAt": "2026-10-20T20:00:00.000Z",
            "endedAt": ended,
        }),
    )
    .await;
    for ((entry, name), rank) in ENTRIES.iter().zip(PARTICIPANTS).zip(1..) {
        let token = s.token(name).await;
        seed(
            engine,
            entry,
            json!({
                "initiativeEntryId": entry,
                "initiativeEntryCombatId": COMBAT,
                "initiativeEntryTokenId": token,
                "roll": null,
                "initiative": 20 - rank,
                "dexScore": null,
                "tiebreak": 0.5,
                "rank": rank,
            }),
        )
        .await;
    }
    assert_eq!(applied(engine).await, 4);
    let ended = ended.map_or("NULL".to_string(), |at| format!("'{at}'"));
    let mut sql = format!(
        "INSERT INTO combat (id, instance_id, round, started_at, ended_at)
         VALUES ('{COMBAT}', '{INSTANCE}', 1, '2026-10-20T20:00:00Z', {ended});"
    );
    for ((entry, name), rank) in ENTRIES.iter().zip(PARTICIPANTS).zip(1..) {
        let token = s.token(name).await;
        sql.push_str(&format!(
            "INSERT INTO initiative_entry (id, combat_id, token_id, initiative, tiebreak, rank)
             VALUES ('{entry}', '{COMBAT}', '{token}', {}, 0.5, {rank});",
            20 - rank
        ));
    }
    sql.push_str(&format!(
        "UPDATE combat SET current_entry_id = '{}' WHERE id = '{COMBAT}';",
        ENTRIES[0]
    ));
    s.client.batch_execute(&sql).await.expect("the combat laid");
}

/// « Suivant » on the instance, queued: its status and answer.
async fn suivant(app: &Router) -> (StatusCode, Value) {
    command(app, "table.nextTurn@1", json!({})).await
}

/// « Suivant », applied: the turn as the table then holds it.
async fn next(s: &Scratch, engine: &Engine, app: &Router) -> (i32, Option<usize>) {
    let (status, body) = suivant(app).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    assert_eq!(applied(engine).await, 1);
    s.turn().await
}

#[tokio::test]
async fn next_turn_three_participants_at_round_1_go_second_third_first_and_round_2() {
    let s = Scratch::new("lap").await;
    let (engine, app) = s.serve().await;
    fight(&s, &engine, &app, None).await;
    assert_eq!(s.turn().await, (1, Some(1)));
    assert_eq!(next(&s, &engine, &app).await, (1, Some(2)));
    assert_eq!(next(&s, &engine, &app).await, (1, Some(3)));
    assert_eq!(next(&s, &engine, &app).await, (2, Some(1)));
    s.drop().await;
}

#[tokio::test]
async fn next_turn_two_suivant_sent_at_once_both_count() {
    let s = Scratch::new("twice").await;
    let (engine, app) = s.serve().await;
    fight(&s, &engine, &app, None).await;
    // Three presses before the applier runs: each starts from the turn the
    // one queued ahead leaves, and the lap is counted once.
    for _ in 0..3 {
        let (status, body) = suivant(&app).await;
        assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    }
    assert_eq!(applied(&engine).await, 3);
    assert_eq!(s.turn().await, (2, Some(1)));
    s.drop().await;
}

#[tokio::test]
async fn dead_derived_the_second_at_0_hp_is_skipped_from_the_first() {
    let s = Scratch::new("dead").await;
    let (engine, app) = s.serve().await;
    fight(&s, &engine, &app, None).await;
    let bandit_1 = s.token("Bandit 1").await;
    let (status, body) = command(
        &app,
        "table.changeHitPoints@1",
        json!({ "token": bandit_1, "delta": -11 }),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    assert_eq!(applied(&engine).await, 1);
    assert_eq!(next(&s, &engine, &app).await, (1, Some(3)));
    // Never again: the lap skips it too.
    assert_eq!(next(&s, &engine, &app).await, (2, Some(1)));
    assert_eq!(next(&s, &engine, &app).await, (2, Some(3)));
    s.drop().await;
}

#[tokio::test]
async fn dead_derived_a_blow_queued_ahead_counts_before_the_turn_is_judged() {
    let s = Scratch::new("queued").await;
    let (engine, app) = s.serve().await;
    fight(&s, &engine, &app, None).await;
    let bandit_1 = s.token("Bandit 1").await;
    let (status, body) = command(
        &app,
        "table.changeHitPoints@1",
        json!({ "token": bandit_1, "delta": -11 }),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    let (status, body) = suivant(&app).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    assert_eq!(applied(&engine).await, 2);
    assert_eq!(s.turn().await, (1, Some(3)));
    s.drop().await;
}

#[tokio::test]
async fn next_turn_a_token_off_the_map_takes_no_turn_and_the_turn_passes_on_from_its_rank() {
    let s = Scratch::new("removed").await;
    let (engine, app) = s.serve().await;
    fight(&s, &engine, &app, None).await;
    assert_eq!(next(&s, &engine, &app).await, (1, Some(2)));
    // Bandit 1, whose turn it is, leaves the map: the turn goes on to the
    // third, not back to the first.
    remove(&engine, &s.token("Bandit 1").await).await;
    assert_eq!(applied(&engine).await, 1);
    assert_eq!(next(&s, &engine, &app).await, (1, Some(3)));
    assert_eq!(next(&s, &engine, &app).await, (2, Some(1)));
    assert_eq!(next(&s, &engine, &app).await, (2, Some(3)));
    s.drop().await;
}

#[tokio::test]
async fn next_turn_a_removal_queued_ahead_counts_before_the_turn_is_judged() {
    let s = Scratch::new("removal_queued").await;
    let (engine, app) = s.serve().await;
    fight(&s, &engine, &app, None).await;
    remove(&engine, &s.token("Bandit 1").await).await;
    let (status, body) = suivant(&app).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    assert_eq!(applied(&engine).await, 2);
    assert_eq!(s.turn().await, (1, Some(3)));
    s.drop().await;
}

#[tokio::test]
async fn hidden_keeps_rank_a_hidden_participant_gets_its_turn_at_its_rank() {
    let s = Scratch::new("hidden").await;
    let (engine, app) = s.serve().await;
    fight(&s, &engine, &app, None).await;
    let bandit_1 = s.token("Bandit 1").await;
    let (status, body) = command(
        &app,
        "table.setTokenVisibility@1",
        json!({ "token": bandit_1, "visible": false }),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    assert_eq!(applied(&engine).await, 1);
    assert_eq!(next(&s, &engine, &app).await, (1, Some(2)));
    s.drop().await;
}

#[tokio::test]
async fn next_turn_an_ended_combat_is_no_open_combat_and_is_refused() {
    let s = Scratch::new("ended").await;
    let (engine, app) = s.serve().await;
    fight(&s, &engine, &app, Some("2026-10-20T21:00:00.000Z")).await;
    let (status, body) = suivant(&app).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(applied(&engine).await, 0, "nothing was queued");
    assert_eq!(s.turn().await, (1, Some(1)));
    s.drop().await;
}

#[tokio::test]
async fn next_turn_a_combat_with_no_one_alive_is_refused() {
    let s = Scratch::new("all_dead").await;
    let (engine, app) = s.serve().await;
    fight(&s, &engine, &app, None).await;
    for name in PARTICIPANTS {
        let token = s.token(name).await;
        let (status, body) = command(
            &app,
            "table.changeHitPoints@1",
            json!({ "token": token, "delta": -50 }),
        )
        .await;
        assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    }
    assert_eq!(applied(&engine).await, 3);
    let (status, body) = suivant(&app).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(applied(&engine).await, 0, "nothing was queued");
    s.drop().await;
}

#[tokio::test]
async fn a_turn_the_table_lost_is_written_again_at_the_next_start() {
    let s = Scratch::new("repair").await;
    {
        let (engine, app) = s.serve().await;
        fight(&s, &engine, &app, None).await;
        assert_eq!(next(&s, &engine, &app).await, (1, Some(2)));
    }
    s.client
        .execute("UPDATE combat SET round = 7, current_entry_id = NULL", &[])
        .await
        .expect("the turn lost");
    let _ = s.serve().await;
    assert_eq!(s.turn().await, (1, Some(2)));
    s.drop().await;
}
