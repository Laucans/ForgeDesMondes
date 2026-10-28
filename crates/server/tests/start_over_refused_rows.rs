//! A server started over a database whose engine rows the BattlemapInstance
//! tables refuse (migration 0014's unique indexes) still starts: the
//! aggregate's `warm` logs the partition a refused write leaves and goes on,
//! and `GET /health` answers. A `ScratchDb` of the test's own.

use std::path::Path;
use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use platform_scratch_db::ScratchDb;
use ports_character_sheets::SharedCharacterSheets;
use ports_character_sheets::fake::FakeCharacterSheets;
use ports_llm::SharedLlm;
use ports_llm::fake::FakeLlm;
use ports_speech_to_text::SharedSpeechToText;
use ports_speech_to_text::fake::FakeSpeechToText;
use server::{Config, app, plant, shared};
use tower::ServiceExt;

const VALOMBRE: &str = "c1000000-0000-4000-8000-000000000001";
const SEANCE: &str = "5e000000-0000-4000-8000-000000000001";
const SCENE: &str = "5c000000-0000-4000-8000-000000000001";
const OTHER_SCENE: &str = "5c000000-0000-4000-8000-000000000002";
const GUE: &str = "d1000000-0000-4000-8000-00000000000a";
const INSTANCE: &str = "b1000000-0000-4000-8000-000000000001";
const SECOND_INSTANCE: &str = "b1000000-0000-4000-8000-000000000002";
const OTHER_INSTANCE: &str = "b1000000-0000-4000-8000-000000000003";

/// The server as the binary plants it over the database `url` names, on the
/// ports' fakes: its engine opens and warms every aggregate's projection.
async fn started(url: &str) -> axum::Router {
    let config = Config::read(|var| match var {
        "DATABASE_URL" | "DATABASE_URL_READONLY" => Some(url.to_string()),
        _ => None,
    })
    .expect("a valid configuration");
    let llm = SharedLlm::new(FakeLlm::new());
    let speech = SharedSpeechToText::new(FakeSpeechToText::new(String::new()));
    let sheets = SharedCharacterSheets::new(FakeCharacterSheets::new());
    let planted = plant(&config, shared(&llm, &speech, &sheets))
        .await
        .expect("the server plants over the database");
    app(&planted, Arc::new(llm))
}

// AC9: the server starts over engine rows the unique indexes refuse, and
// answers GET /health.
#[tokio::test]
async fn the_server_starts_and_answers_health_over_instances_the_unique_index_refuses() {
    let db = ScratchDb::create("start_over_refused_rows")
        .await
        .expect("a scratch database");
    let mut client = db.connect().await.expect("PostgreSQL");
    let migrations = Path::new(env!("CARGO_MANIFEST_DIR")).join("../data/migrations");
    data::migrate(&mut client, &migrations)
        .await
        .expect("migrated");
    client
        .batch_execute(&format!(
            "INSERT INTO campaign (id, name, kind) VALUES
                 ('{VALOMBRE}', 'Les Cendres de Valombre', 'campagne');
             INSERT INTO session (id, campaign_id, position, title, status, microphone_on) VALUES
                 ('{SEANCE}', '{VALOMBRE}', 1, 'L''embuscade du Gué', 'live', false);
             INSERT INTO scene (id, session_id, position, title) VALUES
                 ('{SCENE}', '{SEANCE}', 1, 'Le gué'),
                 ('{OTHER_SCENE}', '{SEANCE}', 2, 'La forêt');
             INSERT INTO media (id, campaign_id, kind, title, file_path, width_px, height_px) VALUES
                 ('{GUE}', '{VALOMBRE}', 'battlemap', 'Le Gué des Corbeaux', 'media/gue.png', 960, 640);
             INSERT INTO battlemap_grid (media_id, hex_size_px, offset_x, offset_y, orientation, color, opacity) VALUES
                 ('{GUE}', 30, 12, 8, 'pointe', 'text', 0.4);"
        ))
        .await
        .expect("Valombre");

    // A first start makes the engine's own tables.
    drop(started(db.url()).await);
    // Two instances of one scene and one of another, in the engine's rows
    // alone: the tables hold none of them.
    for (id, scene) in [
        (INSTANCE, SCENE),
        (SECOND_INSTANCE, SCENE),
        (OTHER_INSTANCE, OTHER_SCENE),
    ] {
        let fields = serde_json::json!({
            "battlemapInstanceId": id, "battlemapMediaId": GUE, "sessionId": SEANCE,
            "sceneId": scene,
        });
        client
            .execute(
                "INSERT INTO dataguard_rows (partition, fields) VALUES ($1, $2::text::jsonb)",
                &[&format!("BattlemapInstance/{id}"), &fields.to_string()],
            )
            .await
            .expect("the engine's row");
    }

    let served = started(db.url()).await;

    let response = served
        .oneshot(
            Request::get("/health")
                .body(Body::empty())
                .expect("a request"),
        )
        .await
        .expect("an answer");
    assert_eq!(response.status(), StatusCode::OK);
    // The scene's second instance was refused and skipped, the others landed.
    let landed: Vec<String> = client
        .query("SELECT id::text FROM battlemap_instance ORDER BY id", &[])
        .await
        .expect("the instances")
        .iter()
        .map(|row| row.get(0))
        .collect();
    assert_eq!(landed, [INSTANCE, OTHER_INSTANCE]);
}
