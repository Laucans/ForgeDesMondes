//! table.renameToken over PostgreSQL (`DATABASE_URL`), as the server plants
//! it — the tables lent to its Prepare: a token the engine holds is renamed
//! on its own partition, and one the engine removed — whose tombstone keeps
//! its fields — is none to rename. Each test works in a scratch database of
//! its own, migrated as the server migrates it, dropped when it ends.

use std::path::Path;
use std::sync::Arc;

use harness_dataguard::aggregate::Aggregate;
use harness_dataguard::api::{Submission, Submitted};
use harness_dataguard::command::State;
use harness_dataguard::engine::Engine;
use harness_dataguard::manifest::DataCapability;
use harness_dataguard::memory::MemoryNotifier;
use harness_dataguard::plugins::{Plugins, RefusalKind};
use harness_dataguard::postgres::{PgStore, ReadOnly, SystemClock};
use harness_dataguard::tokio_postgres::{self, Client, NoTls};
use harness_dataguard::{Error, Result};
use rename_token::partition;
use serde_json::{Value, json};

const INSTANCE: &str = "b1000000-0000-4000-8000-000000000001";
const VALOMBRE: &str = "c1000000-0000-4000-8000-000000000001";
const BANDIT: &str = "e1000000-0000-4000-8000-0000000000b1";
/// Bandit 1, on the instance; Bandit 2, removed from it.
const BANDIT_1: &str = "70000000-0000-4000-8000-000000000001";
const BANDIT_2: &str = "70000000-0000-4000-8000-000000000002";

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

/// Bandit `number`'s row, as table.placeTokens left it on the instance.
fn bandit(id: &str, number: u8) -> Value {
    json!({
        "tokenId": id,
        "tokenInstanceId": INSTANCE,
        "characterId": BANDIT,
        "number": number,
        "name": format!("Bandit {number}"),
        "q": 5,
        "r": 5,
        "visible": true,
        "hpCurrent": 11,
        "hpMax": 11,
        "conditions": [],
    })
}

fn plugins() -> Plugins {
    [
        aggregate_battlemap_instance::plugin(),
        rename_token::plugin(),
    ]
    .into_iter()
    .collect()
}

/// A database of its own, migrated as the server migrates it, with the
/// engine's tables, holding Valombre and the Bandit — whom the tokens stand
/// for, as the tables' foreign keys ask — and Bandit 1 and the tombstone of
/// Bandit 2.
struct Scratch {
    name: String,
    url: String,
    client: Client,
}

impl Scratch {
    async fn new(name: &str) -> Scratch {
        let name = format!("rename_token_test_{name}_{}", std::process::id());
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
                 INSERT INTO entity (id, campaign_id, type, name) VALUES
                     ('{BANDIT}', '{VALOMBRE}', 'pnj', 'Bandit');
                 INSERT INTO character (entity_id, token_color) VALUES ('{BANDIT}', '#888');
                 INSERT INTO entity_version (entity_id, version_number, source, snapshot, diff)
                     SELECT id, 1, 'dm', '{{}}', '[]' FROM entity;"
            ))
            .await
            .expect("Valombre");
        // The engine's own tables, which its store makes when it opens.
        PgStore::open(connect(&url).await)
            .await
            .expect("the engine's tables");
        client
            .execute(
                "INSERT INTO dataguard_rows (partition, fields, tombstoned) VALUES
                     ($1, $2::text::jsonb, false), ($3, $4::text::jsonb, true)",
                &[
                    &partition(BANDIT_1),
                    &bandit(BANDIT_1, 1).to_string(),
                    &partition(BANDIT_2),
                    &bandit(BANDIT_2, 2).to_string(),
                ],
            )
            .await
            .expect("Bandit 1 and the tombstone of Bandit 2");
        Scratch { name, url, client }
    }

    /// The engine as the server plants this unit and its target aggregate
    /// over this database, the tables lent to the Prepare.
    async fn serve(&self) -> Engine {
        let store = PgStore::open(connect(&self.url).await)
            .await
            .expect("the store")
            .with_projections(&plugins())
            .await
            .expect("warmed");
        let aggregate = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../aggregates/BattlemapInstance/aggregate.json");
        let engine = Engine::new(
            Arc::new(store),
            Arc::new(MemoryNotifier::default()),
            Arc::new(SystemClock),
        )
        .with_rows(Arc::new(ReadOnly::new(connect(&self.url).await)))
        .with_aggregate(
            Aggregate::parse(&std::fs::read_to_string(aggregate).expect("aggregate.json"))
                .expect("contract I"),
        )
        .with_data_capability(
            DataCapability::parse(include_str!("../data-capability.json")).expect("contract F"),
        )
        .with_plugins(plugins());
        engine.load().await.expect("the engine loads");
        engine
    }

    /// The fields the engine holds on the partition of `token`.
    async fn token(&self, token: &str) -> Value {
        let row = self
            .client
            .query_one(
                "SELECT fields::text FROM dataguard_rows WHERE partition = $1",
                &[&partition(token)],
            )
            .await
            .expect("the token's row");
        serde_json::from_str(row.get::<_, &str>(0)).expect("JSON")
    }

    /// Drops the database — again, a moment later, while a backend a role
    /// that is no superuser cannot end (an autovacuum worker) is still in it.
    async fn drop(self) {
        drop(self.client);
        let admin = connect(&url()).await;
        let sql = format!("DROP DATABASE {} WITH (FORCE)", self.name);
        for _ in 0..20 {
            if admin.batch_execute(&sql).await.is_ok() {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(250));
        }
        admin
            .batch_execute(&sql)
            .await
            .expect("the scratch database dropped");
    }
}

/// A rename of `token` on the instance, as the Battlemap sends it.
async fn rename(engine: &Engine, token: &str, name: &str) -> Result<Submitted> {
    let body = json!({
        "dataCapability": "table.renameToken@1",
        "target": { "aggregate": "BattlemapInstance", "id": INSTANCE },
        "payload": { "token": token, "name": name },
        "caller": { "by": "dm", "from": "Battlemap" },
    });
    engine
        .submit(serde_json::from_value::<Submission>(body).expect("a submission"))
        .await
}

#[tokio::test]
async fn a_token_the_engine_holds_is_renamed_and_one_it_removed_is_not_found() {
    let scratch = Scratch::new("renamed").await;
    let engine = scratch.serve().await;

    rename(&engine, BANDIT_1, "Bandit blessé")
        .await
        .expect("queued");
    for result in engine.run_once().await.expect("the applier ran") {
        assert_eq!(result.status, State::Applied, "{result:?}");
    }
    let mut renamed = bandit(BANDIT_1, 1);
    renamed["name"] = json!("Bandit blessé");
    assert_eq!(scratch.token(BANDIT_1).await, renamed);

    match rename(&engine, BANDIT_2, "Bandit revenu").await {
        Err(Error::Refused(refusal)) => assert_eq!(refusal.kind, RefusalKind::NotFound),
        other => panic!("a removed token is not found: {other:?}"),
    }
    assert!(engine.run_once().await.expect("ran").is_empty());
    assert_eq!(scratch.token(BANDIT_2).await, bandit(BANDIT_2, 2));
    scratch.drop().await;
}
