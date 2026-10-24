//! Commands sent with the mock-up's names (D8), through the server as the
//! binary serves it: the fixtures loaded into the tables, the engine over the
//! same PostgreSQL, the renaming around its routes. A command naming a
//! fixture's row by its slug — `Entity/morlox`, `campaign: cendres-valombre`
//! — is applied to the uuid row the fixtures loaded, and its answers name it
//! by the slug again; a text that merely holds a slug is stored as it is. A
//! scratch database of its own (`DATABASE_URL`), dropped when the test ends.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use dataguard_server::engine::command::State;
use dataguard_server::engine::engine::Engine;
use dataguard_server::engine::tokio_postgres::{self, Client, NoTls};
use ports_character_sheets::SharedCharacterSheets;
use ports_character_sheets::fake::FakeCharacterSheets;
use ports_llm::SharedLlm;
use ports_speech_to_text::SharedSpeechToText;
use ports_speech_to_text::fake::FakeSpeechToText;
use serde_json::{Value, json};
use server::{Config, Names, Ports, app, fixtures, llm, plant, renaming, shared};
use tower::ServiceExt;

fn url() -> String {
    std::env::var("DATABASE_URL").expect("DATABASE_URL names the harness's PostgreSQL")
}

/// `url()` naming database `name`.
fn named(name: &str) -> String {
    let url = url();
    let (base, query) = url.split_once('?').unwrap_or((url.as_str(), ""));
    let (server, _) = base.rsplit_once('/').expect("a database in the URL");
    if query.is_empty() {
        format!("{server}/{name}")
    } else {
        format!("{server}/{name}?{query}")
    }
}

async fn connect(url: &str) -> Client {
    let (client, connection) = tokio_postgres::connect(url, NoTls)
        .await
        .expect("PostgreSQL");
    tokio::spawn(connection);
    client
}

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../mock/fixtures")
}

/// A database of its own, migrated as the server migrates it, holding the
/// fixtures.
struct Scratch {
    name: String,
    client: Client,
}

impl Scratch {
    async fn new() -> (Scratch, Vec<(String, String)>) {
        let name = format!("command_names_{}", std::process::id());
        let admin = connect(&url()).await;
        for sql in [
            format!("DROP DATABASE IF EXISTS {name} WITH (FORCE)"),
            format!("CREATE DATABASE {name}"),
        ] {
            admin.batch_execute(&sql).await.expect("a scratch database");
        }
        let mut client = connect(&named(&name)).await;
        let migrations = Path::new(env!("CARGO_MANIFEST_DIR")).join("../data/migrations");
        data::migrate(&mut client, &migrations)
            .await
            .expect("migrated");
        let plan = fixtures(&fixtures_dir()).expect("the fixtures");
        data::insert_missing(&mut client, &plan.tables)
            .await
            .expect("the fixtures are inserted");
        (Scratch { name, client }, plan.names)
    }

    /// The description and the campaign of the element `id`, as the table
    /// holds them.
    async fn entity(&self, id: &str) -> (Option<String>, String) {
        let row = self
            .client
            .query_one(
                "SELECT description, campaign_id::text FROM entity WHERE id = ($1::text)::uuid",
                &[&id],
            )
            .await
            .expect("the element");
        (row.get(0), row.get(1))
    }

    /// Dropped; retried, as another role's backend may still hold it a
    /// moment.
    async fn drop(self) {
        drop(self.client);
        let admin = connect(&url()).await;
        let sql = format!("DROP DATABASE {} WITH (FORCE)", self.name);
        for _ in 0..3 {
            if admin.batch_execute(&sql).await.is_ok() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
        admin
            .batch_execute(&sql)
            .await
            .expect("the scratch database dropped");
    }
}

/// The engine as the binary plants it over the database `name`, its `llm`
/// port answering the fixtures' proposals, and its routes renamed by
/// `names`.
async fn serve(name: &str, names: Vec<(String, String)>) -> (Arc<Engine>, Router) {
    let url = named(name);
    let config = Config::read(|var| (var == "DATABASE_URL").then(|| url.clone()))
        .expect("a valid configuration");
    let llm = SharedLlm::new(llm(Ports::Fake, Some(&fixtures_dir())));
    let speech = SharedSpeechToText::new(FakeSpeechToText::new(String::new()));
    let sheets = SharedCharacterSheets::new(FakeCharacterSheets::new());
    let planted = plant(&config, shared(&llm, &speech, &sheets))
        .await
        .expect("the engine");
    let engine = Arc::clone(&planted.engine);
    (
        engine,
        renaming(app(&planted, Arc::new(llm)), Names::new(names)),
    )
}

/// What `app` answers to `body` on `uri` (`POST`), or to a `GET` of `uri`
/// with no body.
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

/// `dataCapability` sent with `target` and `payload` by the DM, approver of
/// their own campaign's critical fields; applied by one pass of the applier.
/// The command's answer.
async fn applied(
    engine: &Engine,
    app: &Router,
    data_capability: &str,
    target: &str,
    payload: Value,
) -> Value {
    let (status, answer) = call(
        app,
        "/commands",
        Some(json!({
            "dataCapability": data_capability,
            "target": { "aggregate": "Entity", "id": target },
            "payload": payload,
            "caller": { "by": "dm", "from": "EntityEditor", "roles": ["role:approver"] },
        })),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{answer}");
    assert_eq!(answer["warnings"], json!([]), "{answer}");
    let settled = engine.run_once().await.expect("the applier ran");
    assert_eq!(settled.len(), 1, "{settled:?}");
    assert_eq!(settled[0].status, State::Applied, "{settled:?}");
    answer
}

/// The sheet `campaign.getEntity` reads for `id`.
async fn sheet(app: &Router, id: &str) -> Value {
    let (status, answer) = call(
        app,
        "/capabilities/campaign.getEntity",
        Some(json!({ "variables": { "id": id } })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{answer}");
    answer["data"].clone()
}

#[tokio::test]
async fn a_command_by_the_mock_ups_names_is_applied_to_the_fixtures_rows_and_answers_by_them() {
    let (scratch, names) = Scratch::new().await;
    let stored = |wire: &str| -> String {
        names
            .iter()
            .find(|(name, _)| name == wire)
            .map(|(_, id)| id.clone())
            .unwrap_or_else(|| panic!("the fixtures name {wire}"))
    };
    let (morlox, valombre) = (stored("morlox"), stored("cendres-valombre"));
    let (engine, app) = serve(&scratch.name, names.clone()).await;

    // Morlox, by his name: saved on his uuid row, and named so again.
    let description = "Il a fermé boutique : morlox ne vend plus à cendres-valombre.";
    let answer = applied(
        &engine,
        &app,
        "campaign.saveEntity@1",
        "morlox",
        json!({ "entity": { "id": "morlox", "description": description } }),
    )
    .await;
    assert_eq!(
        answer["partition"],
        format!("Entity/{morlox}"),
        "a partition holds an id, it is none: {answer}"
    );
    let command = answer["commandId"].as_str().expect("a command id");
    let (status, followed) = call(&app, &format!("/commands/{command}"), None).await;
    assert_eq!(status, StatusCode::OK, "{followed}");
    assert_eq!(followed["result"]["status"], "applied", "{followed}");
    assert_eq!(
        scratch.entity(&morlox).await,
        (Some(description.to_string()), valombre.clone()),
        "the description stored as it was sent, on Morlox's row"
    );
    let read = sheet(&app, "morlox").await;
    assert_eq!(read["id"], "morlox", "{read}");
    assert_eq!(read["description"], description, "{read}");

    // ai.proposeCharacter over him, by his name too.
    applied(
        &engine,
        &app,
        "ai.proposeCharacter@1",
        "morlox",
        json!({ "prompt": "Un gobelin marchand, nerveux." }),
    )
    .await;
    let proposed = sheet(&app, "morlox").await;
    assert_eq!(proposed["id"], "morlox", "{proposed}");
    assert_eq!(
        proposed["prompt"], "Un gobelin marchand, nerveux.",
        "{proposed}"
    );

    // A new NPC of the fixtures' campaign, named by its slug: created there,
    // its own id unchanged.
    let brannoc = "5d0f7a2e-8b1c-4e3f-9a6d-2c4b8e1f0a37";
    let answer = applied(
        &engine,
        &app,
        "campaign.saveEntity@1",
        brannoc,
        json!({ "campaign": "cendres-valombre",
            "entity": { "id": brannoc, "kind": "pnj", "name": "Brannoc le Passeur", "aliases": [],
                "description": "morlox lui doit 40 po." } }),
    )
    .await;
    assert_eq!(answer["partition"], format!("Entity/{brannoc}"), "{answer}");
    assert_eq!(
        scratch.entity(brannoc).await,
        (Some("morlox lui doit 40 po.".to_string()), valombre.clone())
    );
    assert_eq!(sheet(&app, brannoc).await["id"], brannoc);

    // A command naming no slug, as before: by the stored ids.
    applied(
        &engine,
        &app,
        "campaign.saveEntity@1",
        &morlox,
        json!({ "entity": { "id": morlox, "description": "Par son uuid." } }),
    )
    .await;
    assert_eq!(
        scratch.entity(&morlox).await,
        (Some("Par son uuid.".to_string()), valombre)
    );

    drop((engine, app));
    scratch.drop().await;
}
