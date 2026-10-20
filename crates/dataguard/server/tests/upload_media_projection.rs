//! media.uploadMedia over PostgreSQL (`DATABASE_URL`): what the engine
//! applies on `Media/<id>` reaches `media` and, for a battlemap,
//! `battlemap_grid`, whose table makes a hex 5 feet; an upload's place is
//! read from the tables, the fixtures' elements included; and a start
//! projects a media the table lost. Each test works in a scratch database of
//! its own, migrated as the server migrates it, dropped when it ends.

use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use dataguard_server::{Engine, Planted, planted, router};
use harness_dataguard::command::{CommandResult, State};
use serde_json::{Value, json};
use tokio_postgres::{Client, NoTls};
use tower::ServiceExt;

const MIGRATIONS: [&str; 4] = [
    include_str!("../../../data/migrations/0002_campaign.sql"),
    include_str!("../../../data/migrations/0003_player.sql"),
    include_str!("../../../data/migrations/0004_entity.sql"),
    include_str!("../../../data/migrations/0005_media.sql"),
];

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

/// A database of its own, migrated, holding two campaigns and — as the
/// fixtures do, written by no command — Valombre's tavern, a lieu, Morlox,
/// an NPC, and La Brume's lighthouse, a lieu.
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
        let client = connect(&url).await;
        for migration in MIGRATIONS {
            client.batch_execute(migration).await.expect("migrated");
        }
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

/// `POST /commands` of media.uploadMedia by the DM, approver of their own
/// campaign's critical fields: the status and the answer.
async fn submit(app: &Router, payload: Value, key: Option<&str>) -> (StatusCode, Value) {
    let mut body = json!({
        "dataCapability": "media.uploadMedia@1",
        "payload": payload,
        "caller": { "by": "dm", "from": "media.MediaLibrary", "roles": ["role:approver"] },
    });
    if let Some(key) = key {
        body["idempotencyKey"] = json!(key);
    }
    let request = Request::post("/commands")
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .expect("a request");
    let response = app.clone().oneshot(request).await.expect("an answer");
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("a body");
    (status, serde_json::from_slice(&bytes).expect("JSON"))
}

/// Uploads `payload`: the media's id, and what the applier made of it.
async fn upload(engine: &Engine, app: &Router, payload: Value) -> (String, CommandResult) {
    let (status, answer) = submit(app, payload, None).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{answer}");
    let id = answer["partition"]
        .as_str()
        .and_then(|p| p.strip_prefix("Media/"))
        .expect("a Media partition")
        .to_owned();
    let mut settled = engine.run_once().await.expect("the applier ran");
    assert_eq!(settled.len(), 1, "{settled:?}");
    (id, settled.remove(0))
}

fn applied(result: &CommandResult) {
    assert_eq!(result.status, State::Applied, "{result:?}");
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
    let (engine, app) = s.serve().await;
    let (id, result) = upload(&engine, &app, media("battlemap", "La Taverne", None)).await;
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
    let (engine, app) = s.serve().await;
    let (id, result) = upload(&engine, &app, media("illustration", "Elowen", None)).await;
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
    let (engine, app) = s.serve().await;
    // The tavern was written by no command: the tables alone hold it.
    let (id, result) = upload(
        &engine,
        &app,
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
        let (status, answer) = submit(&app, media("illustration", place, Some(place)), None).await;
        assert_eq!(
            status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "{place}: {answer}"
        );
        assert!(
            answer["error"]
                .as_str()
                .is_some_and(|e| e.starts_with("media-place-is-lieu")),
            "{place}: {answer}"
        );
    }
    assert!(engine.run_once().await.expect("the applier ran").is_empty());
    let count = s.rows("SELECT count(*) AS media FROM media").await;
    assert_eq!(count, [json!({ "media": 1 })]);
    s.drop().await;
}

#[tokio::test]
async fn the_same_upload_twice_under_one_idempotency_key_is_one_media_row() {
    let s = Scratch::new("idempotent").await;
    let (engine, app) = s.serve().await;
    let payload = media("battlemap", "La Taverne", None);
    let (_, first) = submit(&app, payload.clone(), Some("upload-1")).await;
    let (_, second) = submit(&app, payload, Some("upload-1")).await;
    assert_eq!(second["commandId"], first["commandId"], "{second}");
    applied(&engine.run_once().await.expect("the applier ran")[0]);
    assert!(engine.run_once().await.expect("the applier ran").is_empty());
    let count = s.rows("SELECT count(*) AS media FROM media").await;
    assert_eq!(count, [json!({ "media": 1 })]);
    s.drop().await;
}

#[tokio::test]
async fn a_start_projects_a_media_the_table_lost() {
    let s = Scratch::new("repair").await;
    let (engine, app) = s.serve().await;
    let (id, result) = upload(&engine, &app, media("battlemap", "La Taverne", None)).await;
    applied(&result);
    drop((engine, app));
    // Lost after its apply: the grid goes with it (onDelete cascade).
    s.client
        .batch_execute(&format!("DELETE FROM media WHERE id = '{id}'"))
        .await
        .expect("the row lost");

    let _restarted = s.serve().await;
    let back = s
        .rows(&format!(
            "SELECT m.title, g.feet_per_hex FROM media m \
             JOIN battlemap_grid g ON g.media_id = m.id WHERE m.id = '{id}'"
        ))
        .await;
    assert_eq!(back, [json!({ "title": "La Taverne", "feet_per_hex": 5 })]);
    s.drop().await;
}

#[tokio::test]
async fn an_upload_to_a_campaign_the_tables_do_not_hold_is_refused_and_not_queued() {
    let s = Scratch::new("campaign").await;
    let (engine, app) = s.serve().await;
    let mut payload = media("battlemap", "La Taverne", None);
    payload["campaign"] = json!("c1000000-0000-4000-8000-0000000000ff");
    let (status, answer) = submit(&app, payload, None).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{answer}");
    assert!(engine.run_once().await.expect("the applier ran").is_empty());
    s.drop().await;
}
