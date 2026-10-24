//! soundbox.setMasterVolume over PostgreSQL (`DATABASE_URL`), as the server
//! plants it beside its aggregate: the DM's master slider, in percent, on a
//! campaign the tables hold — one the fixtures wrote, which no command wrote
//! before — writes `campaign.sound_master_volume` as its fraction and changes
//! no other column of any table; a volume out of range is rejected by the
//! campaign's own invariant (`master-volume-range`) and changes nothing, nor
//! does one naming a campaign the tables do not hold, which is never queued.
//! A volume the table lost is written again at the next start. Each test
//! works in a scratch database of its own, migrated as the server migrates
//! it, dropped when it ends.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use harness_dataguard::Error;
use harness_dataguard::aggregate::Aggregate;
use harness_dataguard::api::Submission;
use harness_dataguard::command::{CommandResult, State};
use harness_dataguard::engine::Engine;
use harness_dataguard::guard::Caller;
use harness_dataguard::manifest::DataCapability;
use harness_dataguard::memory::MemoryNotifier;
use harness_dataguard::plugins::{Plugins, RefusalKind};
use harness_dataguard::postgres::{PgStore, ReadOnly, SystemClock};
use harness_dataguard::tokio_postgres::{Client, NoTls, connect as connected};
use serde_json::{Value, json};
use set_master_volume::DATA_CAPABILITY;

const VALOMBRE: &str = "c1000000-0000-4000-8000-000000000001";
const BRUME: &str = "c1000000-0000-4000-8000-000000000002";
const NOWHERE: &str = "c1000000-0000-4000-8000-0000000000ff";

fn url() -> String {
    std::env::var("DATABASE_URL").expect("DATABASE_URL names the harness's PostgreSQL")
}

async fn connect(url: &str) -> Client {
    let (client, connection) = connected(url, NoTls).await.expect("PostgreSQL");
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

/// A database of its own — migrated unless said otherwise — holding, as the
/// fixtures write them, Valombre, its master volume at 80 %, and Brume,
/// another campaign at full volume.
struct Scratch {
    name: String,
    url: String,
    client: Client,
}

impl Scratch {
    async fn new(name: &str) -> Self {
        let scratch = Self::empty(name).await;
        let mut client = connect(&scratch.url).await;
        let migrations = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../data/migrations");
        data::migrate(&mut client, &migrations)
            .await
            .expect("migrated");
        client
            .batch_execute(&format!(
                "INSERT INTO campaign (id, name, kind, pitch, sound_master_volume)
                     VALUES ('{VALOMBRE}', 'Les Cendres de Valombre', 'campagne', 'Une cité sous la cendre.', 0.8),
                            ('{BRUME}', 'La Brume', 'oneshot', NULL, 1);"
            ))
            .await
            .expect("the fixtures");
        scratch
    }

    /// A database of its own, not migrated: no `campaign` table.
    async fn empty(name: &str) -> Self {
        let name = format!("set_master_volume_test_{name}_{}", std::process::id());
        let admin = connect(&url()).await;
        for sql in [
            format!("DROP DATABASE IF EXISTS {name} WITH (FORCE)"),
            format!("CREATE DATABASE {name}"),
        ] {
            admin.batch_execute(&sql).await.expect("a scratch database");
        }
        let url = naming(&name);
        let client = connect(&url).await;
        Self { name, url, client }
    }

    /// The engine as the server plants the DataCapability and its aggregate
    /// over this database, reading its tables.
    async fn plant(&self) -> Engine {
        let plugins: Plugins = [set_master_volume::plugin(), aggregate_campaign::plugin()]
            .into_iter()
            .collect();
        let store = PgStore::open(connect(&self.url).await)
            .await
            .expect("the store")
            .with_projections(&plugins)
            .await
            .expect("the projections warmed");
        let engine = Engine::new(
            Arc::new(store),
            Arc::new(MemoryNotifier::default()),
            Arc::new(SystemClock),
        )
        .with_aggregate(Aggregate::parse(aggregate_campaign::AGGREGATE_JSON).expect("contract I"))
        .with_data_capability(
            DataCapability::parse(include_str!("../data-capability.json")).expect("contract F"),
        )
        .with_plugins(plugins)
        .with_rows(Arc::new(ReadOnly::new(connect(&self.url).await)));
        engine.load().await.expect("the engine loaded");
        engine
    }

    /// Every row of every table of the product, as JSON, by table: the
    /// DataGuard's own (`dataguard_*`) aside.
    async fn tables(&self) -> BTreeMap<String, Vec<Value>> {
        let names = self
            .client
            .query(
                "SELECT tablename::text FROM pg_tables
                 WHERE schemaname = 'public' AND tablename NOT LIKE 'dataguard%'
                 ORDER BY tablename",
                &[],
            )
            .await
            .expect("the tables");
        let mut tables = BTreeMap::new();
        for name in names {
            let name: String = name.get(0);
            let rows = self
                .client
                .query(
                    &format!("SELECT to_jsonb(t) FROM {name} t ORDER BY to_jsonb(t)::text"),
                    &[],
                )
                .await
                .expect("a table's rows")
                .iter()
                .map(|row| row.get::<_, Value>(0))
                .collect();
            tables.insert(name, rows);
        }
        tables
    }

    /// `campaign`'s master volume, as its column holds it, as text.
    async fn volume(&self, campaign: &str) -> String {
        self.client
            .query_one(
                "SELECT sound_master_volume::text FROM campaign WHERE id = ($1::text)::uuid",
                &[&campaign],
            )
            .await
            .expect("the campaign")
            .get(0)
    }

    /// The database dropped, again while an autovacuum worker the test's
    /// role cannot end still holds it.
    async fn drop(self) {
        drop(self.client);
        let admin = connect(&url()).await;
        let sql = format!("DROP DATABASE {} WITH (FORCE)", self.name);
        for _ in 0..20 {
            if admin.batch_execute(&sql).await.is_ok() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
        admin
            .batch_execute(&sql)
            .await
            .expect("the scratch database dropped");
    }
}

fn submission(payload: Value, target: Option<&str>) -> Submission {
    let Value::Object(payload) = payload else {
        panic!("a payload is an object");
    };
    Submission {
        data_capability: DATA_CAPABILITY.to_owned(),
        payload: payload.into_iter().collect(),
        target_id: target.map(str::to_owned),
        based_on: None,
        idempotency_key: None,
        caller: Caller {
            by: "dm".to_owned(),
            from: "LiveSoundbox".to_owned(),
            permissions: Vec::new(),
            roles: Vec::new(),
        },
    }
}

/// What the single applier made of the DM's slider moved to `volume` on
/// `campaign`.
async fn moved(engine: &Engine, campaign: &str, volume: Value) -> CommandResult {
    engine
        .submit(submission(
            json!({ "campaign": campaign, "volume": volume }),
            None,
        ))
        .await
        .expect("queued");
    let mut results = engine.run_once().await.expect("the applier ran");
    assert_eq!(results.len(), 1, "{results:?}");
    results.remove(0)
}

/// `tables` with Valombre's master volume at `volume`.
fn with_valombre_at(
    mut tables: BTreeMap<String, Vec<Value>>,
    volume: f64,
) -> BTreeMap<String, Vec<Value>> {
    for campaign in tables.get_mut("campaign").expect("the campaign table") {
        if campaign["id"] == VALOMBRE {
            campaign["sound_master_volume"] = json!(volume);
        }
    }
    tables
}

#[tokio::test]
async fn a_master_volume_of_40_percent_writes_0_4_on_the_campaign_and_nothing_else() {
    let scratch = Scratch::new("forty").await;
    let engine = scratch.plant().await;
    let before = scratch.tables().await;

    let result = moved(&engine, VALOMBRE, json!(40)).await;

    assert_eq!(result.status, State::Applied, "{result:?}");
    assert_eq!(scratch.volume(VALOMBRE).await, "0.40");
    assert_eq!(scratch.volume(BRUME).await, "1.00");
    assert_eq!(scratch.tables().await, with_valombre_at(before, 0.4));
    scratch.drop().await;
}

#[tokio::test]
async fn master_volume_range_rejects_150_percent_and_the_tables_stay_as_they_were() {
    let scratch = Scratch::new("too_loud").await;
    let engine = scratch.plant().await;
    let before = scratch.tables().await;

    let result = moved(&engine, VALOMBRE, json!(150)).await;

    assert_eq!(result.status, State::Rejected, "{result:?}");
    assert_eq!(result.violations, ["master-volume-range"], "{result:?}");
    assert_eq!(scratch.tables().await, before);
    scratch.drop().await;
}

#[tokio::test]
async fn a_campaign_the_tables_do_not_hold_is_refused_before_anything_queues() {
    let scratch = Scratch::new("nowhere").await;
    let engine = scratch.plant().await;
    let before = scratch.tables().await;

    let refused = engine
        .submit(submission(
            json!({ "campaign": NOWHERE, "volume": 40 }),
            None,
        ))
        .await;

    let Err(Error::Refused(refusal)) = refused else {
        panic!("refused before it queues: {refused:?}");
    };
    assert_eq!(refusal.kind, RefusalKind::NotFound, "{refusal:?}");
    assert!(engine.run_once().await.expect("the applier ran").is_empty());
    assert_eq!(scratch.tables().await, before);
    scratch.drop().await;
}

#[tokio::test]
async fn a_master_volume_the_table_lost_is_written_again_at_the_next_start() {
    let scratch = Scratch::new("lost").await;
    let engine = scratch.plant().await;
    let result = moved(&engine, VALOMBRE, json!(40)).await;
    assert_eq!(result.status, State::Applied, "{result:?}");
    drop(engine);
    scratch
        .client
        .batch_execute(&format!(
            "UPDATE campaign SET sound_master_volume = 0.8 WHERE id = '{VALOMBRE}'"
        ))
        .await
        .expect("the volume lost");

    let _restarted = scratch.plant().await;

    assert_eq!(scratch.volume(VALOMBRE).await, "0.40");
    assert_eq!(scratch.volume(BRUME).await, "1.00");
    scratch.drop().await;
}

#[tokio::test]
async fn a_database_with_no_campaign_table_starts_and_holds_no_campaign() {
    let scratch = Scratch::empty("unmigrated").await;
    let engine = scratch.plant().await;

    let refused = engine
        .submit(submission(
            json!({ "campaign": VALOMBRE, "volume": 40 }),
            None,
        ))
        .await;

    assert!(
        matches!(refused, Err(Error::Refused(ref r)) if r.kind == RefusalKind::NotFound),
        "{refused:?}"
    );
    scratch.drop().await;
}
