//! session.reorderScenes over PostgreSQL (`DATABASE_URL`), as the server
//! plants it, on a session the tables hold — the fixtures wrote it and its
//! scenes A, B, C, which no command wrote before: the order [C, A, B] ranks
//! them C = 1, A = 2, B = 3 in a single write; an order that omits a scene,
//! repeats one or names a scene of another session is refused naming
//! `scene-order-contiguous`, and no rank changes; two reorders submitted at
//! once queue on the session's order, and neither is rejected. Each test
//! works in a scratch database of its own, migrated as the server migrates
//! it, dropped when it ends.

use std::path::Path;
use std::sync::Arc;

use aggregate_session::order_id;
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
use serde_json::json;

const VALOMBRE: &str = "c1000000-0000-4000-8000-000000000001";
/// Valombre's session 4, and its scenes A, B and C, ranked 1, 2 and 3.
const SEANCE: &str = "51000000-0000-4000-8000-000000000004";
const A: &str = "5c000000-0000-4000-8000-00000000000a";
const B: &str = "5c000000-0000-4000-8000-00000000000b";
const C: &str = "5c000000-0000-4000-8000-00000000000c";
/// Valombre's session 5, and its one scene.
const AUTRE: &str = "51000000-0000-4000-8000-000000000005";
const X: &str = "5c000000-0000-4000-8000-0000000000ff";

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
/// the fixtures write them — Valombre's session 4 with its scenes A, B, C
/// and its session 5 with its scene X.
struct Scratch {
    name: String,
    url: String,
    client: Client,
}

impl Scratch {
    async fn new(name: &str) -> Scratch {
        let name = format!("reorder_scenes_test_{name}_{}", std::process::id());
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
                 INSERT INTO session (id, campaign_id, position, title, status, microphone_on) VALUES
                     ('{SEANCE}', '{VALOMBRE}', 4, 'La séance 4', 'prep', false),
                     ('{AUTRE}', '{VALOMBRE}', 5, 'La séance 5', 'prep', false);
                 INSERT INTO scene (id, session_id, position, title) VALUES
                     ('{A}', '{SEANCE}', 1, 'A'), ('{B}', '{SEANCE}', 2, 'B'),
                     ('{C}', '{SEANCE}', 3, 'C'), ('{X}', '{AUTRE}', 1, 'X');"
            ))
            .await
            .expect("Valombre's sessions 4 and 5 and their scenes");
        Scratch { name, url, client }
    }

    /// The engine as the server plants this unit and its target aggregate
    /// over this database: the aggregate's projection in the apply
    /// transaction, the tables lent to the Prepare.
    async fn serve(&self) -> Engine {
        let plugins = || -> Plugins {
            [aggregate_session::plugin(), reorder_scenes::plugin()]
                .into_iter()
                .collect()
        };
        let store = PgStore::open(connect(&self.url).await)
            .await
            .expect("the store")
            .with_projections(&plugins())
            .await
            .expect("warmed");
        let engine = Engine::new(
            Arc::new(store),
            Arc::new(MemoryNotifier::default()),
            Arc::new(SystemClock),
        )
        .with_rows(Arc::new(ReadOnly::new(connect(&self.url).await)))
        .with_ids(Arc::new(RandomIds))
        .with_aggregate(Aggregate::parse(aggregate_session::DOCUMENT).expect("contract I"))
        .with_plugins(plugins())
        .with_data_capability(
            DataCapability::parse(include_str!("../data-capability.json")).expect("contract F"),
        );
        engine.load().await.expect("the engine loads");
        engine
    }

    /// A session's scenes, by rank: each one's title and rank.
    async fn ranks(&self, session: &str) -> Vec<(String, i32)> {
        self.client
            .query(
                "SELECT title, position FROM scene WHERE session_id = $1::text::uuid
                 ORDER BY position, title",
                &[&session],
            )
            .await
            .expect("the scenes")
            .iter()
            .map(|row| (row.get(0), row.get(1)))
            .collect()
    }

    /// The data version the engine has reached: one more per write.
    async fn version(&self) -> i64 {
        self.client
            .query_one("SELECT version FROM dataguard_version", &[])
            .await
            .expect("the data version")
            .get(0)
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

/// A reorder, as the session's preparation sends it: by the DM, who holds
/// no approver's role.
async fn reorder(engine: &Engine, order: &[&str]) -> harness_dataguard::Result<Submitted> {
    let submission: Submission = serde_json::from_value(json!({
        "dataCapability": "session.reorderScenes@1",
        "payload": { "session": SEANCE, "order": order },
        "caller": { "by": "dm", "from": "session.SceneBoard", "roles": [] },
    }))
    .expect("a submission");
    engine.submit(submission).await
}

/// Every command the applier applies until none is left.
async fn applied(engine: &Engine) -> Vec<harness_dataguard::command::CommandResult> {
    let mut applied = Vec::new();
    for _ in 0..5 {
        applied.extend(engine.run_once().await.expect("the applier ran"));
    }
    applied
}

fn ranked(ranks: &[(&str, i32)]) -> Vec<(String, i32)> {
    ranks
        .iter()
        .map(|(title, rank)| ((*title).to_owned(), *rank))
        .collect()
}

#[tokio::test]
async fn the_order_c_a_b_ranks_c_first_a_second_and_b_third_in_a_single_write() {
    let scratch = Scratch::new("order").await;
    let engine = scratch.serve().await;
    let before = scratch.version().await;
    let submitted = reorder(&engine, &[C, A, B]).await.expect("queued");
    assert_eq!(
        submitted.partition,
        format!("Session/{}", order_id(SEANCE).expect("a uuid"))
    );
    let applied = applied(&engine).await;
    assert_eq!(applied.len(), 1, "{applied:?}");
    assert_eq!(applied[0].status, State::Applied, "{applied:?}");
    assert_eq!(scratch.version().await, before + 1, "one write");
    assert_eq!(
        scratch.ranks(SEANCE).await,
        ranked(&[("C", 1), ("A", 2), ("B", 3)])
    );
    assert_eq!(scratch.ranks(AUTRE).await, ranked(&[("X", 1)]));
    scratch.drop().await;
}

#[tokio::test]
async fn an_order_omitting_repeating_or_borrowing_a_scene_is_refused_and_no_rank_changes() {
    let scratch = Scratch::new("refused").await;
    let engine = scratch.serve().await;
    for order in [
        &[C, A][..],
        &[C, A, B, A][..],
        &[C, A, B, X][..],
        &[C, A, X][..],
    ] {
        let refusal = reorder(&engine, order).await;
        let Err(Error::Refused(refusal)) = refusal else {
            panic!("{order:?} refused: {refusal:?}");
        };
        assert_eq!(refusal.kind, RefusalKind::Unprocessable, "{order:?}");
        assert!(
            refusal.message.starts_with("scene-order-contiguous : "),
            "{order:?}: {}",
            refusal.message
        );
    }
    assert!(applied(&engine).await.is_empty(), "nothing was queued");
    assert_eq!(
        scratch.ranks(SEANCE).await,
        ranked(&[("A", 1), ("B", 2), ("C", 3)])
    );
    assert_eq!(scratch.ranks(AUTRE).await, ranked(&[("X", 1)]));
    scratch.drop().await;
}

#[tokio::test]
async fn a_session_the_tables_do_not_hold_live_is_not_found() {
    let scratch = Scratch::new("missing").await;
    let engine = scratch.serve().await;
    // Session 4 removed: its tombstone stays in the engine's rows.
    scratch
        .client
        .batch_execute(&format!(
            "INSERT INTO dataguard_rows (partition, tombstoned) VALUES ('Session/{SEANCE}', true)"
        ))
        .await
        .expect("the session's tombstone");
    let refusal = reorder(&engine, &[C, A, B]).await;
    let Err(Error::Refused(refusal)) = refusal else {
        panic!("refused: {refusal:?}");
    };
    assert_eq!(refusal.kind, RefusalKind::NotFound, "{refusal:?}");
    scratch.drop().await;
}

#[tokio::test]
async fn a_scene_removed_by_its_tombstone_is_no_longer_one_the_order_must_name() {
    let scratch = Scratch::new("tombstone").await;
    let engine = scratch.serve().await;
    scratch
        .client
        .batch_execute(&format!(
            "INSERT INTO dataguard_rows (partition, tombstoned) VALUES ('Session/{A}', true)"
        ))
        .await
        .expect("A's tombstone");
    reorder(&engine, &[C, B]).await.expect("queued");
    let refusal = reorder(&engine, &[C, A, B]).await;
    assert!(
        matches!(&refusal, Err(Error::Refused(r)) if r.message.contains(A)),
        "{refusal:?}"
    );
    scratch.drop().await;
}

#[tokio::test]
async fn two_reorders_at_once_queue_on_the_session_s_order_and_neither_is_rejected() {
    let scratch = Scratch::new("twice").await;
    let engine = scratch.serve().await;
    let first = reorder(&engine, &[C, A, B]).await.expect("queued");
    let second = reorder(&engine, &[B, C, A])
        .await
        .expect("queued behind the first");
    assert_eq!(second.partition, first.partition);
    assert_eq!(first.result.status, State::Queued);
    assert_eq!(second.result.status, State::AwaitingConfirmation);
    engine
        .confirm(&second.command_id, "dm")
        .await
        .expect("its author confirms the overwrite");
    let applied = applied(&engine).await;
    let statuses: Vec<(&str, State)> = applied
        .iter()
        .map(|result| (result.command_id.as_str(), result.status))
        .collect();
    assert_eq!(
        statuses,
        [
            (first.command_id.as_str(), State::Applied),
            (second.command_id.as_str(), State::Applied)
        ]
    );
    assert_eq!(
        scratch.ranks(SEANCE).await,
        ranked(&[("B", 1), ("C", 2), ("A", 3)])
    );
    scratch.drop().await;
}

#[tokio::test]
async fn a_scene_added_while_the_order_is_queued_goes_after_it_and_ranks_stay_one_to_n() {
    let scratch = Scratch::new("added").await;
    let engine = scratch.serve().await;
    reorder(&engine, &[C, A, B]).await.expect("queued");
    // D is applied before the order, after the session's last.
    scratch
        .client
        .batch_execute(&format!(
            "INSERT INTO scene (session_id, position, title) VALUES ('{SEANCE}', 4, 'D')"
        ))
        .await
        .expect("D added");
    let applied = applied(&engine).await;
    assert_eq!(applied.len(), 1);
    assert_eq!(applied[0].status, State::Applied, "{applied:?}");
    assert_eq!(
        scratch.ranks(SEANCE).await,
        ranked(&[("C", 1), ("A", 2), ("B", 3), ("D", 4)])
    );
    scratch.drop().await;
}
