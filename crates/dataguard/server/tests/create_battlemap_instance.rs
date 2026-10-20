//! table.createBattlemapInstance through the wire protocol, over PostgreSQL
//! (`DATABASE_URL`) as the server plants it: a creation names a session, one
//! of its scenes and a battlemap of its campaign, is judged against the
//! tables before it is queued, and once applied the scene has one instance
//! in `battlemap_instance` — and only one (`one-instance-per-scene`) — while
//! the battlemap and its grid stay as they were (`instance-isolated`). Each
//! test works in a scratch database of its own, migrated by the Data layer,
//! dropped when it ends.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use create_battlemap_instance::instance_id;
use dataguard_server::{Engine, Planted, planted, router};
use harness_dataguard::command::State;
use serde_json::{Value, json};
use tokio_postgres::{Client, NoTls};
use tower::ServiceExt;

const VALOMBRE: &str = "c1000000-0000-4000-8000-000000000001";
const BRUME: &str = "c1000000-0000-4000-8000-000000000002";
const GUE: &str = "6d000000-0000-4000-8000-000000000001";
const PORTRAIT: &str = "6d000000-0000-4000-8000-000000000002";
const FALAISE: &str = "6d000000-0000-4000-8000-000000000003";
const SEANCE: &str = "5e000000-0000-4000-8000-000000000001";
const AUTRE_SEANCE: &str = "5e000000-0000-4000-8000-000000000002";
const ARRIVEE: &str = "5c000000-0000-4000-8000-000000000001";
const DEPART: &str = "5c000000-0000-4000-8000-000000000002";
const AILLEURS: &str = "5c000000-0000-4000-8000-000000000003";
const NOWHERE: &str = "00000000-0000-4000-8000-0000000000ff";

fn url() -> String {
    std::env::var("DATABASE_URL").expect("DATABASE_URL names the harness's PostgreSQL")
}

fn migrations() -> PathBuf {
    Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../data/migrations"
    ))
    .to_path_buf()
}

async fn connect(config: &tokio_postgres::Config) -> Client {
    let (client, connection) = config.connect(NoTls).await.expect("PostgreSQL");
    tokio::spawn(connection);
    client
}

/// A database of its own.
struct Scratch {
    client: Client,
    name: String,
    url: String,
}

impl Scratch {
    /// Migrated by the Data layer, holding Valombre and its session's two
    /// scenes, the ford's battlemap and its grid, a portrait that is no
    /// battlemap, and Brume, another campaign, with its session, its scene
    /// and its cliff's battlemap.
    async fn new(name: &str) -> Scratch {
        let name = format!(
            "create_battlemap_instance_test_{name}_{}",
            std::process::id()
        );
        let config: tokio_postgres::Config = url().parse().expect("a PostgreSQL URL");
        let admin = connect(&config).await;
        for sql in [
            format!("DROP DATABASE IF EXISTS {name} WITH (FORCE)"),
            format!("CREATE DATABASE {name}"),
        ] {
            admin.batch_execute(&sql).await.expect("a scratch database");
        }
        let mut client = connect(config.clone().dbname(&name)).await;
        data::migrate(&mut client, &migrations())
            .await
            .expect("migrated by the Data layer");
        client
            .batch_execute(&format!(
                "INSERT INTO campaign (id, name, kind) VALUES
                     ('{VALOMBRE}', 'Les Cendres de Valombre', 'campagne'),
                     ('{BRUME}', 'La Brume', 'oneshot');
                 INSERT INTO media (id, campaign_id, kind, title, file_path, width_px, height_px) VALUES
                     ('{GUE}', '{VALOMBRE}', 'battlemap', 'Le gué', 'gue.png', 2048, 1536),
                     ('{PORTRAIT}', '{VALOMBRE}', 'illustration', 'Morlox', 'morlox.png', 512, 512),
                     ('{FALAISE}', '{BRUME}', 'battlemap', 'La falaise', 'falaise.png', 1024, 1024);
                 INSERT INTO battlemap_grid (media_id, hex_size_px, offset_x, offset_y, orientation, color, opacity) VALUES
                     ('{GUE}', 64, 0, 0, 'pointe', '#202020', 0.4),
                     ('{FALAISE}', 48, 3, 5, 'plat', '#ffffff', 0.6);
                 INSERT INTO session (id, campaign_id, position, title, status, microphone_on) VALUES
                     ('{SEANCE}', '{VALOMBRE}', 1, 'Le Sanglier', 'prep', false),
                     ('{AUTRE_SEANCE}', '{BRUME}', 1, 'Dans la brume', 'prep', false);
                 INSERT INTO scene (id, session_id, position, title) VALUES
                     ('{ARRIVEE}', '{SEANCE}', 1, 'Arrivée'),
                     ('{DEPART}', '{SEANCE}', 2, 'Départ'),
                     ('{AILLEURS}', '{AUTRE_SEANCE}', 1, 'Ailleurs');"
            ))
            .await
            .expect("the campaigns, their media, sessions and scenes");
        let url = naming(&name);
        Scratch { client, name, url }
    }

    /// The engine as the server plants it over this database, and its wire
    /// protocol.
    async fn serve(&self) -> (Arc<Engine>, Router) {
        let Planted { engine, rows } = planted(&self.url)
            .await
            .expect("the engine over PostgreSQL");
        let engine = Arc::new(engine);
        let app = router(
            engine.clone(),
            rows,
            Arc::new(ports_llm::fake::FakeLlm::new()),
        );
        (engine, app)
    }

    /// The instances of the scene `scene`: `(id, battlemap, session)`.
    async fn instances(&self, scene: &str) -> Vec<(String, String, String)> {
        self.client
            .query(
                "SELECT id::text, battlemap_media_id::text, session_id::text
                 FROM battlemap_instance WHERE scene_id = $1::text::uuid ORDER BY id",
                &[&scene],
            )
            .await
            .expect("battlemap_instance")
            .iter()
            .map(|row| (row.get(0), row.get(1), row.get(2)))
            .collect()
    }

    /// Every row of `media` and `battlemap_grid`, as JSON.
    async fn battlemaps(&self) -> Vec<String> {
        self.client
            .query(
                "SELECT to_jsonb(m)::text FROM media m
                 UNION ALL SELECT to_jsonb(g)::text FROM battlemap_grid g
                 ORDER BY 1",
                &[],
            )
            .await
            .expect("media and battlemap_grid")
            .iter()
            .map(|row| row.get(0))
            .collect()
    }

    async fn drop(self) {
        drop(self.client);
        let config: tokio_postgres::Config = url().parse().expect("a PostgreSQL URL");
        connect(&config)
            .await
            .batch_execute(&format!("DROP DATABASE {} WITH (FORCE)", self.name))
            .await
            .expect("the scratch database dropped");
    }
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

/// `POST /commands` of table.createBattlemapInstance from the SceneBoard,
/// with `payload`: the status and the answer.
async fn create(app: &Router, payload: Value) -> (StatusCode, Value) {
    let body = json!({
        "dataCapability": "table.createBattlemapInstance@1",
        "payload": payload,
        "caller": { "by": "dm", "from": "session.SceneBoard", "roles": ["role:approver"] },
    });
    let request = Request::post("/commands")
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .expect("a request");
    let response = app.clone().oneshot(request).await.expect("an answer");
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("a body");
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

/// The ford's battlemap for the arrival scene of the session.
fn the_ford_for_the_arrival() -> Value {
    json!({ "session": SEANCE, "scene": ARRIVEE, "battlemap": GUE })
}

/// One pass of the single applier, which must apply what it settles.
async fn applied(engine: &Engine) {
    let settled = engine.run_once().await.expect("the applier ran");
    assert_eq!(settled.len(), 1, "{settled:?}");
    assert_eq!(settled[0].status, State::Applied, "{settled:?}");
}

#[tokio::test]
async fn a_creation_gives_the_scene_its_one_instance_and_leaves_the_battlemap_as_it_was() {
    let s = Scratch::new("created").await;
    let (engine, app) = s.serve().await;
    let before = s.battlemaps().await;
    let (status, answer) = create(&app, the_ford_for_the_arrival()).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{answer}");
    let id = instance_id(ARRIVEE);
    assert_eq!(answer["partition"], format!("BattlemapInstance/{id}"));
    applied(&engine).await;
    assert_eq!(
        s.instances(ARRIVEE).await,
        [(id, GUE.to_owned(), SEANCE.to_owned())]
    );
    // instance-isolated: the original battlemap and its grid are untouched.
    assert_eq!(s.battlemaps().await, before);
    // The other scene has none.
    assert!(s.instances(DEPART).await.is_empty());
    s.drop().await;
}

#[tokio::test]
async fn one_instance_per_scene_a_second_creation_is_refused_applied_or_queued() {
    let s = Scratch::new("second").await;
    let (engine, app) = s.serve().await;
    let (status, answer) = create(&app, the_ford_for_the_arrival()).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{answer}");
    // Queued, not applied yet: the scene has its instance already.
    let (status, answer) = create(&app, the_ford_for_the_arrival()).await;
    assert_eq!(status, StatusCode::CONFLICT, "{answer}");
    assert!(
        answer["error"]
            .as_str()
            .is_some_and(|e| e.contains("one-instance-per-scene")),
        "{answer}"
    );
    applied(&engine).await;
    // Applied: still refused, and the scene keeps its one instance.
    let (status, answer) = create(&app, the_ford_for_the_arrival()).await;
    assert_eq!(status, StatusCode::CONFLICT, "{answer}");
    assert!(engine.run_once().await.expect("the applier ran").is_empty());
    assert_eq!(s.instances(ARRIVEE).await.len(), 1);
    // The same battlemap serves another scene of the session.
    let (status, answer) = create(
        &app,
        json!({ "session": SEANCE, "scene": DEPART, "battlemap": GUE }),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{answer}");
    applied(&engine).await;
    assert_eq!(s.instances(DEPART).await.len(), 1);
    s.drop().await;
}

#[tokio::test]
async fn one_instance_per_scene_an_instance_the_tables_hold_already_refuses_a_creation() {
    let s = Scratch::new("tabled").await;
    s.client
        .batch_execute(&format!(
            "INSERT INTO battlemap_instance (battlemap_media_id, session_id, scene_id)
             VALUES ('{GUE}', '{SEANCE}', '{ARRIVEE}')"
        ))
        .await
        .expect("an instance the engine never wrote");
    let (engine, app) = s.serve().await;
    let (status, answer) = create(&app, the_ford_for_the_arrival()).await;
    assert_eq!(status, StatusCode::CONFLICT, "{answer}");
    assert!(engine.run_once().await.expect("the applier ran").is_empty());
    assert_eq!(s.instances(ARRIVEE).await.len(), 1);
    s.drop().await;
}

#[tokio::test]
async fn a_battlemap_of_another_campaign_or_a_scene_of_another_session_is_refused() {
    let s = Scratch::new("refused").await;
    let (engine, app) = s.serve().await;
    let before = s.battlemaps().await;
    for (payload, expected) in [
        // Brume's cliff, for a scene of a Valombre session.
        (
            json!({ "session": SEANCE, "scene": ARRIVEE, "battlemap": FALAISE }),
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
        // Brume's scene, said to be the Valombre session's.
        (
            json!({ "session": SEANCE, "scene": AILLEURS, "battlemap": GUE }),
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
        // A media that is no battlemap.
        (
            json!({ "session": SEANCE, "scene": ARRIVEE, "battlemap": PORTRAIT }),
            StatusCode::NOT_FOUND,
        ),
        // What the tables do not hold.
        (
            json!({ "session": NOWHERE, "scene": ARRIVEE, "battlemap": GUE }),
            StatusCode::NOT_FOUND,
        ),
        (
            json!({ "session": SEANCE, "scene": NOWHERE, "battlemap": GUE }),
            StatusCode::NOT_FOUND,
        ),
        (
            json!({ "session": SEANCE, "scene": ARRIVEE, "battlemap": NOWHERE }),
            StatusCode::NOT_FOUND,
        ),
        // No id.
        (
            json!({ "session": SEANCE, "scene": ARRIVEE, "battlemap": "bm-gue" }),
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
        (
            json!({ "session": SEANCE, "battlemap": GUE }),
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
    ] {
        let (status, answer) = create(&app, payload.clone()).await;
        assert_eq!(status, expected, "{payload}: {answer}");
        assert!(answer["error"].is_string(), "{answer}");
    }
    // None was queued: nothing changed.
    assert!(engine.run_once().await.expect("the applier ran").is_empty());
    for scene in [ARRIVEE, AILLEURS] {
        assert!(s.instances(scene).await.is_empty());
    }
    assert_eq!(s.battlemaps().await, before);
    s.drop().await;
}

#[tokio::test]
async fn an_instance_the_table_lost_is_projected_again_at_the_next_start() {
    let s = Scratch::new("repaired").await;
    let (engine, app) = s.serve().await;
    let (status, answer) = create(&app, the_ford_for_the_arrival()).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{answer}");
    applied(&engine).await;
    drop((engine, app));
    s.client
        .batch_execute("DELETE FROM battlemap_instance")
        .await
        .expect("the projection lost");
    assert!(s.instances(ARRIVEE).await.is_empty());
    let _restarted = s.serve().await;
    assert_eq!(
        s.instances(ARRIVEE).await,
        [(instance_id(ARRIVEE), GUE.to_owned(), SEANCE.to_owned())]
    );
    s.drop().await;
}
