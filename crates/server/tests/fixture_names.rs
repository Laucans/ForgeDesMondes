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
use ports_character_sheets::SharedCharacterSheets;
use ports_character_sheets::fake::FakeCharacterSheets;
use ports_llm::SharedLlm;
use ports_llm::fake::FakeLlm;
use ports_speech_to_text::SharedSpeechToText;
use ports_speech_to_text::fake::FakeSpeechToText;
use serde_json::{Value, json};
use server::{Config, Names, app, fixtures, id as uuid_of, plant, renaming, shared};
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
    // A restart plans the same rows and loads none of them twice.
    let again = fixtures(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../mock/fixtures"))
        .expect("the fixtures");
    assert_eq!(
        data::insert_missing(&mut client, &again.tables)
            .await
            .expect("the fixtures are planned again"),
        0
    );
    // The DM took Kael off the second session and ticked him again since: the
    // row is no longer the fixture's, and the load leaves it alone.
    client
        .batch_execute(&format!(
            "UPDATE session_present_character SET added_at = now() \
             WHERE session_id = '{}' AND character_id = '{}'",
            uuid_of("session", "s2"),
            uuid_of("entity", "kael"),
        ))
        .await
        .expect("an update");
    assert_eq!(
        data::insert_missing(&mut client, &again.tables)
            .await
            .expect("a presence the DM changed is not loaded again"),
        0
    );
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
    let sheets = SharedCharacterSheets::new(FakeCharacterSheets::new());
    let planted = plant(&config, shared(&llm, &speech, &sheets))
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

    // The sessions, the last brief and the to-create list, by their slugs.
    let sessions = read(&renamed, "session.listSessions", campaign.clone()).await;
    let described = fixture("session.listSessions.json");
    let summary = |sessions: &Value| -> Vec<(String, Value, Value, Value)> {
        sessions
            .as_array()
            .expect("a list")
            .iter()
            .map(|s| {
                (
                    s["id"].to_string(),
                    s["order"].clone(),
                    s["title"].clone(),
                    s["present"].clone(),
                )
            })
            .collect()
    };
    assert_eq!(summary(&sessions).len(), 4, "{sessions}");
    assert_eq!(
        summary(&sessions),
        summary(&described),
        "in order: {sessions}"
    );
    assert_eq!(sessions[3]["status"], "prep", "{sessions}");
    assert_eq!(sessions[0]["status"], "closed", "{sessions}");

    let listed = read(&renamed, "campaign.listEntitiesToCreate", campaign.clone()).await;
    let listed = listed["items"].clone();
    let described = fixture("campaign.listEntitiesToCreate.json");
    let entry = |list: &Value, id: &str| -> Value {
        list.as_array()
            .expect("a list")
            .iter()
            .find(|e| e["id"] == id)
            .unwrap_or_else(|| panic!("{id} in {list}"))
            .clone()
    };
    assert_eq!(listed.as_array().map(Vec::len), Some(3), "{listed}");
    for id in ["tc-odric", "tc-vesna", "tc-gue"] {
        let (got, want) = (entry(&listed, id), entry(&described, id));
        for key in ["type", "name", "session", "excerpt", "status", "entity"] {
            assert_eq!(got[key], want[key], "{id}.{key}: {got}");
        }
    }

    let brief = read(&renamed, "session.getLastBrief", json!({ "session": "s4" })).await;
    let described = fixture("session.getLastBrief.json");
    assert_eq!(brief["session"], "s4", "{brief}");
    assert_eq!(brief["transcript"], described["transcript"], "{brief}");
    assert_eq!(brief["duration"], "04:12", "{brief}");
    assert_eq!(brief["lastRun"], described["lastRun"], "{brief}");
    // The brief's scenes and the elements it recognised are other aggregates' rows, not loaded.
    assert_eq!(brief["result"]["scenes"], json!([]), "{brief}");
    let missing = brief["result"]["missing"].as_array().expect("a list");
    let missing: BTreeSet<&str> = missing.iter().filter_map(|m| m["id"].as_str()).collect();
    assert_eq!(
        missing,
        BTreeSet::from(["tc-odric", "tc-gue"]),
        "the created entry is no longer missing: {brief}"
    );

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
