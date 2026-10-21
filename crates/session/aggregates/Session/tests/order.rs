//! The Session aggregate's scene orders in their table, `scene.position`,
//! over PostgreSQL (`DATABASE_URL`): `SceneOrder` rewrites every rank of a
//! session in the transaction that applies its order — those the order
//! lists in its order, then those added since in their ranks, none of a
//! scene removed since — so ranks go from 1 to n (`scene-order-contiguous`).
//! A row that holds no order, or a tombstone, writes nothing; another
//! session's scenes are never ranked. Each test works in a scratch database
//! of its own, migrated as the server migrates it, dropped when it ends.

use std::collections::BTreeMap;
use std::path::Path;

use aggregate_session::{SCENE_POSITION, SceneOrder, order_id};
use harness_dataguard::plugins::ProjectPg;
use harness_dataguard::store::Write;
use harness_dataguard::tokio_postgres::{self, Client, NoTls};
use serde_json::{Value, json};

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

/// A database of its own, migrated, holding Valombre's session 4 with its
/// scenes A, B, C and its session 5 with its scene X.
struct Scratch {
    name: String,
    client: Client,
}

impl Scratch {
    async fn new(name: &str) -> Scratch {
        let name = format!("scene_order_test_{name}_{}", std::process::id());
        let admin = connect(&url()).await;
        for sql in [
            format!("DROP DATABASE IF EXISTS {name} WITH (FORCE)"),
            format!("CREATE DATABASE {name}"),
        ] {
            admin.batch_execute(&sql).await.expect("a scratch database");
        }
        let mut client = connect(&naming(&name)).await;
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
        Scratch { name, client }
    }

    /// What `SceneOrder` makes of `write`, committed.
    async fn project(&mut self, write: &Write) {
        let tx = self.client.transaction().await.expect("a transaction");
        SceneOrder
            .project(write, &tx)
            .await
            .expect("the order projected");
        tx.commit().await.expect("committed");
    }

    /// The session's scenes, by rank: each one's title and rank.
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

/// The write of `row` on `partition`.
fn write(partition: &str, row: Value, tombstone: bool) -> Write {
    let Value::Object(row) = row else {
        panic!("a row is an object");
    };
    Write {
        partition: partition.to_owned(),
        values: row.into_iter().collect::<BTreeMap<_, _>>(),
        tombstone,
    }
}

/// Session 4's order `scenes` written on its row.
fn order(scenes: &[&str]) -> Write {
    let partition = format!("Session/{}", order_id(SEANCE).expect("a uuid"));
    write(&partition, json!({ SCENE_POSITION: scenes }), false)
}

fn ranked(ranks: &[(&str, i32)]) -> Vec<(String, i32)> {
    ranks
        .iter()
        .map(|(title, rank)| ((*title).to_owned(), *rank))
        .collect()
}

#[tokio::test]
async fn an_order_ranks_every_scene_of_its_session_in_its_order_from_one() {
    let mut scratch = Scratch::new("order").await;
    // In capitals, as a screen may send them: the tables' ids are lowercase.
    scratch.project(&order(&[&C.to_uppercase(), A, B])).await;
    assert_eq!(
        scratch.ranks(SEANCE).await,
        ranked(&[("C", 1), ("A", 2), ("B", 3)])
    );
    // Session 5's scene keeps its rank.
    assert_eq!(scratch.ranks(AUTRE).await, ranked(&[("X", 1)]));
    scratch.drop().await;
}

#[tokio::test]
async fn a_scene_added_or_removed_since_the_order_was_asked_leaves_ranks_one_to_n() {
    let mut scratch = Scratch::new("since").await;
    // D added at rank 2 and A removed since the order was asked, which
    // names A and session 5's X besides: D goes after the scenes listed, X
    // is never ranked here.
    scratch
        .client
        .batch_execute(&format!(
            "DELETE FROM scene WHERE id = '{A}';
             UPDATE scene SET position = position + 1 WHERE session_id = '{SEANCE}';
             INSERT INTO scene (session_id, position, title) VALUES ('{SEANCE}', 2, 'D'),
                 ('{SEANCE}', 5, 'E');"
        ))
        .await
        .expect("D and E added, A removed");
    scratch.project(&order(&[C, X, A, B])).await;
    assert_eq!(
        scratch.ranks(SEANCE).await,
        ranked(&[("C", 1), ("B", 2), ("D", 3), ("E", 4)])
    );
    assert_eq!(scratch.ranks(AUTRE).await, ranked(&[("X", 1)]));
    scratch.drop().await;
}

#[tokio::test]
async fn a_row_without_an_order_or_a_tombstone_ranks_nothing() {
    let mut scratch = Scratch::new("none").await;
    let partition = format!("Session/{}", order_id(SEANCE).expect("a uuid"));
    for write in [
        write(&partition, json!({ SCENE_POSITION: [C, A, B] }), true),
        write(
            &format!("Session/{SEANCE}"),
            json!({ "sessionStatus": "live" }),
            false,
        ),
        write(&format!("Session/{C}"), json!({ SCENE_POSITION: 1 }), false),
        write(
            "Session/seance-4",
            json!({ SCENE_POSITION: [C, A, B] }),
            false,
        ),
    ] {
        scratch.project(&write).await;
        assert_eq!(
            scratch.ranks(SEANCE).await,
            ranked(&[("A", 1), ("B", 2), ("C", 3)]),
            "{}",
            write.partition
        );
    }
    scratch.drop().await;
}
