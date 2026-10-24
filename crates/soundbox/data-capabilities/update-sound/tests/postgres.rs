//! soundbox.updateSound over PostgreSQL (`DATABASE_URL`), as the server
//! plants it beside its aggregate: an edit of a sound the tables hold — one
//! the fixtures wrote, which no command wrote before — writes its name,
//! file, tags and volume on its `sound` row and changes no other column of
//! any table; one the invariants reject changes nothing; a sound the tables
//! do not hold, or one removed, is refused before anything is queued. An
//! edit the table lost is written again at the next start. Each test works
//! in a scratch database of its own, migrated as the server migrates it,
//! dropped when it ends.

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
/// « Grincement » and « Pluie », Valombre's sounds as the fixtures wrote
/// them.
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
                     ('{PORTE}', '{VALOMBRE}', 'Grincement', 'sons/porte.ogg', '{{bois}}', 1,
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
        let name = format!("update_sound_test_{name}_{}", std::process::id());
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
            [update_sound::plugin(), aggregate_sound::plugin()]
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

    /// `sound`'s name, file, tags and volume as its row holds them.
    async fn sound(&self, sound: &str) -> Value {
        self.client
            .query_one(
                "SELECT jsonb_build_object('name', name, 'file_path', file_path,
                        'tags', to_jsonb(tags), 'volume', volume::text)
                 FROM sound WHERE id = ($1::text)::uuid",
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
/// as an approver: the file is a critical field.
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
                "caller": { "by": "dm", "from": "SoundLibrary", "roles": ["role:approver"] },
            }))
            .expect("a submission"),
        )
        .await
}

/// What the single applier made of an edit of `sound` with `payload`.
async fn edited(engine: &Engine, sound: &str, payload: Value) -> CommandResult {
    submit(engine, "soundbox.updateSound@1", sound, payload)
        .await
        .expect("queued");
    let mut results = engine.run_once().await.expect("the applier ran");
    assert_eq!(results.len(), 1, "{results:?}");
    results.remove(0)
}

/// `tables` with the door's row changed by `changes`.
fn with_door(
    mut tables: BTreeMap<String, Vec<Value>>,
    changes: &Value,
) -> BTreeMap<String, Vec<Value>> {
    for sound in tables.get_mut("sound").expect("the sound table") {
        if sound["id"] == PORTE {
            for (column, value) in changes.as_object().expect("changes") {
                sound[column] = value.clone();
            }
        }
    }
    tables
}

#[tokio::test]
async fn an_edit_of_a_fixture_s_sound_writes_its_four_columns_and_nothing_else() {
    let scratch = Scratch::new("edit").await;
    let engine = scratch.plant().await;
    let before = scratch.tables().await;

    let result = edited(
        &engine,
        PORTE,
        json!({
            "name": "Porte qui grince",
            "file": "sons/porte-2.ogg",
            "tags": ["Porte", "porte"],
            "volume": 40,
        }),
    )
    .await;

    assert_eq!(result.status, State::Applied, "{result:?}");
    assert_eq!(
        scratch.sound(PORTE).await,
        json!({
            "name": "Porte qui grince",
            "file_path": "sons/porte-2.ogg",
            "tags": ["porte"],
            "volume": "0.40",
        })
    );
    assert_eq!(
        scratch.tables().await,
        with_door(
            before,
            &json!({
                "name": "Porte qui grince",
                "file_path": "sons/porte-2.ogg",
                "tags": ["porte"],
                "volume": 0.4,
            })
        )
    );
    scratch.drop().await;
}

#[tokio::test]
async fn a_field_the_edit_leaves_out_keeps_the_row_s_value() {
    let scratch = Scratch::new("partial").await;
    let engine = scratch.plant().await;

    let result = edited(&engine, PORTE, json!({ "name": "Porte" })).await;
    assert_eq!(result.status, State::Applied, "{result:?}");
    let result = edited(&engine, PORTE, json!({ "volume": 30 })).await;
    assert_eq!(result.status, State::Applied, "{result:?}");

    assert_eq!(
        scratch.sound(PORTE).await,
        json!({ "name": "Porte", "file_path": "sons/porte.ogg", "tags": ["bois"], "volume": "0.30" })
    );
    scratch.drop().await;
}

#[tokio::test]
async fn sound_volume_range_rejects_minus_5_and_the_tables_stay_as_they_were() {
    let scratch = Scratch::new("negative").await;
    let engine = scratch.plant().await;
    let before = scratch.tables().await;

    let result = edited(&engine, PORTE, json!({ "name": "Porte", "volume": -5 })).await;

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
        "soundbox.updateSound@1",
        NULLE_PART,
        json!({ "name": "Cloche" }),
    )
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
async fn a_sound_named_by_no_uuid_is_not_found_rather_than_an_error() {
    let scratch = Scratch::new("no_uuid").await;
    let engine = scratch.plant().await;

    for sound in [
        "porte",
        "cafe",
        "50000000-0000-4000-8000-00000000000g",
        "50000000-0000-4000-8000-0000000000020",
        "500000000000-4000-8000-0000-00000002",
    ] {
        let refused = submit(
            &engine,
            "soundbox.updateSound@1",
            sound,
            json!({ "name": "Porte" }),
        )
        .await;

        assert!(
            matches!(refused, Err(Error::Refused(ref r)) if r.kind == RefusalKind::NotFound),
            "{sound}: {refused:?}"
        );
    }
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
async fn a_sound_removed_is_never_edited_back() {
    let scratch = Scratch::new("removed").await;
    let engine = scratch.plant().await;
    submit(&engine, "test.removeSound@1", PORTE, json!({}))
        .await
        .expect("queued");
    let removed = engine.run_once().await.expect("the applier ran");
    assert_eq!(removed[0].status, State::Applied, "{removed:?}");
    let before = scratch.tables().await;

    let refused = submit(
        &engine,
        "soundbox.updateSound@1",
        PORTE,
        json!({ "name": "Porte" }),
    )
    .await;

    assert!(
        matches!(refused, Err(Error::Refused(ref r)) if r.kind == RefusalKind::NotFound),
        "{refused:?}"
    );
    drop(engine);
    let _restarted = scratch.plant().await;
    assert_eq!(scratch.tables().await, before);
    scratch.drop().await;
}

#[tokio::test]
async fn an_edit_the_table_lost_is_written_again_at_the_next_start() {
    let scratch = Scratch::new("lost").await;
    let engine = scratch.plant().await;
    let result = edited(&engine, PORTE, json!({ "tags": ["Pluie"], "volume": 20 })).await;
    assert_eq!(result.status, State::Applied, "{result:?}");
    drop(engine);
    scratch
        .client
        .batch_execute(&format!(
            "UPDATE sound SET tags = '{{bois}}', volume = 1 WHERE id = '{PORTE}'"
        ))
        .await
        .expect("the edit lost");
    let pluie = scratch.sound(PLUIE).await;

    let _restarted = scratch.plant().await;

    assert_eq!(
        scratch.sound(PORTE).await,
        json!({ "name": "Grincement", "file_path": "sons/porte.ogg", "tags": ["pluie"], "volume": "0.20" })
    );
    assert_eq!(scratch.sound(PLUIE).await, pluie);
    scratch.drop().await;
}

#[tokio::test]
async fn a_database_with_no_sound_table_starts_and_holds_no_sound() {
    let scratch = Scratch::empty("unmigrated").await;
    let engine = scratch.plant().await;

    let refused = submit(
        &engine,
        "soundbox.updateSound@1",
        PORTE,
        json!({ "name": "Porte" }),
    )
    .await;

    assert!(
        matches!(refused, Err(Error::Refused(ref r)) if r.kind == RefusalKind::NotFound),
        "{refused:?}"
    );
    scratch.drop().await;
}

#[tokio::test]
async fn an_edit_the_table_refuses_is_rejected_and_the_tables_stay_as_they_were() {
    let scratch = Scratch::new("refused").await;
    scratch
        .client
        .batch_execute(
            "ALTER TABLE sound ADD CONSTRAINT no_forbidden_name CHECK (name <> 'Interdit')",
        )
        .await
        .expect("a CHECK only this test's table has");
    let engine = scratch.plant().await;
    let before = scratch.tables().await;

    let result = edited(&engine, PORTE, json!({ "name": "Interdit" })).await;

    assert_eq!(result.status, State::Rejected, "{result:?}");
    assert_eq!(scratch.tables().await, before);
    scratch.drop().await;
}

#[tokio::test]
async fn an_edit_the_store_fails_to_project_is_not_rejected_and_writes_nothing() {
    let scratch = Scratch::new("failed").await;
    scratch
        .client
        .batch_execute(
            "CREATE FUNCTION out_of_order() RETURNS trigger LANGUAGE plpgsql AS $$
             BEGIN IF NEW.name = 'Panne' THEN RAISE EXCEPTION 'out of order'; END IF; RETURN NEW; END $$;
             CREATE TRIGGER out_of_order BEFORE UPDATE ON sound
                 FOR EACH ROW EXECUTE FUNCTION out_of_order();",
        )
        .await
        .expect("a trigger only this test's table has");
    let engine = scratch.plant().await;
    let before = scratch.tables().await;
    submit(
        &engine,
        "soundbox.updateSound@1",
        PORTE,
        json!({ "name": "Panne" }),
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
