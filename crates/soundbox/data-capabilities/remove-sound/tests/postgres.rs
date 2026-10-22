//! soundbox.removeSound over PostgreSQL (`DATABASE_URL`), as the server
//! plants it beside the soundbox's other units: a sound removed leaves the
//! `sound` table — what the library's readers read — in the transaction
//! that applies its tombstone, its playbacks with it, and the tombstone in
//! `dataguard_rows` keeps it as it was. A sound only the tables hold, as
//! the fixtures write it, is removed the same way. Each test works in a
//! scratch database of its own, migrated as the server migrates it, dropped
//! when it ends.

use std::path::Path;
use std::sync::Arc;

use harness_dataguard::Error;
use harness_dataguard::aggregate::Aggregate;
use harness_dataguard::api::{Submission, Submitted};
use harness_dataguard::command::{CommandResult, State};
use harness_dataguard::engine::Engine;
use harness_dataguard::manifest::DataCapability;
use harness_dataguard::memory::MemoryNotifier;
use harness_dataguard::plugins::{Plugins, RefusalKind};
use harness_dataguard::postgres::{PgStore, ReadOnly, SystemClock};
use harness_dataguard::store::RandomIds;
use harness_dataguard::tokio_postgres::{self, Client, NoTls};
use serde_json::{Value, json};

const VALOMBRE: &str = "c1000000-0000-4000-8000-000000000001";
const VALOMBRE_4: &str = "5e000000-0000-4000-8000-000000000004";
/// A sound the fixtures wrote, and the playback of it running.
const FOULE: &str = "50000000-0000-4000-8000-000000000001";
const FOULE_EN_COURS: &str = "5b000000-0000-4000-8000-000000000001";
/// A sound the fixtures wrote that no test removes.
const PLUIE: &str = "50000000-0000-4000-8000-000000000002";

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
/// the fixtures write them, by no command — Valombre, its live session, the
/// crowd playing in it and the rain.
struct Scratch {
    name: String,
    url: String,
    client: Client,
}

impl Scratch {
    async fn new(name: &str) -> Scratch {
        let mut scratch = Scratch::bare(name).await;
        let migrations = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../data/migrations");
        data::migrate(&mut scratch.client, &migrations)
            .await
            .expect("migrated");
        scratch
            .client
            .batch_execute(&format!(
                "INSERT INTO campaign (id, name, kind) VALUES
                     ('{VALOMBRE}', 'Les Cendres de Valombre', 'campagne');
                 INSERT INTO session (id, campaign_id, position, title, status, microphone_on)
                     VALUES ('{VALOMBRE_4}', '{VALOMBRE}', 4, 'L''embuscade du Gué', 'live', true);
                 INSERT INTO sound (id, campaign_id, name, file_path, tags, volume, created_at)
                     VALUES
                     ('{FOULE}', '{VALOMBRE}', 'Bruit de foule', 'sons/foule.ogg',
                         '{{taverne,foule}}', 0.4, '2026-10-17 20:00+00'),
                     ('{PLUIE}', '{VALOMBRE}', 'Pluie', 'sons/pluie.ogg', '{{}}', 1, now());
                 INSERT INTO sound_playback (id, session_id, sound_id, origin, started_at)
                     VALUES ('{FOULE_EN_COURS}', '{VALOMBRE_4}', '{FOULE}', 'soundbox',
                         '2026-10-17 20:05+00');"
            ))
            .await
            .expect("the fixtures");
        scratch
    }

    /// A database of its own, empty: the product's tables not migrated yet.
    async fn bare(name: &str) -> Scratch {
        let name = format!("remove_sound_test_{name}_{}", std::process::id());
        let admin = connect(&url()).await;
        for sql in [
            format!("DROP DATABASE IF EXISTS {name} WITH (FORCE)"),
            format!("CREATE DATABASE {name}"),
        ] {
            admin.batch_execute(&sql).await.expect("a scratch database");
        }
        let url = naming(&name);
        let client = connect(&url).await;
        Scratch { name, url, client }
    }

    /// The engine as the server plants the soundbox's sounds and playbacks
    /// over this database: their projections in the apply transaction, the
    /// tables lent to the Prepares.
    async fn serve(&self) -> Engine {
        self.serve_with(|| {
            [
                aggregate_sound::plugin(),
                aggregate_sound_playback::plugin(),
                add_sound::plugin(),
                play_sound::plugin(),
                remove_sound::plugin(),
            ]
            .into_iter()
            .collect()
        })
        .await
    }

    /// The engine over this database with the units `plugins` makes.
    async fn serve_with(&self, plugins: fn() -> Plugins) -> Engine {
        let store = PgStore::open(connect(&self.url).await)
            .await
            .expect("the store")
            .with_projections(&plugins())
            .await
            .expect("warmed");
        let units = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let read = |path: &str| {
            std::fs::read_to_string(units.join(path)).unwrap_or_else(|e| panic!("{path}: {e}"))
        };
        let mut engine = Engine::new(
            Arc::new(store),
            Arc::new(MemoryNotifier::default()),
            Arc::new(SystemClock),
        )
        .with_ids(Arc::new(RandomIds))
        .with_rows(Arc::new(ReadOnly::new(connect(&self.url).await)));
        for aggregate in [
            "aggregates/Sound/aggregate.json",
            "aggregates/SoundPlayback/aggregate.json",
        ] {
            engine = engine.with_aggregate(Aggregate::parse(&read(aggregate)).expect("contract I"));
        }
        for manifest in [
            "data-capabilities/add-sound/data-capability.json",
            "data-capabilities/play-sound/data-capability.json",
            "data-capabilities/remove-sound/data-capability.json",
        ] {
            engine = engine
                .with_data_capability(DataCapability::parse(&read(manifest)).expect("contract F"));
        }
        let engine = engine.with_plugins(plugins());
        engine.load().await.expect("the engine loads");
        engine
    }

    /// The rows `sql` selects, each as JSON.
    async fn rows(&self, sql: &str) -> Vec<Value> {
        self.client
            .query(&format!("SELECT to_jsonb(q)::text FROM ({sql}) q"), &[])
            .await
            .expect("the tables")
            .iter()
            .map(|row| serde_json::from_str(row.get::<_, &str>(0)).expect("JSON"))
            .collect()
    }

    /// The ids of the sounds `sound` holds, as the library reads them.
    async fn sounds(&self) -> Vec<Value> {
        self.rows("SELECT id::text AS id FROM sound ORDER BY id")
            .await
            .into_iter()
            .map(|row| row["id"].clone())
            .collect()
    }

    /// The sounds the playbacks of `sound_playback` play.
    async fn played(&self) -> Vec<Value> {
        self.rows("SELECT sound_id::text AS sound FROM sound_playback ORDER BY sound_id")
            .await
            .into_iter()
            .map(|row| row["sound"].clone())
            .collect()
    }

    /// The engine's row of `Sound/<sound>`: whether it is a tombstone, and
    /// its fields.
    async fn engine_row(&self, sound: &str) -> Value {
        self.engine_partition(&format!("Sound/{sound}")).await
    }

    /// The engine's row of `partition`: whether it is a tombstone, and its
    /// fields.
    async fn engine_partition(&self, partition: &str) -> Value {
        self.rows(&format!(
            "SELECT tombstoned, fields FROM dataguard_rows WHERE partition = '{partition}'"
        ))
        .await
        .pop()
        .unwrap_or(Value::Null)
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

/// A submission of `data_capability` by the DM, approver of their own
/// campaign's critical fields, on `target` when given.
async fn submit(
    engine: &Engine,
    data_capability: &str,
    target: Option<&str>,
    payload: Value,
) -> harness_dataguard::Result<Submitted> {
    let mut submission = json!({
        "dataCapability": data_capability,
        "payload": payload,
        "caller": { "by": "dm", "from": "soundbox.SoundLibrary", "roles": ["role:approver"] },
    });
    if let Some(id) = target {
        submission["targetId"] = json!(id);
    }
    engine
        .submit(serde_json::from_value::<Submission>(submission).expect("a submission"))
        .await
}

/// One pass of the single applier, which settles one command.
async fn settled(engine: &Engine) -> CommandResult {
    let mut settled = engine.run_once().await.expect("the applier ran");
    assert_eq!(settled.len(), 1, "{settled:?}");
    settled.remove(0)
}

/// `data_capability` submitted on `target` and applied: the partition it
/// wrote.
async fn applied(
    engine: &Engine,
    data_capability: &str,
    target: Option<&str>,
    payload: Value,
) -> String {
    let submitted = submit(engine, data_capability, target, payload)
        .await
        .expect("queued");
    let result = settled(engine).await;
    assert_eq!(result.status, State::Applied, "{result:?}");
    submitted.partition
}

/// The door added through soundbox.addSound: its id.
async fn add_door(engine: &Engine) -> String {
    let submitted = submit(
        engine,
        "soundbox.addSound@1",
        None,
        json!({
            "campaign": VALOMBRE, "name": "Porte qui grince", "file": "sons/porte.ogg",
            "tags": ["Porte", "grincement"], "volume": 80,
        }),
    )
    .await
    .expect("queued");
    assert_eq!(settled(engine).await.status, State::Applied);
    submitted
        .partition
        .strip_prefix("Sound/")
        .expect("a Sound partition")
        .to_owned()
}

fn not_found(submitted: harness_dataguard::Result<Submitted>) {
    match submitted {
        Err(Error::Refused(refusal)) => {
            assert_eq!(refusal.kind, RefusalKind::NotFound, "{refusal:?}");
            assert_eq!(refusal.message, "ce son n'existe pas");
        }
        other => panic!("expected a refusal, 404: {other:?}"),
    }
}

#[tokio::test]
async fn a_sound_removed_leaves_the_library_s_readers_and_its_tombstone_keeps_it_as_it_was() {
    let scratch = Scratch::new("removed").await;
    let engine = scratch.serve().await;
    let door = add_door(&engine).await;
    let playback = applied(&engine, "soundbox.playSound@1", Some(&door), json!({})).await;
    assert!(scratch.sounds().await.contains(&json!(door)));
    assert!(scratch.played().await.contains(&json!(door)));
    let added = scratch.engine_row(&door).await["fields"].clone();
    let started = scratch.engine_partition(&playback).await;
    assert_eq!(started["tombstoned"], json!(false));

    applied(&engine, "soundbox.removeSound@1", Some(&door), json!({})).await;

    // The readers no longer see it, nor its playback; the fixtures' sounds
    // and their playback stay.
    assert_eq!(scratch.sounds().await, [json!(FOULE), json!(PLUIE)]);
    assert_eq!(scratch.played().await, [json!(FOULE)]);
    // Its tombstone keeps it as the engine held it.
    let tombstone = scratch.engine_row(&door).await;
    assert_eq!(tombstone["tombstoned"], json!(true));
    assert_eq!(tombstone["fields"], added);
    assert_eq!(tombstone["fields"]["name"], "Porte qui grince");
    assert_eq!(tombstone["fields"]["tags"], json!(["porte", "grincement"]));
    // Its playback followed it in the engine too, a tombstone kept as it was.
    let stopped = scratch.engine_partition(&playback).await;
    assert_eq!(stopped["tombstoned"], json!(true));
    assert_eq!(stopped["fields"], started["fields"]);

    // And so after a start: nothing brings it back.
    drop(engine);
    let engine = scratch.serve().await;
    assert!(engine.run_once().await.expect("the applier ran").is_empty());
    assert!(!scratch.sounds().await.contains(&json!(door)));
    not_found(submit(&engine, "soundbox.removeSound@1", Some(&door), json!({})).await);
    scratch.drop().await;
}

#[tokio::test]
async fn a_sound_only_the_tables_hold_is_removed_and_its_tombstone_keeps_its_row() {
    let scratch = Scratch::new("fixture").await;
    let engine = scratch.serve().await;
    assert_eq!(scratch.engine_row(FOULE).await, Value::Null);

    applied(&engine, "soundbox.removeSound@1", Some(FOULE), json!({})).await;

    assert_eq!(scratch.sounds().await, [json!(PLUIE)]);
    assert!(scratch.played().await.is_empty());
    let tombstone = scratch.engine_row(FOULE).await;
    assert_eq!(tombstone["tombstoned"], json!(true));
    let fields = &tombstone["fields"];
    assert_eq!(fields["id"], FOULE);
    assert_eq!(fields["campaignId"], VALOMBRE);
    assert_eq!(fields["name"], "Bruit de foule");
    assert_eq!(fields["filePath"], "sons/foule.ogg");
    assert_eq!(fields["tags"], json!(["taverne", "foule"]));
    assert_eq!(fields["volume"], json!(0.4));
    assert!(
        fields["createdAt"]
            .as_str()
            .is_some_and(|at| at.starts_with("2026-10-17T20:00:00")),
        "{fields}"
    );
    scratch.drop().await;
}

#[tokio::test]
async fn a_sound_removed_is_none_to_remove_again_and_nothing_is_queued() {
    let scratch = Scratch::new("again").await;
    let engine = scratch.serve().await;
    applied(&engine, "soundbox.removeSound@1", Some(FOULE), json!({})).await;
    not_found(submit(&engine, "soundbox.removeSound@1", Some(FOULE), json!({})).await);
    assert!(engine.run_once().await.expect("the applier ran").is_empty());
    scratch.drop().await;
}

#[tokio::test]
async fn a_removal_naming_no_sound_the_product_holds_is_refused_404() {
    let scratch = Scratch::new("unknown").await;
    let engine = scratch.serve().await;
    for target in ["50000000-0000-4000-8000-0000000000ff", "pluie", "   "] {
        not_found(submit(&engine, "soundbox.removeSound@1", Some(target), json!({})).await);
    }
    assert!(engine.run_once().await.expect("the applier ran").is_empty());
    assert_eq!(scratch.sounds().await, [json!(FOULE), json!(PLUIE)]);
    scratch.drop().await;
}

#[tokio::test]
async fn without_the_sound_table_no_sound_is_held_to_remove() {
    let scratch = Scratch::bare("unmigrated").await;
    let engine = scratch.serve().await;
    not_found(submit(&engine, "soundbox.removeSound@1", Some(FOULE), json!({})).await);
    scratch.drop().await;
}

#[tokio::test]
async fn without_the_sound_table_a_sound_the_engine_holds_is_removed_all_the_same() {
    let scratch = Scratch::bare("engine_only").await;
    // This unit alone with its aggregate: the others' projections need the
    // tables.
    let engine = scratch
        .serve_with(|| {
            [aggregate_sound::plugin(), remove_sound::plugin()]
                .into_iter()
                .collect()
        })
        .await;
    scratch
        .client
        .batch_execute(&format!(
            "INSERT INTO dataguard_rows (partition, fields) VALUES
                 ('Sound/{FOULE}', '{{\"id\": \"{FOULE}\", \"name\": \"Bruit de foule\"}}')"
        ))
        .await
        .expect("the engine's sound");
    applied(&engine, "soundbox.removeSound@1", Some(FOULE), json!({})).await;
    let tombstone = scratch.engine_row(FOULE).await;
    assert_eq!(tombstone["tombstoned"], json!(true));
    assert_eq!(tombstone["fields"]["name"], "Bruit de foule");
    scratch.drop().await;
}
