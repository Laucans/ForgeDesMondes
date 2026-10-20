//! session.addScene over PostgreSQL (`DATABASE_URL`): what the engine
//! applies on `Session/<scene id>` reaches `scene` and `scene_entity`,
//! ranked after the scenes the tables already hold; the media of a place it
//! links follow it to the session through `session_media_view`, never
//! written; a place of another campaign is rejected and reaches nothing; a
//! start adds a scene the tables lost, knows a session only the engine
//! holds, and judges again a scene queued before it; and a session the
//! tables take after the start takes scenes. Each test works in a scratch
//! database of its own, migrated as the server migrates it, dropped when it
//! ends.

use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use dataguard_server::{Engine, Planted, planted, router};
use harness_dataguard::api::Submission;
use harness_dataguard::command::{CommandResult, State};
use harness_dataguard::guard::Caller;
use harness_dataguard::manifest::DataCapability;
use serde_json::{Value, json};
use tokio_postgres::{Client, NoTls};
use tower::ServiceExt;

const MIGRATIONS: [&str; 5] = [
    include_str!("../../../data/migrations/0002_campaign.sql"),
    include_str!("../../../data/migrations/0003_player.sql"),
    include_str!("../../../data/migrations/0004_entity.sql"),
    include_str!("../../../data/migrations/0005_media.sql"),
    include_str!("../../../data/migrations/0006_session.sql"),
];

const VALOMBRE: &str = "c1000000-0000-4000-8000-000000000001";
const BRUME: &str = "c1000000-0000-4000-8000-000000000002";
/// Valombre's places, and Brume's.
const TAVERNE: &str = "e1000000-0000-4000-8000-000000000003";
const TOUR: &str = "e1000000-0000-4000-8000-000000000004";
const PHARE: &str = "e1000000-0000-4000-8000-000000000009";
/// Valombre's session 4, and the two scenes the tables hold of it.
const SEANCE: &str = "51000000-0000-4000-8000-000000000004";
/// Valombre's session 5, which no fixture writes.
const SEANCE_5: &str = "51000000-0000-4000-8000-000000000005";
const ARRIVEE: &str = "5c000000-0000-4000-8000-000000000001";
const ROUTE: &str = "5c000000-0000-4000-8000-000000000002";
/// Two illustrations of the tavern, one of the tower, one at no place.
const SALLE: &str = "d1000000-0000-4000-8000-000000000001";
const GASPARD: &str = "d1000000-0000-4000-8000-000000000002";
const DONJON: &str = "d1000000-0000-4000-8000-000000000003";
const BLASON: &str = "d1000000-0000-4000-8000-000000000004";

/// A session written whole: a stand-in for session.createSession, which no
/// DataCapability of this branch is yet, and whose row no projection writes.
const CREATE_SESSION: &str = r#"{"dataCapability":"test.createSession","owner":"dataguard","version":1,"effect":"insert","target":{"aggregate":"Session","id":"$sessionId"},"mode":"overwrite","callableBy":["*"]}"#;

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

/// A database of its own, migrated, holding what the fixtures would: two
/// campaigns and their places, Valombre's session 4 with two scenes, and
/// four media.
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
        let client = connect(&url).await;
        for migration in MIGRATIONS {
            client.batch_execute(migration).await.expect("migrated");
        }
        client
            .batch_execute(&format!(
                "INSERT INTO campaign (id, name, kind) VALUES
                    ('{VALOMBRE}', 'Les Cendres de Valombre', 'campagne'),
                    ('{BRUME}', 'La Brume', 'campagne');
                 INSERT INTO entity (id, campaign_id, type, name) VALUES
                    ('{TAVERNE}', '{VALOMBRE}', 'lieu', 'La Taverne du Sanglier'),
                    ('{TOUR}', '{VALOMBRE}', 'lieu', 'La Tour d''Ébène'),
                    ('{PHARE}', '{BRUME}', 'lieu', 'Le Phare');
                 INSERT INTO entity_version (entity_id, version_number, source, snapshot, diff) VALUES
                    ('{TAVERNE}', 1, 'dm', '{{}}', '[]'),
                    ('{TOUR}', 1, 'dm', '{{}}', '[]'),
                    ('{PHARE}', 1, 'dm', '{{}}', '[]');
                 INSERT INTO session (id, campaign_id, position, title, status, microphone_on)
                    VALUES ('{SEANCE}', '{VALOMBRE}', 4, 'Le gué', 'prep', false);
                 INSERT INTO scene (id, session_id, position, title) VALUES
                    ('{ARRIVEE}', '{SEANCE}', 1, 'La taverne'),
                    ('{ROUTE}', '{SEANCE}', 2, 'La route');
                 INSERT INTO media (id, campaign_id, kind, title, file_path, width_px, height_px, place_id) VALUES
                    ('{SALLE}', '{VALOMBRE}', 'illustration', 'La salle', 'salle.png', 900, 600, '{TAVERNE}'),
                    ('{GASPARD}', '{VALOMBRE}', 'illustration', 'Gaspard', 'gaspard.png', 600, 900, '{TAVERNE}'),
                    ('{DONJON}', '{VALOMBRE}', 'illustration', 'Le donjon', 'donjon.png', 900, 600, '{TOUR}'),
                    ('{BLASON}', '{VALOMBRE}', 'illustration', 'Le blason', 'blason.png', 600, 600, NULL);"
            ))
            .await
            .expect("the fixtures' rows");
        Scratch { name, url, client }
    }

    /// The engine as the server plants it over this database, and its wire
    /// protocol.
    async fn serve(&self) -> (Arc<Engine>, Router) {
        let Planted { engine, rows } = planted(&self.url)
            .await
            .expect("the engine over PostgreSQL");
        let manifest = DataCapability::parse(CREATE_SESSION).expect("contract F");
        let engine = Arc::new(engine.with_data_capability(manifest));
        let app = router(
            engine.clone(),
            rows,
            Arc::new(ports_llm::fake::FakeLlm::new()),
        );
        (engine, app)
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

    /// The rank the engine's row of `scene` holds.
    async fn rank(&self, scene: &str) -> Value {
        let ranks = self
            .rows(&format!(
                "SELECT fields -> 'scenePosition' AS rank FROM dataguard_rows
                    WHERE partition = 'Session/{scene}'"
            ))
            .await;
        ranks
            .first()
            .map_or(Value::Null, |rank| rank["rank"].clone())
    }

    /// Session 4's scenes, by rank.
    async fn scenes(&self) -> Vec<Value> {
        self.rows(&format!(
            "SELECT id::text, position, title, notes FROM scene
                WHERE session_id = '{SEANCE}' ORDER BY position"
        ))
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

/// Submits a scene of `payload` as the DM, approver of their own campaign:
/// the scene's id and its command's, queued.
async fn submit(app: &Router, payload: Value) -> (String, String) {
    let body = json!({
        "dataCapability": "session.addScene@1",
        "payload": payload,
        "caller": { "by": "dm", "from": "session.SceneBoard", "roles": ["role:approver"] },
    });
    let request = Request::post("/commands")
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .expect("a request");
    let response = app.clone().oneshot(request).await.expect("an answer");
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("a body");
    let answer: Value = serde_json::from_slice(&bytes).expect("JSON");
    let scene = answer["partition"]
        .as_str()
        .and_then(|p| p.strip_prefix("Session/"))
        .unwrap_or_else(|| panic!("a Session partition: {answer}"))
        .to_owned();
    let command = answer["commandId"]
        .as_str()
        .unwrap_or_else(|| panic!("a command: {answer}"))
        .to_owned();
    (scene, command)
}

/// Adds a scene of `payload` as the DM: the scene's id, and what the
/// applier made of it.
async fn add(engine: &Engine, app: &Router, payload: Value) -> (String, CommandResult) {
    let (scene, _) = submit(app, payload).await;
    let mut settled = engine.run_once().await.expect("the applier ran");
    assert_eq!(settled.len(), 1, "{settled:?}");
    (scene, settled.remove(0))
}

fn applied(result: &CommandResult) {
    assert_eq!(result.status, State::Applied, "{result:?}");
}

#[tokio::test]
async fn a_scene_lands_third_after_the_two_the_tables_hold() {
    let scratch = Scratch::new("third").await;
    let (engine, app) = scratch.serve().await;
    let (scene, result) = add(
        &engine,
        &app,
        json!({ "session": SEANCE, "title": "Le gué", "notes": "Trois bandits sur la rive est." }),
    )
    .await;
    applied(&result);
    assert_eq!(
        scratch.scenes().await,
        [
            json!({ "id": ARRIVEE, "position": 1, "title": "La taverne", "notes": null }),
            json!({ "id": ROUTE, "position": 2, "title": "La route", "notes": null }),
            json!({ "id": scene, "position": 3, "title": "Le gué", "notes": "Trois bandits sur la rive est." }),
        ]
    );
    scratch.drop().await;
}

#[tokio::test]
async fn session_media_by_place_attaches_the_media_of_a_place_the_new_scene_links() {
    let scratch = Scratch::new("media").await;
    let (engine, app) = scratch.serve().await;
    let attached = format!(
        "SELECT media_id::text, via_place_id::text FROM session_media_view
            WHERE session_id = '{SEANCE}' ORDER BY media_id"
    );
    assert!(scratch.rows(&attached).await.is_empty());
    let (scene, result) = add(
        &engine,
        &app,
        json!({ "session": SEANCE, "title": "La taverne", "places": [TAVERNE] }),
    )
    .await;
    applied(&result);
    assert_eq!(
        scratch
            .rows(&format!(
                "SELECT scene_id::text, entity_id::text, position FROM scene_entity
                    WHERE scene_id = '{scene}'"
            ))
            .await,
        [json!({ "scene_id": scene, "entity_id": TAVERNE, "position": 1 })]
    );
    // The tavern's two media, through the view: not the tower's, nor the one
    // at no place, which stays valid.
    assert_eq!(
        scratch.rows(&attached).await,
        [
            json!({ "media_id": SALLE, "via_place_id": TAVERNE }),
            json!({ "media_id": GASPARD, "via_place_id": TAVERNE }),
        ]
    );
    scratch.drop().await;
}

#[tokio::test]
async fn scene_links_same_campaign_rejects_a_place_of_another_campaign_and_nothing_reaches_the_tables()
 {
    let scratch = Scratch::new("brume").await;
    let (engine, app) = scratch.serve().await;
    let (_, result) = add(
        &engine,
        &app,
        json!({ "session": SEANCE, "title": "Le phare", "places": [TAVERNE, PHARE] }),
    )
    .await;
    assert_eq!(result.status, State::Rejected, "{result:?}");
    assert_eq!(result.violations, ["scene-links-same-campaign"]);
    assert_eq!(scratch.scenes().await.len(), 2);
    assert!(scratch.rows("SELECT * FROM scene_entity").await.is_empty());
    scratch.drop().await;
}

#[tokio::test]
async fn a_start_adds_a_scene_the_tables_lost_with_its_places() {
    let scratch = Scratch::new("repair").await;
    let (engine, app) = scratch.serve().await;
    let (scene, result) = add(
        &engine,
        &app,
        json!({ "session": SEANCE, "title": "La tour", "places": [TOUR] }),
    )
    .await;
    applied(&result);
    drop((engine, app));
    scratch
        .client
        .batch_execute(&format!("DELETE FROM scene WHERE id = '{scene}'"))
        .await
        .expect("the scene lost");
    let (engine, app) = scratch.serve().await;
    let scenes = scratch.scenes().await;
    assert_eq!(scenes.len(), 3, "{scenes:?}");
    assert_eq!(scenes[2]["id"], scene);
    assert_eq!(scenes[2]["position"], 3);
    assert_eq!(
        scratch
            .rows(&format!(
                "SELECT entity_id::text FROM scene_entity WHERE scene_id = '{scene}'"
            ))
            .await,
        [json!({ "entity_id": TOUR })]
    );
    // And the next scene, judged against what the start took, is the fourth.
    let (_, result) = add(
        &engine,
        &app,
        json!({ "session": SEANCE, "title": "Le gué" }),
    )
    .await;
    applied(&result);
    assert_eq!(scratch.scenes().await[3]["position"], 4);
    scratch.drop().await;
}

#[tokio::test]
async fn a_session_the_tables_take_after_the_start_takes_scenes_after_its_last() {
    let scratch = Scratch::new("later").await;
    let (engine, app) = scratch.serve().await;
    // Written by no command, after the server started: the fixtures'.
    scratch
        .client
        .batch_execute(&format!(
            "INSERT INTO session (id, campaign_id, position, title, status, microphone_on)
                VALUES ('{SEANCE_5}', '{VALOMBRE}', 5, 'La forêt', 'prep', false);
             INSERT INTO scene (id, session_id, position, title)
                VALUES ('5c000000-0000-4000-8000-000000000005', '{SEANCE_5}', 1, 'L''orée');"
        ))
        .await
        .expect("session 5");
    let (scene, result) = add(
        &engine,
        &app,
        json!({ "session": SEANCE_5, "title": "La clairière", "places": [TAVERNE] }),
    )
    .await;
    applied(&result);
    assert_eq!(scratch.rank(&scene).await, 2);
    assert_eq!(
        scratch
            .rows(&format!("SELECT position FROM scene WHERE id = '{scene}'"))
            .await,
        [json!({ "position": 2 })]
    );
    scratch.drop().await;
}

#[tokio::test]
async fn a_session_only_the_engine_holds_takes_scenes_after_its_last_across_a_start() {
    let scratch = Scratch::new("engine").await;
    let first = {
        let (engine, app) = scratch.serve().await;
        let Value::Object(payload) = json!({
            "sessionId": SEANCE_5,
            "sessionCampaignId": VALOMBRE,
            "sessionPosition": 5,
            "sessionTitle": "La forêt",
            "sessionStatus": "prep",
            "microphoneOn": false,
        }) else {
            unreachable!()
        };
        engine
            .submit(Submission {
                data_capability: "test.createSession".to_string(),
                payload: payload.into_iter().collect(),
                target_id: None,
                based_on: None,
                idempotency_key: None,
                caller: Caller {
                    by: "dm".to_string(),
                    from: "session.SessionList".to_string(),
                    permissions: Vec::new(),
                    roles: vec!["role:approver".to_string()],
                },
            })
            .await
            .expect("queued");
        let settled = engine.run_once().await.expect("the applier ran");
        assert_eq!(settled.len(), 1, "{settled:?}");
        applied(&settled[0]);
        let (scene, result) = add(
            &engine,
            &app,
            json!({ "session": SEANCE_5, "title": "L'orée" }),
        )
        .await;
        applied(&result);
        scene
    };
    // Neither the session nor its scene is in the tables: the engine alone
    // holds them.
    assert!(
        scratch
            .rows(&format!("SELECT id FROM session WHERE id = '{SEANCE_5}'"))
            .await
            .is_empty()
    );
    assert_eq!(scratch.rank(&first).await, 1);
    let (engine, app) = scratch.serve().await;
    let (scene, result) = add(
        &engine,
        &app,
        json!({ "session": SEANCE_5, "title": "La clairière" }),
    )
    .await;
    applied(&result);
    assert_eq!(scratch.rank(&scene).await, 2);
    scratch.drop().await;
}

#[tokio::test]
async fn a_scene_queued_before_a_start_is_judged_again_on_its_places() {
    let scratch = Scratch::new("queued").await;
    let (ours, theirs) = {
        let (_engine, app) = scratch.serve().await;
        let ours = submit(
            &app,
            json!({ "session": SEANCE, "title": "La tour", "places": [TOUR] }),
        )
        .await;
        let theirs = submit(
            &app,
            json!({ "session": SEANCE, "title": "Le phare", "places": [PHARE] }),
        )
        .await;
        // The server stops before its applier runs.
        (ours, theirs)
    };
    let (engine, _app) = scratch.serve().await;
    let settled = engine.run_once().await.expect("the applier ran");
    assert_eq!(settled.len(), 2, "{settled:?}");
    let status = |(_, command): &(String, String)| {
        settled
            .iter()
            .find(|result| &result.command_id == command)
            .map(|result| (result.status, result.violations.clone()))
    };
    assert_eq!(status(&ours), Some((State::Applied, Vec::new())));
    assert_eq!(
        status(&theirs),
        Some((
            State::Rejected,
            vec!["scene-links-same-campaign".to_string()]
        ))
    );
    assert_eq!(
        scratch
            .rows(&format!(
                "SELECT entity_id::text FROM scene_entity WHERE scene_id = '{}'",
                ours.0
            ))
            .await,
        [json!({ "entity_id": TOUR })]
    );
    assert_eq!(scratch.scenes().await.len(), 3);
    scratch.drop().await;
}
