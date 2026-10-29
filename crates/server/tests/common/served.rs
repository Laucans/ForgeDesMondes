//! What the tests of the session under way share: the fixtures' folder, the
//! server as the binary plants it over a scratch database, and a read of it.

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
use server::{Config, Names, app, fixtures, plant, renaming, shared};
use tower::ServiceExt;

pub fn folder() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../mock/fixtures")
}

/// What `app` answers to a read of `capability` with `variables`.
pub async fn read(app: &axum::Router, capability: &str, variables: Value) -> Value {
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
pub async fn served(db: &ScratchDb) -> axum::Router {
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
