//! The Session aggregate's scenes over PostgreSQL (`DATABASE_URL`), as the
//! server's store runs its projection: a scene the engine applies on
//! `Session/<its id>` reaches `scene`, ranked after its session's last
//! (`scene-order-contiguous`), and its links `scene_entity`, ranked in
//! their list's order; an element of another campaign, or a session the
//! tables do not hold, rejects the command whole (`scene-links-same-campaign`);
//! the media of a session follow its places (`session-media-by-place`). The
//! DataCapabilities here are test-only: each write is the row as given.
//! Each test works in a scratch database of its own, migrated as the server
//! migrates it, dropped when it ends.

use std::path::Path;
use std::sync::Arc;

use harness_dataguard::aggregate::Aggregate;
use harness_dataguard::api::Submission;
use harness_dataguard::command::{CommandResult, State};
use harness_dataguard::engine::Engine;
use harness_dataguard::manifest::DataCapability;
use harness_dataguard::memory::MemoryNotifier;
use harness_dataguard::plugins::Plugins;
use harness_dataguard::postgres::{PgStore, ReadOnly, SystemClock};
use harness_dataguard::tokio_postgres::{self, Client, NoTls};
use serde_json::{Value, json};

/// A scene written whole, test-only.
const ADD: &str = r#"{"dataCapability":"test.addRow","owner":"dataguard","version":1,"effect":"insert","target":{"aggregate":"Session","id":"$sceneId"},"mode":"overwrite","callableBy":["*"]}"#;
/// Fields of a scene changed, test-only.
const UPDATE: &str = r#"{"dataCapability":"test.updateRow","owner":"dataguard","version":1,"effect":"update","target":{"aggregate":"Session","id":"$sceneId"},"mode":"overwrite","callableBy":["*"]}"#;
/// A scene deleted: the engine's tombstone.
const DELETE: &str = r#"{"dataCapability":"test.deleteRow","owner":"dataguard","version":1,"effect":"delete","target":{"aggregate":"Session","id":"$sceneId"},"mode":"overwrite","callableBy":["*"]}"#;
/// A session's own row, test-only.
const ADD_SESSION: &str = r#"{"dataCapability":"test.addSession","owner":"dataguard","version":1,"effect":"insert","target":{"aggregate":"Session","id":"$sessionId"},"mode":"overwrite","callableBy":["*"]}"#;

const VALOMBRE: &str = "c1000000-0000-4000-8000-000000000001";
const BRUME: &str = "c1000000-0000-4000-8000-000000000002";
/// Valombre's session 4, which holds two scenes already.
const SEANCE: &str = "51000000-0000-4000-8000-000000000004";
const NOWHERE: &str = "51000000-0000-4000-8000-0000000000ff";
const TAVERNE: &str = "e1000000-0000-4000-8000-000000000002";
const MOULIN: &str = "e1000000-0000-4000-8000-000000000005";
const PHARE: &str = "e1000000-0000-4000-8000-000000000004";
/// The tavern's battlemap and illustration, the mill's battlemap, and an
/// illustration with no place.
const CARTE: &str = "a1000000-0000-4000-8000-000000000001";
const SALLE: &str = "a1000000-0000-4000-8000-000000000002";
const ROUE: &str = "a1000000-0000-4000-8000-000000000003";
const CIEL: &str = "a1000000-0000-4000-8000-000000000004";
/// Scenes the tests add.
const ARRIVEE: &str = "5c000000-0000-4000-8000-00000000000a";
const GUE: &str = "5c000000-0000-4000-8000-00000000000b";

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
    notifier: Arc<MemoryNotifier>,
}

impl Scratch {
    async fn new(name: &str) -> Scratch {
        let name = format!("session_scenes_test_{name}_{}", std::process::id());
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
        Scratch {
            name,
            url,
            client,
            notifier: Arc::default(),
        }
    }

    /// The engine as the server plants this aggregate over this database:
    /// its projection in the apply transaction. What it tells the authors
    /// is `self.notifier`'s.
    async fn serve(&self) -> Engine {
        let plugins = || -> Plugins { std::iter::once(aggregate_session::plugin()).collect() };
        let store = PgStore::open(connect(&self.url).await)
            .await
            .expect("the store")
            .with_projections(&plugins())
            .await
            .expect("warmed");
        let mut engine = Engine::new(
            Arc::new(store),
            self.notifier.clone(),
            Arc::new(SystemClock),
        )
        .with_rows(Arc::new(ReadOnly::new(connect(&self.url).await)))
        .with_aggregate(Aggregate::parse(aggregate_session::DOCUMENT).expect("contract I"))
        .with_plugins(plugins());
        for manifest in [ADD, UPDATE, DELETE, ADD_SESSION] {
            engine =
                engine.with_data_capability(DataCapability::parse(manifest).expect("contract F"));
        }
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

/// The scene `id` of session 4 titled `title`, linking `links`.
fn scene(id: &str, title: &str, links: &[&str]) -> Value {
    json!({
        "sceneId": id,
        "sceneSessionId": SEANCE,
        "sceneTitle": title,
        "notes": "Il pleut.",
        "entityId": links,
    })
}

/// Submits `payload` through `data_capability`, by the DM, who approves.
async fn submit(engine: &Engine, data_capability: &str, payload: Value) {
    let submission: Submission = serde_json::from_value(json!({
        "dataCapability": data_capability,
        "payload": payload,
        // The critical fields' review is not what these tests judge.
        "caller": { "by": "dm", "from": "session.prep", "roles": ["role:approver"] },
    }))
    .expect("a submission");
    engine.submit(submission).await.expect("queued");
}

/// What the applier made of every command queued.
async fn applied(engine: &Engine) -> Vec<CommandResult> {
    engine.run_once().await.expect("the applier ran")
}

#[tokio::test]
async fn scene_order_contiguous_ranks_a_scene_after_its_session_s_last_whatever_was_queued() {
    let scratch = Scratch::new("ranks").await;
    let engine = scratch.serve().await;
    // Two scenes queued side by side: neither is rejected, each takes the
    // next rank, in the order of their ids.
    submit(&engine, "test.addRow", scene(GUE, "Le gué", &[])).await;
    submit(&engine, "test.addRow", scene(ARRIVEE, "L'arrivée", &[])).await;
    for result in applied(&engine).await {
        assert_eq!(result.status, State::Applied, "{result:?}");
    }
    let ranked = |titles: [&str; 4]| -> Vec<(i32, String)> {
        titles
            .into_iter()
            .zip(1..)
            .map(|(title, rank)| (rank, title.to_owned()))
            .collect()
    };
    assert_eq!(
        scratch.scenes().await,
        ranked(["La route", "Le bourg", "L'arrivée", "Le gué"])
    );
    // A scene written again keeps its rank, and takes its new title.
    submit(
        &engine,
        "test.updateRow",
        json!({ "sceneId": ARRIVEE, "sceneTitle": "L'arrivée de nuit" }),
    )
    .await;
    assert_eq!(applied(&engine).await[0].status, State::Applied);
    assert_eq!(
        scratch.scenes().await,
        ranked(["La route", "Le bourg", "L'arrivée de nuit", "Le gué"])
    );
    // Its tombstone takes it out: the scene after it moves up one rank.
    submit(&engine, "test.deleteRow", json!({ "sceneId": ARRIVEE })).await;
    assert_eq!(applied(&engine).await[0].status, State::Applied);
    assert_eq!(
        scratch.scenes().await,
        [
            (1, "La route".to_owned()),
            (2, "Le bourg".to_owned()),
            (3, "Le gué".to_owned()),
        ]
    );
    scratch.drop().await;
}

#[tokio::test]
async fn a_scene_written_again_takes_its_row_s_notes_and_links() {
    let scratch = Scratch::new("again").await;
    let engine = scratch.serve().await;
    submit(
        &engine,
        "test.addRow",
        scene(ARRIVEE, "L'arrivée", &[TAVERNE, MOULIN]),
    )
    .await;
    assert_eq!(applied(&engine).await[0].status, State::Applied);
    submit(
        &engine,
        "test.updateRow",
        json!({ "sceneId": ARRIVEE, "notes": null, "entityId": [MOULIN] }),
    )
    .await;
    assert_eq!(applied(&engine).await[0].status, State::Applied);
    assert_eq!(
        scratch
            .rows("SELECT title, notes FROM scene WHERE position = 3")
            .await,
        [json!({ "title": "L'arrivée", "notes": null })]
    );
    // The tavern unlinked, its media with it; the mill first now.
    assert_eq!(
        scratch
            .rows("SELECT entity_id::text AS element, position FROM scene_entity")
            .await,
        [json!({ "element": MOULIN, "position": 1 })]
    );
    assert_eq!(
        scratch
            .rows("SELECT media_id::text AS media FROM session_media_view")
            .await,
        [json!({ "media": ROUE })]
    );
    scratch.drop().await;
}

#[tokio::test]
async fn session_media_by_place_attaches_the_media_of_the_places_a_scene_links() {
    let scratch = Scratch::new("media").await;
    let engine = scratch.serve().await;
    let attached = "SELECT media_id::text AS media, via_place_id::text AS place
        FROM session_media_view ORDER BY media_id";
    assert!(scratch.rows(attached).await.is_empty());
    submit(
        &engine,
        "test.addRow",
        scene(ARRIVEE, "L'arrivée", &[MOULIN, &TAVERNE.to_uppercase()]),
    )
    .await;
    assert_eq!(applied(&engine).await[0].status, State::Applied);
    assert_eq!(
        scratch
            .rows(
                "SELECT scene_id::text AS scene, entity_id::text AS element, position
                 FROM scene_entity ORDER BY position"
            )
            .await,
        [
            json!({ "scene": ARRIVEE, "element": MOULIN, "position": 1 }),
            json!({ "scene": ARRIVEE, "element": TAVERNE, "position": 2 }),
        ]
    );
    assert_eq!(
        scratch
            .rows(&format!(
                "SELECT title, notes FROM scene WHERE id = '{ARRIVEE}'"
            ))
            .await,
        [json!({ "title": "L'arrivée", "notes": "Il pleut." })]
    );
    // The tavern's two media and the mill's, never the one with no place.
    assert_eq!(
        scratch.rows(attached).await,
        [
            json!({ "media": CARTE, "place": TAVERNE }),
            json!({ "media": SALLE, "place": TAVERNE }),
            json!({ "media": ROUE, "place": MOULIN }),
        ]
    );
    scratch.drop().await;
}

#[tokio::test]
async fn scene_links_same_campaign_rejects_an_element_of_another_campaign_or_none_whole() {
    let scratch = Scratch::new("strangers").await;
    let engine = scratch.serve().await;
    for links in [vec![TAVERNE, PHARE], vec![NOWHERE]] {
        submit(&engine, "test.addRow", scene(ARRIVEE, "L'arrivée", &links)).await;
        let result = applied(&engine).await.remove(0);
        assert_eq!(result.status, State::Rejected, "{result:?}");
        let reason = scratch
            .notifier
            .sent()
            .into_iter()
            .rev()
            .find_map(|message| message.reason)
            .unwrap_or_default();
        assert!(reason.contains("scene-links-same-campaign"), "{reason}");
    }
    // A session the tables do not hold, one named by no uuid, none: each
    // rejects its command, and the applier goes on.
    for session in [json!(NOWHERE), json!("seance-4"), Value::Null] {
        let mut lost = scene(GUE, "Le gué", &[]);
        lost["sceneSessionId"] = session;
        submit(&engine, "test.addRow", lost).await;
        let result = applied(&engine).await.remove(0);
        assert_eq!(result.status, State::Rejected, "{result:?}");
    }
    // Nothing of them was written.
    assert_eq!(scratch.scenes().await.len(), 2);
    assert!(scratch.rows("SELECT * FROM scene_entity").await.is_empty());
    scratch.drop().await;
}

#[tokio::test]
async fn a_row_that_is_no_scene_s_or_a_tombstone_writes_no_scene() {
    let scratch = Scratch::new("others").await;
    let engine = scratch.serve().await;
    // A session's own row, then a scene's tombstone.
    submit(
        &engine,
        "test.addSession",
        json!({ "sessionId": NOWHERE, "sessionTitle": "La séance 5" }),
    )
    .await;
    submit(&engine, "test.deleteRow", json!({ "sceneId": ARRIVEE })).await;
    for result in applied(&engine).await {
        assert_eq!(result.status, State::Applied, "{result:?}");
    }
    assert_eq!(scratch.scenes().await.len(), 2);
    assert!(
        scratch
            .rows("SELECT * FROM session WHERE id = '51000000-0000-4000-8000-0000000000ff'")
            .await
            .is_empty()
    );
    scratch.drop().await;
}
