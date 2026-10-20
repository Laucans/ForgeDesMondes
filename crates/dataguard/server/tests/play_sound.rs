//! soundbox.playSound over PostgreSQL (`DATABASE_URL`): a sound triggered
//! from the soundbox and another from the pill both reach `sound_playback`,
//! playing together in the campaign's live session; a trigger with no
//! session to play in, or naming another campaign's, is refused and nothing
//! is written; a trigger from the player side — its source, or its caller —
//! is rejected; a key submitted again replays its one playback; a playback
//! the table lost is projected again at a start.
//! Each test works in a scratch database of its own, migrated by the Data
//! layer, dropped when it ends.

use std::path::Path;
use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use dataguard_server::{Engine, Planted, planted, router};
use harness_dataguard::command::State;
use serde_json::{Value, json};
use tokio_postgres::{Client, NoTls};
use tower::ServiceExt;

const VALOMBRE: &str = "c1000000-0000-4000-8000-000000000001";
const ONESHOT: &str = "c1000000-0000-4000-8000-000000000002";
const PREPAREE: &str = "5e000000-0000-4000-8000-000000000001";
const LIVE: &str = "5e000000-0000-4000-8000-000000000002";
const AILLEURS: &str = "5e000000-0000-4000-8000-000000000003";
const FOULE: &str = "50000000-0000-4000-8000-0000000000b1";
const PORTE: &str = "50000000-0000-4000-8000-0000000000b2";
const SANS_SEANCE: &str = "50000000-0000-4000-8000-0000000000b3";
const CHOPE: &str = "50000000-0000-4000-8000-0000000000b4";
const A_VENIR: &str = "5e000000-0000-4000-8000-000000000004";

/// Valombre's playbacks: the sound's name, the session, the origin, and
/// whether it still plays, sorted.
const PLAYBACKS: &str = "SELECT s.name, p.session_id::text, p.origin, p.stopped_at IS NULL
    FROM sound_playback p JOIN sound s ON s.id = p.sound_id ORDER BY 1, 2, 3";

fn url() -> String {
    std::env::var("DATABASE_URL").expect("DATABASE_URL names the harness's PostgreSQL")
}

async fn connect(config: &tokio_postgres::Config) -> Client {
    let (client, connection) = config.connect(NoTls).await.expect("PostgreSQL");
    tokio::spawn(connection);
    client
}

/// A database of its own, migrated by the Data layer: Valombre, a session in
/// preparation and a live one, and two sounds of its soundbox; a oneshot of
/// another session; a campaign with a sound and no session at all; and one
/// with a sound, a session to come and a later one closed, none live.
struct Scratch {
    name: String,
    url: String,
    client: Client,
}

impl Scratch {
    async fn new(name: &str) -> Scratch {
        let name = format!("play_sound_test_{name}_{}", std::process::id());
        let config: tokio_postgres::Config = url().parse().expect("a PostgreSQL URL");
        let admin = connect(&config).await;
        for sql in [
            format!("DROP DATABASE IF EXISTS {name} WITH (FORCE)"),
            format!("CREATE DATABASE {name}"),
        ] {
            admin.batch_execute(&sql).await.expect("a scratch database");
        }
        let mut scratch = config.clone();
        scratch.dbname(&name);
        let mut client = connect(&scratch).await;
        let migrations = concat!(env!("CARGO_MANIFEST_DIR"), "/../../data/migrations");
        data::migrate(&mut client, Path::new(migrations))
            .await
            .expect("migrated by the Data layer");
        let silent = "c1000000-0000-4000-8000-000000000003";
        let finie = "c1000000-0000-4000-8000-000000000004";
        client
            .batch_execute(&format!(
                "INSERT INTO campaign (id, name, kind) VALUES \
                 ('{VALOMBRE}', 'Les Cendres de Valombre', 'campagne'), \
                 ('{ONESHOT}', 'La Nuit du Gué', 'oneshot'), \
                 ('{silent}', 'Sans séance', 'campagne'), \
                 ('{finie}', 'Entre deux séances', 'campagne'); \
                 INSERT INTO session (id, campaign_id, position, title, status, microphone_on) VALUES \
                 ('{LIVE}', '{VALOMBRE}', 1, 'Le Sanglier', 'live', false), \
                 ('{PREPAREE}', '{VALOMBRE}', 2, 'La Crypte', 'prep', false), \
                 ('{AILLEURS}', '{ONESHOT}', 1, 'La Nuit du Gué', 'prep', false), \
                 ('{A_VENIR}', '{finie}', 1, 'À venir', 'prep', false), \
                 ('5e000000-0000-4000-8000-000000000005', '{finie}', 2, 'Close', 'closed', false); \
                 INSERT INTO sound (id, campaign_id, name, file_path) VALUES \
                 ('{FOULE}', '{VALOMBRE}', 'Bruit de foule', 'sons/foule.ogg'), \
                 ('{PORTE}', '{VALOMBRE}', 'Porte qui grince', 'sons/porte.ogg'), \
                 ('{SANS_SEANCE}', '{silent}', 'Silence', 'sons/silence.ogg'), \
                 ('{CHOPE}', '{finie}', 'Chope qui se brise', 'sons/chope.ogg')"
            ))
            .await
            .expect("the campaigns, their sessions and their sounds");
        let url = scratch_url(&name);
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

    async fn playbacks(&self) -> Vec<(String, String, String, bool)> {
        self.client
            .query(PLAYBACKS, &[])
            .await
            .expect("the playbacks read")
            .iter()
            .map(|row| (row.get(0), row.get(1), row.get(2), row.get(3)))
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
fn scratch_url(name: &str) -> String {
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

/// Triggers `sound` with `payload` as the DM from the LiveSoundbox, approver
/// of their own campaign: the status and the answer.
async fn trigger(app: &Router, sound: &str, payload: Value) -> (StatusCode, Value) {
    submit(app, sound, payload, None).await
}

/// Triggers `sound` as [`trigger`] does, under the idempotency key `key` if
/// any.
async fn submit(
    app: &Router,
    sound: &str,
    payload: Value,
    key: Option<&str>,
) -> (StatusCode, Value) {
    let mut body = json!({
        "dataCapability": "soundbox.playSound@1",
        "payload": payload,
        "target": { "id": sound },
        "caller": { "by": "dm", "from": "LiveSoundbox", "roles": ["role:approver"] },
    });
    if let Some(key) = key {
        body["idempotencyKey"] = json!(key);
    }
    send(app, body).await
}

/// Posts `body` to the wire protocol: the status, and the answer.
async fn send(app: &Router, body: Value) -> (StatusCode, Value) {
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

#[tokio::test]
async fn simultaneous_playback_two_sounds_triggered_play_together_in_the_live_session() {
    let scratch = Scratch::new("simultaneous").await;
    let (engine, app) = scratch.serve().await;
    // « Bruit de foule » from the soundbox, in the rule test's call shape.
    let (status, answer) = trigger(&app, FOULE, json!({})).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{answer}");
    let applied = engine.run_once().await.expect("the applier ran");
    assert_eq!(applied.len(), 1, "{applied:?}");
    // « Porte qui grince » from the pill: the crowd keeps playing.
    let (status, answer) = trigger(&app, PORTE, json!({ "source": "suggestion" })).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{answer}");
    let applied = engine.run_once().await.expect("the applier ran");
    assert_eq!(applied.len(), 1, "{applied:?}");
    assert_eq!(applied[0].status, State::Applied, "{applied:?}");
    assert_eq!(
        scratch.playbacks().await,
        [
            (
                "Bruit de foule".to_string(),
                LIVE.to_string(),
                "soundbox".to_string(),
                true
            ),
            (
                "Porte qui grince".to_string(),
                LIVE.to_string(),
                "pastille".to_string(),
                true
            ),
        ]
    );
    scratch.drop().await;
}

#[tokio::test]
async fn dm_side_only_a_trigger_from_the_player_side_is_rejected_and_writes_nothing() {
    let scratch = Scratch::new("player_side").await;
    let (engine, app) = scratch.serve().await;
    let (status, answer) = trigger(&app, PORTE, json!({ "source": "player" })).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{answer}");
    let applied = engine.run_once().await.expect("the applier ran");
    let [result] = applied.as_slice() else {
        panic!("one command: {applied:?}");
    };
    assert_eq!(result.status, State::Rejected, "{result:?}");
    assert_eq!(result.violations, ["dm-side-only"], "{result:?}");
    assert!(scratch.playbacks().await.is_empty());
    scratch.drop().await;
}

#[tokio::test]
async fn dm_side_only_a_player_caller_with_no_source_is_rejected_and_writes_nothing() {
    let scratch = Scratch::new("player_caller").await;
    let (engine, app) = scratch.serve().await;
    let body = json!({
        "dataCapability": "soundbox.playSound@1",
        "payload": {},
        "target": { "id": PORTE },
        "caller": { "by": "player", "from": "LiveSoundbox", "roles": ["role:approver"] },
    });
    let (status, answer) = send(&app, body).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{answer}");
    let applied = engine.run_once().await.expect("the applier ran");
    let [result] = applied.as_slice() else {
        panic!("one command: {applied:?}");
    };
    assert_eq!(result.status, State::Rejected, "{result:?}");
    assert_eq!(result.violations, ["dm-side-only"], "{result:?}");
    assert!(scratch.playbacks().await.is_empty());
    scratch.drop().await;
}

#[tokio::test]
async fn a_session_named_is_played_in_and_another_campaign_s_is_refused() {
    let scratch = Scratch::new("named_session").await;
    let (engine, app) = scratch.serve().await;
    let (status, answer) = trigger(&app, PORTE, json!({ "session": PREPAREE })).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{answer}");
    engine.run_once().await.expect("the applier ran");
    let (status, answer) = trigger(&app, PORTE, json!({ "session": AILLEURS })).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{answer}");
    assert_eq!(
        answer,
        json!({ "error": "cette séance n'est pas de la campagne du son" })
    );
    let unknown = "5e000000-0000-4000-8000-0000000000ff";
    let (status, answer) = trigger(&app, PORTE, json!({ "session": unknown })).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{answer}");
    assert_eq!(answer, json!({ "error": "cette séance n'existe pas" }));
    assert_eq!(
        scratch.playbacks().await,
        [(
            "Porte qui grince".to_string(),
            PREPAREE.to_string(),
            "soundbox".to_string(),
            true
        )]
    );
    scratch.drop().await;
}

#[tokio::test]
async fn with_no_live_session_a_sound_plays_in_the_latest_one_not_closed() {
    let scratch = Scratch::new("not_closed").await;
    let (engine, app) = scratch.serve().await;
    let (status, answer) = trigger(&app, CHOPE, json!({})).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{answer}");
    engine.run_once().await.expect("the applier ran");
    assert_eq!(
        scratch.playbacks().await,
        [(
            "Chope qui se brise".to_string(),
            A_VENIR.to_string(),
            "soundbox".to_string(),
            true
        )]
    );
    scratch.drop().await;
}

#[tokio::test]
async fn a_sound_with_no_session_to_play_in_or_none_at_all_is_refused() {
    let scratch = Scratch::new("no_session").await;
    let (_engine, app) = scratch.serve().await;
    let (status, answer) = trigger(&app, SANS_SEANCE, json!({})).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{answer}");
    assert_eq!(
        answer,
        json!({ "error": "la campagne de ce son n'a aucune séance" })
    );
    let unknown = "50000000-0000-4000-8000-0000000000ff";
    let (status, answer) = trigger(&app, unknown, json!({})).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{answer}");
    assert_eq!(answer, json!({ "error": "ce son n'existe pas" }));
    assert!(scratch.playbacks().await.is_empty());
    scratch.drop().await;
}

#[tokio::test]
async fn a_key_seen_first_is_prepared_and_the_same_key_again_replays_one_playback() {
    let scratch = Scratch::new("keyed").await;
    let (engine, app) = scratch.serve().await;
    // A key never submitted is no replay: the trigger is judged, and refused.
    let unknown = "50000000-0000-4000-8000-0000000000ff";
    let (status, answer) = submit(&app, unknown, json!({}), Some("play-unknown")).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{answer}");
    let (status, first) = submit(&app, FOULE, json!({}), Some("play-foule")).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{first}");
    let (status, second) = submit(&app, FOULE, json!({}), Some("play-foule")).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{second}");
    assert_eq!(second["replayed"], true, "{second}");
    assert_eq!(second["commandId"], first["commandId"]);
    engine.run_once().await.expect("the applier ran");
    assert!(engine.run_once().await.expect("the applier ran").is_empty());
    assert_eq!(
        scratch.playbacks().await,
        [(
            "Bruit de foule".to_string(),
            LIVE.to_string(),
            "soundbox".to_string(),
            true
        )]
    );
    scratch.drop().await;
}

#[tokio::test]
async fn a_playback_the_table_lost_is_projected_again_at_a_start() {
    let scratch = Scratch::new("repair").await;
    {
        let (engine, app) = scratch.serve().await;
        let (status, answer) = trigger(&app, FOULE, json!({})).await;
        assert_eq!(status, StatusCode::ACCEPTED, "{answer}");
        engine.run_once().await.expect("the applier ran");
    }
    scratch
        .client
        .batch_execute("DELETE FROM sound_playback")
        .await
        .expect("the playback lost");
    let _ = scratch.serve().await;
    assert_eq!(
        scratch.playbacks().await,
        [(
            "Bruit de foule".to_string(),
            LIVE.to_string(),
            "soundbox".to_string(),
            true
        )]
    );
    scratch.drop().await;
}
