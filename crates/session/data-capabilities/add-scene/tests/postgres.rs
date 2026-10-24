//! session.addScene over PostgreSQL (`DATABASE_URL`), as the server plants
//! it: a scene of a session the tables hold — one the fixtures wrote, which
//! no command wrote before — lands after the session's last
//! (`scene-order-contiguous`), links the places of its campaign, whose
//! media become the session's through `session_media_view`
//! (`session-media-by-place`); a place of another campaign is refused
//! naming `scene-links-same-campaign`, and changes nothing. Each test works
//! in a scratch database of its own, migrated as the server migrates it,
//! dropped when it ends.

use std::path::Path;
use std::sync::Arc;

use harness_dataguard::Error;
use harness_dataguard::aggregate::Aggregate;
use harness_dataguard::api::{Submission, Submitted};
use harness_dataguard::command::State;
use harness_dataguard::engine::Engine;
use harness_dataguard::manifest::DataCapability;
use harness_dataguard::memory::MemoryNotifier;
use harness_dataguard::plugins::{Plugins, RefusalKind};
use harness_dataguard::postgres::{PgStore, ReadOnly, SystemClock};
use harness_dataguard::store::RandomIds;
use harness_dataguard::tokio_postgres::{self, Client, NoTls};
use serde_json::{Value, json};

const VALOMBRE: &str = "c1000000-0000-4000-8000-000000000001";
const BRUME: &str = "c1000000-0000-4000-8000-000000000002";
/// Valombre's session 4, which holds two scenes already.
const SEANCE: &str = "51000000-0000-4000-8000-000000000004";
const TAVERNE: &str = "e1000000-0000-4000-8000-000000000002";
const MOULIN: &str = "e1000000-0000-4000-8000-000000000005";
const PHARE: &str = "e1000000-0000-4000-8000-000000000004";
/// The tavern's battlemap and illustration, the mill's battlemap, and an
/// illustration with no place.
const CARTE: &str = "a1000000-0000-4000-8000-000000000001";
const SALLE: &str = "a1000000-0000-4000-8000-000000000002";
const ROUE: &str = "a1000000-0000-4000-8000-000000000003";
const CIEL: &str = "a1000000-0000-4000-8000-000000000004";

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
/// as the fixtures write them — Valombre with the Taverne and the Moulin,
/// Brume with its lighthouse, Valombre's session 4 with its two scenes, and
/// Valombre's media: the tavern's battlemap and illustration, the mill's
/// battlemap, an illustration with no place.
struct Scratch {
    name: String,
    url: String,
    client: Client,
}

impl Scratch {
    async fn new(name: &str) -> Scratch {
        let name = format!("add_scene_test_{name}_{}", std::process::id());
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
        let versions: String = [TAVERNE, MOULIN, PHARE]
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
                     ('{MOULIN}', '{VALOMBRE}', 'lieu', 'Le Moulin'),
                     ('{PHARE}', '{BRUME}', 'lieu', 'Le Phare');
                 {versions}
                 INSERT INTO session (id, campaign_id, position, title, status, microphone_on)
                     VALUES ('{SEANCE}', '{VALOMBRE}', 4, 'La séance 4', 'prep', false);
                 INSERT INTO scene (session_id, position, title) VALUES
                     ('{SEANCE}', 1, 'La route'), ('{SEANCE}', 2, 'Le bourg');
                 INSERT INTO media (id, campaign_id, kind, title, file_path, width_px, height_px, place_id) VALUES
                     ('{CARTE}', '{VALOMBRE}', 'battlemap', 'La salle commune', 'media/salle.png', 960, 640, '{TAVERNE}'),
                     ('{SALLE}', '{VALOMBRE}', 'illustration', 'Le comptoir', 'media/comptoir.png', 800, 600, '{TAVERNE}'),
                     ('{ROUE}', '{VALOMBRE}', 'battlemap', 'La roue', 'media/roue.png', 960, 640, '{MOULIN}'),
                     ('{CIEL}', '{VALOMBRE}', 'illustration', 'Le ciel', 'media/ciel.png', 800, 600, NULL);
                 INSERT INTO battlemap_grid (media_id, hex_size_px, offset_x, offset_y, orientation, color, opacity)
                     VALUES ('{CARTE}', 30, 0, 0, 'pointe', '#202020', 0.4),
                            ('{ROUE}', 30, 0, 0, 'pointe', '#202020', 0.4);"
            ))
            .await
            .expect("Valombre, Brume, session 4 and its media");
        Scratch { name, url, client }
    }

    /// The engine as the server plants this unit and its target aggregate
    /// over this database: the aggregate's projection in the apply
    /// transaction, the tables lent to the Prepare.
    async fn serve(&self) -> Engine {
        let plugins = || -> Plugins {
            [aggregate_session::plugin(), add_scene::plugin()]
                .into_iter()
                .collect()
        };
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
        .with_rows(Arc::new(ReadOnly::new(connect(&self.url).await)))
        .with_ids(Arc::new(RandomIds))
        .with_aggregate(Aggregate::parse(aggregate_session::DOCUMENT).expect("contract I"))
        .with_plugins(plugins());
        engine = engine.with_data_capability(
            DataCapability::parse(include_str!("../data-capability.json")).expect("contract F"),
        );
        engine.load().await.expect("the engine loads");
        engine
    }

    /// Session 4's scenes, by rank: each one's rank and title.
    async fn scenes(&self) -> Vec<(i32, String)> {
        self.client
            .query(
                "SELECT position, title FROM scene WHERE session_id = $1::text::uuid
                 ORDER BY position, title",
                &[&SEANCE],
            )
            .await
            .expect("the scenes")
            .iter()
            .map(|row| (row.get(0), row.get(1)))
            .collect()
    }

    /// What `sql` answers, each row as JSON text, in order.
    async fn rows(&self, sql: &str) -> Vec<Value> {
        self.client
            .query(&format!("SELECT to_jsonb(t)::text FROM ({sql}) t"), &[])
            .await
            .expect("the rows")
            .iter()
            .map(|row| serde_json::from_str(row.get::<_, &str>(0)).expect("JSON"))
            .collect()
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

/// A submission of session.addScene, as the session's preparation sends it.
async fn add(engine: &Engine, payload: Value) -> harness_dataguard::Result<Submitted> {
    let submission: Submission = serde_json::from_value(json!({
        "dataCapability": "session.addScene@1",
        "payload": payload,
        // The critical fields' review is not what these tests judge.
        "caller": { "by": "dm", "from": "session.SceneBoard", "roles": ["role:approver"] },
    }))
    .expect("a submission");
    engine.submit(submission).await
}

/// Adds a scene and applies it: its id.
async fn added(engine: &Engine, payload: Value) -> String {
    let submitted = add(engine, payload).await.expect("queued");
    let result = engine.run_once().await.expect("the applier ran").remove(0);
    assert_eq!(result.status, State::Applied, "{result:?}");
    submitted
        .partition
        .strip_prefix("Session/")
        .expect("a scene's partition")
        .to_owned()
}

#[tokio::test]
async fn a_third_scene_lands_at_rank_three_after_the_two_the_session_holds() {
    let scratch = Scratch::new("third").await;
    let engine = scratch.serve().await;
    let scene = added(
        &engine,
        json!({ "session": SEANCE, "title": "Le gué", "notes": "Les bandits attendent." }),
    )
    .await;
    assert_eq!(
        scratch.scenes().await,
        [
            (1, "La route".to_owned()),
            (2, "Le bourg".to_owned()),
            (3, "Le gué".to_owned()),
        ]
    );
    assert_eq!(
        scratch
            .rows(&format!(
                "SELECT session_id::text AS session, notes FROM scene WHERE id = '{scene}'"
            ))
            .await,
        [json!({ "session": SEANCE, "notes": "Les bandits attendent." })]
    );
    scratch.drop().await;
}

#[tokio::test]
async fn a_place_of_the_campaign_is_linked_and_its_media_are_the_session_s() {
    let scratch = Scratch::new("place").await;
    let engine = scratch.serve().await;
    let scene = added(
        &engine,
        json!({ "session": SEANCE, "title": "L'arrivée", "places": [TAVERNE] }),
    )
    .await;
    assert_eq!(
        scratch
            .rows("SELECT scene_id::text AS scene, entity_id::text AS place, position FROM scene_entity")
            .await,
        [json!({ "scene": scene, "place": TAVERNE, "position": 1 })]
    );
    // The tavern's battlemap and illustration; not the mill's, nor the one
    // with no place.
    assert_eq!(
        scratch
            .rows(
                "SELECT session_id::text AS session, media_id::text AS media,
                    via_place_id::text AS place
                 FROM session_media_view ORDER BY media_id"
            )
            .await,
        [
            json!({ "session": SEANCE, "media": CARTE, "place": TAVERNE }),
            json!({ "session": SEANCE, "media": SALLE, "place": TAVERNE }),
        ]
    );
    scratch.drop().await;
}

#[tokio::test]
async fn a_place_of_another_campaign_is_refused_and_nothing_is_written() {
    let scratch = Scratch::new("phare").await;
    let engine = scratch.serve().await;
    let before = scratch.rows("SELECT * FROM scene ORDER BY position").await;
    let answer = add(
        &engine,
        json!({ "session": SEANCE, "title": "Le phare", "places": [TAVERNE, PHARE] }),
    )
    .await;
    match answer {
        Err(Error::Refused(refusal)) => {
            assert_eq!(refusal.kind, RefusalKind::Unprocessable);
            assert!(
                refusal.message.starts_with("scene-links-same-campaign"),
                "{}",
                refusal.message
            );
        }
        other => panic!("refused: {other:?}"),
    }
    assert!(engine.run_once().await.expect("the applier ran").is_empty());
    assert_eq!(
        scratch.rows("SELECT * FROM scene ORDER BY position").await,
        before
    );
    assert!(scratch.rows("SELECT * FROM scene_entity").await.is_empty());
    scratch.drop().await;
}

#[tokio::test]
async fn scenes_queued_side_by_side_are_ranked_in_the_order_the_dm_added_them() {
    let scratch = Scratch::new("order").await;
    let engine = scratch.serve().await;
    let titles = [
        "Le gué",
        "La tour",
        "Le marché",
        "Les catacombes",
        "Le retour",
    ];
    for title in titles {
        add(&engine, json!({ "session": SEANCE, "title": title }))
            .await
            .expect("queued");
        // The next one is asked a moment later.
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    for result in engine.run_once().await.expect("the applier ran") {
        assert_eq!(result.status, State::Applied, "{result:?}");
    }
    let ranked: Vec<(i32, String)> = scratch.scenes().await;
    let expected: Vec<(i32, String)> = ["La route", "Le bourg"]
        .into_iter()
        .chain(titles)
        .zip(1..)
        .map(|(title, rank)| (rank, title.to_owned()))
        .collect();
    assert_eq!(ranked, expected);
    scratch.drop().await;
}
