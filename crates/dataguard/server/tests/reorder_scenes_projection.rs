//! session.reorderScenes over PostgreSQL (`DATABASE_URL`): an order of a
//! session's scenes over the wire rewrites the `position` of every one of
//! them in the `scene` table, 1 to n in the order's order — scenes the
//! fixtures wrote, which no command did; an order the DataGuard rejects
//! (`scene-order-contiguous`) reaches no table; an order the tables lost is
//! written again at the next start. An order is judged against the scenes the
//! tables hold — read at a start, for an order queued before it, and again
//! when an order is submitted. Each test works in a scratch database of its
//! own, migrated as the server migrates it, dropped when it ends.

use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use dataguard_server::{Engine, Planted, planted, router};
use harness_dataguard::command::{CommandResult, State};
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
/// Valombre's session 1, and session 2.
const SEANCE: &str = "51000000-0000-4000-8000-000000000001";
const SEANCE_2: &str = "51000000-0000-4000-8000-000000000002";
/// Session 1's three scenes, ranked 1, 2 and 3 by the fixtures.
const A: &str = "5c000000-0000-4000-8000-00000000000a";
const B: &str = "5c000000-0000-4000-8000-00000000000b";
const C: &str = "5c000000-0000-4000-8000-00000000000c";
/// Session 2's scene, ranked 1.
const X: &str = "5c000000-0000-4000-8000-0000000000ff";

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

/// A database of its own, migrated, holding Valombre's two sessions and
/// their scenes, as the fixtures wrote them.
struct Scratch {
    name: String,
    url: String,
    client: Client,
}

/// The engine as the server plants it, and its wire protocol.
struct Served {
    engine: Arc<Engine>,
    app: Router,
}

impl Scratch {
    async fn new(name: &str) -> Scratch {
        let name = format!("reorder_scenes_test_{name}_{}", std::process::id());
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
                "INSERT INTO campaign (id, name, kind, sound_master_volume)
                     VALUES ('{VALOMBRE}', 'Les Cendres de Valombre', 'campagne', 0.8);
                 INSERT INTO session (id, campaign_id, position, title, status, microphone_on)
                     VALUES ('{SEANCE}', '{VALOMBRE}', 1, 'L''arrivée', 'prep', false),
                            ('{SEANCE_2}', '{VALOMBRE}', 2, 'La route', 'prep', false);
                 INSERT INTO scene (id, session_id, position, title)
                     VALUES ('{A}', '{SEANCE}', 1, 'La taverne'),
                            ('{B}', '{SEANCE}', 2, 'Le gué'),
                            ('{C}', '{SEANCE}', 3, 'La tour'),
                            ('{X}', '{SEANCE_2}', 1, 'Le moulin');"
            ))
            .await
            .expect("the fixtures");
        Scratch { name, url, client }
    }

    async fn serve(&self) -> Served {
        let Planted { engine, rows } = planted(&self.url)
            .await
            .expect("the engine over PostgreSQL");
        let engine = Arc::new(engine);
        let app = router(
            engine.clone(),
            rows,
            Arc::new(ports_llm::fake::FakeLlm::new()),
        );
        Served { engine, app }
    }

    /// The scenes of `session` as the table ranks them: `(id, position)`.
    async fn scenes(&self, session: &str) -> Vec<(String, i32)> {
        self.client
            .query(
                "SELECT id::text, position FROM scene
                 WHERE session_id = ($1::text)::uuid ORDER BY position, id",
                &[&session],
            )
            .await
            .expect("the scenes")
            .iter()
            .map(|row| (row.get(0), row.get(1)))
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

impl Served {
    /// `payload` submitted as the SceneBoard sends it, by the DM, then one
    /// pass of the applier: the answer, and the commands it settled.
    async fn reorder(&self, payload: Value) -> (StatusCode, Value, Vec<CommandResult>) {
        let (status, answer) = self.submit(payload).await;
        let settled = self.engine.run_once().await.expect("the applier ran");
        (status, answer, settled)
    }

    /// `payload` submitted as the SceneBoard sends it, by the DM, and left
    /// queued: the answer.
    async fn submit(&self, payload: Value) -> (StatusCode, Value) {
        let body = json!({
            "dataCapability": "session.reorderScenes@1",
            "payload": payload,
            "caller": { "by": "dm", "from": "SceneBoard", "roles": [] },
        });
        let request = Request::post("/commands")
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .expect("a request");
        let response = self.app.clone().oneshot(request).await.expect("an answer");
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("a body");
        let answer = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        (status, answer)
    }
}

/// `scenes`, each ranked by its place in the list.
fn ranked(scenes: &[&str]) -> Vec<(String, i32)> {
    scenes
        .iter()
        .zip(1..)
        .map(|(scene, rank)| ((*scene).to_string(), rank))
        .collect()
}

#[tokio::test]
async fn an_order_c_a_b_rewrites_every_rank_of_the_session_s_scenes_in_the_table() {
    let scratch = Scratch::new("ranks").await;
    let served = scratch.serve().await;
    let (status, answer, settled) = served
        .reorder(json!({ "session": SEANCE, "order": [C, A, B] }))
        .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{answer}");
    assert_eq!(settled.len(), 1, "{settled:?}");
    assert_eq!(settled[0].status, State::Applied, "{settled:?}");
    assert_eq!(scratch.scenes(SEANCE).await, ranked(&[C, A, B]));
    // Another session's scenes are not its.
    assert_eq!(scratch.scenes(SEANCE_2).await, ranked(&[X]));

    let (_, _, settled) = served
        .reorder(json!({ "session": SEANCE.to_uppercase(), "order": [B, C.to_uppercase(), A] }))
        .await;
    assert_eq!(settled[0].status, State::Applied, "{settled:?}");
    assert_eq!(scratch.scenes(SEANCE).await, ranked(&[B, C, A]));
    scratch.drop().await;
}

#[tokio::test]
async fn scene_order_contiguous_a_rejected_order_reaches_no_table() {
    let scratch = Scratch::new("rejected").await;
    let served = scratch.serve().await;
    for order in [json!([C, A]), json!([C, A, A]), json!([C, A, B, X])] {
        let (status, answer, settled) = served
            .reorder(json!({ "session": SEANCE, "order": order }))
            .await;
        assert_eq!(status, StatusCode::ACCEPTED, "{answer}");
        assert_eq!(settled.len(), 1, "{settled:?}");
        assert_eq!(settled[0].status, State::Rejected, "{settled:?}");
        assert_eq!(settled[0].violations, ["scene-order-contiguous"]);
        assert_eq!(scratch.scenes(SEANCE).await, ranked(&[A, B, C]), "{order}");
    }
    assert_eq!(scratch.scenes(SEANCE_2).await, ranked(&[X]));
    scratch.drop().await;
}

#[tokio::test]
async fn an_order_the_tables_lost_is_written_again_at_the_next_start() {
    let scratch = Scratch::new("repair").await;
    {
        let served = scratch.serve().await;
        let (_, _, settled) = served
            .reorder(json!({ "session": SEANCE, "order": [C, A, B] }))
            .await;
        assert_eq!(settled[0].status, State::Applied, "{settled:?}");
    }
    // The ranks lost after the engine applied the order.
    scratch
        .client
        .batch_execute(&format!(
            "UPDATE scene SET position = 1 WHERE id = '{A}';
             UPDATE scene SET position = 2 WHERE id = '{B}';
             UPDATE scene SET position = 3 WHERE id = '{C}';"
        ))
        .await
        .expect("the ranks lost");
    let served = scratch.serve().await;
    assert_eq!(scratch.scenes(SEANCE).await, ranked(&[C, A, B]));
    // The engine started again knows the session's scenes from the tables.
    let (_, _, settled) = served
        .reorder(json!({ "session": SEANCE, "order": [A, B] }))
        .await;
    assert_eq!(settled[0].status, State::Rejected, "{settled:?}");
    assert_eq!(scratch.scenes(SEANCE).await, ranked(&[C, A, B]));
    scratch.drop().await;
}

#[tokio::test]
async fn an_order_queued_before_a_restart_is_judged_against_the_tables_scenes() {
    let scratch = Scratch::new("queued").await;
    {
        let served = scratch.serve().await;
        let (status, answer) = served
            .submit(json!({ "session": SEANCE, "order": [C, A, B] }))
            .await;
        assert_eq!(status, StatusCode::ACCEPTED, "{answer}");
    }
    // Started again, the engine knows the fixtures' scenes before any
    // submission reads them.
    let served = scratch.serve().await;
    let settled = served.engine.run_once().await.expect("the applier ran");
    assert_eq!(settled.len(), 1, "{settled:?}");
    assert_eq!(settled[0].status, State::Applied, "{settled:?}");
    assert_eq!(scratch.scenes(SEANCE).await, ranked(&[C, A, B]));
    scratch.drop().await;
}

#[tokio::test]
async fn a_scene_the_tables_hold_since_the_start_is_read_when_its_session_is_reordered() {
    const D: &str = "5c000000-0000-4000-8000-00000000000d";
    let scratch = Scratch::new("refresh").await;
    let served = scratch.serve().await;
    // A scene no command wrote, the tables holding it after the start.
    scratch
        .client
        .batch_execute(&format!(
            "INSERT INTO scene (id, session_id, position, title)
                 VALUES ('{D}', '{SEANCE}', 4, 'Le pont');"
        ))
        .await
        .expect("a fourth scene");
    let (status, answer, settled) = served
        .reorder(json!({ "session": SEANCE, "order": [D, C, A, B] }))
        .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{answer}");
    assert_eq!(settled.len(), 1, "{settled:?}");
    assert_eq!(settled[0].status, State::Applied, "{settled:?}");
    assert_eq!(scratch.scenes(SEANCE).await, ranked(&[D, C, A, B]));
    scratch.drop().await;
}
