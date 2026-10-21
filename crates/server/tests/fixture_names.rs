//! The mock-up's fixtures, loaded into the tables, read back through the
//! server by the names the Micro-UIs know them by (D8): the slugs go in, the
//! query runs on the stored uuids, and the slugs come back out — the answers
//! the mock-up's own fixtures give. A scratch database of its own
//! (`DATABASE_URL`), dropped when the test ends.

use std::collections::BTreeSet;
use std::path::Path;
use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use dataguard_server::MEMORY;
use dataguard_server::engine::tokio_postgres::{self, Client, NoTls};
use ports_llm::SharedLlm;
use ports_llm::fake::FakeLlm;
use ports_speech_to_text::SharedSpeechToText;
use ports_speech_to_text::fake::FakeSpeechToText;
use serde_json::{Value, json};
use server::{Config, Names, app, fixtures, plant, renaming, shared};
use tower::ServiceExt;

fn url() -> String {
    std::env::var("DATABASE_URL").expect("DATABASE_URL names the harness's PostgreSQL")
}

/// `url()` naming database `name`.
fn named(name: &str) -> String {
    let url = url();
    let (base, query) = url
        .split_once('?')
        .map_or((url.as_str(), ""), |(b, q)| (b, q));
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

fn fixture(name: &str) -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../mock/fixtures");
    serde_json::from_str(&std::fs::read_to_string(path.join(name)).expect("the fixture"))
        .expect("JSON")
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

/// The `(id, player)` of every character of `listed`.
fn characters(listed: &Value) -> BTreeSet<(String, String)> {
    listed
        .as_array()
        .expect("a list")
        .iter()
        .map(|c| (c["id"].to_string(), c["player"].to_string()))
        .collect()
}

#[tokio::test]
async fn the_fixtures_are_read_back_by_the_names_the_mock_up_gives_them() {
    let name = format!("fixture_names_{}", std::process::id());
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
    let plan = fixtures(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../mock/fixtures"))
        .expect("the fixtures");
    data::insert_missing(&mut client, &plan.tables)
        .await
        .expect("the fixtures are inserted");
    let names = plan.names;

    // The engine over memory, its Capabilities reading the scratch database,
    // as the binary plants it when DATABASE_URL_READONLY names one.
    let config = Config::read(|var| match var {
        "DATABASE_URL" => Some(MEMORY.to_string()),
        "DATABASE_URL_READONLY" => Some(named(&name)),
        _ => None,
    })
    .expect("a valid configuration");
    let llm = SharedLlm::new(FakeLlm::new());
    let speech = SharedSpeechToText::new(FakeSpeechToText::new(String::new()));
    let planted = plant(&config, shared(&llm, &speech))
        .await
        .expect("the in-memory engine");
    let served = app(&planted, Arc::new(llm));
    let renamed = renaming(served.clone(), Names::new(names));

    let campaign = json!({ "campaign": "cendres-valombre" });
    let listed = read(&renamed, "campaign.listCharacters", campaign.clone()).await;
    assert!(characters(&listed).len() >= 5, "{listed}");
    assert_eq!(
        characters(&listed),
        characters(&fixture("campaign.listCharacters.json")),
        "{listed}"
    );
    let players = read(&renamed, "campaign.listPlayers", campaign.clone()).await;
    let ids = |players: &Value| -> BTreeSet<String> {
        players
            .as_array()
            .expect("a list")
            .iter()
            .map(|p| p["id"].to_string())
            .collect()
    };
    assert_eq!(
        ids(&players),
        ids(&fixture("campaign.listPlayers.json")),
        "{players}"
    );

    let sheet = read(&renamed, "campaign.getEntity", json!({ "id": "elowen" })).await;
    assert_eq!(sheet["id"], "elowen", "{sheet}");
    assert_eq!(sheet["player"], "jou-julie", "{sheet}");
    let overview = read(
        &renamed,
        "campaign.getCampaign",
        json!({ "id": "cendres-valombre" }),
    )
    .await;
    assert_eq!(overview["id"], "cendres-valombre", "{overview}");

    // The media the fixtures describe are loaded with them, by their names.
    let battlemap = read(&renamed, "media.getBattlemap", json!({ "id": "bm-moulin" })).await;
    assert_eq!(battlemap["id"], "bm-moulin", "{battlemap}");
    assert_eq!(battlemap["place"], "moulin", "{battlemap}");
    assert_eq!(battlemap["grid"]["orient"], "flat", "{battlemap}");
    assert_eq!(
        battlemap["grid"]["opacity"].as_f64(),
        Some(0.55),
        "{battlemap}"
    );
    let library = read(
        &renamed,
        "media.listMedia",
        json!({ "campaignId": "cendres-valombre" }),
    )
    .await;
    let described = fixture("media.listMedia.json");
    let media = |library: &Value, kind: &str| -> BTreeSet<(String, String)> {
        library[kind]
            .as_array()
            .expect("a list")
            .iter()
            .map(|m| (m["id"].to_string(), m["place"].to_string()))
            .collect()
    };
    for kind in ["battlemaps", "illustrations"] {
        assert_eq!(media(&library, kind), media(&described, kind), "{library}");
    }
    assert_eq!(library["illustrations"].as_array().map(Vec::len), Some(5));

    // Without the names, a slug names no row: the store knows uuids only.
    assert_eq!(
        read(&served, "campaign.listCharacters", campaign).await,
        json!([])
    );

    drop((renamed, served, client));
    admin
        .batch_execute(&format!("DROP DATABASE {name} WITH (FORCE)"))
        .await
        .expect("the scratch database dropped");
}
