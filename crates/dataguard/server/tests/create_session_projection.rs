//! session.createSession over PostgreSQL (`DATABASE_URL`): what the engine
//! applies on `Session/<id>` reaches `session` and
//! `session_present_character`, and a new session is judged against what the
//! tables hold of its campaign — the fixtures' sessions and PCs, which no
//! command wrote. Each test works in a scratch database of its own, migrated
//! as the server migrates it, dropped when it ends.

use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use dataguard_server::{Engine, Planted, planted, router};
use harness_dataguard::command::{CommandResult, State};
use serde_json::{Value, json};
use tokio_postgres::{Client, NoTls};
use tower::ServiceExt;

const MIGRATIONS: [&str; 5] = [
    include_str!("../../../data/migrations/0002_campaign.sql"),
    include_str!("../../../data/migrations/0003_player.sql"),
    include_str!("../../../data/migrations/0004_entity.sql"),
    include_str!("../../../data/migrations/0005_media.sql"),
    include_str!("../../../data/migrations/0006_session.sql"),
];

/// A campagne with three sessions, its PCs Elowen and Kael, its NPC Morlox.
const VALOMBRE: &str = "c1000000-0000-4000-8000-000000000001";
const ELOWEN: &str = "e1000000-0000-4000-8000-000000000001";
const KAEL: &str = "e1000000-0000-4000-8000-000000000002";
const MORLOX: &str = "e1000000-0000-4000-8000-000000000003";
/// A oneshot that has its session, and Vesna its PC.
const BRUME: &str = "c1000000-0000-4000-8000-000000000002";
const VESNA: &str = "e1000000-0000-4000-8000-000000000004";

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

/// A database of its own, migrated, holding Valombre and Brume as the
/// fixtures would.
struct Scratch {
    name: String,
    url: String,
    client: Client,
}

impl Scratch {
    async fn new(name: &str) -> Scratch {
        let name = format!("create_session_test_{name}_{}", std::process::id());
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
                "INSERT INTO campaign (id, name, kind) VALUES
                    ('{VALOMBRE}', 'Les Cendres de Valombre', 'campagne'),
                    ('{BRUME}', 'Une nuit dans la brume', 'oneshot');
                 INSERT INTO entity (id, campaign_id, type, name) VALUES
                    ('{ELOWEN}', '{VALOMBRE}', 'pj', 'Elowen'),
                    ('{KAEL}', '{VALOMBRE}', 'pj', 'Kael'),
                    ('{MORLOX}', '{VALOMBRE}', 'pnj', 'Morlox'),
                    ('{VESNA}', '{BRUME}', 'pj', 'Vesna');
                 INSERT INTO character (entity_id, token_color) VALUES
                    ('{ELOWEN}', 'accent'), ('{KAEL}', 'accent'),
                    ('{MORLOX}', 'danger'), ('{VESNA}', 'accent');
                 INSERT INTO session (campaign_id, position, title, status, microphone_on) VALUES
                    ('{VALOMBRE}', 1, 'L''arrivée', 'closed', false),
                    ('{VALOMBRE}', 2, 'La forêt', 'closed', false),
                    ('{VALOMBRE}', 3, 'Le marais', 'prep', false),
                    ('{BRUME}', 1, 'La nuit', 'prep', false);"
            ))
            .await
            .expect("Valombre and Brume");
        Scratch { name, url, client }
    }

    /// The engine as the server plants it over this database, and its wire
    /// protocol.
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

    async fn drop(self) {
        drop(self.client);
        connect(&url())
            .await
            .batch_execute(&format!("DROP DATABASE {} WITH (FORCE)", self.name))
            .await
            .expect("the scratch database dropped");
    }
}

/// Creates the session `payload` describes as the DM, approver of their own
/// campaign: its id, and what the applier made of it.
async fn create(engine: &Engine, app: &Router, payload: Value) -> (String, CommandResult) {
    let body = json!({
        "dataCapability": "session.createSession@1",
        "payload": payload,
        "caller": { "by": "dm", "from": "SessionList", "roles": ["role:approver"] },
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
    let id = answer["partition"]
        .as_str()
        .and_then(|p| p.strip_prefix("Session/"))
        .expect("a Session partition")
        .to_owned();
    let mut settled = engine.run_once().await.expect("the applier ran");
    assert_eq!(settled.len(), 1, "{settled:?}");
    (id, settled.remove(0))
}

#[tokio::test]
async fn the_fourth_session_reaches_the_tables_at_position_4_in_prep_with_exactly_its_two_pcs() {
    let s = Scratch::new("tables").await;
    let (engine, app) = s.serve().await;
    let (id, result) = create(
        &engine,
        &app,
        json!({ "campaign": VALOMBRE, "title": "La tour", "plannedOn": "2026-11-07", "present": [ELOWEN, KAEL] }),
    )
    .await;
    assert_eq!(result.status, State::Applied, "{result:?}");
    let sessions = s
        .rows(&format!(
            "SELECT campaign_id::text, position, title, planned_on::text, status, microphone_on
             FROM session WHERE id = '{id}'"
        ))
        .await;
    assert_eq!(
        sessions,
        [json!({
            "campaign_id": VALOMBRE,
            "position": 4,
            "title": "La tour",
            "planned_on": "2026-11-07",
            "status": "prep",
            "microphone_on": false,
        })]
    );
    let present = s
        .rows(&format!(
            "SELECT character_id::text FROM session_present_character
             WHERE session_id = '{id}' ORDER BY character_id"
        ))
        .await;
    assert_eq!(
        present,
        [
            json!({ "character_id": ELOWEN }),
            json!({ "character_id": KAEL })
        ]
    );
    // What the roster offers is exactly the two.
    let roster = s
        .rows(&format!(
            "SELECT character_id::text FROM session_roster_view
             WHERE session_id = '{id}' ORDER BY character_id"
        ))
        .await;
    assert_eq!(roster, present);
    s.drop().await;
}

#[tokio::test]
async fn the_pcs_and_the_oneshot_s_session_the_tables_hold_are_judged_and_nothing_reaches_them() {
    let s = Scratch::new("judged").await;
    let (engine, app) = s.serve().await;
    for present in [[ELOWEN, MORLOX], [ELOWEN, VESNA]] {
        let (_, result) = create(
            &engine,
            &app,
            json!({ "campaign": VALOMBRE, "title": "La tour", "present": present }),
        )
        .await;
        assert_eq!(result.status, State::Rejected, "{result:?}");
        assert_eq!(result.violations, ["present-only-pj"], "{result:?}");
    }
    let (_, result) = create(
        &engine,
        &app,
        json!({ "campaign": BRUME, "title": "Encore", "present": [VESNA] }),
    )
    .await;
    assert_eq!(result.status, State::Rejected, "{result:?}");
    assert_eq!(result.violations, ["oneshot-single-session"], "{result:?}");
    // No session was created, and no PC ticked present.
    let counted = s
        .rows("SELECT (SELECT count(*) FROM session) AS sessions, (SELECT count(*) FROM session_present_character) AS present")
        .await;
    assert_eq!(counted, [json!({ "sessions": 4, "present": 0 })]);
    s.drop().await;
}
