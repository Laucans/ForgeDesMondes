//! table.createBattlemapInstance over PostgreSQL (`DATABASE_URL`), as the
//! server plants it beside its target aggregate: a scene of the session
//! given a battlemap of its campaign gets its instance on its own partition,
//! judged by the BattlemapInstance aggregate and applied; a second creation
//! for the scene — queued or applied, or one the tables already hold — is
//! refused (`one-instance-per-scene`), and replayed when it carries the first
//! one's idempotency key; a battlemap of another campaign, a scene of another
//! session or anything the tables do not hold is refused before it is
//! queued; and the battlemap and its grid read the same after
//! (`instance-isolated`). Each test works in a scratch database of its own,
//! migrated as the server migrates it, dropped when it ends.

use std::path::Path;
use std::sync::Arc;

use create_battlemap_instance::instance_id;
use harness_dataguard::aggregate::Aggregate;
use harness_dataguard::api::{Submission, Submitted};
use harness_dataguard::command::{CommandResult, State};
use harness_dataguard::engine::Engine;
use harness_dataguard::manifest::DataCapability;
use harness_dataguard::memory::{MemoryNotifier, MemoryStore};
use harness_dataguard::plugins::{Plugins, RefusalKind};
use harness_dataguard::postgres::{PgStore, ReadOnly, SystemClock};
use harness_dataguard::tokio_postgres::{self, Client, NoTls};
use harness_dataguard::{Error, Result};
use serde_json::{Value, json};

const VALOMBRE: &str = "c1000000-0000-4000-8000-000000000001";
const BRUME: &str = "c1000000-0000-4000-8000-000000000002";
/// Valombre's session, and its two scenes.
const SEANCE: &str = "5e000000-0000-4000-8000-000000000001";
const ARRIVEE: &str = "5c000000-0000-4000-8000-000000000001";
const GUE_SCENE: &str = "5c000000-0000-4000-8000-000000000002";
/// Another session of Valombre, and its scene.
const AUTRE_SEANCE: &str = "5e000000-0000-4000-8000-000000000002";
const AILLEURS: &str = "5c000000-0000-4000-8000-000000000003";
/// A scene whose instance the tables hold already, created before.
const DEJA: &str = "5c000000-0000-4000-8000-000000000004";
const DEJA_INSTANCE: &str = "b1000000-0000-4000-8000-000000000001";
/// Valombre's battlemap with its grid, its illustration, and Brume's
/// battlemap.
const GUE: &str = "d1000000-0000-4000-8000-00000000000a";
const PORTRAIT: &str = "d1000000-0000-4000-8000-00000000000c";
const RECIF: &str = "d2000000-0000-4000-8000-00000000000a";
const NOWHERE: &str = "d1000000-0000-4000-8000-0000000000ff";

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

/// The BattlemapInstance aggregate's document, read beside this unit.
fn aggregate() -> Aggregate {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../aggregates/BattlemapInstance/aggregate.json");
    Aggregate::parse(&std::fs::read_to_string(path).expect("aggregate.json")).expect("contract I")
}

fn manifest() -> DataCapability {
    DataCapability::parse(include_str!("../data-capability.json")).expect("contract F")
}

fn plugins() -> Plugins {
    [
        aggregate_battlemap_instance::plugin(),
        create_battlemap_instance::plugin(),
    ]
    .into_iter()
    .collect()
}

/// A database of its own, migrated, holding Valombre — two sessions, their
/// scenes, one already with its instance, a battlemap with its grid and an
/// illustration — and Brume with its battlemap.
struct Scratch {
    name: String,
    url: String,
    client: Client,
}

impl Scratch {
    async fn new(name: &str) -> Scratch {
        let name = format!(
            "create_battlemap_instance_test_{name}_{}",
            std::process::id()
        );
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
                     ('{VALOMBRE}', 'Les Cendres de Valombre', 'campagne'),
                     ('{BRUME}', 'Les Brumes', 'campagne');
                 INSERT INTO session (id, campaign_id, position, title, status, microphone_on) VALUES
                     ('{SEANCE}', '{VALOMBRE}', 1, 'L''embuscade du Gué', 'prep', false),
                     ('{AUTRE_SEANCE}', '{VALOMBRE}', 2, 'Le retour', 'prep', false);
                 INSERT INTO scene (id, session_id, position, title) VALUES
                     ('{ARRIVEE}', '{SEANCE}', 1, 'Arrivée'),
                     ('{GUE_SCENE}', '{SEANCE}', 2, 'Le gué'),
                     ('{DEJA}', '{SEANCE}', 3, 'Déjà ouverte'),
                     ('{AILLEURS}', '{AUTRE_SEANCE}', 1, 'Ailleurs');
                 INSERT INTO media (id, campaign_id, kind, title, file_path, width_px, height_px) VALUES
                     ('{GUE}', '{VALOMBRE}', 'battlemap', 'Le Gué des Corbeaux', 'media/gue.png', 960, 640),
                     ('{PORTRAIT}', '{VALOMBRE}', 'illustration', 'Elowen', 'media/elowen.png', 400, 400),
                     ('{RECIF}', '{BRUME}', 'battlemap', 'Le Récif', 'media/recif.png', 800, 600);
                 INSERT INTO battlemap_grid (media_id, hex_size_px, offset_x, offset_y, orientation, color, opacity) VALUES
                     ('{GUE}', 30, 12, 8, 'pointe', 'text', 0.4),
                     ('{RECIF}', 32, 0, 0, 'plat', 'text', 0.45);
                 INSERT INTO battlemap_instance (id, battlemap_media_id, session_id, scene_id) VALUES
                     ('{DEJA_INSTANCE}', '{GUE}', '{SEANCE}', '{DEJA}');"
            ))
            .await
            .expect("Valombre and Brume");
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
        let engine = Engine::new(
            Arc::new(store),
            Arc::new(MemoryNotifier::default()),
            Arc::new(SystemClock),
        )
        .with_rows(Arc::new(ReadOnly::new(connect(&self.url).await)))
        .with_aggregate(aggregate())
        .with_data_capability(manifest())
        .with_plugins(plugins());
        engine.load().await.expect("the engine loads");
        engine
    }

    /// The instances of `scene` the engine holds, each its row's fields.
    async fn instances(&self, scene: &str) -> Vec<Value> {
        self.client
            .query(
                "SELECT fields::text FROM dataguard_rows
                 WHERE partition LIKE 'BattlemapInstance/%' AND NOT tombstoned
                     AND fields ->> 'sceneId' = $1",
                &[&scene],
            )
            .await
            .expect("the engine's rows")
            .iter()
            .map(|row| serde_json::from_str(row.get::<_, &str>(0)).expect("JSON"))
            .collect()
    }

    /// The battlemaps and their grids as the tables hold them, as JSON text.
    async fn battlemaps(&self) -> Vec<String> {
        self.client
            .query(
                "SELECT to_jsonb(m)::text || coalesce(to_jsonb(g)::text, '') FROM media m
                 LEFT JOIN battlemap_grid g ON g.media_id = m.id ORDER BY m.id",
                &[],
            )
            .await
            .expect("the media")
            .iter()
            .map(|row| row.get(0))
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

/// A creation as the SceneBoard sends it — by the DM, approver of their own
/// campaign: the instance's ids are critical fields — with the idempotency
/// key `key` when it carries one.
async fn post(engine: &Engine, payload: Value, key: Option<&str>) -> Result<Submitted> {
    let mut body = json!({
        "dataCapability": "table.createBattlemapInstance@1",
        "target": { "aggregate": "BattlemapInstance" },
        "payload": payload,
        "caller": { "by": "dm", "from": "SceneBoard", "roles": ["role:approver"] },
    });
    if let Some(key) = key {
        body["idempotencyKey"] = json!(key);
    }
    engine
        .submit(serde_json::from_value::<Submission>(body).expect("a submission"))
        .await
}

/// A creation queued then run by the applier: what it made of it.
async fn create(engine: &Engine, payload: Value) -> CommandResult {
    post(engine, payload, None).await.expect("queued");
    let mut settled = engine.run_once().await.expect("the applier ran");
    assert_eq!(settled.len(), 1, "{settled:?}");
    settled.remove(0)
}

/// The refusal `answer` is, of `kind`: its message.
fn refused(answer: Result<Submitted>, kind: RefusalKind) -> String {
    match answer {
        Err(Error::Refused(refusal)) if refusal.kind == kind => refusal.message,
        other => panic!("a {kind:?} refusal: {other:?}"),
    }
}

fn asked(session: &str, scene: &str, battlemap: &str) -> Value {
    json!({ "session": session, "scene": scene, "battlemap": battlemap })
}

#[tokio::test]
async fn a_scene_of_the_session_gets_one_instance_of_a_battlemap_of_its_campaign() {
    let scratch = Scratch::new("created").await;
    let engine = scratch.serve().await;

    // The ids in whatever case the request used: written as the tables
    // write them.
    let result = create(
        &engine,
        asked(&SEANCE.to_uppercase(), ARRIVEE, &GUE.to_uppercase()),
    )
    .await;
    assert_eq!(result.status, State::Applied, "{result:?}");
    let instances = scratch.instances(ARRIVEE).await;
    assert_eq!(instances.len(), 1, "{instances:?}");
    let instance = &instances[0];
    assert_eq!(instance["battlemapInstanceId"], instance_id(ARRIVEE));
    assert_eq!(instance["battlemapMediaId"], GUE);
    assert_eq!(instance["sessionId"], SEANCE);
    assert_eq!(instance["sceneId"], ARRIVEE);
    let created = instance["createdAt"].as_str().expect("a time");
    assert!(
        created.starts_with("20") && created.ends_with('Z'),
        "{created}"
    );
    assert!(scratch.instances(GUE_SCENE).await.is_empty());
    scratch.drop().await;
}

#[tokio::test]
async fn one_instance_per_scene_a_second_creation_is_refused_queued_or_applied() {
    let scratch = Scratch::new("second").await;
    let engine = scratch.serve().await;

    // Queued and not yet applied: the partition as the queue leaves it.
    post(&engine, asked(SEANCE, ARRIVEE, GUE), None)
        .await
        .expect("queued");
    let message = refused(
        post(&engine, asked(SEANCE, ARRIVEE, GUE), None).await,
        RefusalKind::Unprocessable,
    );
    assert!(message.contains("one-instance-per-scene"), "{message}");
    assert_eq!(engine.run_once().await.expect("applied").len(), 1);
    // Applied.
    let message = refused(
        post(&engine, asked(SEANCE, ARRIVEE, GUE), None).await,
        RefusalKind::Unprocessable,
    );
    assert!(message.contains("one-instance-per-scene"), "{message}");
    assert_eq!(scratch.instances(ARRIVEE).await.len(), 1);
    // One the tables held before any command.
    let message = refused(
        post(&engine, asked(SEANCE, DEJA, GUE), None).await,
        RefusalKind::Unprocessable,
    );
    assert!(message.contains("one-instance-per-scene"), "{message}");
    assert!(scratch.instances(DEJA).await.is_empty());
    scratch.drop().await;
}

#[tokio::test]
async fn one_instance_per_scene_a_creation_resent_with_its_key_is_answered_as_the_first() {
    let scratch = Scratch::new("replayed").await;
    let engine = scratch.serve().await;
    let key = format!("table.createBattlemapInstance:{ARRIVEE}");

    let first = post(&engine, asked(SEANCE, ARRIVEE, GUE), Some(&key))
        .await
        .expect("queued");
    engine.run_once().await.expect("applied");
    let again = post(&engine, asked(SEANCE, ARRIVEE, GUE), Some(&key))
        .await
        .expect("replayed");
    assert!(again.replayed, "{again:?}");
    assert_eq!(again.command_id, first.command_id);
    assert_eq!(scratch.instances(ARRIVEE).await.len(), 1);
    scratch.drop().await;
}

#[tokio::test]
async fn one_instance_per_scene_the_battlemap_is_of_the_session_s_campaign_and_the_scene_of_the_session()
 {
    let scratch = Scratch::new("refused").await;
    let engine = scratch.serve().await;

    for (payload, kind, words) in [
        (
            asked(SEANCE, ARRIVEE, RECIF),
            RefusalKind::Unprocessable,
            "campagne",
        ),
        (
            asked(SEANCE, AILLEURS, GUE),
            RefusalKind::Unprocessable,
            "séance",
        ),
        (
            asked(SEANCE, ARRIVEE, PORTRAIT),
            RefusalKind::NotFound,
            "battlemap",
        ),
        (
            asked(SEANCE, ARRIVEE, NOWHERE),
            RefusalKind::NotFound,
            "battlemap",
        ),
        (
            asked(NOWHERE, ARRIVEE, GUE),
            RefusalKind::NotFound,
            "séance",
        ),
        (asked(SEANCE, NOWHERE, GUE), RefusalKind::NotFound, "scène"),
        (
            json!({ "session": SEANCE, "scene": "sc1", "battlemap": GUE }),
            RefusalKind::Unprocessable,
            "scene",
        ),
    ] {
        let message = refused(post(&engine, payload.clone(), None).await, kind);
        assert!(message.contains(words), "{payload}: {message}");
    }
    assert!(scratch.instances(ARRIVEE).await.is_empty());
    assert!(scratch.instances(AILLEURS).await.is_empty());
    scratch.drop().await;
}

#[tokio::test]
async fn a_creation_naming_another_target_than_the_scene_s_instance_is_refused() {
    let scratch = Scratch::new("target").await;
    let engine = scratch.serve().await;
    let submit = |target: String| {
        let engine = &engine;
        async move {
            let body = json!({
                "dataCapability": "table.createBattlemapInstance@1",
                "target": { "aggregate": "BattlemapInstance", "id": target },
                "payload": asked(SEANCE, ARRIVEE, GUE),
                "caller": { "by": "dm", "from": "SceneBoard", "roles": ["role:approver"] },
            });
            engine
                .submit(serde_json::from_value::<Submission>(body).expect("a submission"))
                .await
        }
    };

    let message = refused(
        submit(DEJA_INSTANCE.into()).await,
        RefusalKind::Unprocessable,
    );
    assert!(message.contains("cible"), "{message}");
    // Its own instance, in whatever case: one partition.
    let queued = submit(format!(" {} ", instance_id(ARRIVEE).to_uppercase()))
        .await
        .expect("queued");
    assert_eq!(
        queued.partition,
        format!("BattlemapInstance/{}", instance_id(ARRIVEE))
    );
    scratch.drop().await;
}

#[tokio::test]
async fn instance_isolated_the_battlemap_and_its_grid_read_the_same_after_a_creation() {
    let scratch = Scratch::new("isolated").await;
    let engine = scratch.serve().await;
    let before = scratch.battlemaps().await;

    for scene in [ARRIVEE, GUE_SCENE] {
        let result = create(&engine, asked(SEANCE, scene, GUE)).await;
        assert_eq!(result.status, State::Applied, "{result:?}");
    }
    assert_eq!(scratch.battlemaps().await, before);
    assert_eq!(scratch.instances(ARRIVEE).await.len(), 1);
    assert_eq!(scratch.instances(GUE_SCENE).await.len(), 1);
    scratch.drop().await;
}

// AC3: with the migrations the repository holds (0013 only, before 0014), a
// scene whose instance the engine deleted is created again and has one.
#[tokio::test]
async fn what_the_engine_deleted_is_not_there_any_more() {
    deleted_instance_is_created_again("tombstoned", false).await;
}

// AC2: with the tables refusing a second instance of a scene — the unique
// index standing in for migration 0014, which lands with its own task —, the
// same flow holds.
#[tokio::test]
async fn what_the_engine_deleted_is_not_there_any_more_when_the_tables_hold_one_instance_per_scene()
{
    deleted_instance_is_created_again("tombstoned_unique", true).await;
}

async fn deleted_instance_is_created_again(label: &str, one_instance_per_scene: bool) {
    let scratch = Scratch::new(label).await;
    if one_instance_per_scene {
        scratch
            .client
            .batch_execute(
                "CREATE UNIQUE INDEX create_battlemap_instance_test_scene_unique
                 ON battlemap_instance (scene_id)",
            )
            .await
            .expect("a unique scene");
    }
    // Starting the engine the first time makes the engine's own tables.
    scratch.serve().await;
    let tombstone = |partition: String| {
        let client = &scratch.client;
        async move {
            client
                .execute(
                    "INSERT INTO dataguard_rows (partition, tombstoned) VALUES ($1, true)",
                    &[&partition],
                )
                .await
                .expect("a tombstone");
        }
    };

    // The scene's instance the tables hold, deleted: the scene has none. The
    // server applies a deletion to the tables as it starts (the projection's
    // warm), so the engine starts again after the tombstone.
    tombstone(format!("BattlemapInstance/{DEJA_INSTANCE}")).await;
    let engine = scratch.serve().await;
    let result = create(&engine, asked(SEANCE, DEJA, GUE)).await;
    // AC1: the engine restarted, the new instance is Applied and the scene has one.
    assert_eq!(result.status, State::Applied, "{result:?}");
    assert_eq!(scratch.instances(DEJA).await.len(), 1);
    let tabled: i64 = scratch
        .client
        .query_one(
            "SELECT count(*) FROM battlemap_instance WHERE scene_id = $1::text::uuid",
            &[&DEJA],
        )
        .await
        .expect("the tables' instances")
        .get(0);
    assert_eq!(tabled, 1, "the tables hold the new instance alone");
    // A deleted battlemap, scene or session is no longer found.
    tombstone(format!("Media/{GUE}")).await;
    tombstone(format!("Session/{GUE_SCENE}")).await;
    tombstone(format!("Session/{AUTRE_SEANCE}")).await;
    for (payload, words) in [
        (asked(SEANCE, ARRIVEE, GUE), "battlemap"),
        (asked(SEANCE, GUE_SCENE, GUE), "scène"),
        (asked(AUTRE_SEANCE, AILLEURS, GUE), "séance"),
    ] {
        let message = refused(
            post(&engine, payload.clone(), None).await,
            RefusalKind::NotFound,
        );
        assert!(message.contains(words), "{payload}: {message}");
    }
    scratch.drop().await;
}

#[tokio::test]
async fn over_memory_no_table_holds_the_session() {
    let engine = Engine::new(
        Arc::new(MemoryStore::default()),
        Arc::new(MemoryNotifier::default()),
        Arc::new(SystemClock),
    )
    .with_aggregate(aggregate())
    .with_data_capability(manifest())
    .with_plugins(plugins());
    engine.load().await.expect("the engine loads");

    let message = refused(
        post(&engine, asked(SEANCE, ARRIVEE, GUE), None).await,
        RefusalKind::NotFound,
    );
    assert!(message.contains("séance"), "{message}");
}
