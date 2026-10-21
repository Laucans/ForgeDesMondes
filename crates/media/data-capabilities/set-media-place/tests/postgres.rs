//! media.setMediaPlace over PostgreSQL (`DATABASE_URL`), as the server
//! plants it: a change of place of a media the tables hold — one the
//! fixtures wrote, which no command wrote before — is judged against the
//! media and the place as the tables hold them, and writes `media.place_id`
//! and no other column of any table; a place that is no lieu of the media's
//! campaign is refused naming `media-place-is-lieu`, and changes nothing.
//! Each test works in a scratch database of its own, migrated as the server
//! migrates it, dropped when it ends.

use std::collections::BTreeMap;
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
use serde_json::{Value, json};

const VALOMBRE: &str = "c1000000-0000-4000-8000-000000000001";
const BRUME: &str = "c1000000-0000-4000-8000-000000000002";
const TAVERNE: &str = "e1000000-0000-4000-8000-000000000002";
const MORLOX: &str = "e1000000-0000-4000-8000-000000000003";
const PHARE: &str = "e1000000-0000-4000-8000-000000000004";
const MOULIN: &str = "e1000000-0000-4000-8000-000000000005";
const CARTE: &str = "a1000000-0000-4000-8000-000000000001";
const SALLE: &str = "a1000000-0000-4000-8000-000000000002";

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
/// the fixtures write them — Valombre with the Taverne and the Moulin
/// (places) and Morlox (an NPC), Brume with its lighthouse (a place),
/// Valombre's battlemap of the ford with its grid and no place, and its
/// illustration of the tavern's hall at the Taverne. Each element is at
/// version 1.
struct Scratch {
    name: String,
    url: String,
    client: Client,
}

impl Scratch {
    async fn new(name: &str) -> Scratch {
        let name = format!("set_media_place_test_{name}_{}", std::process::id());
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
        let versions: String = [TAVERNE, MORLOX, PHARE, MOULIN]
            .iter()
            .map(|element| {
                format!(
                    "INSERT INTO entity_version (entity_id, version_number, source, snapshot, diff) \
                     VALUES ('{element}', 1, 'dm', '{{}}', '[]');"
                )
            })
            .collect();
        client
            .batch_execute(&format!(
                "INSERT INTO campaign (id, name, kind) VALUES
                     ('{VALOMBRE}', 'Les Cendres de Valombre', 'campagne'),
                     ('{BRUME}', 'La Brume', 'oneshot');
                 INSERT INTO entity (id, campaign_id, type, name) VALUES
                     ('{TAVERNE}', '{VALOMBRE}', 'lieu', 'La Taverne du Sanglier'),
                     ('{MORLOX}', '{VALOMBRE}', 'pnj', 'Morlox'),
                     ('{PHARE}', '{BRUME}', 'lieu', 'Le Phare'),
                     ('{MOULIN}', '{VALOMBRE}', 'lieu', 'Le Moulin');
                 {versions}
                 INSERT INTO media (id, campaign_id, kind, title, file_path, width_px, height_px, place_id) VALUES
                     ('{CARTE}', '{VALOMBRE}', 'battlemap', 'Le Gué', 'media/gue.png', 960, 640, NULL),
                     ('{SALLE}', '{VALOMBRE}', 'illustration', 'La salle', 'media/salle.png', 800, 600, '{TAVERNE}');
                 INSERT INTO battlemap_grid (media_id, hex_size_px, offset_x, offset_y, orientation, color, opacity)
                     VALUES ('{CARTE}', 30, 12, 8, 'pointe', '#202020', 0.4);"
            ))
            .await
            .expect("Valombre, Brume and their media");
        Scratch { name, url, client }
    }

    /// The engine as the server plants this unit and its target aggregate
    /// over this database: the aggregate's projection in the apply
    /// transaction, the tables lent to the Prepare.
    async fn serve(&self) -> Engine {
        let plugins = || -> Plugins {
            [aggregate_media::plugin(), set_media_place::plugin()]
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
        .with_rows(Arc::new(ReadOnly::new(connect(&self.url).await)))
        .with_aggregate(Aggregate::parse(aggregate_media::DOCUMENT).expect("contract I"))
        .with_data_capability(
            DataCapability::parse(include_str!("../data-capability.json")).expect("contract F"),
        )
        .with_plugins(plugins());
        engine.load().await.expect("the engine loads");
        engine
    }

    /// Every row of every table of the product, as JSON, by table, each
    /// one's rows in one order: the DataGuard's own (`dataguard_*`) aside —
    /// its queue and its journal record every command, whatever it writes.
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
                    &format!("SELECT to_jsonb(t)::text FROM {name} t ORDER BY to_jsonb(t)::text"),
                    &[],
                )
                .await
                .expect("a table's rows")
                .iter()
                .map(|row| serde_json::from_str(row.get::<_, &str>(0)).expect("JSON"))
                .collect();
            tables.insert(name, rows);
        }
        sorted(tables)
    }

    /// `media`'s place as its row holds it: `None` for none.
    async fn place_of(&self, media: &str) -> Option<String> {
        self.client
            .query_one(
                "SELECT place_id::text FROM media WHERE id = ($1::text)::uuid",
                &[&media],
            )
            .await
            .expect("the media")
            .get(0)
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

/// `tables` with `media`'s place made `place`: what a change of place
/// leaves, and nothing else.
fn placed(
    mut tables: BTreeMap<String, Vec<Value>>,
    media: &str,
    place: Option<&str>,
) -> BTreeMap<String, Vec<Value>> {
    for row in tables.get_mut("media").expect("the media table") {
        if row["id"] == json!(media) {
            row["place_id"] = place.map_or(Value::Null, Value::from);
        }
    }
    sorted(tables)
}

/// `tables`, each one's rows in the same order whatever changed in them.
fn sorted(mut tables: BTreeMap<String, Vec<Value>>) -> BTreeMap<String, Vec<Value>> {
    for rows in tables.values_mut() {
        rows.sort_by_key(ToString::to_string);
    }
    tables
}

/// A submission of media.setMediaPlace on `media`, to `place`, as the
/// MediaLibrary card sends it.
async fn place(
    engine: &Engine,
    media: &str,
    place: Option<&str>,
    key: Option<&str>,
) -> Result<Submitted> {
    let submission: Submission = serde_json::from_value(json!({
        "dataCapability": "media.setMediaPlace@1",
        "targetId": media,
        "payload": { "place": place },
        "idempotencyKey": key,
        "caller": { "by": "dm", "from": "media.MediaLibrary", "roles": [] },
    }))
    .expect("a submission");
    engine.submit(submission).await
}

/// Places `media` at `to` and applies it.
async fn applied(engine: &Engine, media: &str, to: Option<&str>) {
    place(engine, media, to, None).await.expect("queued");
    let result = engine.run_once().await.expect("the applier ran").remove(0);
    assert_eq!(result.status, State::Applied, "{result:?}");
}

/// The refusal `answer` is, of `kind`: its message.
fn refused(answer: Result<Submitted>, kind: RefusalKind) -> String {
    match answer {
        Err(Error::Refused(refusal)) if refusal.kind == kind => refusal.message,
        other => panic!("a refusal of kind {kind:?}: {other:?}"),
    }
}

#[tokio::test]
async fn a_lieu_of_the_campaign_reaches_place_id_and_no_other_column() {
    let scratch = Scratch::new("attach").await;
    let engine = scratch.serve().await;
    let before = scratch.tables().await;
    applied(&engine, CARTE, Some(TAVERNE)).await;
    assert_eq!(scratch.place_of(CARTE).await.as_deref(), Some(TAVERNE));
    assert_eq!(
        scratch.tables().await,
        placed(before.clone(), CARTE, Some(TAVERNE))
    );
    // Changed for another lieu of the campaign, in another case.
    applied(&engine, &CARTE.to_uppercase(), Some(&MOULIN.to_uppercase())).await;
    assert_eq!(scratch.place_of(CARTE).await.as_deref(), Some(MOULIN));
    assert_eq!(scratch.tables().await, placed(before, CARTE, Some(MOULIN)));
    scratch.drop().await;
}

#[tokio::test]
async fn a_null_place_empties_place_id_and_the_media_stays_valid() {
    let scratch = Scratch::new("detach").await;
    let engine = scratch.serve().await;
    let before = scratch.tables().await;
    applied(&engine, SALLE, None).await;
    assert_eq!(scratch.place_of(SALLE).await, None);
    assert_eq!(scratch.tables().await, placed(before, SALLE, None));
    scratch.drop().await;
}

#[tokio::test]
async fn a_place_that_is_no_lieu_of_the_campaign_is_refused_and_changes_nothing() {
    let scratch = Scratch::new("refuse").await;
    let engine = scratch.serve().await;
    let before = scratch.tables().await;
    for (to, why) in [(MORLOX, "an NPC"), (PHARE, "a lieu of another campaign")] {
        let error = refused(
            place(&engine, SALLE, Some(to), None).await,
            RefusalKind::Unprocessable,
        );
        assert!(error.starts_with("media-place-is-lieu"), "{why}: {error}");
        assert!(
            engine.run_once().await.expect("the applier ran").is_empty(),
            "{why}: nothing was queued"
        );
    }
    refused(
        place(
            &engine,
            "a1000000-0000-4000-8000-0000000000ff",
            Some(TAVERNE),
            None,
        )
        .await,
        RefusalKind::NotFound,
    );
    assert_eq!(scratch.tables().await, before);
    scratch.drop().await;
}

#[tokio::test]
async fn a_command_replayed_with_its_idempotency_key_writes_the_place_once() {
    let scratch = Scratch::new("replay").await;
    let engine = scratch.serve().await;
    let before = scratch.tables().await;
    let first = place(&engine, CARTE, Some(TAVERNE), Some("attach-1"))
        .await
        .expect("queued");
    let result = engine.run_once().await.expect("the applier ran").remove(0);
    assert_eq!(result.status, State::Applied, "{result:?}");
    let again = place(&engine, CARTE, Some(TAVERNE), Some("attach-1"))
        .await
        .expect("replayed");
    assert!(again.replayed, "{again:?}");
    assert_eq!(again.command_id, first.command_id);
    assert!(engine.run_once().await.expect("the applier ran").is_empty());
    assert_eq!(scratch.tables().await, placed(before, CARTE, Some(TAVERNE)));
    scratch.drop().await;
}
