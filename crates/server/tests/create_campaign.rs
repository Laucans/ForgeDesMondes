//! campaign.createCampaign through the server's wire, as the server plants
//! its units: the creation's `Prepare` gives the campaign — and a oneshot's
//! session — its ids, the Campaign aggregate judges it and projects it into
//! `campaign` and `session`, and `campaign.listCampaigns` lists it. The
//! creation and its aggregate are tested in their own crates
//! (`crates/campaign/data-capabilities/create-campaign`,
//! `crates/campaign/aggregates/Campaign`); this is their composition. Over
//! PostgreSQL (`DATABASE_URL`), each test works in a scratch database of its
//! own, migrated as the server migrates it, dropped when it ends; what the
//! wire refuses before anything is queued is proved over the engine's
//! memory.

use std::path::Path;
use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use dataguard_server::MEMORY;
use dataguard_server::engine::command::State;
use dataguard_server::engine::engine::Engine;
use dataguard_server::engine::tokio_postgres::{self, Client, NoTls};
use ports_llm::SharedLlm;
use ports_llm::fake::FakeLlm;
use ports_speech_to_text::SharedSpeechToText;
use ports_speech_to_text::fake::FakeSpeechToText;
use serde_json::{Value, json};
use server::{Config, app, plant, shared};
use tower::ServiceExt;

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

/// A database of its own, migrated as the server migrates it, holding no
/// campaign.
struct Scratch {
    name: String,
    url: String,
    client: Client,
}

impl Scratch {
    async fn new(name: &str) -> Scratch {
        let name = format!("create_campaign_test_{name}_{}", std::process::id());
        let admin = connect(&url()).await;
        for sql in [
            format!("DROP DATABASE IF EXISTS {name} WITH (FORCE)"),
            format!("CREATE DATABASE {name}"),
        ] {
            admin.batch_execute(&sql).await.expect("a scratch database");
        }
        let url = naming(&name);
        let mut client = connect(&url).await;
        let migrations = Path::new(env!("CARGO_MANIFEST_DIR")).join("../data/migrations");
        data::migrate(&mut client, &migrations)
            .await
            .expect("migrated");
        Scratch { name, url, client }
    }

    /// How many campaigns the table holds, and how many sessions.
    async fn counted(&self) -> (i64, i64) {
        let row = self
            .client
            .query_one(
                "SELECT (SELECT count(*) FROM campaign), (SELECT count(*) FROM session)",
                &[],
            )
            .await
            .expect("the tables");
        (row.get(0), row.get(1))
    }

    /// The sessions of the campaign `id`: `(position, title, status,
    /// microphone on)`.
    async fn sessions(&self, id: &str) -> Vec<(i32, String, String, bool)> {
        self.client
            .query(
                "SELECT position, title, status, microphone_on FROM session
                 WHERE campaign_id = ($1::text)::uuid ORDER BY position",
                &[&id],
            )
            .await
            .expect("the sessions")
            .iter()
            .map(|row| (row.get(0), row.get(1), row.get(2), row.get(3)))
            .collect()
    }

    /// The master volume of the campaign `id`, as its table holds it.
    async fn master_volume(&self, id: &str) -> f64 {
        self.client
            .query_one(
                "SELECT sound_master_volume::float8 FROM campaign WHERE id = ($1::text)::uuid",
                &[&id],
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
    let planted = plant(&config, shared(&llm, &speech))
        .await
        .expect("the engine");
    (Arc::clone(&planted.engine), app(&planted, Arc::new(llm)))
}

/// `body` sent to `uri` with `POST`, and the answer.
async fn post(app: &Router, uri: &str, body: Value) -> (StatusCode, Value) {
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

/// The creation of `payload`, sent as the home page's CampaignList sends
/// it: by the DM, approver of their own campaigns' critical fields.
async fn create(app: &Router, payload: Value, key: Option<&str>) -> (StatusCode, Value) {
    post(
        app,
        "/commands",
        json!({
            "dataCapability": "campaign.createCampaign@1",
            "payload": payload,
            "idempotencyKey": key,
            "caller": { "by": "dm", "from": "CampaignList", "roles": ["role:approver"] },
        }),
    )
    .await
}

/// Creates `payload` and applies it: the new campaign's id.
async fn created(engine: &Engine, app: &Router, payload: Value) -> String {
    let (status, answer) = create(app, payload, None).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{answer}");
    let applied = engine.run_once().await.expect("the applier ran");
    assert_eq!(applied.len(), 1, "{applied:?}");
    assert_eq!(applied[0].status, State::Applied, "{applied:?}");
    answer["partition"]
        .as_str()
        .and_then(|partition| partition.strip_prefix("Campaign/"))
        .expect("a campaign's partition")
        .to_string()
}

/// What `campaign.listCampaigns` lists.
async fn listed(app: &Router) -> Vec<Value> {
    let (status, answer) = post(
        app,
        "/capabilities/campaign.listCampaigns",
        json!({ "variables": {} }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{answer}");
    answer["data"].as_array().cloned().unwrap_or_default()
}

#[tokio::test]
async fn a_created_campagne_is_listed_with_no_session_its_master_volume_in_range() {
    let scratch = Scratch::new("campagne").await;
    let (engine, app) = serve(&scratch.url).await;
    let id = created(
        &engine,
        &app,
        json!({ "name": "Les Brumes de Corbière", "type": "campagne", "art": "forest", "glyph": "B" }),
    )
    .await;
    let listed = listed(&app).await;
    let campaign = listed
        .iter()
        .find(|campaign| campaign["id"] == json!(id))
        .unwrap_or_else(|| panic!("{id} is not listed: {listed:?}"));
    assert_eq!(campaign["name"], "Les Brumes de Corbière");
    assert_eq!(campaign["type"], "campagne");
    assert_eq!(campaign["art"], "forest");
    assert!(scratch.sessions(&id).await.is_empty());
    let volume = scratch.master_volume(&id).await;
    assert!((0.0..=1.0).contains(&volume), "{volume}");
    scratch.drop().await;
}

#[tokio::test]
async fn oneshot_single_session_a_created_oneshot_has_exactly_one_session_in_prep_at_position_1_microphone_off()
 {
    let scratch = Scratch::new("oneshot").await;
    let (engine, app) = serve(&scratch.url).await;
    let id = created(
        &engine,
        &app,
        json!({ "name": "Le Bal", "type": "oneshot" }),
    )
    .await;
    assert!(
        listed(&app)
            .await
            .iter()
            .any(|campaign| campaign["id"] == json!(id) && campaign["type"] == "oneshot")
    );
    assert_eq!(
        scratch.sessions(&id).await,
        [(1, "Le Bal".to_string(), "prep".to_string(), false)]
    );
    scratch.drop().await;
}

#[tokio::test]
async fn the_same_idempotency_key_twice_creates_one_oneshot_and_one_session() {
    let scratch = Scratch::new("idempotent").await;
    let (engine, app) = serve(&scratch.url).await;
    let payload = json!({ "name": "Le Bal", "type": "oneshot" });
    let (status, first) = create(&app, payload.clone(), Some("bal-1")).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{first}");
    let (status, again) = create(&app, payload.clone(), Some("bal-1")).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{again}");
    assert_eq!(again["partition"], first["partition"], "{again}");
    engine.run_once().await.expect("the applier ran");
    let (status, after) = create(&app, payload, Some("bal-1")).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{after}");
    assert_eq!(after["partition"], first["partition"], "{after}");
    assert!(engine.run_once().await.expect("the applier ran").is_empty());
    assert_eq!(scratch.counted().await, (1, 1));
    scratch.drop().await;
}

#[tokio::test]
async fn a_creation_with_no_name_or_of_a_kind_there_is_not_is_refused_before_it_is_queued() {
    let (engine, app) = serve(MEMORY).await;
    for payload in [
        json!({ "type": "campagne" }),
        json!({ "name": "   ", "type": "oneshot" }),
        json!({ "name": "Le Bal", "type": "saga" }),
    ] {
        let (status, answer) = create(&app, payload.clone(), None).await;
        assert_eq!(
            status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "{payload}: {answer}"
        );
    }
    assert!(engine.run_once().await.expect("the applier ran").is_empty());
}
