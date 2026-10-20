//! table.setTokenCondition over PostgreSQL (`DATABASE_URL`): the conditions
//! the engine applies on `BattlemapInstance/<a token's id>` reach
//! `token.conditions`, and no other column nor row — Bandit 1 and 3 keep
//! theirs (`instance-isolated`); conditions the table lost are written again
//! at the next start (`state-persisted-reset`). The tokens are dropped by a test-only
//! DataCapability, into the engine and into the table alike: the product's
//! drop, and its projection, are table.placeTokens'. Each test works in a
//! scratch database of its own, migrated as the server migrates it, dropped
//! when it ends.

use std::path::Path;
use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use dataguard_server::{Engine, Planted, planted, router};
use harness_dataguard::api::Submission;
use harness_dataguard::command::{CommandResult, State};
use harness_dataguard::guard::Caller;
use harness_dataguard::manifest::DataCapability;
use serde_json::{Value, json};
use tokio_postgres::error::SqlState;
use tokio_postgres::{Client, NoTls};
use tower::ServiceExt;

const VALOMBRE: &str = "c1000000-0000-4000-8000-000000000001";
const BANDIT: &str = "e1000000-0000-4000-8000-0000000000b1";
const CARTE: &str = "d1000000-0000-4000-8000-000000000001";
const SEANCE: &str = "51000000-0000-4000-8000-000000000001";
const ARRIVEE: &str = "5c000000-0000-4000-8000-000000000001";
const INSTANCE: &str = "b1000000-0000-4000-8000-000000000001";
const BANDITS: [&str; 3] = [
    "70000000-0000-4000-8000-000000000001",
    "70000000-0000-4000-8000-000000000002",
    "70000000-0000-4000-8000-000000000003",
];

/// The test-only drop: a token inserted whole on its own partition.
const DROP_TOKEN: &str = r#"{"dataCapability":"test.dropToken","owner":"dataguard","version":1,"effect":"insert","target":{"aggregate":"BattlemapInstance","id":"$tokenId"},"mode":"overwrite","callableBy":["*"]}"#;

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

/// Bandit `number`'s token, as the drop writes it.
fn bandit(number: i64) -> Value {
    json!({
        "tokenId": BANDITS[usize::try_from(number - 1).expect("1 to 3")],
        "tokenInstanceId": INSTANCE,
        "characterId": BANDIT,
        "number": number,
        "name": format!("Bandit {number}"),
        "q": 5,
        "r": 5,
        "visible": true,
        "hpCurrent": 11,
        "hpMax": 11,
        "conditions": [],
    })
}

/// A database of its own, migrated by the Data layer, holding the tavern's
/// instance and Bandit 1 to 3 in its `token` table.
struct Scratch {
    name: String,
    url: String,
    client: Client,
}

impl Scratch {
    async fn new(name: &str) -> Scratch {
        let name = format!("set_token_condition_test_{name}_{}", std::process::id());
        let admin = connect(&url()).await;
        for sql in [
            format!("DROP DATABASE IF EXISTS {name} WITH (FORCE)"),
            format!("CREATE DATABASE {name}"),
        ] {
            admin.batch_execute(&sql).await.expect("a scratch database");
        }
        let url = naming(&name);
        let mut client = connect(&url).await;
        let migrations = Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../data/migrations"
        ));
        data::migrate(&mut client, migrations)
            .await
            .expect("migrated by the Data layer");
        client
            .batch_execute(&format!(
                "INSERT INTO campaign (id, name, kind) VALUES ('{VALOMBRE}', 'Les Cendres de Valombre', 'campagne');
                 INSERT INTO entity (id, campaign_id, type, name) VALUES ('{BANDIT}', '{VALOMBRE}', 'pnj', 'Bandit');
                 INSERT INTO character (entity_id, token_color, hp_max) VALUES ('{BANDIT}', '#888', 11);
                 INSERT INTO entity_version (entity_id, version_number, source, snapshot, diff)
                     VALUES ('{BANDIT}', 1, 'dm', '{{}}', '[]');
                 INSERT INTO media (id, campaign_id, kind, title, file_path, width_px, height_px)
                     VALUES ('{CARTE}', '{VALOMBRE}', 'battlemap', 'Taverne', 'carte.png', 2048, 1536);
                 INSERT INTO battlemap_grid (media_id, hex_size_px, offset_x, offset_y, orientation, color, opacity)
                     VALUES ('{CARTE}', 64, 0, 0, 'pointe', '#202020', 0.4);
                 INSERT INTO session (id, campaign_id, position, title, status, microphone_on)
                     VALUES ('{SEANCE}', '{VALOMBRE}', 1, 'Le Sanglier', 'live', true);
                 INSERT INTO scene (id, session_id, position, title) VALUES ('{ARRIVEE}', '{SEANCE}', 1, 'Arrivée');
                 INSERT INTO battlemap_instance (id, battlemap_media_id, session_id, scene_id)
                     VALUES ('{INSTANCE}', '{CARTE}', '{SEANCE}', '{ARRIVEE}');"
            ))
            .await
            .expect("the tavern's instance");
        for number in 1..=3 {
            client
                .execute(
                    "INSERT INTO token (id, instance_id, character_id, number, name, q, r, visible,
                         hp_current, hp_max, conditions)
                     SELECT id, instance_id, character_id, number, name, q, r, visible,
                         hp_current, hp_max, conditions
                     FROM json_populate_record(NULL::token, $1::text::json)",
                    &[&json!({
                        "id": BANDITS[usize::try_from(number - 1).expect("1 to 3")],
                        "instance_id": INSTANCE,
                        "character_id": BANDIT,
                        "number": number,
                        "name": format!("Bandit {number}"),
                        "q": 5,
                        "r": 5,
                        "visible": true,
                        "hp_current": 11,
                        "hp_max": 11,
                        "conditions": [],
                    })
                    .to_string()],
                )
                .await
                .expect("a Bandit in the table");
        }
        Scratch { name, url, client }
    }

    /// The engine as the server plants it over this database, with the
    /// test-only drop, and its wire protocol.
    async fn serve(&self) -> (Arc<Engine>, Router) {
        let Planted { engine, rows } = planted(&self.url)
            .await
            .expect("the engine over PostgreSQL");
        let engine = Arc::new(
            engine.with_data_capability(DataCapability::parse(DROP_TOKEN).expect("contract F")),
        );
        let app = router(
            engine.clone(),
            rows,
            Arc::new(ports_llm::fake::FakeLlm::new()),
        );
        (engine, app)
    }

    /// The tavern's tokens as the table holds them: id and conditions.
    async fn tokens(&self) -> Vec<(String, Vec<String>)> {
        self.client
            .query("SELECT id::text, conditions FROM token ORDER BY id", &[])
            .await
            .expect("the tokens")
            .iter()
            .map(|row| (row.get(0), row.get(1)))
            .collect()
    }

    /// Drops the database, tried again while an autovacuum worker finishes.
    async fn drop(self) {
        drop(self.client);
        let admin = connect(&url()).await;
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

/// One pass of the single applier, which settles one command.
async fn settled(engine: &Engine) -> CommandResult {
    let mut settled = engine.run_once().await.expect("the applier ran");
    assert_eq!(settled.len(), 1, "{settled:?}");
    settled.remove(0)
}

/// Bandit 1 to 3 dropped in the engine, as the table holds them.
async fn dropped(engine: &Engine) {
    for number in 1..=3 {
        let Value::Object(payload) = bandit(number) else {
            unreachable!("a payload is an object");
        };
        engine
            .submit(Submission {
                data_capability: "test.dropToken".to_string(),
                payload: payload.into_iter().collect(),
                target_id: None,
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
        assert_eq!(settled(engine).await.status, State::Applied);
    }
}

/// `POST /commands` putting the badge `state` on `token` of the tavern's
/// instance, or taking it off, applied.
async fn marked(engine: &Engine, app: &Router, token: &str, state: &str, on: bool) {
    let body = json!({
        "dataCapability": "table.setTokenCondition@1",
        "target": { "id": INSTANCE },
        "payload": { "token": token, "state": state, "on": on },
        "caller": { "by": "dm", "from": "table.Battlemap" },
    });
    let request = Request::post("/commands")
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .expect("a request");
    let response = app.clone().oneshot(request).await.expect("an answer");
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    let result = settled(engine).await;
    assert_eq!(result.status, State::Applied, "{result:?}");
}

fn expected(conditions: [&[&str]; 3]) -> Vec<(String, Vec<String>)> {
    BANDITS
        .into_iter()
        .zip(conditions)
        .map(|(id, held)| {
            (
                id.to_owned(),
                held.iter().map(|state| (*state).to_owned()).collect(),
            )
        })
        .collect()
}

#[tokio::test]
async fn a_badge_reaches_its_token_s_conditions_alone_instance_isolated() {
    let s = Scratch::new("mark").await;
    let (engine, app) = s.serve().await;
    dropped(&engine).await;
    assert_eq!(s.tokens().await, expected([&[], &[], &[]]));

    marked(&engine, &app, BANDITS[1], "Empoisonné", true).await;
    marked(&engine, &app, BANDITS[1], "À terre", true).await;
    assert_eq!(
        s.tokens().await,
        expected([&[], &["Empoisonné", "À terre"], &[]])
    );
    marked(&engine, &app, BANDITS[1], "Empoisonné", false).await;
    assert_eq!(s.tokens().await, expected([&[], &["À terre"], &[]]));
    // And every other column of the three is as it was.
    let others = s
        .client
        .query_one(
            "SELECT count(*) FROM token WHERE instance_id = $1::text::uuid AND character_id = $2::text::uuid
                 AND q = 5 AND r = 5 AND visible AND hp_current = 11 AND hp_max = 11
                 AND name = 'Bandit ' || number",
            &[&INSTANCE, &BANDIT],
        )
        .await
        .expect("the tokens")
        .get::<_, i64>(0);
    assert_eq!(others, 3);
    s.drop().await;
}

#[tokio::test]
async fn conditions_the_table_lost_are_written_again_at_the_next_start_state_persisted_reset() {
    let s = Scratch::new("repair").await;
    {
        let (engine, app) = s.serve().await;
        dropped(&engine).await;
        marked(&engine, &app, BANDITS[1], "Empoisonné", true).await;
    }
    s.client
        .batch_execute(&format!(
            "UPDATE token SET conditions = '{{}}' WHERE id = '{}'",
            BANDITS[1]
        ))
        .await
        .expect("the conditions lost");
    // A start projects what the engine holds and the table does not.
    let _ = s.serve().await;
    assert_eq!(s.tokens().await, expected([&[], &["Empoisonné"], &[]]));
    s.drop().await;
}
