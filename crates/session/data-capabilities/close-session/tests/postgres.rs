//! session.closeSession over PostgreSQL (`DATABASE_URL`), as the server
//! plants it: a live session the tables hold — one the fixtures wrote,
//! which no command wrote before — is closed on its `session` row, its
//! microphone off and its summary stored with the active provider's summary
//! model; every playback of the session still running stops at the close
//! (`stop-all`), the others' untouched; its journal stays as it was. A
//! session in prep is refused under `status-lifecycle`, and changes
//! nothing. Each test works in a scratch database of its own, migrated as
//! the server migrates it, dropped when it ends.

use std::path::Path;
use std::sync::Arc;

use harness_dataguard::Error;
use harness_dataguard::aggregate::Aggregate;
use harness_dataguard::api::{Submission, Submitted};
use harness_dataguard::command::State;
use harness_dataguard::engine::Engine;
use harness_dataguard::manifest::DataCapability;
use harness_dataguard::memory::MemoryNotifier;
use harness_dataguard::plugins::{Plugins, ProjectPg, RefusalKind, Shared};
use harness_dataguard::postgres::{PgStore, ReadOnly, SystemClock};
use harness_dataguard::store::{RandomIds, Write};
use harness_dataguard::tokio_postgres::{self, Client, NoTls};
use ports_llm::fake::FakeLlm;
use ports_llm::{Llm, Provider, RequestKind, SharedLlm};
use serde_json::{Value, json};

const VALOMBRE: &str = "c1000000-0000-4000-8000-000000000001";
/// Valombre's session 4, live, its microphone on.
const SEANCE: &str = "51000000-0000-4000-8000-000000000004";
/// Another live session, whose playbacks a close of session 4 leaves alone.
const AUTRE: &str = "51000000-0000-4000-8000-000000000005";
/// A session still in preparation.
const PREP: &str = "51000000-0000-4000-8000-000000000006";
/// Session 3, closed on the 3rd, a playback of it left running.
const CLOSE: &str = "51000000-0000-4000-8000-000000000003";
const SON: &str = "50000000-0000-4000-8000-000000000001";
/// Session 4's two running playbacks, the one it stopped at 21:00, the
/// other live session's running one, and closed session 3's.
const PLUIE: &str = "b1000000-0000-4000-8000-000000000001";
const TAVERNE: &str = "b1000000-0000-4000-8000-000000000002";
const PORTE: &str = "b1000000-0000-4000-8000-000000000003";
const VENT: &str = "b1000000-0000-4000-8000-000000000004";
const MARCHE: &str = "b1000000-0000-4000-8000-000000000005";
/// The opencode binary's row, as the migration seeds it.
const OPENCODE_BINARY: &str = "a1000000-0000-4000-8000-000000000003";

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

/// A database of its own, migrated as the server migrates it — the cloud
/// API the active provider — holding, as the fixtures write them, Valombre
/// with its three sessions, a sound, its four playbacks and session 4's
/// journal.
struct Scratch {
    name: String,
    url: String,
    client: Client,
}

impl Scratch {
    async fn new(name: &str) -> Scratch {
        let name = format!("close_session_test_{name}_{}", std::process::id());
        let admin = connect(&url()).await;
        for sql in [
            format!("DROP DATABASE IF EXISTS {name} WITH (FORCE)"),
            format!("CREATE DATABASE {name}"),
        ] {
            admin.batch_execute(&sql).await.expect("a scratch database");
        }
        let url = naming(&name);
        let mut client = connect(&url).await;
        let migrations = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../data/migrations");
        data::migrate(&mut client, &migrations)
            .await
            .expect("migrated");
        client
            .batch_execute(&format!(
                "INSERT INTO campaign (id, name, kind) VALUES
                     ('{VALOMBRE}', 'Les Cendres de Valombre', 'campagne');
                 INSERT INTO session (id, campaign_id, position, title, status, started_at, microphone_on)
                     VALUES ('{SEANCE}', '{VALOMBRE}', 4, 'L''embuscade du Gué', 'live', '2026-10-17 20:02', true),
                            ('{AUTRE}', '{VALOMBRE}', 5, 'Le phare', 'live', '2026-10-24 20:00', true),
                            ('{PREP}', '{VALOMBRE}', 6, 'La tour', 'prep', NULL, false);
                 INSERT INTO session (id, campaign_id, position, title, status, started_at, closed_at,
                                      microphone_on, summary, summary_model)
                     VALUES ('{CLOSE}', '{VALOMBRE}', 3, 'Le marché noir', 'closed', '2026-10-03 20:00',
                             '2026-10-03 23:00', false, 'Morlox a avoué.', 'qwen2.5-14b-instruct');
                 INSERT INTO sound (id, campaign_id, name, file_path)
                     VALUES ('{SON}', '{VALOMBRE}', 'Pluie', 'sons/pluie.ogg');
                 INSERT INTO sound_playback (id, session_id, sound_id, origin, started_at, stopped_at) VALUES
                     ('{PLUIE}', '{SEANCE}', '{SON}', 'soundbox', '2026-10-17 20:05', NULL),
                     ('{TAVERNE}', '{SEANCE}', '{SON}', 'pastille', '2026-10-17 22:10', NULL),
                     ('{PORTE}', '{SEANCE}', '{SON}', 'soundbox', '2026-10-17 20:30', '2026-10-17 21:00'),
                     ('{VENT}', '{AUTRE}', '{SON}', 'soundbox', '2026-10-24 20:05', NULL),
                     ('{MARCHE}', '{CLOSE}', '{SON}', 'soundbox', '2026-10-03 20:10', NULL);
                 INSERT INTO session_journal_line (session_id, spoken_at, text) VALUES
                     ('{SEANCE}', '2026-10-17 21:05', 'Morlox finit par avouer.'),
                     ('{SEANCE}', '2026-10-17 20:04', 'On reprend sous le pont.');"
            ))
            .await
            .expect("Valombre, its sessions, playbacks and journal");
        Scratch { name, url, client }
    }

    /// The engine as the server plants this unit and its target aggregate
    /// over this database, `llm` shared: the close's projection in the
    /// apply transaction, the tables lent to the Prepare.
    async fn serve(&self, llm: &FakeLlm) -> Engine {
        let plugins = || -> Plugins {
            [aggregate_session::plugin(), close_session::plugin()]
                .into_iter()
                .collect()
        };
        let store = PgStore::open(connect(&self.url).await)
            .await
            .expect("the store")
            .with_projections(&plugins())
            .await
            .expect("warmed");
        let path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../aggregates/Session/aggregate.json");
        let aggregate = std::fs::read_to_string(path).expect("Session's aggregate.json");
        let mut shared = Shared::default();
        shared.insert(Arc::new(SharedLlm::new(llm.clone())));
        let engine = Engine::new(
            Arc::new(store),
            Arc::new(MemoryNotifier::default()),
            Arc::new(SystemClock),
        )
        .with_rows(Arc::new(ReadOnly::new(connect(&self.url).await)))
        .with_ids(Arc::new(RandomIds))
        .with_aggregate(Aggregate::parse(&aggregate).expect("contract I"))
        .with_data_capability(
            DataCapability::parse(include_str!("../data-capability.json")).expect("contract F"),
        )
        .with_plugins(plugins())
        .with_shared(shared);
        engine.load().await.expect("the engine loads");
        engine
    }

    /// What `sql` answers, each row as JSON, in order.
    async fn rows(&self, sql: &str) -> Vec<Value> {
        self.client
            .query(&format!("SELECT to_jsonb(t)::text FROM ({sql}) t"), &[])
            .await
            .expect("the rows")
            .iter()
            .map(|row| serde_json::from_str(row.get::<_, &str>(0)).expect("JSON"))
            .collect()
    }

    /// The session `id` as its row says it: status, closed, microphone,
    /// summary and its model.
    async fn session(&self, id: &str) -> Value {
        self.rows(&format!(
            "SELECT status, closed_at IS NOT NULL AS closed, microphone_on, summary, summary_model
             FROM session WHERE id = '{id}'"
        ))
        .await
        .remove(0)
    }

    /// Drops the scratch database. A role that is no superuser cannot end
    /// an autovacuum worker still on it: it is tried again a moment later.
    async fn drop(self) {
        drop(self.client);
        let admin = connect(&url()).await;
        let sql = format!("DROP DATABASE {} WITH (FORCE)", self.name);
        for _ in 0..20 {
            if admin.batch_execute(&sql).await.is_ok() {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        }
        admin
            .batch_execute(&sql)
            .await
            .expect("the scratch database dropped");
    }
}

/// session.closeSession of `id`, as the live session's bar sends it.
async fn close(engine: &Engine, id: &str) -> harness_dataguard::Result<Submitted> {
    let submission: Submission = serde_json::from_value(json!({
        "dataCapability": "session.closeSession@1",
        "payload": {},
        "target": { "id": id },
        // The critical fields' review is not what these tests judge.
        "caller": { "by": "dm", "from": "LiveSessionBar", "roles": ["role:approver"] },
    }))
    .expect("a submission");
    engine.submit(submission).await
}

#[tokio::test]
async fn close_session_stop_all_a_live_session_is_closed_its_sounds_stopped_its_journal_kept() {
    let scratch = Scratch::new("close").await;
    let llm = FakeLlm::new();
    let engine = scratch.serve(&llm).await;
    let journal = "SELECT to_char(spoken_at, 'YYYY-MM-DD HH24:MI') AS at, text
        FROM session_journal_line ORDER BY spoken_at";
    let before = scratch.rows(journal).await;

    let submitted = close(&engine, SEANCE).await.expect("queued");
    assert!(submitted.warnings.is_empty(), "{:?}", submitted.warnings);
    let result = engine.run_once().await.expect("the applier ran").remove(0);
    assert_eq!(result.status, State::Applied, "{result:?}");

    let session = scratch.session(SEANCE).await;
    assert_eq!(session["status"], "closed");
    assert_eq!(session["closed"], true);
    assert_eq!(session["microphone_on"], false);
    assert_eq!(session["summary_model"], "claude-haiku-4-5-20251001");
    assert!(
        session["summary"]
            .as_str()
            .is_some_and(|summary| !summary.trim().is_empty()),
        "{session}"
    );
    // stop-all: session 4's running playbacks stop at its close; the one it
    // stopped keeps its instant, the other session's plays on.
    assert_eq!(
        scratch
            .rows(&format!(
                "SELECT p.id::text, p.stopped_at = s.closed_at AS at_close,
                        to_char(p.stopped_at, 'HH24:MI') AS stopped
                 FROM sound_playback p JOIN session s ON s.id = '{SEANCE}'
                 WHERE p.stopped_at IS DISTINCT FROM s.closed_at ORDER BY p.id"
            ))
            .await,
        [
            json!({ "id": PORTE, "at_close": false, "stopped": "21:00" }),
            json!({ "id": VENT, "at_close": null, "stopped": null }),
            json!({ "id": MARCHE, "at_close": null, "stopped": null }),
        ]
    );
    assert_eq!(
        scratch
            .rows(&format!(
                "SELECT p.id::text FROM sound_playback p JOIN session s ON s.id = '{SEANCE}'
                 WHERE p.stopped_at = s.closed_at ORDER BY p.id"
            ))
            .await,
        [json!({ "id": PLUIE }), json!({ "id": TAVERNE })]
    );
    // The journal, as it was; the other session still live.
    assert_eq!(scratch.rows(journal).await, before);
    assert_eq!(scratch.session(AUTRE).await["status"], "live");
    // text-only-no-audio: the journal's text, in order, with its times,
    // once, to the active provider.
    let sent = llm.journal().list();
    assert_eq!(sent.len(), 1, "{sent:?}");
    assert_eq!(sent[0].kind, RequestKind::Journal);
    assert_eq!(sent[0].provider, Provider::Cloud);
    assert_eq!(
        sent[0].text,
        "Journal de la séance « L'embuscade du Gué »\n\
         20:04 On reprend sous le pont.\n\
         21:05 Morlox finit par avouer."
    );
    scratch.drop().await;
}

#[tokio::test]
async fn close_session_the_summary_is_written_by_the_provider_the_tables_make_active() {
    let scratch = Scratch::new("binary").await;
    scratch
        .client
        .batch_execute(&format!(
            "UPDATE ai_settings SET active_provider_id = '{OPENCODE_BINARY}' WHERE id = 1"
        ))
        .await
        .expect("the opencode binary active");
    let llm = FakeLlm::new();
    let engine = scratch.serve(&llm).await;
    close(&engine, AUTRE).await.expect("queued");
    engine.run_once().await.expect("the applier ran");
    let session = scratch.session(AUTRE).await;
    assert_eq!(session["status"], "closed");
    assert_eq!(session["summary_model"], "qwen2.5-14b-instruct");
    let sent = llm.journal().list();
    assert_eq!(sent.len(), 1, "{sent:?}");
    assert_eq!(sent[0].provider, Provider::OpencodeBinary);
    // A session with no journal says so, and nothing else.
    assert_eq!(
        sent[0].text,
        "Journal de la séance « Le phare »\n(aucune ligne transcrite)"
    );
    // Session 4 neither closed nor stopped.
    assert_eq!(scratch.session(SEANCE).await["status"], "live");
    assert_eq!(
        scratch
            .rows(&format!(
                "SELECT count(*) AS n FROM sound_playback WHERE session_id = '{SEANCE}' AND stopped_at IS NULL"
            ))
            .await[0]["n"],
        2
    );
    scratch.drop().await;
}

#[tokio::test]
async fn status_lifecycle_a_session_the_tables_hold_in_prep_is_refused_and_nothing_changes() {
    let scratch = Scratch::new("prep").await;
    let llm = FakeLlm::new();
    let engine = scratch.serve(&llm).await;
    let before = scratch.session(PREP).await;
    match close(&engine, PREP).await {
        Err(Error::Refused(refusal)) => {
            assert_eq!(refusal.kind, RefusalKind::Unprocessable);
            assert!(
                refusal.message.starts_with("status-lifecycle"),
                "{}",
                refusal.message
            );
        }
        other => panic!("refused: {other:?}"),
    }
    assert!(engine.run_once().await.expect("the applier ran").is_empty());
    assert_eq!(scratch.session(PREP).await, before);
    assert!(llm.journal().list().is_empty());
    // A session neither the engine nor the tables hold, by a uuid or not.
    for id in ["51000000-0000-4000-8000-0000000000ff", "order-1", "0a1b"] {
        match close(&engine, id).await {
            Err(Error::Refused(refusal)) => assert_eq!(refusal.kind, RefusalKind::NotFound),
            other => panic!("{id} not found: {other:?}"),
        }
    }
    scratch.drop().await;
}

#[tokio::test]
async fn status_lifecycle_a_session_the_tables_hold_closed_is_never_closed_again() {
    let scratch = Scratch::new("again").await;
    let engine = scratch.serve(&FakeLlm::new()).await;
    match close(&engine, CLOSE).await {
        Err(Error::Refused(refusal)) => assert!(
            refusal.message.starts_with("status-lifecycle"),
            "{}",
            refusal.message
        ),
        other => panic!("refused: {other:?}"),
    }
    let session = "SELECT status, to_char(closed_at, 'YYYY-MM-DD HH24:MI') AS closed_at,
            microphone_on, summary, summary_model FROM session ORDER BY id";
    let playbacks = "SELECT id::text, stopped_at FROM sound_playback ORDER BY id";
    let before = (scratch.rows(session).await, scratch.rows(playbacks).await);
    // A close that got past its preparation all the same, a later write of
    // the closed session, its tombstone, a partition no session's: the
    // tables keep session 3's close, its summary, its playbacks as they are.
    let mut client = connect(&scratch.url).await;
    let tx = client.transaction().await.expect("a transaction");
    let closed = |partition: &str, tombstone| Write {
        partition: partition.to_owned(),
        values: serde_json::from_value(json!({
            "sessionStatus": "closed",
            "closedAt": "2026-10-17T22:40:00.123Z",
            "microphoneOn": false,
            "summary": "Un autre résumé.",
            "summaryModel": "claude-haiku-4-5-20251001",
        }))
        .expect("a row"),
        tombstone,
    };
    for write in [
        closed(&format!("Session/{CLOSE}"), false),
        closed(&format!("Session/{SEANCE}"), true),
        closed("Session/order-1", false),
    ] {
        close_session::Closes
            .project(&write, &tx)
            .await
            .expect("projected");
    }
    tx.commit().await.expect("committed");
    assert_eq!(
        (scratch.rows(session).await, scratch.rows(playbacks).await),
        before
    );
    scratch.drop().await;
}
