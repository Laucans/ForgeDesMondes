//! session.removePresentCharacter over PostgreSQL (`DATABASE_URL`), as the
//! server plants it: a PC present at a session — ticked by its creation, as
//! the tables hold it — unticked, leaves `session_roster_view` (« present »)
//! while her character, her sheet and the other sessions' PCs stay as they
//! were; a PC who is not present changes nothing, at any session; a session
//! the tables do not hold, or whose tombstone the engine keeps, is not
//! found, and a malformed request refused — none queued. Each test works in
//! a scratch database of its own, migrated as the server migrates it,
//! dropped when it ends.

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

const VALOMBRE: &str = "c1000000-0000-4000-8000-000000000001";
/// Valombre's two PCs and its NPC.
const ELOWEN: &str = "e1000000-0000-4000-8000-000000000001";
const KAEL: &str = "e1000000-0000-4000-8000-000000000002";
const GARDE: &str = "e1000000-0000-4000-8000-000000000003";
/// Session 4, Elowen and Kael present, the guard in its scene; session 5,
/// Elowen present; session 3, whose tombstone the engine keeps, Kael
/// present; a session the tables do not hold.
const SEANCE: &str = "51000000-0000-4000-8000-000000000004";
const AUTRE: &str = "51000000-0000-4000-8000-000000000005";
const OUBLIEE: &str = "51000000-0000-4000-8000-000000000003";
const NULLE: &str = "51000000-0000-4000-8000-0000000000ff";
const SCENE: &str = "5c000000-0000-4000-8000-00000000000a";

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
/// the fixtures and the sessions' creation write them — Valombre, its two
/// PCs and its NPC, and its sessions 3, 4 and 5 with their PCs present.
struct Scratch {
    name: String,
    url: String,
    client: Client,
}

impl Scratch {
    async fn new(name: &str) -> Scratch {
        let name = format!("remove_present_test_{name}_{}", std::process::id());
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
        let versions: String = [ELOWEN, KAEL, GARDE]
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
                     ('{VALOMBRE}', 'Les Cendres de Valombre', 'campagne');
                 INSERT INTO entity (id, campaign_id, type, name) VALUES
                     ('{ELOWEN}', '{VALOMBRE}', 'pj', 'Elowen'),
                     ('{KAEL}', '{VALOMBRE}', 'pj', 'Kael'),
                     ('{GARDE}', '{VALOMBRE}', 'pnj', 'Le garde');
                 {versions}
                 INSERT INTO character (entity_id, token_color, hp_max) VALUES
                     ('{ELOWEN}', '#2a6', 24), ('{KAEL}', '#26a', 31), ('{GARDE}', '#a62', 11);
                 INSERT INTO session (id, campaign_id, position, title, status, microphone_on) VALUES
                     ('{OUBLIEE}', '{VALOMBRE}', 3, 'La séance 3', 'closed', false),
                     ('{SEANCE}', '{VALOMBRE}', 4, 'La séance 4', 'prep', false),
                     ('{AUTRE}', '{VALOMBRE}', 5, 'La séance 5', 'prep', false);
                 INSERT INTO session_present_character (session_id, character_id) VALUES
                     ('{OUBLIEE}', '{KAEL}'),
                     ('{SEANCE}', '{ELOWEN}'), ('{SEANCE}', '{KAEL}'),
                     ('{AUTRE}', '{ELOWEN}');
                 INSERT INTO scene (id, session_id, position, title) VALUES
                     ('{SCENE}', '{SEANCE}', 1, 'Le gué');
                 INSERT INTO scene_entity (scene_id, entity_id, position) VALUES
                     ('{SCENE}', '{GARDE}', 1);"
            ))
            .await
            .expect("Valombre, its characters, its sessions and their PCs");
        Scratch { name, url, client }
    }

    /// The engine as the server plants this unit and its target aggregate
    /// over this database: the aggregate's projections in the apply
    /// transaction, the tables lent to the Prepare. Session 3's tombstone is
    /// the engine's.
    async fn serve(&self) -> Engine {
        let plugins = || -> Plugins {
            [
                aggregate_session::plugin(),
                remove_present_character::plugin(),
            ]
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
                "INSERT INTO dataguard_rows (partition, tombstoned) VALUES ('Session/{OUBLIEE}', true)"
            ))
            .await
            .expect("session 3's tombstone");
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

    /// What `session_roster_view` offers, every session's.
    async fn roster(&self) -> Vec<Value> {
        self.rows(
            "SELECT session_id::text AS session, character_id::text AS character, via
             FROM session_roster_view ORDER BY 1, 2, 3",
        )
        .await
    }

    /// Every character, its element and its versions, as the tables hold
    /// them; and every session.
    async fn characters_and_sessions(&self) -> Vec<Value> {
        self.rows(
            "SELECT to_jsonb(e) AS entity, to_jsonb(c) AS character,
                (SELECT count(*) FROM entity_version v WHERE v.entity_id = e.id) AS versions
             FROM entity e JOIN character c ON c.entity_id = e.id
             UNION ALL SELECT to_jsonb(s), NULL, NULL FROM session s
             ORDER BY 1",
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

/// `character` at `session` in the roster, offered as `via`.
fn offered(session: &str, character: &str, via: &str) -> Value {
    json!({ "session": session, "character": character, "via": via })
}

/// A submission of session.removePresentCharacter, as SceneBoard sends it:
/// `payload`, the session as its target when one is given.
async fn untick(
    engine: &Engine,
    target: Option<&str>,
    payload: Value,
) -> harness_dataguard::Result<Submitted> {
    let submission: Submission = serde_json::from_value(json!({
        "dataCapability": "session.removePresentCharacter@1",
        "targetId": target,
        "payload": payload,
        // The critical fields' review is not what these tests judge.
        "caller": { "by": "dm", "from": "SceneBoard", "roles": ["role:approver"] },
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

/// Every command the applier runs now, each applied.
async fn applied(engine: &Engine, count: usize) {
    let results = engine.run_once().await.expect("the applier ran");
    assert_eq!(results.len(), count, "{results:?}");
    for result in results {
        assert_eq!(result.status, State::Applied, "{result:?}");
    }
}

#[tokio::test]
async fn present_only_pj_a_pc_unticked_is_no_longer_offered_and_her_character_stays() {
    let scratch = Scratch::new("unticked").await;
    let engine = scratch.serve().await;
    let before = scratch.characters_and_sessions().await;
    let submitted = untick(
        &engine,
        Some(SEANCE),
        json!({ "character": ELOWEN.to_uppercase() }),
    )
    .await
    .expect("queued");
    // The presence's own partition, never the session's.
    assert_eq!(submitted.partition, format!("Session/{SEANCE}.{ELOWEN}"));
    applied(&engine, 1).await;
    assert_eq!(
        scratch.roster().await,
        [
            offered(OUBLIEE, KAEL, "present"),
            offered(SEANCE, KAEL, "present"),
            offered(SEANCE, GARDE, "scene"),
            offered(AUTRE, ELOWEN, "present"),
        ]
    );
    // Her character, her sheet, her versions and every session stay.
    assert_eq!(scratch.characters_and_sessions().await, before);
    assert_eq!(
        scratch
            .rows(&format!(
                "SELECT partition, tombstoned FROM dataguard_rows
                 WHERE partition LIKE 'Session/{SEANCE}%' ORDER BY 1"
            ))
            .await,
        [json!({ "partition": format!("Session/{SEANCE}.{ELOWEN}"), "tombstoned": true })]
    );
    scratch.drop().await;
}

#[tokio::test]
async fn a_pc_who_is_not_present_changes_nothing_at_any_session() {
    let scratch = Scratch::new("absent").await;
    let engine = scratch.serve().await;
    let roster = scratch.roster().await;
    let before = scratch.characters_and_sessions().await;
    // Kael is not present at session 5; the guard is in session 4's scene,
    // never ticked present; a character the tables do not hold.
    untick(
        &engine,
        None,
        json!({ "session": AUTRE, "character": KAEL }),
    )
    .await
    .expect("queued");
    untick(&engine, Some(SEANCE), json!({ "character": GARDE }))
        .await
        .expect("queued");
    untick(&engine, Some(SEANCE), json!({ "character": NULLE }))
        .await
        .expect("queued");
    applied(&engine, 3).await;
    assert_eq!(scratch.roster().await, roster);
    assert_eq!(scratch.characters_and_sessions().await, before);
    scratch.drop().await;
}

#[tokio::test]
async fn a_pc_unticked_twice_is_unticked_once_and_the_second_changes_nothing() {
    let scratch = Scratch::new("twice").await;
    let engine = scratch.serve().await;
    for _ in 0..2 {
        untick(&engine, Some(SEANCE), json!({ "character": KAEL }))
            .await
            .expect("queued");
    }
    applied(&engine, 2).await;
    untick(&engine, Some(SEANCE), json!({ "character": KAEL }))
        .await
        .expect("queued again");
    applied(&engine, 1).await;
    assert_eq!(
        scratch.roster().await,
        [
            offered(OUBLIEE, KAEL, "present"),
            offered(SEANCE, ELOWEN, "present"),
            offered(SEANCE, GARDE, "scene"),
            offered(AUTRE, ELOWEN, "present"),
        ]
    );
    scratch.drop().await;
}

#[tokio::test]
async fn a_session_there_is_not_is_not_found_a_malformed_untick_refused_and_none_queued() {
    let scratch = Scratch::new("refused").await;
    let engine = scratch.serve().await;
    let roster = scratch.roster().await;
    // A session the tables do not hold, and one whose tombstone the engine
    // keeps.
    for session in [NULLE, OUBLIEE] {
        let (kind, message) =
            refused(untick(&engine, Some(session), json!({ "character": KAEL })).await);
        assert_eq!(kind, RefusalKind::NotFound, "{session}");
        assert_eq!(message, "séance introuvable");
    }
    for (target, payload) in [
        (Some(SEANCE), json!({ "character": "kael" })),
        (Some("s4"), json!({ "character": KAEL })),
        (None, json!({ "character": KAEL })),
        (Some(SEANCE), json!({ "session": AUTRE, "character": KAEL })),
        (Some(SEANCE), json!({ "character": KAEL, "present": [] })),
    ] {
        let (kind, _) = refused(untick(&engine, target, payload.clone()).await);
        assert_eq!(kind, RefusalKind::Unprocessable, "{payload} on {target:?}");
    }
    assert!(engine.run_once().await.expect("the applier ran").is_empty());
    assert_eq!(scratch.roster().await, roster);
    scratch.drop().await;
}
