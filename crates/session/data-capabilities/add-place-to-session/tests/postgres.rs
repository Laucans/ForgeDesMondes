//! session.addPlaceToSession over PostgreSQL (`DATABASE_URL`), as the
//! server plants it: a place of the campaign joins a scene of session 4 —
//! one the fixtures wrote, which no command wrote before — after its last
//! element, its title, notes and rank kept, and the place's battlemap and
//! illustration become the session's through `session_media_view`
//! (`session-media-by-place`); asked again, a single link remains; a place
//! of another campaign or a scene of another session is refused naming
//! `scene-links-same-campaign`, and changes nothing. Each test works in a
//! scratch database of its own, migrated as the server migrates it, dropped
//! when it ends.

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
/// Valombre's session 4, which holds two scenes, and its session 5, one.
const SEANCE: &str = "51000000-0000-4000-8000-000000000004";
const SUIVANTE: &str = "51000000-0000-4000-8000-000000000005";
/// Session 4's road (rank 1) and town (rank 2, linking the mill); session
/// 5's ford.
const ROUTE: &str = "5c000000-0000-4000-8000-000000000001";
const BOURG: &str = "5c000000-0000-4000-8000-000000000002";
const GUE: &str = "5c000000-0000-4000-8000-000000000003";
const TAVERNE: &str = "e1000000-0000-4000-8000-000000000002";
const MOULIN: &str = "e1000000-0000-4000-8000-000000000005";
const ECHOPPE: &str = "e1000000-0000-4000-8000-000000000006";
const PHARE: &str = "e1000000-0000-4000-8000-000000000004";
/// The tavern's battlemap and illustration, the mill's battlemap, and an
/// illustration with no place.
const CARTE: &str = "a1000000-0000-4000-8000-000000000001";
const SALLE: &str = "a1000000-0000-4000-8000-000000000002";
const ROUE: &str = "a1000000-0000-4000-8000-000000000003";
const CIEL: &str = "a1000000-0000-4000-8000-000000000004";

/// A scene deleted, test-only: no DataCapability of the session system
/// deletes one yet.
const REMOVE_SCENE: &str = r#"{"dataCapability":"test.removeScene","owner":"dataguard","version":1,"effect":"delete","target":{"aggregate":"Session","id":"$sceneId"},"mode":"overwrite","callableBy":["*"]}"#;

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
/// as the fixtures write them — Valombre with the Taverne, the Moulin and
/// the Échoppe, Brume with its lighthouse, Valombre's session 4 with its
/// road and its town linking the mill, its session 5 with its ford, and
/// Valombre's media: the tavern's battlemap and illustration, the mill's
/// battlemap, an illustration with no place.
struct Scratch {
    name: String,
    url: String,
    client: Client,
}

impl Scratch {
    async fn new(name: &str) -> Scratch {
        let name = format!("add_place_to_session_test_{name}_{}", std::process::id());
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
        let versions: String = [TAVERNE, MOULIN, ECHOPPE, PHARE]
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
                     ('{ECHOPPE}', '{VALOMBRE}', 'lieu', 'L''Échoppe'),
                     ('{PHARE}', '{BRUME}', 'lieu', 'Le Phare');
                 {versions}
                 INSERT INTO session (id, campaign_id, position, title, status, microphone_on) VALUES
                     ('{SEANCE}', '{VALOMBRE}', 4, 'La séance 4', 'prep', false),
                     ('{SUIVANTE}', '{VALOMBRE}', 5, 'La séance 5', 'prep', false);
                 INSERT INTO scene (id, session_id, position, title, notes) VALUES
                     ('{ROUTE}', '{SEANCE}', 1, 'La route', NULL),
                     ('{BOURG}', '{SEANCE}', 2, 'Le bourg', 'Il pleut.'),
                     ('{GUE}', '{SUIVANTE}', 1, 'Le gué', NULL);
                 INSERT INTO scene_entity (scene_id, entity_id, position) VALUES
                     ('{BOURG}', '{MOULIN}', 1);
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
            .expect("Valombre, Brume, sessions 4 and 5 and their media");
        Scratch { name, url, client }
    }

    /// The engine as the server plants this unit and its target aggregate
    /// over this database: the aggregate's projection in the apply
    /// transaction, the tables lent to the Prepare.
    async fn serve(&self) -> Engine {
        let plugins = || -> Plugins {
            [aggregate_session::plugin(), add_place_to_session::plugin()]
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
        for manifest in [include_str!("../data-capability.json"), REMOVE_SCENE] {
            engine =
                engine.with_data_capability(DataCapability::parse(manifest).expect("contract F"));
        }
        engine.load().await.expect("the engine loads");
        engine
    }

    /// What `sql` answers, each row as JSON, in order.
    async fn rows(&self, sql: &str) -> Vec<Value> {
        self.client
            .query(&format!("SELECT to_jsonb(t)::text FROM ({sql}) t"), &[])
            .await
            .expect("the rows")
            .iter()
            .map(|row| serde_json::from_str(row.get::<_, &str>(0)).expect("JSON"))
            .collect()
    }

    /// Every link of every scene: its scene, its element and its rank.
    async fn links(&self) -> Vec<Value> {
        self.rows(
            "SELECT scene_id::text AS scene, entity_id::text AS place, position
             FROM scene_entity ORDER BY scene_id, position",
        )
        .await
    }

    /// Session 4's media, as `session_media_view` computes them.
    async fn media(&self) -> Vec<Value> {
        self.rows(&format!(
            "SELECT media_id::text AS media, via_place_id::text AS place
             FROM session_media_view WHERE session_id = '{SEANCE}' ORDER BY media_id"
        ))
        .await
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

/// A submission of session.addPlaceToSession, as the Médias page sends it.
async fn add(engine: &Engine, payload: Value) -> harness_dataguard::Result<Submitted> {
    let submission: Submission = serde_json::from_value(json!({
        "dataCapability": "session.addPlaceToSession@1",
        "payload": payload,
        // The critical fields' review is not what these tests judge.
        "caller": { "by": "dm", "from": "MediaLibrary", "roles": ["role:approver"] },
    }))
    .expect("a submission");
    engine.submit(submission).await
}

/// Puts a place in a scene and applies it.
async fn added(engine: &Engine, payload: Value) {
    add(engine, payload).await.expect("queued");
    for result in engine.run_once().await.expect("the applier ran") {
        assert_eq!(result.status, State::Applied, "{result:?}");
    }
}

#[tokio::test]
async fn a_place_with_a_battlemap_and_an_illustration_joins_the_scene_and_its_media_the_session() {
    let scratch = Scratch::new("media").await;
    let engine = scratch.serve().await;
    // Absent from session 4: only the mill's battlemap is the session's.
    assert_eq!(
        scratch.media().await,
        [json!({ "media": ROUE, "place": MOULIN })]
    );
    added(
        &engine,
        json!({ "session": SEANCE, "scene": BOURG, "place": TAVERNE }),
    )
    .await;
    // The tavern after the mill, at the next position.
    assert_eq!(
        scratch.links().await,
        [
            json!({ "scene": BOURG, "place": MOULIN, "position": 1 }),
            json!({ "scene": BOURG, "place": TAVERNE, "position": 2 }),
        ]
    );
    // Its battlemap and its illustration are the session's; the one with no
    // place stays none of it.
    assert_eq!(
        scratch.media().await,
        [
            json!({ "media": CARTE, "place": TAVERNE }),
            json!({ "media": SALLE, "place": TAVERNE }),
            json!({ "media": ROUE, "place": MOULIN }),
        ]
    );
    // The scene keeps its rank, its title and its notes.
    assert_eq!(
        scratch
            .rows(&format!(
                "SELECT position, title, notes FROM scene WHERE id = '{BOURG}'"
            ))
            .await,
        [json!({ "position": 2, "title": "Le bourg", "notes": "Il pleut." })]
    );
    scratch.drop().await;
}

#[tokio::test]
async fn a_place_linked_already_is_linked_once_when_it_is_asked_again() {
    let scratch = Scratch::new("again").await;
    let engine = scratch.serve().await;
    for _ in 0..2 {
        added(
            &engine,
            json!({ "session": SEANCE, "scene": BOURG, "place": TAVERNE }),
        )
        .await;
    }
    // The mill too, linked by the fixtures.
    added(
        &engine,
        json!({ "session": SEANCE, "scene": BOURG, "place": MOULIN }),
    )
    .await;
    assert_eq!(
        scratch.links().await,
        [
            json!({ "scene": BOURG, "place": MOULIN, "position": 1 }),
            json!({ "scene": BOURG, "place": TAVERNE, "position": 2 }),
        ]
    );
    scratch.drop().await;
}

#[tokio::test]
async fn with_no_scene_named_the_place_joins_the_session_s_first() {
    let scratch = Scratch::new("first").await;
    let engine = scratch.serve().await;
    added(&engine, json!({ "session": SEANCE, "place": TAVERNE })).await;
    assert_eq!(
        scratch.links().await,
        [
            json!({ "scene": ROUTE, "place": TAVERNE, "position": 1 }),
            json!({ "scene": BOURG, "place": MOULIN, "position": 1 }),
        ]
    );
    scratch.drop().await;
}

#[tokio::test]
async fn places_queued_side_by_side_are_each_linked_in_the_order_they_were_asked() {
    let scratch = Scratch::new("side").await;
    let engine = scratch.serve().await;
    for place in [TAVERNE, ECHOPPE] {
        add(
            &engine,
            json!({ "session": SEANCE, "scene": BOURG, "place": place }),
        )
        .await
        .expect("queued");
    }
    for result in engine.run_once().await.expect("the applier ran") {
        assert_eq!(result.status, State::Applied, "{result:?}");
    }
    assert_eq!(
        scratch.links().await,
        [
            json!({ "scene": BOURG, "place": MOULIN, "position": 1 }),
            json!({ "scene": BOURG, "place": TAVERNE, "position": 2 }),
            json!({ "scene": BOURG, "place": ECHOPPE, "position": 3 }),
        ]
    );
    scratch.drop().await;
}

#[tokio::test]
async fn scene_links_same_campaign_refuses_a_place_of_another_campaign_or_a_scene_of_another_session()
 {
    let scratch = Scratch::new("refused").await;
    let engine = scratch.serve().await;
    let before = scratch.links().await;
    for (scene, place) in [(BOURG, PHARE), (GUE, TAVERNE)] {
        let answer = add(
            &engine,
            json!({ "session": SEANCE, "scene": scene, "place": place }),
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
    }
    assert!(engine.run_once().await.expect("the applier ran").is_empty());
    assert_eq!(scratch.links().await, before);
    scratch.drop().await;
}

#[tokio::test]
async fn a_scene_deleted_takes_no_place_and_the_first_scene_is_then_the_next() {
    let scratch = Scratch::new("deleted").await;
    let engine = scratch.serve().await;
    let removal: Submission = serde_json::from_value(json!({
        "dataCapability": "test.removeScene",
        "payload": { "sceneId": ROUTE },
        "targetId": ROUTE,
        "caller": { "by": "dm", "from": "test", "roles": ["role:approver"] },
    }))
    .expect("a submission");
    engine.submit(removal).await.expect("queued");
    for result in engine.run_once().await.expect("the applier ran") {
        assert_eq!(result.status, State::Applied, "{result:?}");
    }
    let answer = add(
        &engine,
        json!({ "session": SEANCE, "scene": ROUTE, "place": TAVERNE }),
    )
    .await;
    match answer {
        Err(Error::Refused(refusal)) => {
            assert_eq!(refusal.kind, RefusalKind::NotFound);
            assert_eq!(refusal.message, "scène introuvable");
        }
        other => panic!("refused: {other:?}"),
    }
    // The town moved up to rank 1: it is the session's first now.
    added(&engine, json!({ "session": SEANCE, "place": TAVERNE })).await;
    assert_eq!(
        scratch.links().await,
        [
            json!({ "scene": BOURG, "place": MOULIN, "position": 1 }),
            json!({ "scene": BOURG, "place": TAVERNE, "position": 2 }),
        ]
    );
    assert!(
        scratch
            .rows(&format!("SELECT id FROM scene WHERE id = '{ROUTE}'"))
            .await
            .is_empty(),
        "the road stays deleted"
    );
    scratch.drop().await;
}
