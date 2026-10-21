//! The Diffusion aggregate's table over PostgreSQL (`DATABASE_URL`), as the
//! server plants it: every applied write of a session's diffusion is its
//! `diffusion` row, written in the transaction that applies it; what the
//! table refuses — a media it does not hold, a zoom past its precision —
//! rejects the command and leaves the row as it was; a diffusion only the
//! table holds is the engine's row until a command writes it. The
//! DataCapabilities here are test-only. Each test works in a scratch
//! database of its own, migrated as the server migrates it, dropped when it
//! ends.

use std::path::Path;
use std::sync::Arc;

use aggregate_diffusion::Tables;
use harness_dataguard::aggregate::Aggregate;
use harness_dataguard::api::Submission;
use harness_dataguard::command::{CommandResult, State};
use harness_dataguard::engine::Engine;
use harness_dataguard::manifest::DataCapability;
use harness_dataguard::memory::MemoryNotifier;
use harness_dataguard::plugins::{Plugins, ProjectPg};
use harness_dataguard::postgres::{PgStore, SystemClock};
use harness_dataguard::tokio_postgres::{self, Client, NoTls};
use serde_json::{Value, json};

/// What a session's player view shows, written under the session its
/// payload names.
const SHOW: &str = r#"{"dataCapability":"test.show","owner":"dataguard","version":1,"effect":"upsert","target":{"aggregate":"Diffusion","id":"$sessionId"},"mode":"overwrite","callableBy":["*"]}"#;
/// A diffusion deleted: the engine's soft delete, a tombstone.
const DELETE: &str = r#"{"dataCapability":"test.delete","owner":"dataguard","version":1,"effect":"delete","target":{"aggregate":"Diffusion","id":"$sessionId"},"mode":"overwrite","callableBy":["*"]}"#;

const VALOMBRE: &str = "c1000000-0000-4000-8000-000000000001";
const SEANCE: &str = "51000000-0000-4000-8000-000000000002";
/// An illustration of 800 × 1200.
const PORTRAIT: &str = "d1000000-0000-4000-8000-000000000002";
/// A media the tables do not hold.
const ABSENT: &str = "d1000000-0000-4000-8000-00000000ffff";

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

/// A database of its own, migrated, holding Valombre's live session and its
/// portrait; no diffusion row.
struct Scratch {
    name: String,
    url: String,
    client: Client,
}

impl Scratch {
    /// A database of its own, empty: no table of the product.
    async fn bare(name: &str) -> Scratch {
        let name = format!("diffusion_tables_test_{name}_{}", std::process::id());
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

    async fn new(name: &str) -> Scratch {
        let Scratch {
            name,
            url,
            mut client,
        } = Scratch::bare(name).await;
        let migrations = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../data/migrations");
        data::migrate(&mut client, &migrations)
            .await
            .expect("migrated");
        client
            .batch_execute(&format!(
                "INSERT INTO campaign (id, name, kind)
                     VALUES ('{VALOMBRE}', 'Les Cendres de Valombre', 'campagne');
                 INSERT INTO session (id, campaign_id, position, title, status, microphone_on)
                     VALUES ('{SEANCE}', '{VALOMBRE}', 1, 'Séance 1', 'live', false);
                 INSERT INTO media (id, campaign_id, kind, title, file_path, width_px, height_px)
                     VALUES ('{PORTRAIT}', '{VALOMBRE}', 'illustration', 'Morlox',
                         'media/morlox.png', 800, 1200);"
            ))
            .await
            .expect("Valombre, its session and its portrait");
        Scratch { name, url, client }
    }

    /// The engine as the server plants this aggregate over this database,
    /// with the test-only DataCapabilities.
    async fn serve(&self) -> Engine {
        let plugins = || -> Plugins { std::iter::once(aggregate_diffusion::plugin()).collect() };
        let store = PgStore::open(connect(&self.url).await)
            .await
            .expect("the store")
            .with_projections(&plugins())
            .await
            .expect("warmed");
        let mut engine = Engine::new(
            Arc::new(store),
            Arc::new(MemoryNotifier::default()),
            Arc::new(SystemClock),
        )
        .with_aggregate(Aggregate::parse(include_str!("../aggregate.json")).expect("contract I"))
        .with_plugins(plugins());
        for manifest in [SHOW, DELETE] {
            engine =
                engine.with_data_capability(DataCapability::parse(manifest).expect("contract F"));
        }
        engine.load().await.expect("the engine loads");
        engine
    }

    /// Every `diffusion` row, as JSON.
    async fn diffusions(&self) -> Vec<Value> {
        self.client
            .query(
                "SELECT to_jsonb(d)::text FROM diffusion d ORDER BY session_id",
                &[],
            )
            .await
            .expect("the diffusion table")
            .iter()
            .map(|row| serde_json::from_str(row.get::<_, &str>(0)).expect("JSON"))
            .collect()
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

/// What the single applier made of one command of `data_capability`.
async fn written(engine: &Engine, data_capability: &str, payload: Value) -> CommandResult {
    let submission: Submission = serde_json::from_value(json!({
        "dataCapability": data_capability,
        "payload": payload,
        // The critical fields' review is not what these tests judge.
        "caller": { "by": "dm", "from": "live.PlayerStage", "roles": ["role:approver"] },
    }))
    .expect("a submission");
    engine.submit(submission).await.expect("queued");
    let mut applied = engine.run_once().await.expect("the applier ran");
    assert_eq!(applied.len(), 1, "{applied:?}");
    applied.remove(0)
}

/// Session 1's player view on `PORTRAIT`, framed at `zoom`, its window
/// `window` when one is open.
fn portrait(zoom: Value, window: Option<(f64, f64, f64)>) -> Value {
    let mut row = json!({
        "sessionId": SEANCE, "baseKind": "illustration", "baseMediaId": PORTRAIT,
        "baseInstanceId": null, "viewZoom": zoom, "viewPanX": 10, "viewPanY": -5.5,
        "updatedAt": "2026-10-21T10:00:00.123Z",
    });
    if let Some((x, y, width)) = window {
        for (field, value) in [
            ("windowMediaId", json!(PORTRAIT)),
            ("windowX", json!(x)),
            ("windowY", json!(y)),
            ("windowWidth", json!(width)),
        ] {
            row[field] = value;
        }
    }
    row
}

/// Its `diffusion` row, as the table holds it.
fn table_row(zoom: f64, window: Option<(f64, f64, f64)>) -> Value {
    let (media, x, y, width) = window.map_or((None, None, None, None), |(x, y, width)| {
        (Some(PORTRAIT), Some(x), Some(y), Some(width))
    });
    json!({
        "session_id": SEANCE, "base_kind": "illustration", "base_media_id": PORTRAIT,
        "base_instance_id": null, "window_media_id": media, "window_x": x, "window_y": y,
        "window_width": width, "view_zoom": zoom, "view_pan_x": 10.0, "view_pan_y": -5.5,
        "updated_at": "2026-10-21T10:00:00.123+00:00",
    })
}

#[tokio::test]
async fn every_applied_write_is_the_session_s_diffusion_row() {
    let scratch = Scratch::new("written").await;
    let engine = scratch.serve().await;
    let result = written(
        &engine,
        "test.show@1",
        portrait(json!(1.25), Some((0.1, 0.2, 0.3))),
    )
    .await;
    assert_eq!(result.status, State::Applied, "{result:?}");
    assert_eq!(
        scratch.diffusions().await,
        [table_row(1.25, Some((0.1, 0.2, 0.3)))]
    );

    // A later write is the same row, every column as the engine's row is:
    // the fields it does not send kept, the ones it empties emptied.
    let result = written(&engine, "test.show@1", portrait(json!(2), None)).await;
    assert_eq!(result.status, State::Applied, "{result:?}");
    assert_eq!(
        scratch.diffusions().await,
        [table_row(2.0, Some((0.1, 0.2, 0.3)))]
    );
    let mut closed = portrait(json!(2), None);
    for field in ["windowMediaId", "windowX", "windowY", "windowWidth"] {
        closed[field] = Value::Null;
    }
    let result = written(&engine, "test.show@1", closed).await;
    assert_eq!(result.status, State::Applied, "{result:?}");
    assert_eq!(scratch.diffusions().await, [table_row(2.0, None)]);

    // A tombstone writes nothing.
    let result = written(&engine, "test.delete@1", json!({ "sessionId": SEANCE })).await;
    assert_eq!(result.status, State::Applied, "{result:?}");
    assert_eq!(scratch.diffusions().await, [table_row(2.0, None)]);
    scratch.drop().await;
}

#[tokio::test]
async fn what_the_table_refuses_rejects_the_command_and_the_row_stays() {
    let scratch = Scratch::new("refused").await;
    let engine = scratch.serve().await;
    written(&engine, "test.show@1", portrait(json!(1), None)).await;
    let before = scratch.diffusions().await;

    let mut absent = portrait(json!(1), None);
    absent["baseMediaId"] = json!(ABSENT);
    // A media the table does not hold (a foreign key), a zoom past its
    // `numeric(6,3)`.
    for payload in [absent, portrait(json!(5000), None)] {
        let result = written(&engine, "test.show@1", payload.clone()).await;
        assert_eq!(result.status, State::Rejected, "{payload}: {result:?}");
        assert_eq!(scratch.diffusions().await, before, "{payload}");
    }
    scratch.drop().await;
}

#[tokio::test]
async fn a_diffusion_only_the_table_holds_is_the_engine_s_row_until_a_command_writes_it() {
    let scratch = Scratch::new("hydrated").await;
    scratch
        .client
        .batch_execute(&format!(
            "INSERT INTO diffusion (session_id, base_kind, window_media_id, window_x, window_y,
                 window_width, view_zoom, view_pan_x, view_pan_y, updated_at)
             VALUES ('{SEANCE}', 'cover', '{PORTRAIT}', 0.1, 0.2, 0.3, 1, 0, 0,
                 '2026-10-21T09:00:00Z')"
        ))
        .await
        .expect("the fixture's diffusion");
    // The engine's own tables, which the read looks a tombstone up in.
    scratch.serve().await;
    let hydrated = Tables
        .hydrate(
            &format!("Diffusion/{}", SEANCE.to_uppercase()),
            &scratch.client,
        )
        .await
        .expect("read");
    let expected = json!({
        "sessionId": SEANCE, "baseKind": "cover", "windowMediaId": PORTRAIT,
        "windowX": 0.1, "windowY": 0.2, "windowWidth": 0.3,
        "viewZoom": 1.0, "viewPanX": 0.0, "viewPanY": 0.0,
        "updatedAt": "2026-10-21T09:00:00+00:00",
    });
    assert_eq!(
        hydrated.map(|row| Value::Object(row.into_iter().collect())),
        Some(expected)
    );
    for partition in [
        format!("Diffusion/{}", "51000000-0000-4000-8000-00000000ffff"),
        format!("Media/{SEANCE}"),
    ] {
        let none = Tables
            .hydrate(&partition, &scratch.client)
            .await
            .expect("read");
        assert_eq!(none, None, "{partition}");
    }
    scratch.drop().await;
}

#[tokio::test]
async fn a_table_it_cannot_write_fails_the_step_unrefused() {
    let scratch = Scratch::bare("bare").await;
    // A `diffusion` with every column and no key: nothing to hydrate, and an
    // upsert with no conflict to resolve on — neither a CHECK, a foreign
    // key nor a value refused.
    scratch
        .client
        .batch_execute(
            "CREATE TABLE diffusion (session_id uuid, base_kind text, base_media_id uuid,
                 base_instance_id uuid, window_media_id uuid, window_x numeric,
                 window_y numeric, window_width numeric, view_zoom numeric,
                 view_pan_x numeric, view_pan_y numeric, updated_at timestamptz)",
        )
        .await
        .expect("a diffusion with no key");
    let engine = scratch.serve().await;
    let submission: Submission = serde_json::from_value(json!({
        "dataCapability": "test.show@1",
        "payload": portrait(json!(1), None),
        "caller": { "by": "dm", "from": "live.PlayerStage", "roles": ["role:approver"] },
    }))
    .expect("a submission");
    engine.submit(submission).await.expect("queued");
    // The store fails, the command is neither applied nor rejected: it
    // stays at the head.
    assert!(engine.run_once().await.is_err());
    assert!(engine.run_once().await.is_err());
    let rows = scratch
        .client
        .query(
            "SELECT partition FROM dataguard_rows WHERE partition LIKE 'Diffusion/%'",
            &[],
        )
        .await
        .expect("the engine's rows");
    assert!(rows.is_empty(), "{rows:?}");
    scratch.drop().await;
}
