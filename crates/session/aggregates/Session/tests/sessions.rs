//! The Session aggregate's sessions over PostgreSQL (`DATABASE_URL`), as the
//! server's store runs its projection: a session the engine applies on
//! `Session/<its id>` reaches `session`, after its campaign's last, and the
//! PCs it ticks `session_present_character`; a oneshot's second session
//! (`oneshot-single-session`), a character that is no PC of the campaign
//! (`present-only-pj`) or a campaign the tables do not hold rejects the
//! command whole. The DataCapabilities here are test-only: each write is the
//! row as given. Each test works in a scratch database of its own, migrated
//! as the server migrates it, dropped when it ends.

use std::path::Path;
use std::sync::Arc;

use harness_dataguard::aggregate::Aggregate;
use harness_dataguard::api::Submission;
use harness_dataguard::command::{CommandResult, State};
use harness_dataguard::engine::Engine;
use harness_dataguard::manifest::DataCapability;
use harness_dataguard::memory::MemoryNotifier;
use harness_dataguard::plugins::Plugins;
use harness_dataguard::postgres::{PgStore, ReadOnly, SystemClock};
use harness_dataguard::tokio_postgres::{self, Client, NoTls};
use serde_json::{Value, json};

/// A session written whole, test-only.
const ADD: &str = r#"{"dataCapability":"test.addSession","owner":"dataguard","version":1,"effect":"insert","target":{"aggregate":"Session","id":"$sessionId"},"mode":"overwrite","callableBy":["*"]}"#;
/// Fields of a session changed, test-only.
const UPDATE: &str = r#"{"dataCapability":"test.updateSession","owner":"dataguard","version":1,"effect":"update","target":{"aggregate":"Session","id":"$sessionId"},"mode":"overwrite","callableBy":["*"]}"#;
/// A session deleted: the engine's tombstone.
const DELETE: &str = r#"{"dataCapability":"test.deleteSession","owner":"dataguard","version":1,"effect":"delete","target":{"aggregate":"Session","id":"$sessionId"},"mode":"overwrite","callableBy":["*"]}"#;

/// A campagne holding three sessions, a oneshot holding its one, a oneshot
/// holding none yet.
const VALOMBRE: &str = "c1000000-0000-4000-8000-000000000001";
const BRUME: &str = "c1000000-0000-4000-8000-000000000002";
const LUNE: &str = "c1000000-0000-4000-8000-000000000003";
const NOWHERE: &str = "c1000000-0000-4000-8000-0000000000ff";
/// Valombre's PCs with their sheets, its NPC, a « pj » of it with no sheet,
/// and Brume's PC.
const ELOWEN: &str = "e1000000-0000-4000-8000-000000000001";
const KAEL: &str = "e1000000-0000-4000-8000-000000000002";
const GARDE: &str = "e1000000-0000-4000-8000-000000000003";
const ESQUISSE: &str = "e1000000-0000-4000-8000-000000000004";
const MARIN: &str = "e1000000-0000-4000-8000-000000000005";
/// Sessions the tests add.
const QUATRE: &str = "51000000-0000-4000-8000-00000000000a";
const CINQ: &str = "51000000-0000-4000-8000-00000000000b";

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

/// A database of its own, migrated as the server migrates it, holding —
/// as the fixtures write them — Valombre with its three sessions, its two
/// PCs, its NPC and its sketch of a PC, Brume with its one session and its
/// PC, and Lune with none.
struct Scratch {
    name: String,
    url: String,
    client: Client,
    notifier: Arc<MemoryNotifier>,
}

impl Scratch {
    async fn new(name: &str) -> Scratch {
        let name = format!("session_sessions_test_{name}_{}", std::process::id());
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
        let versions: String = [ELOWEN, KAEL, GARDE, ESQUISSE, MARIN]
            .iter()
            .map(|element| {
                format!(
                    "INSERT INTO entity_version (entity_id, version_number, source, snapshot, diff) \
                     VALUES ('{element}', 1, 'dm', '{{}}', '[]');"
                )
            })
            .collect();
        client
            .batch_execute(&format!(
                "INSERT INTO campaign (id, name, kind) VALUES
                     ('{VALOMBRE}', 'Les Cendres de Valombre', 'campagne'),
                     ('{BRUME}', 'La Brume', 'oneshot'),
                     ('{LUNE}', 'La Lune', 'oneshot');
                 INSERT INTO entity (id, campaign_id, type, name) VALUES
                     ('{ELOWEN}', '{VALOMBRE}', 'pj', 'Elowen'),
                     ('{KAEL}', '{VALOMBRE}', 'pj', 'Kael'),
                     ('{GARDE}', '{VALOMBRE}', 'pnj', 'Le garde'),
                     ('{ESQUISSE}', '{VALOMBRE}', 'pj', 'Une esquisse'),
                     ('{MARIN}', '{BRUME}', 'pj', 'Le marin');
                 {versions}
                 INSERT INTO character (entity_id, token_color) VALUES
                     ('{ELOWEN}', '#2a6'), ('{KAEL}', '#26a'), ('{GARDE}', '#a62'),
                     ('{MARIN}', '#6a2');
                 INSERT INTO session (campaign_id, position, title, status, microphone_on) VALUES
                     ('{VALOMBRE}', 1, 'La séance 1', 'closed', false),
                     ('{VALOMBRE}', 2, 'La séance 2', 'closed', false),
                     ('{VALOMBRE}', 3, 'La séance 3', 'closed', false),
                     ('{BRUME}', 1, 'La Brume', 'prep', false);"
            ))
            .await
            .expect("Valombre, Brume, Lune, their sessions and their characters");
        Scratch {
            name,
            url,
            client,
            notifier: Arc::default(),
        }
    }

    /// The engine as the server plants this aggregate over this database:
    /// its projection in the apply transaction. What it tells the authors
    /// is `self.notifier`'s.
    async fn serve(&self) -> Engine {
        let plugins = || -> Plugins { std::iter::once(aggregate_session::plugin()).collect() };
        let store = PgStore::open(connect(&self.url).await)
            .await
            .expect("the store")
            .with_projections(&plugins())
            .await
            .expect("warmed");
        let mut engine = Engine::new(
            Arc::new(store),
            self.notifier.clone(),
            Arc::new(SystemClock),
        )
        .with_rows(Arc::new(ReadOnly::new(connect(&self.url).await)))
        .with_aggregate(Aggregate::parse(aggregate_session::DOCUMENT).expect("contract I"))
        .with_plugins(plugins());
        for manifest in [ADD, UPDATE, DELETE] {
            engine =
                engine.with_data_capability(DataCapability::parse(manifest).expect("contract F"));
        }
        engine.load().await.expect("the engine loads");
        engine
    }

    /// What `sql` answers, each row as JSON text, in order.
    async fn rows(&self, sql: &str) -> Vec<Value> {
        self.client
            .query(&format!("SELECT to_jsonb(t)::text FROM ({sql}) t"), &[])
            .await
            .expect("the rows")
            .iter()
            .map(|row| serde_json::from_str(row.get::<_, &str>(0)).expect("JSON"))
            .collect()
    }

    /// The session `id` as the tables hold it, with its PCs present.
    async fn session(&self, id: &str) -> Vec<Value> {
        self.rows(&format!(
            "SELECT s.campaign_id::text AS campaign, s.position, s.title,
                s.planned_on::text AS planned_on, s.status, s.microphone_on,
                coalesce((SELECT jsonb_agg(p.character_id::text ORDER BY p.character_id)
                    FROM session_present_character p WHERE p.session_id = s.id), '[]') AS present
             FROM session s WHERE s.id = '{id}'"
        ))
        .await
    }

    /// How many sessions the tables hold.
    async fn count(&self) -> usize {
        self.rows("SELECT id FROM session").await.len()
    }

    /// Why the last command was rejected, as the authors are told.
    fn reason(&self) -> String {
        self.notifier
            .sent()
            .into_iter()
            .rev()
            .find_map(|message| message.reason)
            .unwrap_or_default()
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

/// The session `id` of `campaign`, in prep, its microphone off, ticking
/// `present`.
fn session(id: &str, campaign: &str, present: &[&str]) -> Value {
    json!({
        "sessionId": id,
        "sessionCampaignId": campaign,
        "sessionTitle": "La suite",
        "plannedOn": "2026-11-05",
        "sessionStatus": "prep",
        "microphoneOn": false,
        "sessionPresentCharacterCharacterId": present,
    })
}

/// Submits `payload` through `data_capability`, by the DM, who approves.
async fn submit(engine: &Engine, data_capability: &str, payload: Value) {
    let submission: Submission = serde_json::from_value(json!({
        "dataCapability": data_capability,
        "payload": payload,
        // The critical fields' review is not what these tests judge.
        "caller": { "by": "dm", "from": "SessionList", "roles": ["role:approver"] },
    }))
    .expect("a submission");
    engine.submit(submission).await.expect("queued");
}

/// What the applier made of the one command queued.
async fn applied(engine: &Engine) -> CommandResult {
    let mut applied = engine.run_once().await.expect("the applier ran");
    assert_eq!(applied.len(), 1, "{applied:?}");
    applied.remove(0)
}

#[tokio::test]
async fn a_new_session_takes_the_position_after_its_campaign_s_last_with_its_pcs_present() {
    let scratch = Scratch::new("fourth").await;
    let engine = scratch.serve().await;
    submit(
        &engine,
        "test.addSession",
        session(QUATRE, VALOMBRE, &[KAEL, ELOWEN]),
    )
    .await;
    let result = applied(&engine).await;
    assert_eq!(result.status, State::Applied, "{result:?}");
    assert_eq!(
        scratch.session(QUATRE).await,
        [json!({
            "campaign": VALOMBRE, "position": 4, "title": "La suite",
            "planned_on": "2026-11-05", "status": "prep", "microphone_on": false,
            "present": [ELOWEN, KAEL],
        })]
    );
    scratch.drop().await;
}

#[tokio::test]
async fn sessions_queued_side_by_side_each_take_the_next_position() {
    let scratch = Scratch::new("side").await;
    let engine = scratch.serve().await;
    for id in [QUATRE, CINQ] {
        submit(&engine, "test.addSession", session(id, VALOMBRE, &[])).await;
    }
    let mut results = Vec::new();
    loop {
        let settled = engine.run_once().await.expect("the applier ran");
        if settled.is_empty() {
            break;
        }
        results.extend(settled);
    }
    assert_eq!(results.len(), 2, "{results:?}");
    assert!(
        results.iter().all(|result| result.status == State::Applied),
        "{results:?}"
    );
    let positions = scratch
        .rows("SELECT position FROM session WHERE title = 'La suite' ORDER BY position")
        .await;
    assert_eq!(
        positions,
        [json!({ "position": 4 }), json!({ "position": 5 })]
    );
    scratch.drop().await;
}

#[tokio::test]
async fn oneshot_single_session_rejects_a_oneshot_s_second_session_whole() {
    let scratch = Scratch::new("oneshot").await;
    let engine = scratch.serve().await;
    let before = scratch.count().await;
    submit(&engine, "test.addSession", session(QUATRE, BRUME, &[MARIN])).await;
    let result = applied(&engine).await;
    assert_eq!(result.status, State::Rejected, "{result:?}");
    assert!(
        scratch.reason().contains("oneshot-single-session"),
        "{}",
        scratch.reason()
    );
    assert_eq!(scratch.count().await, before);
    assert!(
        scratch
            .rows("SELECT * FROM session_present_character")
            .await
            .is_empty()
    );
    // A oneshot with no session yet takes its one, at position 1.
    submit(&engine, "test.addSession", session(CINQ, LUNE, &[])).await;
    assert_eq!(applied(&engine).await.status, State::Applied);
    assert_eq!(scratch.session(CINQ).await[0]["position"], 1);
    scratch.drop().await;
}

#[tokio::test]
async fn present_only_pj_rejects_an_npc_a_pc_of_another_campaign_or_one_with_no_sheet_whole() {
    let scratch = Scratch::new("present").await;
    let engine = scratch.serve().await;
    let before = scratch.count().await;
    for stranger in [GARDE, MARIN, ESQUISSE] {
        submit(
            &engine,
            "test.addSession",
            session(QUATRE, VALOMBRE, &[ELOWEN, stranger]),
        )
        .await;
        let result = applied(&engine).await;
        assert_eq!(result.status, State::Rejected, "{stranger}: {result:?}");
        assert!(
            scratch.reason().contains("present-only-pj"),
            "{}",
            scratch.reason()
        );
    }
    assert_eq!(scratch.count().await, before);
    assert!(
        scratch
            .rows("SELECT * FROM session_present_character")
            .await
            .is_empty()
    );
    scratch.drop().await;
}

#[tokio::test]
async fn a_session_written_again_keeps_its_position_and_takes_its_row() {
    let scratch = Scratch::new("again").await;
    let engine = scratch.serve().await;
    submit(
        &engine,
        "test.addSession",
        session(QUATRE, VALOMBRE, &[ELOWEN, KAEL]),
    )
    .await;
    assert_eq!(applied(&engine).await.status, State::Applied);
    submit(
        &engine,
        "test.updateSession",
        json!({
            "sessionId": QUATRE, "sessionTitle": "Le retour", "plannedOn": null,
            "sessionStatus": "live", "microphoneOn": true,
            "sessionPresentCharacterCharacterId": [KAEL],
        }),
    )
    .await;
    let result = applied(&engine).await;
    assert_eq!(result.status, State::Applied, "{result:?}");
    assert_eq!(
        scratch.session(QUATRE).await,
        [json!({
            "campaign": VALOMBRE, "position": 4, "title": "Le retour",
            "planned_on": null, "status": "live", "microphone_on": true,
            "present": [KAEL],
        })]
    );
    scratch.drop().await;
}

#[tokio::test]
async fn a_row_with_no_list_of_pcs_leaves_those_present_as_they_are() {
    let scratch = Scratch::new("nolist").await;
    let engine = scratch.serve().await;
    let mut row = session(QUATRE, VALOMBRE, &[]);
    row.as_object_mut()
        .expect("a row")
        .remove("sessionPresentCharacterCharacterId");
    submit(&engine, "test.addSession", row).await;
    assert_eq!(applied(&engine).await.status, State::Applied);
    scratch
        .client
        .batch_execute(&format!(
            "INSERT INTO session_present_character (session_id, character_id)
                 VALUES ('{QUATRE}', '{ELOWEN}')"
        ))
        .await
        .expect("Elowen present");
    submit(
        &engine,
        "test.updateSession",
        json!({ "sessionId": QUATRE, "sessionTitle": "Le retour" }),
    )
    .await;
    assert_eq!(applied(&engine).await.status, State::Applied);
    assert_eq!(scratch.session(QUATRE).await[0]["present"], json!([ELOWEN]));
    scratch.drop().await;
}

#[tokio::test]
async fn a_tombstone_or_a_campaign_there_is_not_writes_no_session() {
    let scratch = Scratch::new("others").await;
    let engine = scratch.serve().await;
    submit(&engine, "test.addSession", session(QUATRE, VALOMBRE, &[])).await;
    assert_eq!(applied(&engine).await.status, State::Applied);
    // The engine's tombstone: the tables keep the session.
    submit(
        &engine,
        "test.deleteSession",
        json!({ "sessionId": QUATRE }),
    )
    .await;
    assert_eq!(applied(&engine).await.status, State::Applied);
    assert_eq!(scratch.session(QUATRE).await.len(), 1);
    // A campaign the tables do not hold: rejected, nothing written.
    submit(&engine, "test.addSession", session(CINQ, NOWHERE, &[])).await;
    let result = applied(&engine).await;
    assert_eq!(result.status, State::Rejected, "{result:?}");
    assert!(
        scratch.reason().contains("sessionCampaignId"),
        "{}",
        scratch.reason()
    );
    assert!(scratch.session(CINQ).await.is_empty());
    scratch.drop().await;
}
