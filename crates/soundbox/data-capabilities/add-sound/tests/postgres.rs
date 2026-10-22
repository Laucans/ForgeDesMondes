//! soundbox.addSound over PostgreSQL (`DATABASE_URL`), as the server plants
//! it: what the engine applies on `Sound/<id>` reaches the `sound` table —
//! its tags normalised, its volume a fraction of 1 — in the transaction that
//! applies it; what the DataGuard or the table refuses writes nothing. The
//! campaign is read from the tables, the fixtures' included. Each test works
//! in a scratch database of its own, migrated as the server migrates it,
//! dropped when it ends.

use std::path::Path;
use std::sync::Arc;

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
use harness_dataguard::{Error, Result};
use serde_json::{Value, json};

const VALOMBRE: &str = "c1000000-0000-4000-8000-000000000001";
const NOWHERE: &str = "c1000000-0000-4000-8000-0000000000ff";

/// A write of a sound's name alone, by no unit of the product: what an edit
/// of an added sound does to its partition.
const RENAME: &str = r#"{
    "dataCapability": "test.renameSound", "owner": "dataguard", "version": 1,
    "effect": "update", "target": {"aggregate": "Sound"}, "touches": ["Sound.name"],
    "payload": {"name": "string"}, "mode": "overwrite", "invariants": [],
    "permissions": [], "callableBy": ["*"], "idempotencyKey": "optional"
}"#;

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

/// A database of its own, migrated as the server migrates it, holding —
/// as the fixtures do, written by no command — Valombre.
struct Scratch {
    name: String,
    url: String,
    client: Client,
}

impl Scratch {
    async fn new(name: &str) -> Scratch {
        let name = format!("add_sound_test_{name}_{}", std::process::id());
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
                     ('{VALOMBRE}', 'Les Cendres de Valombre', 'campagne');"
            ))
            .await
            .expect("Valombre");
        Scratch { name, url, client }
    }

    /// The engine as the server plants this unit and its target aggregate
    /// over this database: the sound's projection in the apply transaction,
    /// the tables lent to the Prepare.
    async fn serve(&self) -> Engine {
        let plugins = || -> Plugins {
            [aggregate_sound::plugin(), add_sound::plugin()]
                .into_iter()
                .collect()
        };
        let store = PgStore::open(connect(&self.url).await)
            .await
            .expect("the store")
            .with_projections(&plugins())
            .await
            .expect("warmed");
        let path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../aggregates/Sound/aggregate.json");
        let aggregate = std::fs::read_to_string(path).expect("Sound's aggregate.json");
        let engine = Engine::new(
            Arc::new(store),
            Arc::new(MemoryNotifier::default()),
            Arc::new(SystemClock),
        )
        .with_ids(Arc::new(RandomIds))
        .with_rows(Arc::new(ReadOnly::new(connect(&self.url).await)))
        .with_aggregate(Aggregate::parse(&aggregate).expect("contract I"))
        .with_data_capability(
            DataCapability::parse(include_str!("../data-capability.json")).expect("contract F"),
        )
        .with_data_capability(DataCapability::parse(RENAME).expect("contract F"))
        .with_plugins(plugins());
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

    /// The sounds `sound` holds, as the soundbox reads them.
    async fn sounds(&self) -> Vec<Value> {
        self.rows(
            "SELECT id::text, campaign_id::text AS campaign, name, file_path, tags, \
             volume::text AS volume FROM sound ORDER BY created_at",
        )
        .await
    }

    async fn drop(self) {
        drop(self.client);
        connect(&url())
            .await
            .batch_execute(&format!("DROP DATABASE {} WITH (FORCE)", self.name))
            .await
            .expect("the scratch database dropped");
    }
}

/// A submission of `data_capability` by the DM, approver of their own
/// campaign's critical fields, on the sound `target` when given.
async fn submit(
    engine: &Engine,
    data_capability: &str,
    payload: Value,
    target: Option<&str>,
) -> Result<Submitted> {
    let mut submission = json!({
        "dataCapability": data_capability,
        "payload": payload,
        "caller": { "by": "dm", "from": "soundbox.SoundLibrary", "roles": ["role:approver"] },
    });
    if let Some(id) = target {
        submission["target"] = json!({ "id": id });
    }
    let submission: Submission = serde_json::from_value(submission).expect("a submission");
    engine.submit(submission).await
}

/// One pass of the single applier, which settles one command.
async fn settled(engine: &Engine) -> CommandResult {
    let mut settled = engine.run_once().await.expect("the applier ran");
    assert_eq!(settled.len(), 1, "{settled:?}");
    settled.remove(0)
}

/// Adds `payload`: the sound's id, and what the applier made of it.
async fn add(engine: &Engine, payload: Value) -> (String, CommandResult) {
    let submitted = submit(engine, "soundbox.addSound@1", payload, None)
        .await
        .expect("queued");
    let id = submitted
        .partition
        .strip_prefix("Sound/")
        .expect("a Sound partition")
        .to_owned();
    (id, settled(engine).await)
}

fn crowd() -> Value {
    json!({
        "campaign": VALOMBRE,
        "name": "Bruit de foule",
        "tags": ["Taverne"],
        "volume": 55,
        "file": "son.ogg",
    })
}

#[tokio::test]
async fn a_sound_reaches_the_table_its_tags_normalised_its_volume_a_fraction() {
    let s = Scratch::new("crowd").await;
    let engine = s.serve().await;
    let (id, result) = add(&engine, crowd()).await;
    assert_eq!(result.status, State::Applied, "{result:?}");
    assert_eq!(
        s.sounds().await,
        [json!({
            "id": id, "campaign": VALOMBRE, "name": "Bruit de foule", "file_path": "son.ogg",
            "tags": ["taverne"], "volume": "0.55",
        })]
    );
    // Stamped by the server, to the millisecond: now, not the table's default.
    let stamped = s
        .rows(&format!(
            "SELECT created_at > now() - interval '1 minute' AS recent, \
             created_at = date_trunc('milliseconds', created_at) AS to_the_millisecond \
             FROM sound WHERE id = '{id}'"
        ))
        .await;
    assert_eq!(
        stamped,
        [json!({ "recent": true, "to_the_millisecond": true })]
    );
    s.drop().await;
}

#[tokio::test]
async fn tags_normalized_two_spellings_reach_the_table_as_one_tag() {
    let s = Scratch::new("bois").await;
    let engine = s.serve().await;
    let mut payload = crowd();
    payload["tags"] = json!(["Bois", "bois "]);
    payload.as_object_mut().expect("an object").remove("volume");
    let (_, result) = add(&engine, payload).await;
    assert_eq!(result.status, State::Applied, "{result:?}");
    let sounds = s.sounds().await;
    assert_eq!(sounds.len(), 1, "{sounds:?}");
    assert_eq!(sounds[0]["tags"], json!(["bois"]));
    // No volume given: full volume.
    assert_eq!(sounds[0]["volume"], json!("1.00"));
    s.drop().await;
}

#[tokio::test]
async fn sound_volume_range_a_volume_of_120_is_rejected_and_nothing_written() {
    let s = Scratch::new("loud").await;
    let engine = s.serve().await;
    let mut payload = crowd();
    payload["volume"] = json!(120);
    let (_, result) = add(&engine, payload).await;
    assert_eq!(result.status, State::Rejected, "{result:?}");
    assert_eq!(result.violations, ["sound-volume-range"]);
    assert_eq!(s.sounds().await, Vec::<Value>::new());
    s.drop().await;
}

#[tokio::test]
async fn a_campaign_the_tables_do_not_hold_is_not_found_and_nothing_queued() {
    let s = Scratch::new("nowhere").await;
    let engine = s.serve().await;
    let mut payload = crowd();
    payload["campaign"] = json!(NOWHERE);
    match submit(&engine, "soundbox.addSound@1", payload, None).await {
        Err(Error::Refused(refusal)) if refusal.kind == RefusalKind::NotFound => {
            assert_eq!(refusal.message, "cette campagne n'existe pas");
        }
        other => panic!("not found: {other:?}"),
    }
    assert!(engine.run_once().await.expect("the applier ran").is_empty());
    assert_eq!(s.sounds().await, Vec::<Value>::new());
    s.drop().await;
}

#[tokio::test]
async fn a_campaign_deleted_before_the_sound_is_applied_rejects_it() {
    let s = Scratch::new("deleted").await;
    let engine = s.serve().await;
    submit(&engine, "soundbox.addSound@1", crowd(), None)
        .await
        .expect("queued");
    s.client
        .batch_execute(&format!("DELETE FROM campaign WHERE id = '{VALOMBRE}'"))
        .await
        .expect("Valombre deleted");
    let result = settled(&engine).await;
    assert_eq!(result.status, State::Rejected, "{result:?}");
    assert_eq!(s.sounds().await, Vec::<Value>::new());
    s.drop().await;
}

#[tokio::test]
async fn a_later_write_of_an_added_sound_leaves_its_row_to_its_own_projection() {
    let s = Scratch::new("later").await;
    let engine = s.serve().await;
    let (id, _) = add(&engine, crowd()).await;
    submit(
        &engine,
        "test.renameSound@1",
        json!({ "name": "Foule" }),
        Some(&id),
    )
    .await
    .expect("queued");
    let result = settled(&engine).await;
    assert_eq!(result.status, State::Applied, "{result:?}");
    let names = s.rows("SELECT name FROM sound").await;
    assert_eq!(names, [json!({ "name": "Bruit de foule" })]);
    s.drop().await;
}
