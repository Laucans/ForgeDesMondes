//! table.placeTokens over PostgreSQL (`DATABASE_URL`): a drop on a battlemap
//! instance the tables hold is judged against them and against the queue
//! before it is queued, and each token it makes, once applied, is a row of
//! `token` — numbered after the numbers taken (`multi-drop-numbering`), an
//! NPC's HP copied from its sheet and a PC's left empty
//! (`pj-hp-not-tracked`), a PC placed once and only when present
//! (`token-roster`). A drop refused queues nothing. Each test works in a
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
const BRUME: &str = "c1000000-0000-4000-8000-000000000002";
const ELOWEN: &str = "e1000000-0000-4000-8000-000000000001";
const KAEL: &str = "e1000000-0000-4000-8000-000000000002";
const BANDIT: &str = "e1000000-0000-4000-8000-000000000003";
const SPECTRE: &str = "e1000000-0000-4000-8000-000000000004";
const TAVERNE: &str = "e1000000-0000-4000-8000-000000000005";
const OMBRE: &str = "e1000000-0000-4000-8000-000000000006";
const CARTE: &str = "6d000000-0000-4000-8000-000000000001";
const SEANCE: &str = "5e000000-0000-4000-8000-000000000001";
const ARRIVEE: &str = "5c000000-0000-4000-8000-000000000001";
const INSTANCE: &str = "b1a00000-0000-4000-8000-000000000001";
const NOWHERE: &str = "b1a00000-0000-4000-8000-0000000000ff";

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

/// The SQL adding element `id` of `campaign`, with the version every element
/// is written with, and its sheet for a PC or an NPC.
fn element(id: &str, campaign: &str, kind: &str, name: &str, hp_max: Option<i16>) -> String {
    let sheet = if kind == "lieu" {
        String::new()
    } else {
        let hp = hp_max.map_or("NULL".to_string(), |hp| hp.to_string());
        format!(
            "INSERT INTO character (entity_id, token_color, hp_max) VALUES ('{id}', '#888', {hp});"
        )
    };
    format!(
        "INSERT INTO entity (id, campaign_id, type, name) VALUES ('{id}', '{campaign}', '{kind}', '{name}'); \
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
    /// Migrated by the Data layer, holding the tavern's instance and who may
    /// stand on it: Elowen, a PC present at the session, Kael, a PC who is
    /// not, the Bandit, an NPC of 11 HP, the Ombre, an NPC with no sheet, and
    /// the Spectre, an NPC of another campaign. The instance is written as the tables hold it.
    async fn new(name: &str) -> Scratch {
        let name = format!("place_tokens_test_{name}_{}", std::process::id());
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
                "INSERT INTO campaign (id, name, kind) VALUES
                     ('{VALOMBRE}', 'Les Cendres de Valombre', 'campagne'),
                     ('{BRUME}', 'La Brume', 'oneshot');
                 {}{}{}{}{}
                 INSERT INTO entity (id, campaign_id, type, name) VALUES ('{OMBRE}', '{VALOMBRE}', 'pnj', 'Ombre');
                 INSERT INTO entity_version (entity_id, version_number, source, snapshot, diff)
                 VALUES ('{OMBRE}', 1, 'dm', '{{}}', '[]');
                 INSERT INTO media (id, campaign_id, kind, title, file_path, width_px, height_px, place_id)
                 VALUES ('{CARTE}', '{VALOMBRE}', 'battlemap', 'Taverne', 'carte.png', 2048, 1536, '{TAVERNE}');
                 INSERT INTO battlemap_grid (media_id, hex_size_px, offset_x, offset_y, orientation, color, opacity)
                 VALUES ('{CARTE}', 64, 0, 0, 'pointe', '#202020', 0.4);
                 INSERT INTO session (id, campaign_id, position, title, status, microphone_on)
                 VALUES ('{SEANCE}', '{VALOMBRE}', 1, 'Le Sanglier', 'live', true);
                 INSERT INTO session_present_character (session_id, character_id) VALUES ('{SEANCE}', '{ELOWEN}');
                 INSERT INTO scene (id, session_id, position, title) VALUES ('{ARRIVEE}', '{SEANCE}', 1, 'Arrivée');
                 INSERT INTO battlemap_instance (id, battlemap_media_id, session_id, scene_id)
                 VALUES ('{INSTANCE}', '{CARTE}', '{SEANCE}', '{ARRIVEE}');",
                element(ELOWEN, VALOMBRE, "pj", "Elowen", Some(38)),
                element(KAEL, VALOMBRE, "pj", "Kael", None),
                element(BANDIT, VALOMBRE, "pnj", "Bandit", Some(11)),
                element(SPECTRE, BRUME, "pnj", "Spectre", Some(22)),
                element(TAVERNE, VALOMBRE, "lieu", "La Taverne du Sanglier", None),
            ))
            .await
            .expect("the tavern's instance");
        let url = naming(&name);
        Scratch { client, name, url }
    }

    /// The engine as the server plants it over this database, and its wire
    /// protocol.
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

    /// The tokens of the instance as `token` holds them, by number then name.
    async fn tokens(&self) -> Vec<Value> {
        self.client
            .query(
                "SELECT jsonb_build_object('id', id, 'character', character_id, 'number', number,
                     'name', name, 'q', q, 'r', r, 'visible', visible, 'hpCurrent', hp_current,
                     'hpMax', hp_max, 'conditions', to_jsonb(conditions))::text
                 FROM token WHERE instance_id = $1::text::uuid ORDER BY character_id, number, name",
                &[&INSTANCE],
            )
            .await
            .expect("the tokens")
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

/// `POST /commands` of table.placeTokens on `instance`: its status and
/// answer.
async fn place(app: &Router, instance: &str, payload: Value) -> (StatusCode, Value) {
    place_keyed(app, instance, payload, None).await
}

/// [`place`], under the idempotency key `key` when one is given.
async fn place_keyed(
    app: &Router,
    instance: &str,
    payload: Value,
    key: Option<&str>,
) -> (StatusCode, Value) {
    let mut body = json!({
        "dataCapability": "table.placeTokens@1",
        "payload": payload,
        "target": { "id": instance },
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

/// Runs the applier once, every result applied: how many commands it ran.
async fn applied(engine: &Engine) -> usize {
    let results = engine.run_once().await.expect("the applier ran");
    for result in &results {
        assert_eq!(result.status, State::Applied, "{result:?}");
    }
    results.len()
}

fn summary(tokens: &[Value]) -> Vec<(Value, Value)> {
    tokens
        .iter()
        .map(|t| (t["number"].clone(), t["name"].clone()))
        .collect()
}

#[tokio::test]
async fn multi_drop_numbering_three_bandits_then_two_more_are_numbered_1_to_5() {
    let s = Scratch::new("numbering").await;
    let (engine, app) = s.serve().await;
    let (status, body) = place(
        &app,
        INSTANCE,
        json!({ "character": BANDIT, "q": 4, "r": -1, "count": 3 }),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    assert_eq!(applied(&engine).await, 3);
    let tokens = s.tokens().await;
    assert_eq!(
        summary(&tokens),
        [
            (json!(1), json!("Bandit 1")),
            (json!(2), json!("Bandit 2")),
            (json!(3), json!("Bandit 3")),
        ]
    );
    // pj-hp-not-tracked: each has the sheet's 11 HP, on the hex dropped on.
    for token in &tokens {
        assert_eq!(
            (&token["hpCurrent"], &token["hpMax"]),
            (&json!(11), &json!(11))
        );
        assert_eq!((&token["q"], &token["r"]), (&json!(4), &json!(-1)));
        assert_eq!(token["visible"], true);
        assert_eq!(token["conditions"], json!([]));
    }
    let (status, body) = place(
        &app,
        INSTANCE,
        json!({ "character": BANDIT, "q": 0, "r": 0, "count": 2 }),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    assert_eq!(applied(&engine).await, 2);
    let numbers: Vec<Value> = s
        .tokens()
        .await
        .iter()
        .map(|t| t["number"].clone())
        .collect();
    assert_eq!(numbers, [json!(1), json!(2), json!(3), json!(4), json!(5)]);
    s.drop().await;
}

#[tokio::test]
async fn multi_drop_numbering_a_drop_still_queued_takes_its_numbers() {
    let s = Scratch::new("queued").await;
    let (engine, app) = s.serve().await;
    for count in [2, 1] {
        let (status, body) = place(
            &app,
            INSTANCE,
            json!({ "character": BANDIT, "q": 1, "r": 1, "count": count }),
        )
        .await;
        assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    }
    assert_eq!(applied(&engine).await, 3);
    let numbers: Vec<Value> = s
        .tokens()
        .await
        .iter()
        .map(|t| t["number"].clone())
        .collect();
    assert_eq!(numbers, [json!(1), json!(2), json!(3)]);
    s.drop().await;
}

#[tokio::test]
async fn token_roster_a_pc_is_placed_once_and_only_when_present() {
    let s = Scratch::new("roster").await;
    let (engine, app) = s.serve().await;
    let elowen = json!({ "character": ELOWEN, "q": 2, "r": 3 });
    let (status, body) = place(&app, INSTANCE, elowen.clone()).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    // Placed again while its token is still queued, then once it is applied.
    let (status, body) = place(&app, INSTANCE, elowen.clone()).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(applied(&engine).await, 1);
    let (status, body) = place(&app, INSTANCE, elowen).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(
        body,
        json!({ "error": "ce PJ a déjà son token sur la carte" })
    );
    // pj-hp-not-tracked: no number, its name, no HP whatever the sheet says.
    let tokens = s.tokens().await;
    assert_eq!(tokens.len(), 1, "{tokens:?}");
    assert_eq!(tokens[0]["character"], ELOWEN);
    assert_eq!(tokens[0]["name"], "Elowen");
    assert_eq!(tokens[0]["number"], Value::Null);
    assert_eq!(
        (&tokens[0]["hpCurrent"], &tokens[0]["hpMax"]),
        (&Value::Null, &Value::Null)
    );
    // Kael is not present at the session.
    let (status, body) = place(&app, INSTANCE, json!({ "character": KAEL, "q": 0, "r": 0 })).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(
        body,
        json!({ "error": "ce PJ n'est pas présent à la séance" })
    );
    assert_eq!(applied(&engine).await, 0, "a refused drop queues nothing");
    assert_eq!(s.tokens().await.len(), 1);
    s.drop().await;
}

#[tokio::test]
async fn token_roster_tokens_queued_for_another_character_are_not_a_pc_s() {
    let s = Scratch::new("others").await;
    let (engine, app) = s.serve().await;
    let (status, body) = place(
        &app,
        INSTANCE,
        json!({ "character": BANDIT, "q": 1, "r": 1, "count": 2 }),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    // The bandits still queued are no token of Elowen's.
    let (status, body) = place(
        &app,
        INSTANCE,
        json!({ "character": ELOWEN, "q": 2, "r": 3 }),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    assert_eq!(applied(&engine).await, 3);
    s.drop().await;
}

#[tokio::test]
async fn a_drop_sent_again_under_its_key_is_replayed_as_its_last_token() {
    let s = Scratch::new("replayed").await;
    let (engine, app) = s.serve().await;
    let payload = json!({ "character": BANDIT, "q": 0, "r": 0, "count": 2 });
    let key = Some("table.placeTokens:drop-1");
    let (status, first) = place_keyed(&app, INSTANCE, payload.clone(), key).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{first}");
    // The answer is the last token's command, the one queued under the key.
    let (status, again) = place_keyed(&app, INSTANCE, payload, key).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{again}");
    assert_eq!(again["replayed"], true, "{again}");
    assert_eq!(again["commandId"], first["commandId"], "{again}");
    assert_eq!(applied(&engine).await, 2);
    assert_eq!(s.tokens().await.len(), 2);
    s.drop().await;
}

#[tokio::test]
async fn a_drop_of_what_the_instance_cannot_hold_queues_nothing() {
    let s = Scratch::new("refused").await;
    let (engine, app) = s.serve().await;
    for (instance, payload, expected) in [
        (
            NOWHERE,
            json!({ "character": BANDIT, "q": 0, "r": 0 }),
            StatusCode::NOT_FOUND,
        ),
        (
            INSTANCE,
            json!({ "character": TAVERNE, "q": 0, "r": 0 }),
            StatusCode::NOT_FOUND,
        ),
        // An NPC with no sheet: no token's foreign key could name it.
        (
            INSTANCE,
            json!({ "character": OMBRE, "q": 0, "r": 0 }),
            StatusCode::NOT_FOUND,
        ),
        (
            INSTANCE,
            json!({ "character": SPECTRE, "q": 0, "r": 0 }),
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
        (
            INSTANCE,
            json!({ "character": ELOWEN, "q": 0, "r": 0, "count": 2 }),
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
    ] {
        let (status, body) = place(&app, instance, payload.clone()).await;
        assert_eq!(status, expected, "{payload}: {body}");
    }
    assert_eq!(applied(&engine).await, 0);
    assert!(s.tokens().await.is_empty());
    s.drop().await;
}

#[tokio::test]
async fn a_token_the_table_lost_is_projected_again_at_the_next_start() {
    let s = Scratch::new("repair").await;
    {
        let (engine, app) = s.serve().await;
        let (status, body) = place(
            &app,
            INSTANCE,
            json!({ "character": BANDIT, "q": 0, "r": 0, "count": 2 }),
        )
        .await;
        assert_eq!(status, StatusCode::ACCEPTED, "{body}");
        assert_eq!(applied(&engine).await, 2);
    }
    let before = s.tokens().await;
    s.client
        .execute("DELETE FROM token WHERE number = 2", &[])
        .await
        .expect("a token lost");
    let _ = s.serve().await;
    assert_eq!(s.tokens().await, before);
    s.drop().await;
}
