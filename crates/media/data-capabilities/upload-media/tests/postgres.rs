//! media.uploadMedia over PostgreSQL (`DATABASE_URL`), as the server plants
//! it: what the engine applies on `Media/<id>` reaches `media` and, for a
//! battlemap, `battlemap_grid`, whose table makes a hex 5 feet; an upload's
//! campaign and place are read from the tables, the fixtures' included. Each
//! test works in a scratch database of its own, migrated as the server
//! migrates it, dropped when it ends.

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
const BRUME: &str = "c1000000-0000-4000-8000-000000000002";
const TAVERNE: &str = "e1000000-0000-4000-8000-000000000002";
const MORLOX: &str = "e1000000-0000-4000-8000-000000000003";
const PHARE: &str = "e1000000-0000-4000-8000-000000000004";

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

/// A database of its own, migrated as the server migrates it, holding two
/// campaigns and — as the fixtures do, written by no command — Valombre's
/// tavern, a lieu, Morlox, an NPC, and La Brume's lighthouse, a lieu.
struct Scratch {
    name: String,
    url: String,
    client: Client,
}

impl Scratch {
    async fn new(name: &str) -> Scratch {
        let name = format!("upload_media_test_{name}_{}", std::process::id());
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
        let version = |element: &str| {
            format!(
                "INSERT INTO entity_version (entity_id, version_number, source, snapshot, diff) \
                 VALUES ('{element}', 1, 'dm', '{{}}', '[]');"
            )
        };
        client
            .batch_execute(&format!(
                "BEGIN;
                 INSERT INTO campaign (id, name, kind) VALUES
                     ('{VALOMBRE}', 'Les Cendres de Valombre', 'campagne'),
                     ('{BRUME}', 'La Brume', 'oneshot');
                 INSERT INTO entity (id, campaign_id, type, name) VALUES
                     ('{TAVERNE}', '{VALOMBRE}', 'lieu', 'La Taverne du Sanglier'),
                     ('{MORLOX}', '{VALOMBRE}', 'pnj', 'Morlox'),
                     ('{PHARE}', '{BRUME}', 'lieu', 'Le Phare');
                 {}{}{}
                 COMMIT;",
                version(TAVERNE),
                version(MORLOX),
                version(PHARE),
            ))
            .await
            .expect("two campaigns and their elements");
        Scratch { name, url, client }
    }

    /// The engine as the server plants this unit and its target aggregate
    /// over this database: the aggregate's projection in the apply
    /// transaction, the tables lent to the Prepare.
    async fn serve(&self) -> Engine {
        let plugins = || -> Plugins {
            [aggregate_media::plugin(), upload_media::plugin()]
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
        .with_ids(Arc::new(RandomIds))
        .with_rows(Arc::new(ReadOnly::new(connect(&self.url).await)))
        .with_aggregate(Aggregate::parse(aggregate_media::DOCUMENT).expect("contract I"))
        .with_data_capability(
            DataCapability::parse(include_str!("../data-capability.json")).expect("contract F"),
        )
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

    async fn drop(self) {
        drop(self.client);
        connect(&url())
            .await
            .batch_execute(&format!("DROP DATABASE {} WITH (FORCE)", self.name))
            .await
            .expect("the scratch database dropped");
    }
}

/// A submission of media.uploadMedia by the DM, approver of their own
/// campaign's critical fields.
async fn submit(engine: &Engine, payload: Value, key: Option<&str>) -> Result<Submitted> {
    let submission: Submission = serde_json::from_value(json!({
        "dataCapability": "media.uploadMedia@1",
        "payload": payload,
        "idempotencyKey": key,
        "caller": { "by": "dm", "from": "media.MediaLibrary", "roles": ["role:approver"] },
    }))
    .expect("a submission");
    engine.submit(submission).await
}

/// Uploads `payload`: the media's id, and what the applier made of it.
async fn upload(engine: &Engine, payload: Value) -> (String, CommandResult) {
    let submitted = submit(engine, payload, None).await.expect("queued");
    let id = submitted
        .partition
        .strip_prefix("Media/")
        .expect("a Media partition")
        .to_owned();
    let mut settled = engine.run_once().await.expect("the applier ran");
    assert_eq!(settled.len(), 1, "{settled:?}");
    (id, settled.remove(0))
}

fn applied(result: &CommandResult) {
    assert_eq!(result.status, State::Applied, "{result:?}");
}

/// The refusal `answer` is, of `kind`: its message.
fn refused(answer: Result<Submitted>, kind: RefusalKind) -> String {
    match answer {
        Err(Error::Refused(refusal)) if refusal.kind == kind => refusal.message,
        other => panic!("a refusal of kind {kind:?}: {other:?}"),
    }
}

fn media(kind: &str, title: &str, place: Option<&str>) -> Value {
    json!({
        "campaign": VALOMBRE,
        "kind": kind,
        "title": title,
        "file": "/Users/dm/cartes/taverne.png",
        "width": 2048,
        "height": 1536,
        "place": place,
    })
}

#[tokio::test]
async fn a_battlemap_reaches_media_with_its_grid_at_five_feet_a_hex() {
    let s = Scratch::new("battlemap").await;
    let engine = s.serve().await;
    let (id, result) = upload(&engine, media("battlemap", "La Taverne", None)).await;
    applied(&result);

    let rows = s
        .rows(&format!(
            "SELECT campaign_id, kind, title, file_path, width_px, height_px, place_id \
             FROM media WHERE id = '{id}'"
        ))
        .await;
    assert_eq!(
        rows,
        [json!({
            "campaign_id": VALOMBRE, "kind": "battlemap", "title": "La Taverne",
            "file_path": "/Users/dm/cartes/taverne.png", "width_px": 2048, "height_px": 1536,
            "place_id": null,
        })]
    );
    // grid-only-for-battlemap, hex-is-five-feet, grid-values-range.
    let grid = s
        .rows(&format!(
            "SELECT hex_size_px > 0 AS sized, opacity BETWEEN 0 AND 1 AS opacity_in_range, \
             opacity::text AS opacity, orientation, color, offset_x::text AS ox, \
             offset_y::text AS oy, feet_per_hex FROM battlemap_grid WHERE media_id = '{id}'"
        ))
        .await;
    assert_eq!(
        grid,
        [json!({
            "sized": true, "opacity_in_range": true, "opacity": "0.45", "orientation": "pointe",
            "color": "text", "ox": "0.00", "oy": "0.00", "feet_per_hex": 5,
        })]
    );
    s.drop().await;
}

#[tokio::test]
async fn an_illustration_reaches_media_with_no_grid() {
    let s = Scratch::new("illustration").await;
    let engine = s.serve().await;
    let (id, result) = upload(&engine, media("illustration", "Elowen", None)).await;
    applied(&result);
    let kinds = s
        .rows(&format!("SELECT kind FROM media WHERE id = '{id}'"))
        .await;
    assert_eq!(kinds, [json!({ "kind": "illustration" })]);
    let grids = s
        .rows(&format!(
            "SELECT media_id FROM battlemap_grid WHERE media_id = '{id}'"
        ))
        .await;
    assert!(grids.is_empty(), "{grids:?}");
    s.drop().await;
}

#[tokio::test]
async fn a_lieu_the_tables_hold_is_the_media_s_place_and_any_other_is_refused() {
    let s = Scratch::new("place").await;
    let engine = s.serve().await;
    // The tavern was written by no command: the tables alone hold it.
    let (id, result) = upload(
        &engine,
        media("battlemap", "La Taverne", Some(&TAVERNE.to_uppercase())),
    )
    .await;
    applied(&result);
    let places = s
        .rows(&format!("SELECT place_id FROM media WHERE id = '{id}'"))
        .await;
    assert_eq!(places, [json!({ "place_id": TAVERNE })]);

    // media-place-is-lieu: an NPC, another campaign's lieu, no element.
    for place in [MORLOX, PHARE, "e1000000-0000-4000-8000-0000000000ff"] {
        let error = refused(
            submit(&engine, media("illustration", place, Some(place)), None).await,
            RefusalKind::Unprocessable,
        );
        assert!(error.starts_with("media-place-is-lieu"), "{place}: {error}");
    }
    assert!(engine.run_once().await.expect("the applier ran").is_empty());
    let count = s.rows("SELECT count(*) AS media FROM media").await;
    assert_eq!(count, [json!({ "media": 1 })]);
    s.drop().await;
}

#[tokio::test]
async fn the_same_upload_twice_under_one_idempotency_key_is_one_media_row() {
    let s = Scratch::new("idempotent").await;
    let engine = s.serve().await;
    let payload = media("battlemap", "La Taverne", None);
    let first = submit(&engine, payload.clone(), Some("upload-1"))
        .await
        .expect("queued");
    let second = submit(&engine, payload, Some("upload-1"))
        .await
        .expect("replayed");
    assert_eq!(second.command_id, first.command_id, "{second:?}");
    applied(&engine.run_once().await.expect("the applier ran")[0]);
    assert!(engine.run_once().await.expect("the applier ran").is_empty());
    let count = s.rows("SELECT count(*) AS media FROM media").await;
    assert_eq!(count, [json!({ "media": 1 })]);
    s.drop().await;
}

#[tokio::test]
async fn an_upload_to_a_campaign_the_tables_do_not_hold_is_refused_and_not_queued() {
    let s = Scratch::new("campaign").await;
    let engine = s.serve().await;
    let mut payload = media("battlemap", "La Taverne", None);
    payload["campaign"] = json!("c1000000-0000-4000-8000-0000000000ff");
    refused(
        submit(&engine, payload, None).await,
        RefusalKind::Unprocessable,
    );
    assert!(engine.run_once().await.expect("the applier ran").is_empty());
    s.drop().await;
}

#[tokio::test]
async fn a_place_that_is_no_lieu_any_more_when_the_upload_is_applied_rejects_it_whole() {
    let s = Scratch::new("retyped").await;
    let engine = s.serve().await;
    let submitted = submit(
        &engine,
        media("battlemap", "La Taverne", Some(TAVERNE)),
        None,
    )
    .await
    .expect("queued: the tavern is a lieu");
    // Made an NPC behind the engine's back before the upload is applied.
    s.client
        .batch_execute(&format!(
            "BEGIN;
             SELECT set_config(entity_versioned_setting('{TAVERNE}'), 'on', true);
             UPDATE entity SET type = 'pnj' WHERE id = '{TAVERNE}';
             COMMIT"
        ))
        .await
        .expect("the tavern retyped");
    let result = engine.run_once().await.expect("the applier ran").remove(0);
    // The table's trigger refuses it in the apply transaction: the command
    // is rejected and nothing of it is written, the engine's row neither.
    assert_eq!(result.status, State::Rejected, "{result:?}");
    let count = s.rows("SELECT count(*) AS media FROM media").await;
    assert_eq!(count, [json!({ "media": 0 })]);
    let engine_rows = s
        .rows(&format!(
            "SELECT partition FROM dataguard_rows WHERE partition = '{}'",
            submitted.partition
        ))
        .await;
    assert!(engine_rows.is_empty(), "{engine_rows:?}");
    s.drop().await;
}
