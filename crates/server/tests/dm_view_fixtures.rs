//! What the DM view finds in the live session's fixtures, through the server as
//! the binary serves it: the engine over the same PostgreSQL as the tables, the
//! renaming around its routes. The PCs' last initiative is the seats of a combat
//! that ended, the player view has received its cover, and the keys of what is
//! sent and read are renamed as its values are. A `ScratchDb` of each test's own.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use dataguard_server::engine::command::State;
use dataguard_server::engine::engine::Engine;
use platform_scratch_db::ScratchDb;
use ports_character_sheets::SharedCharacterSheets;
use ports_character_sheets::fake::FakeCharacterSheets;
use ports_llm::SharedLlm;
use ports_llm::fake::FakeLlm;
use ports_speech_to_text::SharedSpeechToText;
use ports_speech_to_text::fake::FakeSpeechToText;
use serde_json::{Value, json};
use server::{Config, Names, app, fixtures, plant, renaming, shared};
use tower::ServiceExt;

fn folder() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../mock/fixtures")
}

fn fixture(name: &str) -> Value {
    serde_json::from_str(&std::fs::read_to_string(folder().join(name)).expect("a fixture"))
        .expect("JSON")
}

/// The server as the binary plants it over a scratch database migrated and
/// loaded with the fixtures — the engine and the reads on the one PostgreSQL —
/// its routes renamed by the fixtures' slugs.
async fn served(label: &str) -> (ScratchDb, Arc<Engine>, Router) {
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
    let url = db.url().to_string();
    let config = Config::read(|var| match var {
        "DATABASE_URL" | "DATABASE_URL_READONLY" => Some(url.clone()),
        _ => None,
    })
    .expect("a valid configuration");
    let llm = SharedLlm::new(FakeLlm::new());
    let speech = SharedSpeechToText::new(FakeSpeechToText::new(String::new()));
    let sheets = SharedCharacterSheets::new(FakeCharacterSheets::new());
    let planted = plant(&config, shared(&llm, &speech, &sheets))
        .await
        .expect("the engine");
    let engine = Arc::clone(&planted.engine);
    let app = renaming(app(&planted, Arc::new(llm)), Names::new(plan.names));
    (db, engine, app)
}

/// What `app` answers to `body` on `uri` (`POST`).
async fn post(app: &Router, uri: &str, body: &Value) -> (StatusCode, Value) {
    let request = Request::post(uri)
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .expect("a request");
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

/// What `app` answers to a read of `capability` with `variables`.
async fn read(app: &Router, capability: &str, variables: Value) -> Value {
    let (status, answer) = post(
        app,
        &format!("/capabilities/{capability}"),
        &json!({ "variables": variables }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{capability}: {answer}");
    answer["data"].clone()
}

/// `body` sent to `/commands` as the DM; accepted, then every command queued
/// (the follow-ups too) applied. The answer.
async fn applied(engine: &Engine, app: &Router, body: Value) -> Value {
    let (status, answer) = post(app, "/commands", &body).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{answer}");
    loop {
        let settled = engine.run_once().await.expect("the applier ran");
        if settled.is_empty() {
            return answer;
        }
        assert!(
            settled.iter().all(|r| r.status == State::Applied),
            "{settled:?}"
        );
    }
}

fn dm() -> Value {
    json!({ "by": "dm", "from": "Battlemap", "roles": ["role:approver"] })
}

fn instance(view: &str) -> Value {
    json!({ "instance": "bmi-gue-s4", "view": view })
}

// AC1: the instance of the live scene, freshly loaded, gives each PC's last
// initiative by character slug and runs no combat
#[tokio::test]
async fn the_dm_finds_each_pcs_last_initiative_by_name_and_no_combat_running() {
    let (_db, _engine, app) = served("dm_view_initiative").await;

    for view in ["dm", "player"] {
        let found = read(&app, "table.getBattlemapInstance", instance(view)).await;
        assert_eq!(
            found["pjInitiative"],
            json!({ "brannoc": 9, "elowen": 18, "kael": 12, "sirielle": 14 }),
            "{view}: {found}"
        );
        assert_eq!(
            found["pjInitiative"],
            fixture("table.getBattlemapInstance.json")["pjInitiative"]
        );
        assert_eq!(found["state"]["combat"], json!(null), "{view}: {found}");
    }
}

// AC1: the combat starts from them — the start sent by the PCs' names is
// applied, its eight seats too — and the next start remembers it
#[tokio::test]
async fn a_combat_starts_on_the_instance_the_fixtures_loaded_from_the_pcs_initiative() {
    let (_db, engine, app) = served("dm_view_start_combat").await;
    let initiative = fixture("table.getBattlemapInstance.json")["pjInitiative"].clone();

    let answer = applied(
        &engine,
        &app,
        json!({
            "dataCapability": "table.startCombat@1",
            "target": { "aggregate": "BattlemapInstance", "id": "bmi-gue-s4" },
            "payload": { "initiative": initiative },
            "caller": dm(),
        }),
    )
    .await;
    assert_eq!(answer["warnings"], json!([]), "{answer}");

    let found = read(&app, "table.getBattlemapInstance", instance("dm")).await;
    let combat = &found["state"]["combat"];
    assert_eq!(combat["round"], 1, "{found}");
    let order = combat["order"].as_array().expect("the order");
    let tokens = found["state"]["tokens"].as_array().expect("the tokens");
    assert_eq!(order.len(), tokens.len(), "every token is seated: {found}");
    assert_eq!(order.len(), 8, "{found}");
    // The PCs seat as they were entered: Elowen, Sirielle, Kael, Brannoc, among the NPCs.
    let pc_seats = |seat: &&Value| {
        tokens
            .iter()
            .any(|token| token["id"] == seat["id"] && token["kind"] == "pj")
    };
    let ranked: Vec<&Value> = order
        .iter()
        .filter(pc_seats)
        .map(|seat| &seat["id"])
        .collect();
    assert_eq!(
        ranked,
        [
            &json!("t-elowen"),
            &json!("t-sirielle"),
            &json!("t-kael"),
            &json!("t-brannoc")
        ],
        "{found}"
    );
    // The initiative the DM entered is the one read back, by name, for the next.
    assert_eq!(found["pjInitiative"], initiative, "{found}");
}

// AC2: the player view of the session under way has received its cover, no
// window; a window opened on it is applied, not refused
#[tokio::test]
async fn the_player_view_has_the_cover_and_a_window_opened_on_it_is_applied() {
    let (_db, engine, app) = served("dm_view_cover").await;

    let shown = read(&app, "live.getDiffusion", json!({})).await;
    // The campaign's cover is no row the fixtures load (cover_media_id and the media cycle).
    assert_eq!(shown["base"]["kind"], "cover", "{shown}");
    assert_eq!(shown["window"], json!(null), "{shown}");
    assert_eq!(shown["view"]["zoom"], 1.0, "the framing at rest: {shown}");

    applied(
        &engine,
        &app,
        json!({
            "dataCapability": "live.showWindowOnPlayerView@1",
            "target": { "aggregate": "Diffusion" },
            "payload": { "campaign": "cendres-valombre", "media": "ill-gaspard" },
            "caller": dm(),
        }),
    )
    .await;

    let shown = read(&app, "live.getDiffusion", json!({})).await;
    assert_eq!(shown["window"]["media"], "ill-gaspard", "{shown}");
    assert_eq!(
        shown["base"]["kind"], "cover",
        "the base is not touched: {shown}"
    );
}
