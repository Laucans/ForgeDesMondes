//! soundbox.setSoundVolume over PostgreSQL (`DATABASE_URL`), as the server
//! plants it beside its aggregate: a move of the slider of a sound the
//! tables hold — one the fixtures wrote, which no command wrote before —
//! writes its `volume` column and changes no other column of any table; one
//! `sound-volume-range` rejects changes nothing; a sound the tables do not
//! hold, or one removed, is refused before anything is queued. A volume the
//! table lost is written again at the next start. Each test works in a
//! scratch database of its own, migrated as the server migrates it, dropped
//! when it ends.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use harness_dataguard::aggregate::Aggregate;
use harness_dataguard::api::{Submission, Submitted};
use harness_dataguard::command::{CommandResult, State};
use harness_dataguard::engine::Engine;
use harness_dataguard::manifest::DataCapability;
use harness_dataguard::memory::MemoryNotifier;
use harness_dataguard::plugins::{Plugins, RefusalKind};
use harness_dataguard::postgres::{PgStore, ReadOnly, SystemClock};
use harness_dataguard::tokio_postgres::{Client, NoTls, connect as connected};
use harness_dataguard::{Error, Result};
use serde_json::{Value, json};

const VALOMBRE: &str = "c1000000-0000-4000-8000-000000000001";
/// « Grincement » at 80 % and « Pluie » at 50 %, Valombre's sounds as the
/// fixtures wrote them.
const PORTE: &str = "50000000-0000-4000-8000-000000000002";
const PLUIE: &str = "50000000-0000-4000-8000-000000000003";
/// A sound the tables do not hold.
const NULLE_PART: &str = "50000000-0000-4000-8000-0000000000ff";

/// A sound removed, test-only: `soundbox.removeSound` is a unit of its own.
const REMOVE: &str = r#"{"dataCapability":"test.removeSound","owner":"dataguard","version":1,"effect":"delete","target":{"aggregate":"Sound","id":"$id"},"mode":"overwrite","callableBy":["*"]}"#;

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
/// fixtures write them, Valombre and two of its sounds.
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
                "INSERT INTO campaign (id, name, kind, pitch)
                     VALUES ('{VALOMBRE}', 'Les Cendres de Valombre', 'campagne', NULL);
                 INSERT INTO sound (id, campaign_id, name, file_path, tags, volume, created_at) VALUES
                     ('{PORTE}', '{VALOMBRE}', 'Grincement', 'sons/porte.ogg', '{{bois}}', 0.8,
                      '2026-10-17 20:00+00'),
                     ('{PLUIE}', '{VALOMBRE}', 'Pluie', 'sons/pluie.ogg', '{{}}', 0.5,
                      '2026-10-17 20:01+00');"
            ))
            .await
            .expect("the fixtures");
        scratch
    }

    /// A database of its own, not migrated: no `sound` table.
    async fn empty(name: &str) -> Self {
        let name = format!("set_sound_volume_test_{name}_{}", std::process::id());
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
        let plugins = || -> Plugins {
            [set_sound_volume::plugin(), aggregate_sound::plugin()]
                .into_iter()
                .collect()
        };
        let store = PgStore::open(connect(&self.url).await)
            .await
            .expect("the store")
            .with_projections(&plugins())
            .await
            .expect("the projections warmed");
        let path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../aggregates/Sound/aggregate.json");
        let aggregate = std::fs::read_to_string(path).expect("Sound's aggregate.json");
        let mut engine = Engine::new(
            Arc::new(store),
            Arc::new(MemoryNotifier::default()),
            Arc::new(SystemClock),
        )
        .with_aggregate(Aggregate::parse(&aggregate).expect("contract I"))
        .with_rows(Arc::new(ReadOnly::new(connect(&self.url).await)));
        for manifest in [include_str!("../data-capability.json"), REMOVE] {
            engine =
                engine.with_data_capability(DataCapability::parse(manifest).expect("contract F"));
        }
        let engine = engine.with_plugins(plugins());
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

    /// `sound`'s volume as its column holds it, as text.
    async fn volume(&self, sound: &str) -> String {
        self.client
            .query_one(
                "SELECT volume::text FROM sound WHERE id = ($1::text)::uuid",
                &[&sound],
            )
            .await
            .expect("the sound")
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

/// A submission of `data_capability` on `target` with `payload`, by the DM
/// from the LiveSoundbox.
async fn submit(
    engine: &Engine,
    data_capability: &str,
    target: &str,
    payload: Value,
) -> Result<Submitted> {
    engine
        .submit(
            serde_json::from_value::<Submission>(json!({
                "dataCapability": data_capability,
                "targetId": target,
                "payload": payload,
                "caller": { "by": "dm", "from": "LiveSoundbox" },
            }))
            .expect("a submission"),
        )
        .await
}

/// What the single applier made of `sound`'s slider moved to `volume`.
async fn moved(engine: &Engine, sound: &str, volume: Value) -> CommandResult {
    submit(
        engine,
        "soundbox.setSoundVolume@1",
        sound,
        json!({ "volume": volume }),
    )
    .await
    .expect("queued");
    let mut results = engine.run_once().await.expect("the applier ran");
    assert_eq!(results.len(), 1, "{results:?}");
    results.remove(0)
}

/// `tables` with the door's volume at `volume`.
fn with_door_at(
    mut tables: BTreeMap<String, Vec<Value>>,
    volume: f64,
) -> BTreeMap<String, Vec<Value>> {
    for sound in tables.get_mut("sound").expect("the sound table") {
        if sound["id"] == PORTE {
            sound["volume"] = json!(volume);
        }
    }
    tables
}

/// Whether `refused` is a refusal before anything queued, 404.
fn not_found(refused: &Result<Submitted>) -> bool {
    matches!(refused, Err(Error::Refused(r)) if r.kind == RefusalKind::NotFound)
}

#[tokio::test]
async fn a_fixture_s_sound_at_80_set_to_40_writes_0_40_and_nothing_else() {
    let scratch = Scratch::new("forty").await;
    let engine = scratch.plant().await;
    let before = scratch.tables().await;

    let result = moved(&engine, PORTE, json!(40)).await;

    assert_eq!(result.status, State::Applied, "{result:?}");
    assert_eq!(scratch.volume(PORTE).await, "0.40");
    assert_eq!(scratch.volume(PLUIE).await, "0.50");
    assert_eq!(scratch.tables().await, with_door_at(before, 0.4));
    scratch.drop().await;
}

#[tokio::test]
async fn sound_volume_range_rejects_101_and_the_tables_stay_as_they_were() {
    let scratch = Scratch::new("over").await;
    let engine = scratch.plant().await;
    let before = scratch.tables().await;

    let result = moved(&engine, PORTE, json!(101)).await;

    assert_eq!(result.status, State::Rejected, "{result:?}");
    assert_eq!(result.violations, ["sound-volume-range"], "{result:?}");
    assert_eq!(scratch.tables().await, before);
    scratch.drop().await;
}

#[tokio::test]
async fn a_sound_the_tables_do_not_hold_is_refused_before_anything_queues() {
    let scratch = Scratch::new("nowhere").await;
    let engine = scratch.plant().await;
    let before = scratch.tables().await;

    let refused = submit(
        &engine,
        "soundbox.setSoundVolume@1",
        NULLE_PART,
        json!({ "volume": 40 }),
    )
    .await;

    assert!(not_found(&refused), "{refused:?}");
    assert!(engine.run_once().await.expect("the applier ran").is_empty());
    assert_eq!(scratch.tables().await, before);
    scratch.drop().await;
}

#[tokio::test]
async fn a_sound_named_by_no_uuid_is_not_found_rather_than_an_error() {
    let scratch = Scratch::new("no_uuid").await;
    let engine = scratch.plant().await;

    for sound in [
        "porte",
        "cafe",
        "50000000-0000-4000-8000-00000000000g",
        "50000000-0000-4000-8000-0000000000020",
        "50000000-0000-4000-8000-00000000002",
        "500000000000-4000-8000-0000-00000002",
        "50000000x0000-4000-8000-000000000002",
    ] {
        let refused = submit(
            &engine,
            "soundbox.setSoundVolume@1",
            sound,
            json!({ "volume": 40 }),
        )
        .await;

        assert!(not_found(&refused), "{sound}: {refused:?}");
    }
    scratch.drop().await;
}

#[tokio::test]
async fn a_sound_named_in_capitals_is_the_table_s() {
    let scratch = Scratch::new("capitals").await;
    let engine = scratch.plant().await;

    let result = moved(&engine, &PORTE.to_uppercase(), json!(25)).await;

    assert_eq!(result.status, State::Applied, "{result:?}");
    assert_eq!(scratch.volume(PORTE).await, "0.25");
    scratch.drop().await;
}

#[tokio::test]
async fn a_write_of_a_sound_partition_the_table_could_never_hold_projects_nothing() {
    let scratch = Scratch::new("no_uuid_write").await;
    let engine = scratch.plant().await;
    let before = scratch.tables().await;
    submit(&engine, "test.removeSound@1", "cafe", json!({}))
        .await
        .expect("queued");

    let removed = engine.run_once().await.expect("the applier ran");

    assert_eq!(removed[0].status, State::Applied, "{removed:?}");
    assert_eq!(scratch.tables().await, before);
    scratch.drop().await;
}

#[tokio::test]
async fn a_sound_removed_is_refused_and_its_volume_never_written_back() {
    let scratch = Scratch::new("removed").await;
    let engine = scratch.plant().await;
    assert_eq!(
        moved(&engine, PORTE, json!(40)).await.status,
        State::Applied
    );
    submit(&engine, "test.removeSound@1", PORTE, json!({}))
        .await
        .expect("queued");
    let removed = engine.run_once().await.expect("the applier ran");
    assert_eq!(removed[0].status, State::Applied, "{removed:?}");
    scratch
        .client
        .batch_execute(&format!("UPDATE sound SET volume = 1 WHERE id = '{PORTE}'"))
        .await
        .expect("the row changed behind the engine");
    let before = scratch.tables().await;

    let refused = submit(
        &engine,
        "soundbox.setSoundVolume@1",
        PORTE,
        json!({ "volume": 30 }),
    )
    .await;

    assert!(not_found(&refused), "{refused:?}");
    drop(engine);
    let _restarted = scratch.plant().await;
    assert_eq!(scratch.tables().await, before);
    scratch.drop().await;
}

#[tokio::test]
async fn a_volume_the_table_lost_is_written_again_at_the_next_start() {
    let scratch = Scratch::new("lost").await;
    let engine = scratch.plant().await;
    assert_eq!(
        moved(&engine, PORTE, json!(20)).await.status,
        State::Applied
    );
    drop(engine);
    scratch
        .client
        .batch_execute(&format!(
            "UPDATE sound SET volume = 0.8, name = 'Porte' WHERE id = '{PORTE}'"
        ))
        .await
        .expect("the volume lost");
    let pluie = scratch.volume(PLUIE).await;

    let _restarted = scratch.plant().await;

    assert_eq!(scratch.volume(PORTE).await, "0.20");
    assert_eq!(scratch.volume(PLUIE).await, pluie);
    let name: String = scratch
        .client
        .query_one(&format!("SELECT name FROM sound WHERE id = '{PORTE}'"), &[])
        .await
        .expect("the door")
        .get(0);
    assert_eq!(name, "Porte", "the volume is the one column it writes");
    scratch.drop().await;
}

#[tokio::test]
async fn a_database_with_no_sound_table_starts_and_holds_no_sound() {
    let scratch = Scratch::empty("unmigrated").await;
    let engine = scratch.plant().await;

    let refused = submit(
        &engine,
        "soundbox.setSoundVolume@1",
        PORTE,
        json!({ "volume": 40 }),
    )
    .await;

    assert!(not_found(&refused), "{refused:?}");
    scratch.drop().await;
}

#[tokio::test]
async fn a_volume_the_table_refuses_is_rejected_and_the_tables_stay_as_they_were() {
    let scratch = Scratch::new("refused").await;
    scratch
        .client
        .batch_execute(
            "ALTER TABLE sound ADD CONSTRAINT never_silent CHECK (volume > 0);
             CREATE FUNCTION unreadable() RETURNS trigger LANGUAGE plpgsql AS $$
             BEGIN IF NEW.volume = 0.13 THEN
                 RAISE EXCEPTION 'unreadable' USING ERRCODE = '22023'; END IF;
             RETURN NEW; END $$;
             CREATE TRIGGER unreadable BEFORE UPDATE ON sound
                 FOR EACH ROW EXECUTE FUNCTION unreadable();",
        )
        .await
        .expect("a CHECK and a trigger only this test's table has");
    let engine = scratch.plant().await;
    let before = scratch.tables().await;

    for percent in [0, 13] {
        let result = moved(&engine, PORTE, json!(percent)).await;

        assert_eq!(result.status, State::Rejected, "{percent}: {result:?}");
        assert_eq!(scratch.tables().await, before, "{percent}");
    }
    scratch.drop().await;
}

#[tokio::test]
async fn a_volume_the_store_fails_to_project_is_not_rejected_and_writes_nothing() {
    let scratch = Scratch::new("failed").await;
    scratch
        .client
        .batch_execute(
            "CREATE FUNCTION out_of_order() RETURNS trigger LANGUAGE plpgsql AS $$
             BEGIN IF NEW.volume = 0.66 THEN RAISE EXCEPTION 'out of order'; END IF; RETURN NEW; END $$;
             CREATE TRIGGER out_of_order BEFORE UPDATE ON sound
                 FOR EACH ROW EXECUTE FUNCTION out_of_order();",
        )
        .await
        .expect("a trigger only this test's table has");
    let engine = scratch.plant().await;
    let before = scratch.tables().await;
    submit(
        &engine,
        "soundbox.setSoundVolume@1",
        PORTE,
        json!({ "volume": 66 }),
    )
    .await
    .expect("queued");

    let failed = engine.run_once().await;

    assert!(
        !matches!(&failed, Ok(results) if results.iter().any(|r| r.status == State::Rejected)),
        "{failed:?}"
    );
    assert!(failed.is_err(), "{failed:?}");
    assert_eq!(scratch.tables().await, before);
    scratch.drop().await;
}
