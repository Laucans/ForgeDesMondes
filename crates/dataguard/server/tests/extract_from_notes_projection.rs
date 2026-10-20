//! ai.extractFromNotes over PostgreSQL (`DATABASE_URL`): what the engine
//! applies reaches `imported_note` and `note_proposal` — the raw note kept,
//! one proposal per element the notes name, the ones the campaign knows by
//! a name or an alias unchecked — and no row of `entity`, `character` or
//! `entity_version` is added (no-creation-before-validation). A note whose
//! rows the tables lost is written again at the next start. Each test works
//! in a scratch database of its own, dropped when it ends.

use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use dataguard_server::{Engine, Planted, planted, router};
use harness_dataguard::api::Submission;
use harness_dataguard::command::State;
use harness_dataguard::guard::Caller;
use harness_dataguard::manifest::DataCapability;
use serde_json::{Value, json};
use tokio_postgres::{Client, NoTls};
use tower::ServiceExt;

const MIGRATIONS: [&str; 4] = [
    include_str!("../../../data/migrations/0002_campaign.sql"),
    include_str!("../../../data/migrations/0003_player.sql"),
    include_str!("../../../data/migrations/0004_entity.sql"),
    include_str!("../../../data/migrations/0008_note_import.sql"),
];

/// A proposal written on its own partition, test-only.
const LATE_PROPOSAL: &str = r#"{"dataCapability":"test.lateProposal","owner":"dataguard","version":1,"effect":"insert","target":{"aggregate":"NoteImport"},"mode":"overwrite","callableBy":["*"]}"#;

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
        let name = format!("extract_from_notes_test_{name}_{}", std::process::id());
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
        // A proposal written alone: one a review let through after its note.
        let late = DataCapability::parse(LATE_PROPOSAL).expect("contract F");
        let engine = Arc::new(engine.with_data_capability(late));
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

/// The draft pasted for Valombre by the DM, approver of their own campaign;
/// every command applied: the note's id.
async fn extract(engine: &Engine, app: &Router) -> String {
    let body = json!({
        "dataCapability": "ai.extractFromNotes@1",
        "payload": { "campaign": VALOMBRE, "text": draft() },
        "caller": { "by": "dm", "from": "NoteImport", "roles": ["role:approver"] },
    });
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
    answer["partition"]
        .as_str()
        .and_then(|p| p.strip_prefix("NoteImport/"))
        .expect("a NoteImport partition")
        .to_owned()
}

#[tokio::test]
async fn the_raw_note_and_its_proposals_reach_the_tables_and_no_element_is_created() {
    let s = Scratch::new("tables").await;
    let before = [
        s.count("entity").await,
        s.count("character").await,
        s.count("entity_version").await,
    ];
    let (engine, app) = s.serve().await;
    let note = extract(&engine, &app).await;

    let notes = s
        .rows("SELECT id, campaign_id, raw_text, analyzed_at IS NOT NULL AS analyzed FROM imported_note")
        .await;
    assert_eq!(
        notes,
        [json!({ "id": note, "campaign_id": VALOMBRE, "raw_text": draft(), "analyzed": true })]
    );
    let proposals = s
        .rows(&format!(
            "SELECT type, name, excerpt, existing_entity_id, checked, created_entity_id \
             FROM note_proposal WHERE note_id = '{note}' ORDER BY name"
        ))
        .await;
    assert!(proposals.len() >= 2, "{proposals:?}");
    for proposal in &proposals {
        assert!(
            proposal["excerpt"].as_str().is_some_and(|e| !e.is_empty()),
            "{proposal}"
        );
        assert_eq!(proposal["created_entity_id"], Value::Null, "{proposal}");
    }
    let named = |name: &str| {
        proposals
            .iter()
            .find(|p| p["name"] == name)
            .unwrap_or_else(|| panic!("{name} is not proposed: {proposals:?}"))
            .clone()
    };
    // Gaspard is known by his alias: a probable duplicate, unchecked.
    let gaspard = named("Gaspard");
    assert_eq!(gaspard["existing_entity_id"], json!(GASPARD));
    assert_eq!(gaspard["checked"], json!(false));
    let ombeline = named("Mère Ombeline");
    assert_eq!(ombeline["type"], json!("pnj"));
    assert_eq!(ombeline["existing_entity_id"], Value::Null);
    assert_eq!(ombeline["checked"], json!(true));

    let after = [
        s.count("entity").await,
        s.count("character").await,
        s.count("entity_version").await,
    ];
    assert_eq!(after, before, "no-creation-before-validation");
    s.drop().await;
}

#[tokio::test]
async fn a_note_the_tables_lost_is_written_again_at_the_next_start() {
    let s = Scratch::new("repair").await;
    let (engine, app) = s.serve().await;
    let note = extract(&engine, &app).await;
    let proposed = s.count("note_proposal").await;
    assert!(proposed >= 2);
    drop((engine, app));
    // Lost: the proposals go with their note.
    s.client
        .batch_execute("DELETE FROM imported_note")
        .await
        .expect("deleted");
    assert_eq!(s.count("note_proposal").await, 0);

    let _restarted = s.serve().await;
    let notes = s.rows("SELECT id, raw_text FROM imported_note").await;
    assert_eq!(notes, [json!({ "id": note, "raw_text": draft() })]);
    assert_eq!(s.count("note_proposal").await, proposed);
    s.drop().await;
}

#[tokio::test]
async fn a_proposal_applied_after_its_note_reaches_the_tables_too() {
    let s = Scratch::new("late").await;
    let (engine, app) = s.serve().await;
    let note = extract(&engine, &app).await;
    let proposed = s.count("note_proposal").await;
    let late = "f1000000-0000-4000-8000-0000000000aa";
    let payload = json!({
        "noteProposalId": late,
        "noteId": note,
        "type": "lieu",
        "name": "Le gué",
        "excerpt": "le péage illégal au gué, 5 po par tête.",
        "existingEntityId": null,
        "checked": true,
    });
    let Value::Object(payload) = payload else {
        panic!("an object");
    };
    engine
        .submit(Submission {
            data_capability: "test.lateProposal".to_string(),
            payload: payload.into_iter().collect(),
            target_id: Some(late.to_string()),
            caller: Caller {
                by: "dm".to_string(),
                from: "NoteImport".to_string(),
                permissions: Vec::new(),
                roles: vec!["role:approver".to_string()],
            },
            ..Submission::default()
        })
        .await
        .expect("queued");
    let applied = engine.run_once().await.expect("the applier ran");
    assert!(
        applied.iter().all(|r| r.status == State::Applied),
        "{applied:?}"
    );
    assert_eq!(s.count("note_proposal").await, proposed + 1);
    let gue = s
        .rows(&format!(
            "SELECT note_id, name, checked FROM note_proposal WHERE id = '{late}'"
        ))
        .await;
    assert_eq!(
        gue,
        [json!({ "note_id": note, "name": "Le gué", "checked": true })]
    );
    s.drop().await;
}
