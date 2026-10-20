//! table.removeParticipant over PostgreSQL (`DATABASE_URL`): a participant
//! taken out of the combat, once applied, is a tombstone the engine keeps on
//! its entry's partition (a soft delete) and its `initiative_entry` row out of
//! the table — its token stays on the map, the carousel's places
//! (`initiative_order_view`, derived) close up with no rank written, and the
//! participant that holds the turn is refused; a removal
//! the table lost is made again at the next start. The combat is started by
//! table.startCombat itself. Each test works in a scratch database of its
//! own, migrated by the Data layer, dropped when it ends.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use dataguard_server::{Engine, Planted, planted, router};
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
const AUTRE_INSTANCE: &str = "b1a00000-0000-4000-8000-0000000000ff";
const T_ELOWEN: &str = "70000000-0000-4000-8000-000000000001";
const T_KAEL: &str = "70000000-0000-4000-8000-000000000002";
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
    /// tokens as the tables hold them: Elowen, a PC of DEX 14, and Kael, a PC
    /// of no DEX, both present; the Bandit, an NPC of DEX 12; Vesna, an NPC
    /// of DEX 16, hidden.
    async fn new(name: &str) -> Scratch {
        let name = format!("remove_participant_test_{name}_{}", std::process::id());
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
                 {}{}{}{}",
                element(ELOWEN, "pj", "Elowen", Some(14)),
                element(KAEL, "pj", "Kael", None),
                element(BANDIT, "pnj", "Bandit", Some(12)),
                element(VESNA, "pnj", "Vesna", Some(16)),
                element(TAVERNE, "lieu", "La Taverne du Sanglier", None),
                token(T_ELOWEN, ELOWEN, "Elowen", true, None),
                token(T_KAEL, KAEL, "Kael", true, None),
                token(T_BANDIT, BANDIT, "Bandit 1", true, Some(11)),
                token(T_VESNA, VESNA, "Vesna", false, Some(30)),
            ))
            .await
            .expect("the tavern's instance");
        let url = naming(&name);
        Scratch { client, name, url }
    }

    /// The engine as the server plants it over this database — repairing
    /// what the tables lost — with the test-only DataCapabilities that end a
    /// combat and delete it, table.endCombat's and table.resetBattlemap's
    /// stand-ins, and its wire protocol.
    async fn serve(&self) -> (Arc<Engine>, Router) {
        let Planted { engine, rows } = planted(&self.url)
            .await
            .expect("the engine over PostgreSQL");
        let stand_in = |name: &str, effect: &str, id: &str| {
            let manifest = json!({
                "dataCapability": name,
                "owner": "dataguard",
                "version": 1,
                "effect": effect,
                "target": { "aggregate": "BattlemapInstance", "id": id },
                "mode": "overwrite",
                "callableBy": ["*"],
            });
            DataCapability::parse(&manifest.to_string()).expect("contract F")
        };
        let engine = Arc::new(
            engine
                .with_data_capability(stand_in("test.endCombat", "update", "$combatId"))
                .with_data_capability(stand_in("test.resetCombat", "delete", "$combatId")),
        );
        let app = router(
            engine.clone(),
            rows,
            Arc::new(ports_llm::fake::FakeLlm::new()),
        );
        (engine, app)
    }

    /// The carousel of the combat: each token's id, its entry's stored
    /// rank, its place, in order.
    async fn carousel(&self) -> Vec<(String, i32, i32)> {
        self.client
            .query(
                "SELECT e.token_id::text, e.rank, o.order_index
                 FROM initiative_order_view o
                 JOIN initiative_entry e ON e.id = o.entry_id
                 JOIN combat c ON c.id = o.combat_id
                 WHERE c.instance_id = $1::text::uuid ORDER BY o.order_index",
                &[&INSTANCE],
            )
            .await
            .expect("the carousel")
            .iter()
            .map(|row| (row.get(0), row.get(1), row.get(2)))
            .collect()
    }

    /// The id of the entry of the token `token`.
    async fn entry(&self, token: &str) -> String {
        self.client
            .query_one(
                "SELECT id::text FROM initiative_entry WHERE token_id = $1::text::uuid",
                &[&token],
            )
            .await
            .expect("the entry")
            .get(0)
    }

    /// The combat's id and the token whose turn it is, as `combat` holds
    /// them.
    async fn turn(&self) -> (String, Option<String>) {
        let row = self
            .client
            .query_one(
                "SELECT c.id::text, e.token_id::text FROM combat c
                 LEFT JOIN initiative_entry e ON e.id = c.current_entry_id
                 WHERE c.instance_id = $1::text::uuid",
                &[&INSTANCE],
            )
            .await
            .expect("the combat");
        (row.get(0), row.get(1))
    }

    /// Whether the table holds the token `token`.
    async fn has_token(&self, token: &str) -> bool {
        self.client
            .query_one(
                "SELECT EXISTS (SELECT 1 FROM token WHERE id = $1::text::uuid)",
                &[&token],
            )
            .await
            .expect("the token")
            .get(0)
    }

    /// Whether the engine holds the partition of `id`, and tombstoned.
    async fn tombstoned(&self, id: &str) -> Option<bool> {
        self.client
            .query_opt(
                "SELECT tombstoned FROM dataguard_rows WHERE partition = $1",
                &[&format!("BattlemapInstance/{id}")],
            )
            .await
            .expect("the engine's row")
            .map(|row| row.get(0))
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

/// The combat started and applied: Elowen at 18, Kael at 15, Vesna at 14
/// (11 + 3), the Bandit at 6 (5 + 1) — ranks 1 to 4, the turn on Elowen.
async fn started(engine: &Engine, app: &Router) {
    let (status, body) = send(
        app,
        "table.startCombat@1",
        INSTANCE,
        json!({ "initiative": { ELOWEN: 18, KAEL: 15 }, "rolls": { T_BANDIT: 5, T_VESNA: 11 } }),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    assert_eq!(applied(engine).await, 5);
}

/// table.removeParticipant of `entry`, on the instance.
async fn remove(app: &Router, entry: &str, key: Option<&str>) -> (StatusCode, Value) {
    send(
        app,
        "table.removeParticipant@1",
        INSTANCE,
        json!({ "entry": entry }),
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

fn place(token: &str, rank: i32, index: i32) -> (String, i32, i32) {
    (token.to_string(), rank, index)
}

#[tokio::test]
async fn a_participant_at_rank_2_of_4_leaves_ranks_1_2_3_in_the_previous_order_its_token_kept() {
    let s = Scratch::new("rank").await;
    let (engine, app) = s.serve().await;
    started(&engine, &app).await;
    assert_eq!(
        s.carousel().await,
        [
            place(T_ELOWEN, 1, 1),
            place(T_KAEL, 2, 2),
            place(T_VESNA, 3, 3),
            place(T_BANDIT, 4, 4),
        ]
    );
    let kael = s.entry(T_KAEL).await;
    let (status, body) = remove(&app, &kael, None).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    assert_eq!(body["partition"], format!("BattlemapInstance/{kael}"));
    assert_eq!(applied(&engine).await, 1);
    // Its entry is gone, the places close up, the stored ranks untouched.
    assert_eq!(
        s.carousel().await,
        [
            place(T_ELOWEN, 1, 1),
            place(T_VESNA, 3, 2),
            place(T_BANDIT, 4, 3),
        ]
    );
    // Its token stays on the map; the entry's partition is a tombstone the
    // engine keeps, the token's untouched.
    assert!(s.has_token(T_KAEL).await);
    assert_eq!(s.tombstoned(&kael).await, Some(true));
    assert_eq!(s.turn().await.1.as_deref(), Some(T_ELOWEN));
    // Removed, it is none to remove again.
    assert_eq!(remove(&app, &kael, None).await.0, StatusCode::NOT_FOUND);
    s.drop().await;
}

#[tokio::test]
async fn the_participant_that_holds_the_turn_is_refused_and_nothing_queued() {
    let s = Scratch::new("turn").await;
    let (engine, app) = s.serve().await;
    started(&engine, &app).await;
    let elowen = s.entry(T_ELOWEN).await;
    // No table.nextTurn sent first: the engine's combat would keep naming a
    // tombstone while the table nullified its turn, so the removal is refused.
    let (status, body) = remove(&app, &elowen, None).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(applied(&engine).await, 0);
    assert_eq!(s.turn().await.1.as_deref(), Some(T_ELOWEN));
    assert_eq!(s.tombstoned(&elowen).await, Some(false));
    assert_eq!(s.carousel().await.len(), 4);
    s.drop().await;
}

#[tokio::test]
async fn a_removal_queued_or_sent_again_under_its_key_is_none_twice() {
    let s = Scratch::new("again").await;
    let (engine, app) = s.serve().await;
    started(&engine, &app).await;
    let vesna = s.entry(T_VESNA).await;
    let (status, first) = remove(&app, &vesna, Some("k")).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{first}");
    // Queued and not applied: a second removal is none, under its key the
    // engine's to replay.
    assert_eq!(remove(&app, &vesna, None).await.0, StatusCode::NOT_FOUND);
    let (status, again) = remove(&app, &vesna, Some("k")).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{again}");
    assert_eq!(again["replayed"], true, "{again}");
    assert_eq!(again["commandId"], first["commandId"]);
    assert_eq!(applied(&engine).await, 1);
    assert_eq!(s.carousel().await.len(), 3);
    s.drop().await;
}

#[tokio::test]
async fn an_entry_of_another_instance_or_of_an_ended_combat_is_refused_and_nothing_queued() {
    let s = Scratch::new("refused").await;
    let (engine, app) = s.serve().await;
    started(&engine, &app).await;
    let bandit = s.entry(T_BANDIT).await;
    let (status, body) = send(
        &app,
        "table.removeParticipant@1",
        AUTRE_INSTANCE,
        json!({ "entry": bandit }),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    // A token's id is no entry's.
    assert_eq!(remove(&app, T_BANDIT, None).await.0, StatusCode::NOT_FOUND);
    assert_eq!(
        remove(&app, "bandit", None).await.0,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    let (combat, _) = s.turn().await;
    let (status, body) = send(
        &app,
        "test.endCombat@1",
        &combat,
        json!({ "combatId": combat, "endedAt": "2026-10-20T21:00:00Z" }),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    // Queued, then applied: the combat ended, there is no order to leave.
    assert_eq!(remove(&app, &bandit, None).await.0, StatusCode::CONFLICT);
    assert_eq!(applied(&engine).await, 1);
    assert_eq!(remove(&app, &bandit, None).await.0, StatusCode::CONFLICT);
    assert_eq!(applied(&engine).await, 0);
    assert_eq!(s.carousel().await.len(), 4);
    s.drop().await;
}

#[tokio::test]
async fn an_entry_of_a_combat_whose_deletion_is_queued_is_none_to_remove() {
    let s = Scratch::new("reset").await;
    let (engine, app) = s.serve().await;
    started(&engine, &app).await;
    let bandit = s.entry(T_BANDIT).await;
    let (combat, _) = s.turn().await;
    let (status, body) = send(&app, "test.resetCombat@1", &combat, json!({}), None).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    assert_eq!(remove(&app, &bandit, None).await.0, StatusCode::NOT_FOUND);
    drop(engine);
    s.drop().await;
}

#[tokio::test]
async fn a_removal_the_table_lost_is_made_again_at_the_next_start() {
    let s = Scratch::new("repair").await;
    let (engine, app) = s.serve().await;
    started(&engine, &app).await;
    let kael = s.entry(T_KAEL).await;
    assert_eq!(remove(&app, &kael, None).await.0, StatusCode::ACCEPTED);
    assert_eq!(applied(&engine).await, 1);
    drop((engine, app));
    // The table holds the entry again, as a projection that failed leaves it.
    let (combat, _) = s.turn().await;
    s.client
        .batch_execute(&format!(
            "INSERT INTO initiative_entry (id, combat_id, token_id, roll, initiative, dex_score, tiebreak, rank)
             VALUES ('{kael}', '{combat}', '{T_KAEL}', NULL, 15, NULL, 0.5, 2)"
        ))
        .await
        .expect("the entry back");
    assert_eq!(s.carousel().await.len(), 4);
    let (_engine, _app) = s.serve().await;
    assert_eq!(
        s.carousel().await,
        [
            place(T_ELOWEN, 1, 1),
            place(T_VESNA, 3, 2),
            place(T_BANDIT, 4, 3),
        ]
    );
    assert!(s.has_token(T_KAEL).await);
    s.drop().await;
}
