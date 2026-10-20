//! table.removeToken over PostgreSQL (`DATABASE_URL`): a token taken off the
//! map, once applied, is a tombstone the engine keeps on its partition (a
//! soft delete) and its `token` row out of the table, the foreign keys
//! following the aggregate's relations — its initiative entry cascades, a
//! turn still on it is nullified — so the carousel's places
//! (`initiative_order_view`, derived) close up with no rank written; a removal
//! the table lost is made again at the next start. Each test works in a
//! scratch database of its own, migrated by the Data layer, dropped when it
//! ends.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use dataguard_server::{Engine, Planted, planted, router};
use harness_dataguard::command::State;
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
        let name = format!("remove_token_test_{name}_{}", std::process::id());
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

    /// The tokens of the instance as `token` holds them, by name.
    async fn tokens(&self) -> Vec<Value> {
        self.client
            .query(
                "SELECT jsonb_build_object('id', id, 'number', number, 'name', name, 'q', q,
                     'r', r, 'visible', visible, 'hpCurrent', hp_current, 'hpMax', hp_max,
                     'conditions', to_jsonb(conditions))::text
                 FROM token WHERE instance_id = $1::text::uuid ORDER BY name",
                &[&INSTANCE],
            )
            .await
            .expect("the tokens")
            .iter()
            .map(|row| serde_json::from_str(row.get::<_, &str>(0)).expect("JSON"))
            .collect()
    }

    /// The token named `name`'s id.
    async fn token(&self, name: &str) -> String {
        self.client
            .query_one("SELECT id::text FROM token WHERE name = $1", &[&name])
            .await
            .expect("the token")
            .get(0)
    }

    /// The carousel of the combat: each token's name, rank, place and
    /// whether the players see it, in order.
    async fn carousel(&self) -> Vec<(String, i32, i32, bool)> {
        self.client
            .query(
                "SELECT t.name, e.rank, o.order_index, o.shown_to_players
                 FROM initiative_order_view o
                 JOIN initiative_entry e ON e.id = o.entry_id
                 JOIN token t ON t.id = e.token_id
                 WHERE o.combat_id = $1::text::uuid ORDER BY o.order_index",
                &[&COMBAT],
            )
            .await
            .expect("the carousel")
            .iter()
            .map(|row| (row.get(0), row.get(1), row.get(2), row.get(3)))
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

/// The token `token` taken off the map, and applied.
async fn remove(engine: &Engine, app: &Router, token: &str) {
    let (status, body) = command(app, "table.removeToken@1", json!({ "token": token })).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    assert_eq!(applied(engine).await, 1);
}

/// Bandit 1, 2 and 3 and Chef bandit 1 dropped on the instance and applied.
async fn dropped(engine: &Engine, app: &Router) {
    for (character, count) in [(BANDIT, 3), (VESNA, 1)] {
        let (status, body) = command(
            app,
            "table.placeTokens@1",
            json!({ "character": character, "q": 5, "r": 5, "count": count }),
        )
        .await;
        assert_eq!(status, StatusCode::ACCEPTED, "{body}");
        assert_eq!(applied(engine).await, count);
    }
}

impl Scratch {
    /// The combat as a launch leaves it, of four in this order — Bandit 1,
    /// Bandit 2, Bandit 3, Chef bandit 1 — the turn on the entry of `current`.
    async fn combat(&self, current: &str) {
        let mut entries = Vec::new();
        for (rank, name) in ["Bandit 1", "Bandit 2", "Bandit 3", "Chef bandit 1"]
            .into_iter()
            .enumerate()
        {
            let token = self.token(name).await;
            entries.push(format!(
                "('{COMBAT}', '{token}', 10, 10, 12, 0.{rank}, {})",
                rank + 1
            ));
        }
        let current = self.token(current).await;
        self.client
            .batch_execute(&format!(
                "INSERT INTO combat (id, instance_id, round, started_at)
                 VALUES ('{COMBAT}', '{INSTANCE}', 2, now());
                 INSERT INTO initiative_entry (combat_id, token_id, roll, initiative, dex_score, tiebreak, rank)
                 VALUES {};
                 UPDATE combat SET current_entry_id =
                     (SELECT id FROM initiative_entry WHERE token_id = '{current}')
                 WHERE id = '{COMBAT}';",
                entries.join(", ")
            ))
            .await
            .expect("the combat");
    }

    /// The combat's round, and the name of the token whose turn it is.
    async fn turn(&self) -> (i32, Option<String>) {
        let row = self
            .client
            .query_one(
                "SELECT c.round, t.name FROM combat c
                 LEFT JOIN initiative_entry e ON e.id = c.current_entry_id
                 LEFT JOIN token t ON t.id = e.token_id
                 WHERE c.id = $1::text::uuid",
                &[&COMBAT],
            )
            .await
            .expect("the combat");
        (row.get(0), row.get(1))
    }

    /// The entries the table holds of the token `token`.
    async fn entries_of(&self, token: &str) -> i64 {
        self.client
            .query_one(
                "SELECT count(*) FROM initiative_entry WHERE token_id = $1::text::uuid",
                &[&token],
            )
            .await
            .expect("the entries")
            .get(0)
    }

    /// Whether the engine holds the token's partition, and tombstoned.
    async fn tombstoned(&self, token: &str) -> Option<bool> {
        self.client
            .query_opt(
                "SELECT tombstoned FROM dataguard_rows WHERE partition = $1",
                &[&format!("BattlemapInstance/{token}")],
            )
            .await
            .expect("the engine's row")
            .map(|row| row.get(0))
    }
}

#[tokio::test]
async fn a_token_at_rank_2_of_4_removed_leaves_ranks_1_2_3_in_the_previous_order() {
    let s = Scratch::new("rank").await;
    let (engine, app) = s.serve().await;
    dropped(&engine, &app).await;
    s.combat("Bandit 1").await;
    let bandit_2 = s.token("Bandit 2").await;
    let place = |name: &str, rank: i32, index: i32| (name.to_string(), rank, index, true);
    assert_eq!(
        s.carousel().await,
        [
            place("Bandit 1", 1, 1),
            place("Bandit 2", 2, 2),
            place("Bandit 3", 3, 3),
            place("Chef bandit 1", 4, 4),
        ]
    );
    remove(&engine, &app, &bandit_2).await;
    // The token and its entry are gone; the places close up, the stored
    // ranks as they were: none is written.
    let names: Vec<Value> = s.tokens().await.iter().map(|t| t["name"].clone()).collect();
    assert_eq!(
        names,
        [json!("Bandit 1"), json!("Bandit 3"), json!("Chef bandit 1")]
    );
    assert_eq!(s.entries_of(&bandit_2).await, 0);
    assert_eq!(
        s.carousel().await,
        [
            place("Bandit 1", 1, 1),
            place("Bandit 3", 3, 2),
            place("Chef bandit 1", 4, 3),
        ]
    );
    // A turn on another participant stays where it is.
    assert_eq!(s.turn().await, (2, Some("Bandit 1".to_string())));
    // A soft delete: the engine keeps the token's row, tombstoned.
    assert_eq!(s.tombstoned(&bandit_2).await, Some(true));
    s.drop().await;
}

#[tokio::test]
async fn a_turn_still_on_the_token_removed_is_nullified_as_its_relation_declares() {
    let s = Scratch::new("turn").await;
    let (engine, app) = s.serve().await;
    dropped(&engine, &app).await;
    s.combat("Bandit 2").await;
    let bandit_2 = s.token("Bandit 2").await;
    remove(&engine, &app, &bandit_2).await;
    // BattlemapInstance.currentEntryId: onDelete nullify. The Battlemap sends
    // table.nextTurn first, so the turn has moved on before the removal.
    assert_eq!(s.turn().await, (2, None));
    assert_eq!(s.carousel().await.len(), 3);
    s.drop().await;
}

#[tokio::test]
async fn a_removal_the_table_lost_is_made_again_at_the_next_start() {
    let s = Scratch::new("repair").await;
    let bandit_2 = {
        let (engine, app) = s.serve().await;
        dropped(&engine, &app).await;
        s.combat("Bandit 1").await;
        let bandit_2 = s.token("Bandit 2").await;
        remove(&engine, &app, &bandit_2).await;
        bandit_2
    };
    let after = s.tokens().await;
    let carousel = s.carousel().await;
    s.client
        .execute(
            &format!(
                "INSERT INTO token (id, instance_id, character_id, number, name, q, r, visible,
                     hp_current, hp_max, conditions)
                 VALUES ('{bandit_2}', '{INSTANCE}', '{BANDIT}', 2, 'Bandit 2', 5, 5, true, 11, 11, '{{}}')"
            ),
            &[],
        )
        .await
        .expect("a removal lost");
    assert_eq!(s.tokens().await.len(), after.len() + 1);
    let _ = s.serve().await;
    assert_eq!(s.tokens().await, after);
    // The tokens still on the map keep their rows, and so their entries.
    assert_eq!(s.carousel().await, carousel);
    assert_eq!(carousel.len(), 3);
    assert_eq!(s.turn().await, (2, Some("Bandit 1".to_string())));
    s.drop().await;
}
