//! The session under way, its scenes and the battlemap instance of its live
//! scene, loaded from the mock-up's fixtures and read back through the server
//! by the names the Micro-UIs know them by. A `ScratchDb` of each test's own.

use std::path::Path;
use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use dataguard_server::MEMORY;
use platform_scratch_db::ScratchDb;
use ports_character_sheets::SharedCharacterSheets;
use ports_character_sheets::fake::FakeCharacterSheets;
use ports_llm::SharedLlm;
use ports_llm::fake::FakeLlm;
use ports_speech_to_text::SharedSpeechToText;
use ports_speech_to_text::fake::FakeSpeechToText;
use serde_json::{Value, json};
use server::{Config, Names, app, fixtures, id as uuid_of, plant, renaming, shared};
use tower::ServiceExt;

fn folder() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../mock/fixtures")
}

fn fixture(name: &str) -> Value {
    serde_json::from_str(&std::fs::read_to_string(folder().join(name)).expect("a fixture"))
        .expect("JSON")
}

/// A scratch database migrated and loaded with the fixtures, once.
async fn loaded(label: &str) -> (ScratchDb, data::Client) {
    let db = ScratchDb::create(label).await.expect("a scratch database");
    let mut client = db.connect().await.expect("PostgreSQL");
    let migrations = Path::new(env!("CARGO_MANIFEST_DIR")).join("../data/migrations");
    data::migrate(&mut client, &migrations)
        .await
        .expect("migrated");
    let plan = fixtures(&folder()).expect("the fixtures");
    data::insert_missing(&mut client, &plan.tables)
        .await
        .expect("the fixtures are inserted");
    (db, client)
}

/// What `app` answers to a read of `capability` with `variables`.
async fn read(app: &axum::Router, capability: &str, variables: Value) -> Value {
    let request = Request::post(format!("/capabilities/{capability}"))
        .header("content-type", "application/json")
        .body(Body::from(json!({ "variables": variables }).to_string()))
        .expect("a request");
    let response = app.clone().oneshot(request).await.expect("an answer");
    assert_eq!(response.status(), StatusCode::OK, "{capability}");
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("a body");
    serde_json::from_slice::<Value>(&bytes).expect("JSON")["data"].clone()
}

/// The server as the binary plants it over `db`, its reads named by the fixtures' slugs.
async fn served(db: &ScratchDb) -> axum::Router {
    // `session.listScenes` leaves out a tombstoned battlemap, in the engine's rows table.
    db.connect()
        .await
        .expect("PostgreSQL")
        .batch_execute(
            "CREATE TABLE IF NOT EXISTS dataguard_rows (partition text PRIMARY KEY, \
             fields jsonb NOT NULL DEFAULT '{}'::jsonb, tombstoned boolean NOT NULL DEFAULT false)",
        )
        .await
        .expect("the engine's rows table");
    let url = db.url().to_string();
    let config = Config::read(|var| match var {
        "DATABASE_URL" => Some(MEMORY.to_string()),
        "DATABASE_URL_READONLY" => Some(url.clone()),
        _ => None,
    })
    .expect("a valid configuration");
    let llm = SharedLlm::new(FakeLlm::new());
    let speech = SharedSpeechToText::new(FakeSpeechToText::new(String::new()));
    let sheets = SharedCharacterSheets::new(FakeCharacterSheets::new());
    let planted = plant(&config, shared(&llm, &speech, &sheets))
        .await
        .expect("the in-memory engine");
    let names = fixtures(&folder()).expect("the fixtures").names;
    renaming(app(&planted, Arc::new(llm)), Names::new(names))
}

#[tokio::test]
async fn the_real_store_has_the_session_under_way_its_scenes_and_the_live_battlemap() {
    let (db, _client) = loaded("live_session_reads").await;
    let served = served(&db).await;

    // AC1: with no id, the running session is s4, live.
    let live = read(&served, "session.getLiveSession", json!({})).await;
    assert_eq!(live["session"]["id"], "s4", "{live}");
    assert_eq!(live["session"]["status"], "live", "{live}");
    assert_eq!(live["session"]["startedAt"], "20:02", "{live}");

    // AC2: the scenes of the board, in order, sc3 naming its battlemap.
    let scenes = read(&served, "session.listScenes", json!({ "session": "s4" })).await;
    let board = fixture("SceneBoard.json");
    let expected: Vec<&Value> = board["scenes"]
        .as_array()
        .expect("scenes")
        .iter()
        .map(|s| &s["id"])
        .collect();
    let listed: Vec<&Value> = scenes
        .as_array()
        .expect("a list")
        .iter()
        .map(|s| &s["id"])
        .collect();
    assert_eq!(listed, expected, "{scenes}");
    let sc3 = &scenes[2];
    assert_eq!(sc3["id"], "sc3");
    assert_eq!(sc3["battlemap"], "bmi-gue-s4", "{sc3}");

    // AC3: the instance, with its PCs and the three bandits at 11/11.
    let instance = read(
        &served,
        "table.getBattlemapInstance",
        json!({ "instance": "bmi-gue-s4", "view": "dm" }),
    )
    .await;
    assert_eq!(instance["id"], "bmi-gue-s4", "{instance}");
    let tokens = instance["state"]["tokens"].as_array().expect("tokens");
    let token = |name: &str| {
        tokens
            .iter()
            .find(|t| t["name"] == name)
            .unwrap_or_else(|| panic!("no token {name}: {instance}"))
    };
    assert_eq!(token("Elowen")["kind"], "pj");
    for bandit in ["Bandit 1", "Bandit 2", "Bandit 3"] {
        assert_eq!(token(bandit)["hp"], 11, "{bandit}");
        assert_eq!(token(bandit)["hpMax"], 11, "{bandit}");
    }
}

#[tokio::test]
async fn a_restart_loads_nothing_twice_and_leaves_the_session_the_dm_closed() {
    let (db, mut client) = loaded("live_session_restart").await;
    let count = |sql: &'static str| {
        let client = &client;
        async move {
            client
                .query_one(sql, &[])
                .await
                .expect("a count")
                .get::<_, i64>(0)
        }
    };
    let before = (
        count("SELECT count(*) FROM session").await,
        count("SELECT count(*) FROM scene").await,
        count("SELECT count(*) FROM battlemap_instance").await,
        count("SELECT count(*) FROM token").await,
    );
    // Four sessions of the list and the one a oneshot's creation writes.
    assert_eq!(before, (5, 3, 1, 8));

    // AC4: the same plan again inserts nothing.
    let again = fixtures(&folder()).expect("the fixtures");
    assert_eq!(
        data::insert_missing(&mut client, &again.tables)
            .await
            .expect("planned again"),
        0
    );

    // AC4: the DM closed the session since; the load leaves their row alone.
    let s4 = uuid_of("session", "s4");
    client
        .batch_execute(&format!(
            "UPDATE session SET status = 'closed', closed_at = now() WHERE id = '{s4}'"
        ))
        .await
        .expect("the DM closes the session");
    let restarted = fixtures(&folder()).expect("the fixtures");
    assert_eq!(
        data::insert_missing(&mut client, &restarted.tables)
            .await
            .expect("planned after the restart"),
        0
    );
    let row = client
        .query_one(
            &format!("SELECT status, started_at IS NOT NULL FROM session WHERE id = '{s4}'"),
            &[],
        )
        .await
        .expect("the session");
    assert_eq!(row.get::<_, String>(0), "closed", "the DM's row is kept");
    assert!(row.get::<_, bool>(1), "and so is its start time");
    let served = served(&db).await;
    assert_eq!(
        read(&served, "session.getLiveSession", json!({})).await,
        Value::Null,
        "no session is under way once the DM closed it"
    );
}
