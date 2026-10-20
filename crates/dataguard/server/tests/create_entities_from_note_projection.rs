//! campaign.createEntitiesFromNote over PostgreSQL (`DATABASE_URL`): what the
//! engine applies of a validation reaches the tables — one `entity` row per
//! accepted proposal, a `character` row with its token colour for a PC or an
//! NPC alone, its first `entity_version` with its snapshot and diff, and
//! each `note_proposal` checked or unchecked, the accepted ones pointing at
//! the element they became. A second validation creates no second element,
//! and the proposals the engine held before a start are unchecked as well.
//! Each test works in a scratch database of its own, dropped when it ends.

use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use dataguard_server::{Engine, Planted, planted, router};
use harness_dataguard::command::State;
use serde_json::{Value, json};
use tokio_postgres::{Client, NoTls};
use tower::ServiceExt;

const MIGRATIONS: [&str; 4] = [
    include_str!("../../../data/migrations/0002_campaign.sql"),
    include_str!("../../../data/migrations/0003_player.sql"),
    include_str!("../../../data/migrations/0004_entity.sql"),
    include_str!("../../../data/migrations/0008_note_import.sql"),
];

const VALOMBRE: &str = "c1000000-0000-4000-8000-000000000001";
const GASPARD: &str = "e1000000-0000-4000-8000-000000000001";

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

fn draft() -> String {
    let fixture: Value =
        serde_json::from_str(include_str!("../../../../mock/fixtures/NoteImport.json"))
            .expect("the fixture is JSON");
    fixture["draft"].as_str().expect("a draft").to_owned()
}

/// A database of its own, migrated, holding Valombre and Gaspard Grosmollet,
/// known as « Gaspard ».
struct Scratch {
    name: String,
    url: String,
    client: Client,
}

impl Scratch {
    async fn new(name: &str) -> Scratch {
        let name = format!("create_entities_test_{name}_{}", std::process::id());
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
                 INSERT INTO entity (id, campaign_id, type, name) VALUES ('{GASPARD}', '{VALOMBRE}', 'pnj', 'Gaspard Grosmollet');
                 INSERT INTO entity_alias (entity_id, alias) VALUES ('{GASPARD}', 'Gaspard');
                 INSERT INTO entity_version (entity_id, version_number, source, snapshot, diff)
                     VALUES ('{GASPARD}', 1, 'dm', '{{}}', '[]');"
            ))
            .await
            .expect("Valombre and Gaspard");
        Scratch { name, url, client }
    }

    /// The engine as the server plants it over this database — which reads
    /// the elements the tables hold — and its wire protocol.
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

    /// The rows `sql` selects, each as JSON.
    async fn rows(&self, sql: &str) -> Vec<Value> {
        self.client
            .query(&format!("SELECT to_jsonb(q)::text FROM ({sql}) q"), &[])
            .await
            .expect("the tables")
            .iter()
            .map(|row| serde_json::from_str(row.get::<_, &str>(0)).expect("JSON"))
            .collect()
    }

    /// How many rows `table` holds.
    async fn count(&self, table: &str) -> i64 {
        self.client
            .query_one(&format!("SELECT count(*) FROM {table}"), &[])
            .await
            .expect("a count")
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

/// `body` sent by the DM, approver of their own campaign; every command
/// applied: the answer.
async fn sent(engine: &Engine, app: &Router, mut body: Value) -> Value {
    body["caller"] = json!({ "by": "dm", "from": "NoteImport", "roles": ["role:approver"] });
    let request = Request::post("/commands")
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .expect("a request");
    let response = app.clone().oneshot(request).await.expect("an answer");
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("a body");
    let answer: Value = serde_json::from_slice(&bytes).expect("JSON");
    let applied = engine.run_once().await.expect("the applier ran");
    assert!(
        applied.iter().all(|r| r.status == State::Applied),
        "{applied:?}"
    );
    answer
}

/// The draft pasted for Valombre, analysed: the note's id.
async fn extract(engine: &Engine, app: &Router) -> String {
    let body = json!({
        "dataCapability": "ai.extractFromNotes@1",
        "payload": { "campaign": VALOMBRE, "text": draft() },
    });
    sent(engine, app, body).await["partition"]
        .as_str()
        .and_then(|p| p.strip_prefix("NoteImport/"))
        .expect("a NoteImport partition")
        .to_owned()
}

/// `accepted` validated for Valombre.
async fn validate(engine: &Engine, app: &Router, accepted: &[&str]) {
    let body = json!({
        "dataCapability": "campaign.createEntitiesFromNote@1",
        "payload": { "campaign": VALOMBRE, "accepted": accepted },
    });
    sent(engine, app, body).await;
}

impl Scratch {
    /// The id of the note's proposal named `name`.
    async fn proposal(&self, note: &str, name: &str) -> String {
        let found = self
            .rows(&format!(
                "SELECT id FROM note_proposal WHERE note_id = '{note}' AND name = '{name}'"
            ))
            .await;
        found[0]["id"]
            .as_str()
            .unwrap_or_else(|| panic!("{name} is not proposed"))
            .to_owned()
    }

    /// The note's proposals: name, checked, created element.
    async fn proposals(&self, note: &str) -> Vec<Value> {
        self.rows(&format!(
            "SELECT name, checked, created_entity_id FROM note_proposal \
             WHERE note_id = '{note}' ORDER BY name"
        ))
        .await
    }
}

#[tokio::test]
async fn the_accepted_proposals_reach_the_tables_as_versioned_elements_and_the_others_unchecked() {
    let s = Scratch::new("tables").await;
    let before = s.count("entity").await;
    let (engine, app) = s.serve().await;
    let note = extract(&engine, &app).await;
    let ombeline = s.proposal(&note, "Mère Ombeline").await;
    let lieu = s
        .rows(&format!(
            "SELECT id, name, excerpt FROM note_proposal WHERE note_id = '{note}' \
             AND type = 'lieu' AND existing_entity_id IS NULL ORDER BY name LIMIT 1"
        ))
        .await
        .remove(0);
    let lieu_id = lieu["id"].as_str().expect("a place proposed");
    validate(&engine, &app, &[&ombeline, lieu_id]).await;
    assert!(engine.run_once().await.expect("the applier ran").is_empty());

    assert_eq!(s.count("entity").await, before + 2);
    let proposals = s.proposals(&note).await;
    for proposal in &proposals {
        let accepted = proposal["name"] == "Mère Ombeline" || proposal["name"] == lieu["name"];
        assert_eq!(proposal["checked"], json!(accepted), "{proposal}");
        assert_eq!(
            proposal["created_entity_id"].is_string(),
            accepted,
            "{proposal}"
        );
    }
    let elements = s
        .rows(&format!(
            "SELECT e.campaign_id, e.type, e.name, e.description, c.token_color, \
                 v.version_number, v.source, v.snapshot, jsonb_array_length(v.diff) AS changes \
             FROM note_proposal p JOIN entity e ON e.id = p.created_entity_id \
             LEFT JOIN character c ON c.entity_id = e.id \
             JOIN entity_version v ON v.entity_id = e.id \
             WHERE p.note_id = '{note}' ORDER BY e.name"
        ))
        .await;
    let ombeline = elements
        .iter()
        .find(|e| e["name"] == "Mère Ombeline")
        .expect("Mère Ombeline is an element");
    assert_eq!(ombeline["campaign_id"], VALOMBRE);
    assert_eq!(ombeline["type"], "pnj");
    assert_eq!(ombeline["token_color"], "accent");
    assert_eq!(ombeline["version_number"], 1);
    assert_eq!(ombeline["source"], "dm");
    assert_eq!(ombeline["snapshot"]["name"], "Mère Ombeline");
    assert_eq!(ombeline["snapshot"]["tokenColor"], "accent");
    assert_eq!(ombeline["changes"], 6, "{ombeline}");
    let place = elements
        .iter()
        .find(|e| e["name"] == lieu["name"])
        .expect("the place is an element");
    assert_eq!(place["type"], "lieu");
    assert_eq!(place["description"], lieu["excerpt"]);
    assert_eq!(place["token_color"], Value::Null, "a place has no sheet");
    assert_eq!(place["version_number"], 1);
    assert_eq!(elements.len(), 2, "one version each: {elements:?}");
    assert_eq!(
        s.count("character c JOIN note_proposal p ON p.created_entity_id = c.entity_id")
            .await,
        1
    );

    // Validated again, nothing is made twice.
    let ombeline = s.proposal(&note, "Mère Ombeline").await;
    validate(&engine, &app, &[&ombeline]).await;
    assert_eq!(s.count("entity").await, before + 2);
    assert_eq!(s.proposals(&note).await, proposals);
    s.drop().await;
}

#[tokio::test]
async fn the_proposals_held_before_a_start_are_unchecked_by_a_validation_after_it() {
    let s = Scratch::new("restart").await;
    let (engine, app) = s.serve().await;
    let note = extract(&engine, &app).await;
    drop((engine, app));

    let (engine, app) = s.serve().await;
    let ombeline = s.proposal(&note, "Mère Ombeline").await;
    validate(&engine, &app, &[&ombeline]).await;
    let proposals = s.proposals(&note).await;
    assert!(proposals.len() >= 3, "{proposals:?}");
    for proposal in &proposals {
        let accepted = proposal["name"] == "Mère Ombeline";
        assert_eq!(proposal["checked"], json!(accepted), "{proposal}");
    }
    s.drop().await;
}
