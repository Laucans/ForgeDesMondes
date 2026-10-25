//! campaign.exportCampaign through the server's wire, as the server plants
//! its units: the export's `Prepare` reads the tables over the engine's
//! read-only connection, the Campaign aggregate hydrates a campaign the
//! fixtures wrote and projects its stamp, and `GET /commands/<id>` answers
//! the archive as the command's attachment once applied — never before.
//! The export and its aggregate are tested in their own crates
//! (`crates/campaign/data-capabilities/export-campaign`,
//! `crates/campaign/aggregates/Campaign`); this is their composition. Over
//! PostgreSQL (`DATABASE_URL`), each test works in a scratch database of its
//! own, migrated as the server migrates it, dropped when it ends.

use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use dataguard_server::MEMORY;
use dataguard_server::engine::command::{CommandResult, State};
use dataguard_server::engine::engine::Engine;
use dataguard_server::engine::tokio_postgres::{self, Client, NoTls};
use ports_character_sheets::SharedCharacterSheets;
use ports_character_sheets::fake::FakeCharacterSheets;
use ports_llm::SharedLlm;
use ports_llm::fake::FakeLlm;
use ports_speech_to_text::SharedSpeechToText;
use ports_speech_to_text::fake::FakeSpeechToText;
use serde_json::{Value, json};
use server::{Config, app, plant, shared};
use tower::ServiceExt;

const MIGRATIONS: [&str; 6] = [
    include_str!("../../data/migrations/0002_campaign.sql"),
    include_str!("../../data/migrations/0003_player.sql"),
    include_str!("../../data/migrations/0004_entity.sql"),
    include_str!("../../data/migrations/0005_media.sql"),
    include_str!("../../data/migrations/0006_session.sql"),
    include_str!("../../data/migrations/0007_sound.sql"),
];

const VALOMBRE: &str = "c1000000-0000-4000-8000-000000000001";
const CLAIRE: &str = "b1000000-0000-4000-8000-000000000001";
const NOWHERE: &str = "c1000000-0000-4000-8000-0000000000ff";

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

/// A database of its own, migrated, holding — as the fixtures write them —
/// Valombre and Claire, its player.
struct Scratch {
    name: String,
    url: String,
    client: Client,
}

impl Scratch {
    async fn new(name: &str) -> Scratch {
        let name = format!("export_campaign_test_{name}_{}", std::process::id());
        let admin = connect(&url()).await;
        for sql in [
            format!("DROP DATABASE IF EXISTS {name} WITH (FORCE)"),
            format!("CREATE DATABASE {name}"),
        ] {
            admin.batch_execute(&sql).await.expect("a scratch database");
        }
        let url = naming(&name);
        let client = connect(&url).await;
        for migration in MIGRATIONS {
            client.batch_execute(migration).await.expect("migrated");
        }
        client
            .batch_execute(&format!(
                "INSERT INTO campaign (id, name, kind) VALUES ('{VALOMBRE}', 'Les Cendres de Valombre', 'campagne');
                 INSERT INTO player (id, campaign_id, name) VALUES ('{CLAIRE}', '{VALOMBRE}', 'Claire');"
            ))
            .await
            .expect("Valombre and Claire");
        Scratch { name, url, client }
    }

    /// Whether `campaign`'s last export is the instant `at` names.
    async fn exported_at(&self, campaign: &str, at: &str) -> bool {
        self.client
            .query_one(
                "SELECT last_exported_at IS NOT DISTINCT FROM ($2::text)::timestamptz
                 FROM campaign WHERE id = ($1::text)::uuid",
                &[&campaign, &at],
            )
            .await
            .expect("the campaign")
            .get(0)
    }

    async fn drop(self) {
        drop(self.client);
        connect(&url())
            .await
            .batch_execute(&format!("DROP DATABASE {} WITH (FORCE)", self.name))
            .await
            .expect("the scratch database dropped");
    }
}

/// The engine as the server plants it over `url`, and its wire protocol.
async fn serve(url: &str) -> (Arc<Engine>, Router) {
    let config = Config::read(|var| (var == "DATABASE_URL").then(|| url.to_string()))
        .expect("a valid configuration");
    let llm = SharedLlm::new(FakeLlm::new());
    let speech = SharedSpeechToText::new(FakeSpeechToText::new(String::new()));
    let sheets = SharedCharacterSheets::new(FakeCharacterSheets::new());
    let planted = plant(&config, shared(&llm, &speech, &sheets))
        .await
        .expect("the engine");
    (Arc::clone(&planted.engine), app(&planted, Arc::new(llm)))
}

/// `body` sent to `uri` — `POST` with a body, `GET` without — and the answer.
async fn call(app: &Router, uri: &str, body: Option<Value>) -> (StatusCode, Value) {
    let request = match body {
        Some(body) => Request::post(uri)
            .header("content-type", "application/json")
            .body(Body::from(body.to_string())),
        None => Request::get(uri).body(Body::empty()),
    }
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

/// The export of `id`, sent as the CampaignOverview panel sends it.
async fn export(app: &Router, id: &str) -> (StatusCode, Value) {
    call(
        app,
        "/commands",
        Some(json!({
            "dataCapability": "campaign.exportCampaign@1",
            "payload": { "id": id },
            "caller": { "by": "dm", "from": "CampaignOverview", "roles": [] },
        })),
    )
    .await
}

#[tokio::test]
async fn an_export_of_a_fixture_campaign_is_stamped_and_its_result_answers_its_archive_once_applied()
 {
    let scratch = Scratch::new("wire").await;
    let (engine, app) = serve(&scratch.url).await;
    let (status, answer) = export(&app, VALOMBRE).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{answer}");
    assert_eq!(answer["warnings"], json!([]), "{answer}");
    let status_of = format!("/commands/{}", answer["commandId"].as_str().expect("an id"));
    // Queued: no archive yet.
    let (status, queued) = call(&app, &status_of, None).await;
    assert_eq!(status, StatusCode::OK, "{queued}");
    assert_eq!(queued["result"]["attachment"], Value::Null, "{queued}");

    let applied: Vec<CommandResult> = engine.run_once().await.expect("the applier ran");
    assert_eq!(applied[0].status, State::Applied, "{applied:?}");
    let (status, answer) = call(&app, &status_of, None).await;
    assert_eq!(status, StatusCode::OK, "{answer}");
    assert_eq!(answer["result"]["status"], "applied", "{answer}");
    let archive = &answer["result"]["attachment"];
    let file = archive["file"].as_str().expect("a file name");
    assert!(
        file.starts_with("les-cendres-de-valombre-") && file.ends_with("Z.json"),
        "{file}"
    );
    assert_eq!(archive["content"]["campaign"]["id"], VALOMBRE);
    assert_eq!(archive["content"]["players"][0]["id"], CLAIRE);
    let stamp = archive["content"]["exported_at"]
        .as_str()
        .expect("the export's time");
    assert!(scratch.exported_at(VALOMBRE, stamp).await, "{answer}");
    scratch.drop().await;
}

#[tokio::test]
async fn an_export_of_a_campaign_the_tables_do_not_hold_is_404_saying_why() {
    let scratch = Scratch::new("unknown").await;
    let (engine, app) = serve(&scratch.url).await;
    let (status, answer) = export(&app, NOWHERE).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{answer}");
    assert_eq!(answer["error"], "cette campagne n'existe pas");
    assert_eq!(
        engine.run_once().await.expect("the applier ran"),
        Vec::new()
    );
    scratch.drop().await;
}

#[tokio::test]
async fn over_the_engine_s_memory_no_campaign_is_exported() {
    let (_, app) = serve(MEMORY).await;
    let (status, answer) = export(&app, VALOMBRE).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{answer}");
    assert_eq!(answer["error"], "cette campagne n'existe pas");
}
