//! session.createSession over PostgreSQL (`DATABASE_URL`), as the server
//! plants it: the next session of a campaign the tables hold — one the
//! fixtures wrote, which no command wrote before — lands after its last, in
//! prep, its microphone off, with exactly the PCs ticked present; an NPC or
//! a PC of another campaign is refused naming `present-only-pj`, a oneshot's
//! second session naming `oneshot-single-session`, a campaign there is not
//! as not found — none changes anything; the same idempotency key twice
//! creates one session. Each test works in a scratch database of its own,
//! migrated as the server migrates it, dropped when it ends.

use std::path::Path;
use std::sync::Arc;

use harness_dataguard::Error;
use harness_dataguard::aggregate::Aggregate;
use harness_dataguard::api::{Submission, Submitted};
use harness_dataguard::command::State;
use harness_dataguard::engine::Engine;
use harness_dataguard::manifest::DataCapability;
use harness_dataguard::memory::MemoryNotifier;
use harness_dataguard::plugins::{Plugins, RefusalKind};
use harness_dataguard::postgres::{PgStore, ReadOnly, SystemClock};
use harness_dataguard::store::RandomIds;
use harness_dataguard::tokio_postgres::{self, Client, NoTls};
use serde_json::{Value, json};

/// A campagne holding three sessions, a oneshot holding its one, a
/// campagne the engine keeps the tombstone of.
const VALOMBRE: &str = "c1000000-0000-4000-8000-000000000001";
const BRUME: &str = "c1000000-0000-4000-8000-000000000002";
const OUBLIEE: &str = "c1000000-0000-4000-8000-000000000003";
/// Valombre's two PCs and its NPC, and Brume's PC.
const ELOWEN: &str = "e1000000-0000-4000-8000-000000000001";
const KAEL: &str = "e1000000-0000-4000-8000-000000000002";
const GARDE: &str = "e1000000-0000-4000-8000-000000000003";
const MARIN: &str = "e1000000-0000-4000-8000-000000000005";

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
/// PCs and its NPC, Brume with its one session and its PC, and Oubliée.
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
        let mut client = connect(&url).await;
        let migrations = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../data/migrations");
        data::migrate(&mut client, &migrations)
            .await
            .expect("migrated");
        let versions: String = [ELOWEN, KAEL, GARDE, MARIN]
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
                     ('{OUBLIEE}', 'L''Oubliée', 'campagne');
                 INSERT INTO entity (id, campaign_id, type, name) VALUES
                     ('{ELOWEN}', '{VALOMBRE}', 'pj', 'Elowen'),
                     ('{KAEL}', '{VALOMBRE}', 'pj', 'Kael'),
                     ('{GARDE}', '{VALOMBRE}', 'pnj', 'Le garde'),
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
            .expect("Valombre, Brume, Oubliée, their sessions and their characters");
        Scratch { name, url, client }
    }

    /// The engine as the server plants this unit and its target aggregate
    /// over this database: the aggregate's projection in the apply
    /// transaction, the tables lent to the Prepare. Oubliée's tombstone is
    /// the engine's.
    async fn serve(&self) -> Engine {
        let plugins = || -> Plugins {
            [aggregate_session::plugin(), create_session::plugin()]
                .into_iter()
                .collect()
        };
        let store = PgStore::open(connect(&self.url).await)
            .await
            .expect("the store")
            .with_projections(&plugins())
            .await
            .expect("warmed");
        self.client
            .batch_execute(&format!(
                "INSERT INTO dataguard_rows (partition, tombstoned) VALUES ('Campaign/{OUBLIEE}', true)"
            ))
            .await
            .expect("Oubliée's tombstone");
        let mut engine = Engine::new(
            Arc::new(store),
            Arc::new(MemoryNotifier::default()),
            Arc::new(SystemClock),
        )
        .with_rows(Arc::new(ReadOnly::new(connect(&self.url).await)))
        .with_ids(Arc::new(RandomIds))
        .with_aggregate(Aggregate::parse(aggregate_session::DOCUMENT).expect("contract I"))
        .with_plugins(plugins());
        engine = engine.with_data_capability(
            DataCapability::parse(include_str!("../data-capability.json")).expect("contract F"),
        );
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

    /// Every session the tables hold, with its PCs present, by campaign and
    /// position.
    async fn sessions(&self) -> Vec<Value> {
        self.rows(
            "SELECT s.campaign_id::text AS campaign, s.position, s.title,
                s.planned_on::text AS planned_on, s.status, s.microphone_on,
                coalesce((SELECT jsonb_agg(p.character_id::text ORDER BY p.character_id)
                    FROM session_present_character p WHERE p.session_id = s.id), '[]') AS present
             FROM session s ORDER BY s.campaign_id, s.position",
        )
        .await
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

/// A submission of session.createSession, as SessionList sends it, under
/// `key` when one is given.
async fn create(
    engine: &Engine,
    payload: Value,
    key: Option<&str>,
) -> harness_dataguard::Result<Submitted> {
    let submission: Submission = serde_json::from_value(json!({
        "dataCapability": "session.createSession@1",
        "payload": payload,
        "idempotencyKey": key,
        // The critical fields' review is not what these tests judge.
        "caller": { "by": "dm", "from": "SessionList", "roles": ["role:approver"] },
    }))
    .expect("a submission");
    engine.submit(submission).await
}

/// What a refused submission was refused as, and why.
fn refused(answer: harness_dataguard::Result<Submitted>) -> (RefusalKind, String) {
    match answer {
        Err(Error::Refused(refusal)) => (refusal.kind, refusal.message),
        other => panic!("refused: {other:?}"),
    }
}

#[test]
fn the_manifest_is_an_insert_on_the_session_aggregate() {
    let manifest: Value =
        serde_json::from_str(include_str!("../data-capability.json")).expect("JSON");
    assert_eq!(manifest["dataCapability"], "session.createSession");
    assert_eq!(manifest["effect"], "insert");
    assert_eq!(manifest["target"]["aggregate"], "Session");
    DataCapability::parse(include_str!("../data-capability.json")).expect("contract F");
}

#[tokio::test]
async fn a_fourth_session_lands_at_position_4_in_prep_with_exactly_the_two_pcs_ticked() {
    let scratch = Scratch::new("fourth").await;
    let engine = scratch.serve().await;
    let submitted = create(
        &engine,
        json!({
            "campaign": VALOMBRE,
            "title": "La séance 4",
            "plannedOn": "2026-11-05",
            "present": [ELOWEN, KAEL],
        }),
        None,
    )
    .await
    .expect("queued");
    let result = engine.run_once().await.expect("the applier ran").remove(0);
    assert_eq!(result.status, State::Applied, "{result:?}");
    let id = submitted
        .partition
        .strip_prefix("Session/")
        .expect("a session's partition");
    assert_eq!(
        scratch
            .rows(&format!("SELECT position FROM session WHERE id = '{id}'"))
            .await,
        [json!({ "position": 4 })]
    );
    assert_eq!(
        scratch.sessions().await[3],
        json!({
            "campaign": VALOMBRE, "position": 4, "title": "La séance 4",
            "planned_on": "2026-11-05", "status": "prep", "microphone_on": false,
            "present": [ELOWEN, KAEL],
        })
    );
    scratch.drop().await;
}

#[tokio::test]
async fn present_only_pj_refuses_an_npc_or_a_pc_of_another_campaign_and_creates_nothing() {
    let scratch = Scratch::new("present").await;
    let engine = scratch.serve().await;
    let before = scratch.sessions().await;
    for stranger in [GARDE, MARIN, "elowen"] {
        let (kind, message) = refused(
            create(
                &engine,
                json!({ "campaign": VALOMBRE, "title": "La suite", "present": [ELOWEN, stranger] }),
                None,
            )
            .await,
        );
        assert_eq!(kind, RefusalKind::Unprocessable, "{stranger}");
        assert!(
            message.starts_with("present-only-pj"),
            "{stranger}: {message}"
        );
    }
    assert!(engine.run_once().await.expect("the applier ran").is_empty());
    assert_eq!(scratch.sessions().await, before);
    scratch.drop().await;
}

#[tokio::test]
async fn oneshot_single_session_refuses_a_session_for_a_oneshot_that_has_its_session() {
    let scratch = Scratch::new("oneshot").await;
    let engine = scratch.serve().await;
    let before = scratch.sessions().await;
    let (kind, message) = refused(
        create(
            &engine,
            json!({ "campaign": BRUME, "title": "Encore", "present": [MARIN] }),
            None,
        )
        .await,
    );
    assert_eq!(kind, RefusalKind::Unprocessable);
    assert!(message.starts_with("oneshot-single-session"), "{message}");
    assert!(engine.run_once().await.expect("the applier ran").is_empty());
    assert_eq!(scratch.sessions().await, before);
    scratch.drop().await;
}

#[tokio::test]
async fn the_same_idempotency_key_twice_creates_one_session() {
    let scratch = Scratch::new("replay").await;
    let engine = scratch.serve().await;
    let payload = json!({ "campaign": VALOMBRE, "title": "La séance 4", "present": [ELOWEN] });
    let first = create(&engine, payload.clone(), Some("k-1"))
        .await
        .expect("queued");
    let again = create(&engine, payload.clone(), Some("k-1"))
        .await
        .expect("replayed");
    assert!(again.replayed, "{again:?}");
    assert_eq!(again.partition, first.partition);
    for result in engine.run_once().await.expect("the applier ran") {
        assert_eq!(result.status, State::Applied, "{result:?}");
    }
    // Applied, a replay answers the first creation and queues none.
    let after = create(&engine, payload, Some("k-1"))
        .await
        .expect("replayed");
    assert_eq!(after.partition, first.partition);
    assert!(engine.run_once().await.expect("the applier ran").is_empty());
    assert_eq!(
        scratch
            .rows(&format!(
                "SELECT count(*) AS sessions FROM session WHERE campaign_id = '{VALOMBRE}'"
            ))
            .await,
        [json!({ "sessions": 4 })]
    );
    scratch.drop().await;
}

#[tokio::test]
async fn a_campaign_there_is_not_or_tombstoned_is_not_found_and_a_malformed_request_unprocessable()
{
    let scratch = Scratch::new("unknown").await;
    let engine = scratch.serve().await;
    let before = scratch.sessions().await;
    // With no PC ticked or with one: the campaign is what is not found.
    for campaign in [
        "cendres-valombre",
        "c1000000-0000-4000-8000-0000000000ff",
        OUBLIEE,
    ] {
        for present in [json!([]), json!([ELOWEN])] {
            let (kind, message) = refused(
                create(
                    &engine,
                    json!({ "campaign": campaign, "title": "La suite", "present": present }),
                    None,
                )
                .await,
            );
            assert_eq!(kind, RefusalKind::NotFound, "{campaign}: {message}");
        }
    }
    for payload in [
        json!({ "campaign": VALOMBRE, "title": "  " }),
        json!({ "campaign": VALOMBRE, "title": "La suite", "present": ELOWEN }),
    ] {
        let (kind, _) = refused(create(&engine, payload.clone(), None).await);
        assert_eq!(kind, RefusalKind::Unprocessable, "{payload}");
    }
    assert!(engine.run_once().await.expect("the applier ran").is_empty());
    assert_eq!(scratch.sessions().await, before);
    scratch.drop().await;
}
