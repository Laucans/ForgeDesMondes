//! The Session aggregate's PCs unticked, out of `session_present_character`,
//! over PostgreSQL (`DATABASE_URL`): `Absences` takes the one link row of a
//! session and a PC out when their presence's partition is tombstoned, so
//! `session_roster_view` offers only the PCs that remain (`present-only-pj`)
//! — the character, its sheet and the other sessions' PCs untouched. A
//! presence ticked, any other row, or the tombstone of a row that is no
//! presence writes nothing. Each test works in a scratch database of its
//! own, migrated as the server migrates it, dropped when it ends.

use std::collections::BTreeMap;
use std::path::Path;

use aggregate_session::{Absences, order_id};
use harness_dataguard::plugins::ProjectPg;
use harness_dataguard::store::Write;
use harness_dataguard::tokio_postgres::{self, Client, NoTls};

const VALOMBRE: &str = "c1000000-0000-4000-8000-000000000001";
const ELOWEN: &str = "e1000000-0000-4000-8000-000000000001";
const KAEL: &str = "e1000000-0000-4000-8000-000000000002";
/// Session 4, Elowen and Kael present; session 5, Elowen present.
const SEANCE: &str = "51000000-0000-4000-8000-000000000004";
const AUTRE: &str = "51000000-0000-4000-8000-000000000005";

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

/// A database of its own, migrated, holding Valombre's two PCs and its
/// sessions 4 and 5 with their PCs present.
struct Scratch {
    name: String,
    client: Client,
}

impl Scratch {
    async fn new(name: &str) -> Scratch {
        let name = format!("absences_test_{name}_{}", std::process::id());
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
                 INSERT INTO entity (id, campaign_id, type, name) VALUES
                     ('{ELOWEN}', '{VALOMBRE}', 'pj', 'Elowen'),
                     ('{KAEL}', '{VALOMBRE}', 'pj', 'Kael');
                 INSERT INTO entity_version (entity_id, version_number, source, snapshot, diff)
                     VALUES ('{ELOWEN}', 1, 'dm', '{{}}', '[]'), ('{KAEL}', 1, 'dm', '{{}}', '[]');
                 INSERT INTO character (entity_id, token_color) VALUES
                     ('{ELOWEN}', '#2a6'), ('{KAEL}', '#26a');
                 INSERT INTO session (id, campaign_id, position, title, status, microphone_on) VALUES
                     ('{SEANCE}', '{VALOMBRE}', 4, 'La séance 4', 'prep', false),
                     ('{AUTRE}', '{VALOMBRE}', 5, 'La séance 5', 'prep', false);
                 INSERT INTO session_present_character (session_id, character_id) VALUES
                     ('{SEANCE}', '{ELOWEN}'), ('{SEANCE}', '{KAEL}'), ('{AUTRE}', '{ELOWEN}');"
            ))
            .await
            .expect("Valombre's PCs, its sessions 4 and 5 and their PCs");
        Scratch { name, client }
    }

    /// What `Absences` makes of `write`, committed.
    async fn project(&mut self, write: &Write) {
        let tx = self.client.transaction().await.expect("a transaction");
        Absences
            .project(write, &tx)
            .await
            .expect("the absence projected");
        tx.commit().await.expect("committed");
    }

    /// What `session_roster_view` offers, every session's: its session and
    /// its character, each as the session's position and the PC's name.
    async fn roster(&self) -> Vec<(i32, String, String)> {
        self.client
            .query(
                "SELECT s.position, e.name, r.via FROM session_roster_view r
                 JOIN session s ON s.id = r.session_id JOIN entity e ON e.id = r.character_id
                 ORDER BY 1, 2, 3",
                &[],
            )
            .await
            .expect("the roster")
            .iter()
            .map(|row| (row.get(0), row.get(1), row.get(2)))
            .collect()
    }

    /// How many characters, elements and sessions the tables hold.
    async fn held(&self) -> i64 {
        self.client
            .query_one(
                "SELECT (SELECT count(*) FROM character) + (SELECT count(*) FROM entity)
                    + (SELECT count(*) FROM session)",
                &[],
            )
            .await
            .expect("the count")
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

/// The write of nothing on `partition`: a tombstone, or a row ticked.
fn on(partition: &str, tombstone: bool) -> Write {
    Write {
        partition: partition.to_owned(),
        values: BTreeMap::new(),
        tombstone,
    }
}

fn offered(rows: &[(i32, &str)]) -> Vec<(i32, String, String)> {
    rows.iter()
        .map(|(session, name)| (*session, (*name).to_owned(), "present".to_owned()))
        .collect()
}

#[tokio::test]
async fn present_only_pj_a_presence_s_tombstone_takes_its_one_link_row_out() {
    let mut scratch = Scratch::new("tombstone").await;
    let held = scratch.held().await;
    // In capitals, as a screen may send them: the tables' ids are lowercase.
    let presence = format!("Session/{}.{}", SEANCE.to_uppercase(), KAEL.to_uppercase());
    scratch.project(&on(&presence, true)).await;
    assert_eq!(
        scratch.roster().await,
        offered(&[(4, "Elowen"), (5, "Elowen")])
    );
    // Unticked again: nothing left to take, nothing else taken.
    scratch.project(&on(&presence, true)).await;
    assert_eq!(
        scratch.roster().await,
        offered(&[(4, "Elowen"), (5, "Elowen")])
    );
    assert_eq!(scratch.held().await, held);
    scratch.drop().await;
}

#[tokio::test]
async fn a_presence_ticked_or_any_other_tombstone_unticks_no_one() {
    let mut scratch = Scratch::new("others").await;
    let roster = scratch.roster().await;
    let order = order_id(SEANCE).expect("a uuid");
    for write in [
        // The presence ticked, not unticked.
        on(&format!("Session/{SEANCE}.{KAEL}"), false),
        // The session's own row, its order's, a scene's, or another
        // aggregate's partition named like a presence.
        on(&format!("Session/{SEANCE}"), true),
        on(&format!("Session/{order}"), true),
        on(&format!("Session/{SEANCE}.kael"), true),
        on(&format!("Entity/{SEANCE}.{KAEL}"), true),
        // A presence of a PC at another session than hers.
        on(&format!("Session/{AUTRE}.{KAEL}"), true),
    ] {
        scratch.project(&write).await;
        assert_eq!(scratch.roster().await, roster, "{write:?}");
    }
    scratch.drop().await;
}
