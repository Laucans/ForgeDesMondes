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
                 INSERT INTO entity_version (entity_id, version_number, source, snapshot, diff) VALUES
                    ('{ELOWEN}', 1, 'dm', '{{}}', '[]'), ('{KAEL}', 1, 'dm', '{{}}', '[]'),
                    ('{MORLOX}', 1, 'dm', '{{}}', '[]'), ('{VESNA}', 1, 'dm', '{{}}', '[]');
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

/// Posts the session `payload` describes as the DM, approver of their own
/// campaign: the status and the answer.
async fn post(app: &Router, payload: Value) -> (StatusCode, Value) {
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
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("a body");
    (status, serde_json::from_slice(&bytes).expect("JSON"))
}

/// Submits the session `payload` describes as the DM: its id, queued.
async fn submit(app: &Router, payload: Value) -> String {
    let (status, answer) = post(app, payload).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{answer}");
    answer["partition"]
        .as_str()
        .and_then(|p| p.strip_prefix("Session/"))
        .expect("a Session partition")
        .to_owned()
}

/// Creates the session `payload` describes as the DM: its id, and what the
/// applier made of it.
async fn create(engine: &Engine, app: &Router, payload: Value) -> (String, CommandResult) {
    let id = submit(app, payload).await;
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

#[tokio::test]
async fn a_session_queued_before_a_restart_is_judged_and_placed_against_the_tables_after_it() {
    let s = Scratch::new("restart").await;
    let (_, app) = s.serve().await;
    let id = submit(
        &app,
        json!({ "campaign": VALOMBRE, "title": "La tour", "present": [ELOWEN] }),
    )
    .await;
    // The server stops before applying it, and starts again.
    drop(app);
    let (engine, _) = s.serve().await;
    let mut settled = engine.run_once().await.expect("the applier ran");
    assert_eq!(settled.len(), 1, "{settled:?}");
    let result = settled.remove(0);
    assert_eq!(result.status, State::Applied, "{result:?}");
    let sessions = s
        .rows(&format!(
            "SELECT position, (SELECT count(*) FROM session_present_character p
                WHERE p.session_id = q.id AND p.character_id = '{ELOWEN}') AS elowen
             FROM session q WHERE id = '{id}'"
        ))
        .await;
    assert_eq!(sessions, [json!({ "position": 4, "elowen": 1 })]);
    s.drop().await;
}

#[tokio::test]
async fn a_restart_leaves_a_session_the_tables_hold_as_they_hold_it() {
    let s = Scratch::new("reprojected").await;
    let (engine, app) = s.serve().await;
    let (id, result) = create(
        &engine,
        &app,
        json!({ "campaign": VALOMBRE, "title": "La tour", "present": [ELOWEN] }),
    )
    .await;
    assert_eq!(result.status, State::Applied, "{result:?}");
    // Kael is ticked present by another path than the engine's.
    s.client
        .batch_execute(&format!(
            "INSERT INTO session_present_character (session_id, character_id)
             VALUES ('{id}', '{KAEL}')"
        ))
        .await
        .expect("Kael present");
    // The server starts again: the session is projected already.
    drop(app);
    let _ = s.serve().await;
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
    s.drop().await;
}

#[tokio::test]
async fn a_session_of_a_campaign_the_engine_removed_is_refused_404_and_not_queued() {
    let s = Scratch::new("removed").await;
    let (_, app) = s.serve().await;
    // The engine removed Valombre: its row tombstoned, the tables' row kept.
    s.client
        .batch_execute(&format!(
            "INSERT INTO dataguard_rows (partition, tombstoned)
             VALUES ('Campaign/{VALOMBRE}', true)"
        ))
        .await
        .expect("Valombre removed");
    drop(app);
    let (engine, app) = s.serve().await;
    let (status, answer) = post(
        &app,
        json!({ "campaign": VALOMBRE, "title": "Après la fin", "present": [] }),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{answer}");
    assert!(
        engine.run_once().await.expect("the applier ran").is_empty(),
        "nothing was queued"
    );
    s.drop().await;
}
