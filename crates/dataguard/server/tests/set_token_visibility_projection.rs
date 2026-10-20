//! table.setTokenVisibility over PostgreSQL (`DATABASE_URL`): a token hidden
//! or revealed, once applied, is `token.visible` and no other column — the
//! carousel's order (`initiative_order_view`) keeps every rank, the token
//! hidden from the players' side alone (`hidden-keeps-rank`) — and a
//! visibility the table lost is written again at the next start. Each test
//! works in a scratch database of its own, migrated by the Data layer,
//! dropped when it ends.

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
        let name = format!("set_token_visibility_test_{name}_{}", std::process::id());
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

/// The token named `name` hidden, `false`, or revealed, `true`, and applied.
async fn show(engine: &Engine, app: &Router, token: &str, visible: bool) {
    let (status, body) = command(
        app,
        "table.setTokenVisibility@1",
        json!({ "token": token, "visible": visible }),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    assert_eq!(applied(engine).await, 1);
}

/// Bandit 1, Bandit 2 and Chef bandit 1 dropped on the instance and applied.
async fn dropped(engine: &Engine, app: &Router) {
    for (character, count) in [(BANDIT, 2), (VESNA, 1)] {
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

#[tokio::test]
async fn a_hidden_token_is_token_visible_false_and_no_other_column_changes() {
    let s = Scratch::new("hide").await;
    let (engine, app) = s.serve().await;
    dropped(&engine, &app).await;
    let chief = s.token("Chef bandit 1").await;
    let before = s.tokens().await;
    show(&engine, &app, &chief, false).await;
    let expected: Vec<Value> = before
        .iter()
        .cloned()
        .map(|mut token| {
            if token["id"] == chief.as_str() {
                token["visible"] = json!(false);
            }
            token
        })
        .collect();
    assert_eq!(s.tokens().await, expected);
    show(&engine, &app, &chief, true).await;
    assert_eq!(s.tokens().await, before);
    s.drop().await;
}

#[tokio::test]
async fn hidden_keeps_rank_the_chief_revealed_is_second_and_no_other_place_moves() {
    let s = Scratch::new("rank").await;
    let (engine, app) = s.serve().await;
    dropped(&engine, &app).await;
    let (bandit_1, chief, bandit_2) = (
        s.token("Bandit 1").await,
        s.token("Chef bandit 1").await,
        s.token("Bandit 2").await,
    );
    show(&engine, &app, &chief, false).await;
    // The combat as a launch leaves it, the hidden chief second.
    s.client
        .batch_execute(&format!(
            "INSERT INTO combat (id, instance_id, round, started_at)
             VALUES ('{COMBAT}', '{INSTANCE}', 1, now());
             INSERT INTO initiative_entry (combat_id, token_id, roll, initiative, dex_score, tiebreak, rank) VALUES
                 ('{COMBAT}', '{bandit_1}', 17, 18, 12, 0.1, 1),
                 ('{COMBAT}', '{chief}', 14, 15, 12, 0.2, 2),
                 ('{COMBAT}', '{bandit_2}', 9, 10, 12, 0.3, 3);"
        ))
        .await
        .expect("the combat");
    let place = |name: &str, rank: i32, shown: bool| (name.to_string(), rank, rank, shown);
    assert_eq!(
        s.carousel().await,
        [
            place("Bandit 1", 1, true),
            place("Chef bandit 1", 2, false),
            place("Bandit 2", 3, true),
        ]
    );
    show(&engine, &app, &chief, true).await;
    assert_eq!(
        s.carousel().await,
        [
            place("Bandit 1", 1, true),
            place("Chef bandit 1", 2, true),
            place("Bandit 2", 3, true),
        ]
    );
    s.drop().await;
}

#[tokio::test]
async fn a_visibility_the_table_lost_is_written_again_at_the_next_start() {
    let s = Scratch::new("repair").await;
    {
        let (engine, app) = s.serve().await;
        dropped(&engine, &app).await;
        let chief = s.token("Chef bandit 1").await;
        show(&engine, &app, &chief, false).await;
    }
    let before = s.tokens().await;
    s.client
        .execute("UPDATE token SET visible = true", &[])
        .await
        .expect("a visibility lost");
    let _ = s.serve().await;
    assert_eq!(s.tokens().await, before);
    s.drop().await;
}
