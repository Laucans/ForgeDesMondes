//! session.closeSession over PostgreSQL (`DATABASE_URL`): a live session the
//! tables hold — one no command of the engine wrote — is closed on its row
//! (`close-session`): status closed, closed_at set, microphone_on false, its
//! summary stored with the active provider's summary model; every playback
//! of the session still running is stopped and the other sessions' are
//! untouched (`stop-all`); every journal line keeps its time and its text.
//! A session in prep is refused under `status-lifecycle` and no table
//! changes. Each test works in a scratch database of its own, migrated by
//! the Data layer, dropped when it ends.

use std::path::Path;
use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use dataguard_server::{Engine, Planted, planted, router};
use harness_dataguard::command::{CommandResult, State};
use ports_llm::fake::FakeLlm;
use ports_llm::{Llm, RequestKind};
use serde_json::{Value, json};
use tokio_postgres::{Client, NoTls};
use tower::ServiceExt;

const VALOMBRE: &str = "c1000000-0000-4000-8000-000000000001";
/// Valombre's session 4, live; session 3, live in another tab; session 5,
/// in preparation.
const SEANCE: &str = "5e000000-0000-4000-8000-000000000004";
const AUTRE_SEANCE: &str = "5e000000-0000-4000-8000-000000000003";
const PROCHAINE: &str = "5e000000-0000-4000-8000-000000000005";
const FOULE: &str = "50000000-0000-4000-8000-000000000001";
const PORTE: &str = "50000000-0000-4000-8000-000000000002";

fn url() -> String {
    std::env::var("DATABASE_URL").expect("DATABASE_URL names the harness's PostgreSQL")
}

async fn connect(config: &tokio_postgres::Config) -> Client {
    let (client, connection) = config.connect(NoTls).await.expect("PostgreSQL");
    tokio::spawn(connection);
    client
}

/// A database of its own, migrated by the Data layer, holding Valombre, its
/// three sessions — 4 and 3 live, their microphones on, 5 in prep — two
/// lines of session 4's journal, two playbacks of session 4 still running
/// and one stopped, and one of session 3 running.
struct Scratch {
    client: Client,
    name: String,
    url: String,
}

impl Scratch {
    async fn new(name: &str) -> Scratch {
        let name = format!("close_session_test_{name}_{}", std::process::id());
        let config: tokio_postgres::Config = url().parse().expect("a PostgreSQL URL");
        let admin = connect(&config).await;
        for sql in [
            format!("DROP DATABASE IF EXISTS {name} WITH (FORCE)"),
            format!("CREATE DATABASE {name}"),
        ] {
            admin.batch_execute(&sql).await.expect("a scratch database");
        }
        let mut client = connect(config.clone().dbname(&name)).await;
        let migrations = concat!(env!("CARGO_MANIFEST_DIR"), "/../../data/migrations");
        data::migrate(&mut client, Path::new(migrations))
            .await
            .expect("migrated by the Data layer");
        client
            .batch_execute(&format!(
                "INSERT INTO campaign (id, name, kind) VALUES ('{VALOMBRE}', 'Les Cendres de Valombre', 'campagne');
                 INSERT INTO session (id, campaign_id, position, title, status, started_at, microphone_on) VALUES
                     ('{AUTRE_SEANCE}', '{VALOMBRE}', 3, 'La Crypte', 'live', now(), true),
                     ('{SEANCE}', '{VALOMBRE}', 4, 'L''embuscade du Gué', 'live', now(), true),
                     ('{PROCHAINE}', '{VALOMBRE}', 5, 'Le Col', 'prep', NULL, false);
                 INSERT INTO session_journal_line (session_id, spoken_at, text) VALUES
                     ('{SEANCE}', '2026-10-17T20:14:03Z', 'Elowen entre dans la taverne.'),
                     ('{SEANCE}', '2026-10-17T20:16:40Z', 'Morlox lui tend la carte.');
                 INSERT INTO sound (id, campaign_id, name, file_path) VALUES
                     ('{FOULE}', '{VALOMBRE}', 'Bruit de foule', 'sons/foule.ogg'),
                     ('{PORTE}', '{VALOMBRE}', 'Porte qui grince', 'sons/porte.ogg');
                 INSERT INTO sound_playback (session_id, sound_id, origin, started_at, stopped_at) VALUES
                     ('{SEANCE}', '{FOULE}', 'soundbox', now(), NULL),
                     ('{SEANCE}', '{PORTE}', 'pastille', now(), NULL),
                     ('{SEANCE}', '{PORTE}', 'soundbox', '2026-10-17T20:00:00Z', '2026-10-17T20:01:00Z'),
                     ('{AUTRE_SEANCE}', '{FOULE}', 'soundbox', now(), NULL);"
            ))
            .await
            .expect("Valombre, its sessions, its journal and its playbacks");
        let url = {
            let url = url();
            let (address, query) = url.split_once('?').unwrap_or((&url, ""));
            let (server, _) = address.rsplit_once('/').expect("a URL naming its database");
            let query = if query.is_empty() {
                String::new()
            } else {
                format!("?{query}")
            };
            format!("{server}/{name}{query}")
        };
        Scratch { client, name, url }
    }

    /// The engine as the server plants it over this database, its wire
    /// protocol, and the fake `llm` port it reaches.
    async fn serve(&self) -> (Arc<Engine>, Router, Arc<FakeLlm>) {
        let Planted { engine, rows } = planted(&self.url)
            .await
            .expect("the engine over PostgreSQL");
        let engine = Arc::new(engine);
        let llm = Arc::new(FakeLlm::new());
        (engine.clone(), router(engine, rows, llm.clone()), llm)
    }

    /// One table's rows, as JSON text, in a stable order.
    async fn rows(&self, table: &str) -> Vec<String> {
        self.client
            .query(
                &format!("SELECT to_jsonb(t)::text FROM {table} t ORDER BY to_jsonb(t)::text"),
                &[],
            )
            .await
            .expect("a table's rows")
            .iter()
            .map(|row| row.get(0))
            .collect()
    }

    /// A session's row, as JSON.
    async fn session(&self, id: &str) -> Value {
        let text: String = self
            .client
            .query_one(
                "SELECT to_jsonb(s)::text FROM session s WHERE id = ($1::text)::uuid",
                &[&id],
            )
            .await
            .expect("the session")
            .get(0);
        serde_json::from_str(&text).expect("JSON")
    }

    /// How many playbacks of `session` still run.
    async fn running(&self, session: &str) -> i64 {
        self.client
            .query_one(
                "SELECT count(*) FROM sound_playback
                 WHERE session_id = ($1::text)::uuid AND stopped_at IS NULL",
                &[&session],
            )
            .await
            .expect("the playbacks")
            .get(0)
    }

    async fn drop(self) {
        drop(self.client);
        let config: tokio_postgres::Config = url().parse().expect("a PostgreSQL URL");
        connect(&config)
            .await
            .batch_execute(&format!("DROP DATABASE {} WITH (FORCE)", self.name))
            .await
            .expect("the scratch database dropped");
    }
}

/// « Clore la séance » on `session`, as the LiveSessionBar sends it, by an
/// approver: the answer, and what the applier made of it.
async fn close(engine: &Engine, app: &Router, session: &str) -> (Value, CommandResult) {
    let body = json!({
        "dataCapability": "session.closeSession@1",
        "payload": {},
        "target": { "aggregate": "Session", "id": session },
        "caller": { "by": "dm", "from": "LiveSessionBar", "roles": ["role:approver"] },
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
    let answer: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    assert_eq!(status, StatusCode::ACCEPTED, "{answer}");
    let result = engine.run_once().await.expect("the applier ran").remove(0);
    (answer, result)
}

#[tokio::test]
async fn close_session_closes_the_row_stops_its_playbacks_keeps_its_journal_and_stores_the_summary()
{
    let scratch = Scratch::new("close").await;
    let (engine, app, llm) = scratch.serve().await;
    let journal = scratch.rows("session_journal_line").await;
    let other = scratch.session(AUTRE_SEANCE).await;

    let (answer, result) = close(&engine, &app, SEANCE).await;
    assert_eq!(result.status, State::Applied, "{result:?} {answer}");

    let row = scratch.session(SEANCE).await;
    assert_eq!(row["status"], "closed", "{row}");
    assert!(!row["closed_at"].is_null(), "{row}");
    assert_eq!(row["microphone_on"], false, "{row}");
    assert!(
        row["summary"]
            .as_str()
            .is_some_and(|s| !s.trim().is_empty()),
        "{row}"
    );
    // The cloud API is the active provider from the first start: Haiku
    // summarises.
    assert_eq!(row["summary_model"], "claude-haiku-4-5-20251001", "{row}");

    // stop-all: session 4's playbacks stop, session 3's plays on.
    assert_eq!(scratch.running(SEANCE).await, 0);
    assert_eq!(scratch.running(AUTRE_SEANCE).await, 1);
    let stopped_at_close: i64 = scratch
        .client
        .query_one(
            "SELECT count(*) FROM sound_playback p JOIN session s ON s.id = p.session_id
             WHERE s.id = ($1::text)::uuid AND p.stopped_at = s.closed_at",
            &[&SEANCE],
        )
        .await
        .expect("the playbacks")
        .get(0);
    assert_eq!(
        stopped_at_close, 2,
        "the two running playbacks stop at the close, the stopped one keeps its time"
    );

    // The journal is kept, line for line, and its text alone was sent.
    assert_eq!(scratch.rows("session_journal_line").await, journal);
    assert_eq!(scratch.session(AUTRE_SEANCE).await, other);
    let sent = llm.journal().list();
    assert_eq!(sent.len(), 1, "{sent:?}");
    assert_eq!(sent[0].kind, RequestKind::Journal);
    assert!(
        sent[0].text.contains("Elowen entre dans la taverne."),
        "{sent:?}"
    );
    assert!(
        sent[0].text.contains("Morlox lui tend la carte."),
        "{sent:?}"
    );

    // A second close of the same session is refused, and changes nothing.
    let closed = scratch.session(SEANCE).await;
    let playbacks = scratch.rows("sound_playback").await;
    let (_, again) = close(&engine, &app, SEANCE).await;
    assert_eq!(again.status, State::Rejected, "{again:?}");
    assert_eq!(again.violations, ["status-lifecycle"], "{again:?}");
    assert_eq!(scratch.session(SEANCE).await, closed);
    assert_eq!(scratch.rows("sound_playback").await, playbacks);

    scratch.drop().await;
}

#[tokio::test]
async fn status_lifecycle_refuses_to_close_a_session_in_prep_and_no_table_changes() {
    let scratch = Scratch::new("prep").await;
    let (engine, app, llm) = scratch.serve().await;
    let before = (
        scratch.rows("session").await,
        scratch.rows("sound_playback").await,
    );

    let (answer, result) = close(&engine, &app, PROCHAINE).await;
    assert_eq!(result.status, State::Rejected, "{result:?} {answer}");
    assert_eq!(result.violations, ["status-lifecycle"], "{result:?}");

    let after = (
        scratch.rows("session").await,
        scratch.rows("sound_playback").await,
    );
    assert_eq!(after, before);
    assert!(llm.journal().list().is_empty());

    scratch.drop().await;
}

#[tokio::test]
async fn a_close_the_tables_lost_is_written_again_at_the_next_start() {
    let scratch = Scratch::new("repair").await;
    let (engine, app, llm) = scratch.serve().await;
    let (answer, result) = close(&engine, &app, SEANCE).await;
    assert_eq!(result.status, State::Applied, "{result:?} {answer}");
    let closed = scratch.session(SEANCE).await;
    let sent = llm.journal().list().len();

    // The projection lost: the row live again, a playback running again.
    scratch
        .client
        .batch_execute(&format!(
            "UPDATE session SET status = 'live', closed_at = NULL, microphone_on = true,
                 summary = NULL, summary_model = NULL WHERE id = '{SEANCE}';
             UPDATE sound_playback SET stopped_at = NULL
                 WHERE session_id = '{SEANCE}' AND origin = 'pastille';"
        ))
        .await
        .expect("the projection undone");

    // The engine closed it: a close again is refused, whatever the table
    // still says, and nothing is summarised again.
    let (_, again) = close(&engine, &app, SEANCE).await;
    assert_eq!(again.status, State::Rejected, "{again:?}");
    assert_eq!(again.violations, ["status-lifecycle"], "{again:?}");
    assert_eq!(llm.journal().list().len(), sent);
    drop((engine, app, llm));

    // The next start writes the close again.
    let _restarted = scratch.serve().await;
    assert_eq!(scratch.session(SEANCE).await, closed);
    assert_eq!(scratch.running(SEANCE).await, 0);
    assert_eq!(scratch.running(AUTRE_SEANCE).await, 1);

    scratch.drop().await;
}
