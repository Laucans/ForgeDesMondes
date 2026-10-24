//! session.addPresentCharacter over PostgreSQL (`DATABASE_URL`), as the
//! server plants it: a PC of the session's campaign, ticked once its
//! session is created — one the fixtures wrote, which no command wrote
//! before — is listed by `session_roster_view` « present »; an NPC, a PC of
//! another campaign or an id the tables do not hold is refused naming
//! `present-only-pj`, a session there is not as not found — none changes
//! anything; a PC that stops being one before it is applied is rejected
//! then; a PC present already, by its creation or ticked again, keeps its
//! one row. Each test works in a scratch database of its own, migrated as
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
use harness_dataguard::plugins::{Plugins, RefusalKind};
use harness_dataguard::postgres::{PgStore, ReadOnly, SystemClock};
use harness_dataguard::store::RandomIds;
use harness_dataguard::tokio_postgres::{self, Client, NoTls};
use serde_json::{Value, json};

/// A campagne holding a session in prep, and another campaign.
const VALOMBRE: &str = "c1000000-0000-4000-8000-000000000001";
const BRUME: &str = "c1000000-0000-4000-8000-000000000002";
/// Valombre's session, Elowen ticked present when it was created, and a
/// session of it the engine keeps the tombstone of.
const SEANCE: &str = "51000000-0000-4000-8000-000000000001";
const OUBLIEE: &str = "51000000-0000-4000-8000-000000000002";
/// Valombre's three PCs and its NPC, and Brume's PC.
const ELOWEN: &str = "e1000000-0000-4000-8000-000000000001";
const KAEL: &str = "e1000000-0000-4000-8000-000000000002";
const SIRIELLE: &str = "e1000000-0000-4000-8000-000000000004";
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

/// A database of its own, migrated as the server migrates it, holding — as
/// the fixtures write them — Valombre with its two sessions, Elowen present
/// at the first, its three PCs and its NPC, and Brume with its PC.
struct Scratch {
    name: String,
    url: String,
    client: Client,
}

impl Scratch {
    async fn new(name: &str) -> Scratch {
        let name = format!("add_present_character_test_{name}_{}", std::process::id());
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
        let versions: String = [ELOWEN, KAEL, SIRIELLE, GARDE, MARIN]
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
                     ('{BRUME}', 'La Brume', 'oneshot');
                 INSERT INTO entity (id, campaign_id, type, name) VALUES
                     ('{ELOWEN}', '{VALOMBRE}', 'pj', 'Elowen'),
                     ('{KAEL}', '{VALOMBRE}', 'pj', 'Kael'),
                     ('{SIRIELLE}', '{VALOMBRE}', 'pj', 'Sirielle'),
                     ('{GARDE}', '{VALOMBRE}', 'pnj', 'Le garde'),
                     ('{MARIN}', '{BRUME}', 'pj', 'Le marin');
                 {versions}
                 INSERT INTO character (entity_id, token_color) VALUES
                     ('{ELOWEN}', '#2a6'), ('{KAEL}', '#26a'), ('{SIRIELLE}', '#a26'),
                     ('{GARDE}', '#a62'), ('{MARIN}', '#6a2');
                 INSERT INTO session (id, campaign_id, position, title, status, microphone_on)
                 VALUES
                     ('{SEANCE}', '{VALOMBRE}', 1, 'La séance 1', 'prep', false),
                     ('{OUBLIEE}', '{VALOMBRE}', 2, 'L''oubliée', 'prep', false);
                 INSERT INTO session_present_character (session_id, character_id, added_at)
                 VALUES ('{SEANCE}', '{ELOWEN}', '2026-10-01T20:00:00Z');"
            ))
            .await
            .expect("Valombre, Brume, the sessions and their characters");
        Scratch { name, url, client }
    }

    /// The engine as the server plants this unit and its target aggregate
    /// over this database: the aggregate's projections in the apply
    /// transaction, the tables lent to the Prepare. The forgotten session's
    /// tombstone is the engine's.
    async fn serve(&self) -> Engine {
        let plugins = || -> Plugins {
            [aggregate_session::plugin(), add_present_character::plugin()]
                .into_iter()
                .collect()
        };
        let store = PgStore::open(connect(&self.url).await)
            .await
            .expect("the store")
            .with_projections(&plugins())
            .await
            .expect("warmed");
        self.tombstone(&format!("Session/{OUBLIEE}")).await;
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

    /// The engine's tombstone of `partition`, as a deletion leaves it.
    async fn tombstone(&self, partition: &str) {
        self.client
            .batch_execute(&format!(
                "INSERT INTO dataguard_rows (partition, tombstoned) VALUES ('{partition}', true)"
            ))
            .await
            .expect("a tombstone");
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

    /// The characters `session_roster_view` offers at every session, and
    /// how.
    async fn roster(&self) -> Vec<Value> {
        self.rows(
            "SELECT session_id::text AS session, character_id::text AS character, via
             FROM session_roster_view ORDER BY session_id, character_id",
        )
        .await
    }

    /// Every PC present, and since when.
    async fn present(&self) -> Vec<Value> {
        self.rows(
            "SELECT session_id::text AS session, character_id::text AS character,
                extract(epoch FROM added_at) AS added_at
             FROM session_present_character ORDER BY session_id, character_id",
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

/// A submission of session.addPresentCharacter, as SceneBoard sends it —
/// the session as `target.id` — under `key` when one is given.
async fn tick(
    engine: &Engine,
    session: &str,
    character: &str,
    key: Option<&str>,
) -> harness_dataguard::Result<Submitted> {
    submit(
        engine,
        json!({ "character": character }),
        Some(session),
        key,
    )
    .await
}

async fn submit(
    engine: &Engine,
    payload: Value,
    target: Option<&str>,
    key: Option<&str>,
) -> harness_dataguard::Result<Submitted> {
    let mut submission = json!({
        "dataCapability": "session.addPresentCharacter@1",
        "payload": payload,
        "idempotencyKey": key,
        // The critical fields' review is not what these tests judge.
        "caller": { "by": "dm", "from": "SceneBoard", "roles": ["role:approver"] },
    });
    if let Some(id) = target {
        submission["target"] = json!({ "id": id });
    }
    let submission: Submission = serde_json::from_value(submission).expect("a submission");
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
    assert_eq!(manifest["dataCapability"], "session.addPresentCharacter");
    assert_eq!(manifest["effect"], "insert");
    assert_eq!(manifest["target"]["aggregate"], "Session");
    DataCapability::parse(include_str!("../data-capability.json")).expect("contract F");
}

#[tokio::test]
async fn a_pc_of_the_campaign_ticked_late_is_listed_present_by_the_roster() {
    let scratch = Scratch::new("late").await;
    let engine = scratch.serve().await;
    // As SceneBoard sends it, and with the session in the payload.
    let sirielle = tick(&engine, SEANCE, SIRIELLE, None).await.expect("queued");
    submit(
        &engine,
        json!({ "session": SEANCE, "character": KAEL }),
        None,
        None,
    )
    .await
    .expect("queued");
    // Each presence on a partition of its own, never the session's row.
    assert_eq!(sirielle.partition, format!("Session/{SEANCE}.{SIRIELLE}"));
    for result in engine.run_once().await.expect("the applier ran") {
        assert_eq!(result.status, State::Applied, "{result:?}");
    }
    assert_eq!(
        scratch.roster().await,
        [
            json!({ "session": SEANCE, "character": ELOWEN, "via": "present" }),
            json!({ "session": SEANCE, "character": KAEL, "via": "present" }),
            json!({ "session": SEANCE, "character": SIRIELLE, "via": "present" }),
        ]
    );
    // Ticked now, not when the session began.
    let recent = scratch
        .rows(&format!(
            "SELECT added_at > now() - interval '1 minute' AS recent
             FROM session_present_character WHERE character_id = '{SIRIELLE}'"
        ))
        .await;
    assert_eq!(recent, [json!({ "recent": true })]);
    scratch.drop().await;
}

#[tokio::test]
async fn present_only_pj_refuses_an_npc_or_a_pc_of_another_campaign_and_ticks_nothing() {
    let scratch = Scratch::new("strangers").await;
    let engine = scratch.serve().await;
    let before = scratch.present().await;
    for stranger in [GARDE, MARIN, "e1000000-0000-4000-8000-0000000000ff"] {
        let (kind, message) = refused(tick(&engine, SEANCE, stranger, None).await);
        assert_eq!(kind, RefusalKind::Unprocessable, "{stranger}");
        assert!(
            message.starts_with("present-only-pj"),
            "{stranger}: {message}"
        );
    }
    // A PC whose tombstone the engine keeps is no PC of the campaign.
    scratch.tombstone(&format!("Entity/{KAEL}")).await;
    let (kind, message) = refused(tick(&engine, SEANCE, KAEL, None).await);
    assert_eq!(kind, RefusalKind::Unprocessable);
    assert!(message.starts_with("present-only-pj"), "{message}");
    assert!(engine.run_once().await.expect("the applier ran").is_empty());
    assert_eq!(scratch.present().await, before);
    scratch.drop().await;
}

#[tokio::test]
async fn a_session_there_is_not_is_not_found_and_a_malformed_request_unprocessable() {
    let scratch = Scratch::new("unknown").await;
    let engine = scratch.serve().await;
    let before = scratch.present().await;
    for session in [OUBLIEE, "51000000-0000-4000-8000-0000000000ff"] {
        let (kind, message) = refused(tick(&engine, session, SIRIELLE, None).await);
        assert_eq!(kind, RefusalKind::NotFound, "{session}: {message}");
    }
    for (payload, target) in [
        (json!({ "character": SIRIELLE }), None),
        (json!({ "character": "sirielle" }), Some(SEANCE)),
        (
            json!({ "session": OUBLIEE, "character": SIRIELLE }),
            Some(SEANCE),
        ),
    ] {
        let (kind, _) = refused(submit(&engine, payload.clone(), target, None).await);
        assert_eq!(kind, RefusalKind::Unprocessable, "{payload}");
    }
    assert!(engine.run_once().await.expect("the applier ran").is_empty());
    assert_eq!(scratch.present().await, before);
    scratch.drop().await;
}

/// Applies what is queued: `commands` commands, each rejected.
async fn rejected(engine: &Engine, commands: usize) {
    let results = engine.run_once().await.expect("the applier ran");
    assert_eq!(results.len(), commands, "{results:?}");
    for result in results {
        assert_eq!(result.status, State::Rejected, "{result:?}");
    }
}

#[tokio::test]
async fn a_pc_deleted_before_it_is_applied_is_rejected_and_ticks_nothing() {
    let scratch = Scratch::new("deleted").await;
    let engine = scratch.serve().await;
    let before = scratch.present().await;
    tick(&engine, SEANCE, SIRIELLE, None).await.expect("queued");
    scratch.tombstone(&format!("Entity/{SIRIELLE}")).await;
    rejected(&engine, 1).await;
    assert_eq!(scratch.present().await, before);
    scratch.drop().await;
}

#[tokio::test]
async fn a_pc_of_another_campaign_when_applied_is_rejected_and_ticks_nothing() {
    let scratch = Scratch::new("moved").await;
    let engine = scratch.serve().await;
    let before = scratch.present().await;
    tick(&engine, SEANCE, KAEL, None).await.expect("queued");
    // Since submitted, the session is Brume's, whose PC Kael is not.
    scratch
        .client
        .batch_execute(&format!(
            "UPDATE session SET campaign_id = '{BRUME}' WHERE id = '{SEANCE}'"
        ))
        .await
        .expect("the session moved to Brume");
    rejected(&engine, 1).await;
    assert_eq!(scratch.present().await, before);
    scratch.drop().await;
}

#[tokio::test]
async fn a_session_gone_when_applied_is_rejected_and_ticks_nothing() {
    let scratch = Scratch::new("gone").await;
    let engine = scratch.serve().await;
    let before = scratch.present().await;
    tick(&engine, SEANCE, SIRIELLE, None).await.expect("queued");
    scratch.tombstone(&format!("Session/{SEANCE}")).await;
    let results = engine.run_once().await.expect("the applier ran");
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].status, State::Rejected, "{results:?}");
    assert_eq!(scratch.present().await, before);
    scratch.drop().await;
}

#[tokio::test]
async fn a_pc_present_already_keeps_its_one_row() {
    let scratch = Scratch::new("once").await;
    let engine = scratch.serve().await;
    // The same idempotency key twice: one command.
    let first = tick(&engine, SEANCE, SIRIELLE, Some("k-1"))
        .await
        .expect("queued");
    let again = tick(&engine, SEANCE, SIRIELLE, Some("k-1"))
        .await
        .expect("replayed");
    assert!(again.replayed, "{again:?}");
    assert_eq!(again.partition, first.partition);
    // Elowen, present since the session's creation, ticked again.
    tick(&engine, SEANCE, ELOWEN, Some("k-2"))
        .await
        .expect("queued");
    let results = engine.run_once().await.expect("the applier ran");
    assert_eq!(results.len(), 2, "{results:?}");
    for result in results {
        assert_eq!(result.status, State::Applied, "{result:?}");
    }
    // Applied, a replay answers the first command and queues none; ticked
    // again under another key, the one row stays.
    let after = tick(&engine, SEANCE, SIRIELLE, Some("k-1"))
        .await
        .expect("replayed");
    assert_eq!(after.partition, first.partition);
    assert!(engine.run_once().await.expect("the applier ran").is_empty());
    let retick = tick(&engine, SEANCE, SIRIELLE, Some("k-3"))
        .await
        .expect("queued");
    assert!(!retick.replayed, "{retick:?}");
    assert_eq!(retick.partition, first.partition);
    let results = engine.run_once().await.expect("the applier ran");
    assert_eq!(results.len(), 1, "{results:?}");
    assert_eq!(results[0].status, State::Applied, "{results:?}");
    let present = scratch.present().await;
    let characters: Vec<&Value> = present.iter().map(|row| &row["character"]).collect();
    assert_eq!(characters, [ELOWEN, SIRIELLE]);
    let kept = scratch
        .rows(&format!(
            "SELECT added_at = '2026-10-01T20:00:00Z'::timestamptz AS kept
             FROM session_present_character WHERE character_id = '{ELOWEN}'"
        ))
        .await;
    assert_eq!(kept, [json!({ "kept": true })]);
    scratch.drop().await;
}
