//! soundbox.stopAll over PostgreSQL (`DATABASE_URL`), through the wire
//! protocol: « Tout arrêter » gives every playback still playing in the
//! campaign — or in the one session named — its stop time, in the engine's
//! row and in `sound_playback`; a playback stopped already keeps its own,
//! another campaign's plays on, and with nothing playing the command
//! succeeds and writes nothing (`stop-all`). It is relative: the DM view
//! confirms nothing. Each test works in a scratch database of its own,
//! migrated by the Data layer, dropped when it ends.

use std::path::Path;
use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use dataguard_server::{Engine, Planted, planted, router};
use harness_dataguard::api::Submission;
use harness_dataguard::guard::Caller;
use harness_dataguard::manifest::DataCapability;
use serde_json::{Value, json};
use tokio_postgres::{Client, NoTls};
use tower::ServiceExt;

const VALOMBRE: &str = "c1000000-0000-4000-8000-000000000001";
const BRUME: &str = "c1000000-0000-4000-8000-000000000002";
/// Valombre's session 1, live.
const SEANCE: &str = "5e000000-0000-4000-8000-000000000001";
/// Valombre's session 2.
const CRYPTE: &str = "5e000000-0000-4000-8000-000000000002";
/// Brume's one session.
const BRUME_SEANCE: &str = "5e000000-0000-4000-8000-000000000003";
const FOULE: &str = "50000000-0000-4000-8000-000000000001";
const PORTE: &str = "50000000-0000-4000-8000-000000000002";
const VENT: &str = "50000000-0000-4000-8000-000000000003";
/// Playbacks: the crowd and the door in session 1, the crowd in the crypt,
/// the wind in Brume.
const P_FOULE: &str = "a0000000-0000-4000-8000-000000000001";
const P_PORTE: &str = "a0000000-0000-4000-8000-000000000002";
const P_CRYPTE: &str = "a0000000-0000-4000-8000-000000000003";
const P_VENT: &str = "a0000000-0000-4000-8000-000000000004";
/// A playback the DM started, still queued.
const P_QUEUED: &str = "a0000000-0000-4000-8000-000000000005";
/// A playback started, test-only: the engine's insert on its partition, as
/// soundbox.playSound queues it.
const START: &str = r#"{"dataCapability":"test.startPlayback","owner":"dataguard","version":1,"effect":"insert","target":{"aggregate":"SoundPlayback","id":"$id"},"mode":"overwrite","callableBy":["*"]}"#;
/// When the door stopped, before any « Tout arrêter ».
const EARLIER: &str = "2026-10-20 09:00:00+00";

fn url() -> String {
    std::env::var("DATABASE_URL").expect("DATABASE_URL names the harness's PostgreSQL")
}

async fn connect(config: &tokio_postgres::Config) -> Client {
    let (client, connection) = config.connect(NoTls).await.expect("PostgreSQL");
    tokio::spawn(connection);
    client
}

/// A database of its own, migrated by the Data layer: Valombre and its two
/// sessions, Brume and its one, three sounds — and nothing playing yet.
struct Scratch {
    name: String,
    url: String,
    client: Client,
}

impl Scratch {
    async fn new(name: &str) -> Scratch {
        let name = format!("stop_all_test_{name}_{}", std::process::id());
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
        client
            .batch_execute(&format!(
                "INSERT INTO campaign (id, name, kind) VALUES \
                 ('{VALOMBRE}', 'Les Cendres de Valombre', 'campagne'), ('{BRUME}', 'La Brume', 'oneshot'); \
                 INSERT INTO session (id, campaign_id, position, title, status, microphone_on) VALUES \
                 ('{SEANCE}', '{VALOMBRE}', 1, 'Le Sanglier', 'live', false), \
                 ('{CRYPTE}', '{VALOMBRE}', 2, 'La Crypte', 'prep', false), \
                 ('{BRUME_SEANCE}', '{BRUME}', 1, 'La Brume', 'live', false); \
                 INSERT INTO sound (id, campaign_id, name, file_path) VALUES \
                 ('{FOULE}', '{VALOMBRE}', 'Bruit de foule', 'sons/foule.ogg'), \
                 ('{PORTE}', '{VALOMBRE}', 'Porte qui grince', 'sons/porte.ogg'), \
                 ('{VENT}', '{BRUME}', 'Vent', 'sons/vent.ogg')"
            ))
            .await
            .expect("two campaigns, their sessions and their sounds");
        let url = scratch_url(&name);
        Scratch { name, url, client }
    }

    /// `playback` of `sound` playing in `session` since a minute ago, as the
    /// fixtures write it.
    async fn play(&self, playback: &str, session: &str, sound: &str) {
        self.client
            .execute(
                "INSERT INTO sound_playback (id, session_id, sound_id, origin, started_at) \
                 VALUES ($1::text::uuid, $2::text::uuid, $3::text::uuid, 'soundbox', now() - interval '1 minute')",
                &[&playback, &session, &sound],
            )
            .await
            .expect("a playback added");
    }

    /// `playback` stopped at `at`, as the fixtures write it.
    async fn stopped(&self, playback: &str, at: &str) {
        self.client
            .execute(
                "UPDATE sound_playback SET stopped_at = $2::text::timestamptz WHERE id = $1::text::uuid",
                &[&playback, &at],
            )
            .await
            .expect("a playback stopped");
    }

    /// When `playback` stopped, in UTC, as text; `None` while it plays.
    async fn stop_of(&self, playback: &str) -> Option<String> {
        self.client
            .query_one(
                "SELECT to_char(stopped_at AT TIME ZONE 'UTC', 'YYYY-MM-DD HH24:MI:SS') \
                 FROM sound_playback WHERE id = $1::text::uuid",
                &[&playback],
            )
            .await
            .expect("the playback read")
            .get(0)
    }

    /// Every column of every playback, as text: what a stop-all may change.
    async fn playbacks(&self) -> Vec<String> {
        self.client
            .query(
                "SELECT to_jsonb(p)::text FROM sound_playback p ORDER BY id",
                &[],
            )
            .await
            .expect("the playbacks read")
            .iter()
            .map(|row| row.get(0))
            .collect()
    }

    /// The stop the engine's row of `playback` holds, if any.
    async fn engine_stop_of(&self, playback: &str) -> Option<String> {
        self.client
            .query_opt(
                "SELECT fields ->> 'stoppedAt' FROM dataguard_rows WHERE partition = $1",
                &[&format!("SoundPlayback/{playback}")],
            )
            .await
            .expect("the engine's row read")
            .and_then(|row| row.get(0))
    }

    /// The engine as the server plants it over this database — with the
    /// test-only start of a playback — and its wire protocol.
    async fn serve(&self) -> (Arc<Engine>, Router) {
        let Planted { engine, rows } = planted(&self.url)
            .await
            .expect("the engine over PostgreSQL");
        let engine = Arc::new(
            engine.with_data_capability(DataCapability::parse(START).expect("contract F")),
        );
        let app = router(
            engine.clone(),
            rows,
            Arc::new(ports_llm::fake::FakeLlm::new()),
        );
        (engine, app)
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

/// `body` sent to `uri` — `POST` with a body, `GET` without — and the answer.
async fn call(app: &Router, uri: &str, body: Option<Value>) -> (StatusCode, Value) {
    let request = match body {
        Some(body) => Request::post(uri)
            .header("content-type", "application/json")
            .body(Body::from(body.to_string())),
        None => Request::get(uri).body(Body::empty()),
    }
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

/// « Tout arrêter » as the LiveSoundbox sends it — no role, nothing to
/// confirm — with `payload`, and the answer.
async fn stop_all(app: &Router, payload: Value) -> (StatusCode, Value) {
    let body = json!({
        "dataCapability": "soundbox.stopAll@1",
        "payload": payload,
        "caller": { "by": "dm", "from": "LiveSoundbox", "roles": [] },
    });
    call(app, "/commands", Some(body)).await
}

/// What `GET /commands/<id>` answers of the command `answer` names.
async fn result(app: &Router, answer: &Value) -> Value {
    let id = answer["commandId"].as_str().expect("a command id");
    let (status, read) = call(app, &format!("/commands/{id}"), None).await;
    assert_eq!(status, StatusCode::OK, "{read}");
    read["result"].clone()
}

#[tokio::test]
async fn stop_all_stops_every_playback_of_the_campaign_and_none_of_another() {
    let scratch = Scratch::new("campaign").await;
    scratch.play(P_FOULE, SEANCE, FOULE).await;
    scratch.play(P_PORTE, SEANCE, PORTE).await;
    scratch.play(P_CRYPTE, CRYPTE, FOULE).await;
    scratch.play(P_VENT, BRUME_SEANCE, VENT).await;
    let (engine, app) = scratch.serve().await;

    let (status, answer) = stop_all(&app, json!({ "campaign": VALOMBRE })).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{answer}");
    assert_eq!(answer["partition"], format!("SoundPlayback/{P_FOULE}"));
    assert_ne!(result(&app, &answer).await["status"], "applied");

    // One command per playback still playing, each on its own partition,
    // none waiting for a confirmation or a review.
    let settled = engine.run_once().await.expect("the applier ran");
    assert_eq!(settled.len(), 3, "{settled:?}");
    let read = result(&app, &answer).await;
    assert_eq!(read["status"], "applied", "{read}");
    for playback in [P_FOULE, P_PORTE, P_CRYPTE] {
        assert!(
            scratch.stop_of(playback).await.is_some(),
            "{playback} plays"
        );
    }
    // Every stop is the same instant: one click.
    assert_eq!(
        scratch.stop_of(P_FOULE).await,
        scratch.stop_of(P_CRYPTE).await
    );
    // Another campaign's playback plays on.
    assert_eq!(scratch.stop_of(P_VENT).await, None);
    scratch.drop().await;
}

#[tokio::test]
async fn stop_all_keeps_the_stop_a_playback_had_and_stops_nothing_twice() {
    let scratch = Scratch::new("twice").await;
    scratch.play(P_FOULE, SEANCE, FOULE).await;
    scratch.play(P_PORTE, SEANCE, PORTE).await;
    scratch.stopped(P_PORTE, EARLIER).await;
    let (engine, app) = scratch.serve().await;

    let (status, first) = stop_all(&app, json!({ "campaign": VALOMBRE })).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{first}");
    // A second click before the first is applied: its stop is queued, so
    // nothing is left to stop.
    let (status, second) = stop_all(&app, json!({ "campaign": VALOMBRE })).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{second}");
    assert_eq!(second["result"]["status"], "applied", "{second}");
    assert_eq!(second["commandId"], "", "nothing queued: {second}");

    let settled = engine.run_once().await.expect("the applier ran");
    assert_eq!(settled.len(), 1, "the crowd alone: {settled:?}");
    assert_eq!(result(&app, &first).await["status"], "applied");
    assert!(scratch.stop_of(P_FOULE).await.is_some());
    // The door keeps the instant it stopped at.
    assert_eq!(
        scratch.stop_of(P_PORTE).await.as_deref(),
        Some("2026-10-20 09:00:00")
    );
    scratch.drop().await;
}

#[tokio::test]
async fn a_playback_the_table_stopped_while_the_stop_was_queued_keeps_its_own_stop() {
    let scratch = Scratch::new("raced").await;
    scratch.play(P_FOULE, SEANCE, FOULE).await;
    let (engine, app) = scratch.serve().await;
    let (status, answer) = stop_all(&app, json!({ "campaign": VALOMBRE })).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{answer}");
    scratch.stopped(P_FOULE, EARLIER).await;
    engine.run_once().await.expect("the applier ran");
    assert_eq!(result(&app, &answer).await["status"], "applied");
    assert_eq!(
        scratch.stop_of(P_FOULE).await.as_deref(),
        Some("2026-10-20 09:00:00")
    );
    scratch.drop().await;
}

#[tokio::test]
async fn a_playback_still_queued_when_stop_all_is_clicked_is_stopped_too() {
    let scratch = Scratch::new("queued").await;
    scratch.play(P_FOULE, SEANCE, FOULE).await;
    let (engine, app) = scratch.serve().await;
    let started = |id: &str, session: &str| {
        let payload = json!({ "id": id, "sessionId": session, "soundId": PORTE,
                              "origin": "soundbox", "startedAt": "2026-10-20T09:59:00Z" });
        Submission {
            data_capability: "test.startPlayback@1".to_string(),
            payload: serde_json::from_value(payload).expect("a payload"),
            target_id: Some(id.to_string()),
            based_on: None,
            idempotency_key: None,
            caller: Caller {
                by: "dm".to_string(),
                from: "LiveSoundbox".to_string(),
                permissions: Vec::new(),
                roles: vec!["role:approver".to_string()],
            },
        }
    };
    engine
        .submit(started(P_QUEUED, &SEANCE.to_uppercase()))
        .await
        .expect("a play queued");
    // Brume's play, queued too, plays on.
    engine
        .submit(started(P_VENT, BRUME_SEANCE))
        .await
        .expect("a play queued");

    let (status, answer) = stop_all(&app, json!({ "campaign": VALOMBRE })).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{answer}");
    assert_ne!(result(&app, &answer).await["status"], "applied");
    // The two plays, the crowd's stop, and the queued play's stop behind it
    // on its partition.
    assert_eq!(engine.run_once().await.expect("the applier ran").len(), 4);
    assert_eq!(result(&app, &answer).await["status"], "applied");
    assert!(scratch.engine_stop_of(P_QUEUED).await.is_some());
    assert!(scratch.engine_stop_of(P_FOULE).await.is_some());
    assert_eq!(scratch.engine_stop_of(P_VENT).await, None);
    scratch.drop().await;
}

#[tokio::test]
async fn two_clicks_at_once_queue_one_stop_per_playback() {
    let scratch = Scratch::new("at_once").await;
    scratch.play(P_FOULE, SEANCE, FOULE).await;
    scratch.play(P_PORTE, SEANCE, PORTE).await;
    scratch.play(P_CRYPTE, CRYPTE, FOULE).await;
    let (engine, app) = scratch.serve().await;
    let (first, second) = tokio::join!(
        stop_all(&app, json!({ "campaign": VALOMBRE })),
        stop_all(&app, json!({ "campaign": VALOMBRE })),
    );
    assert_eq!(first.0, StatusCode::ACCEPTED, "{}", first.1);
    assert_eq!(second.0, StatusCode::ACCEPTED, "{}", second.1);
    let settled = engine.run_once().await.expect("the applier ran");
    assert_eq!(settled.len(), 3, "{settled:?}");
    assert!(engine.run_once().await.expect("the applier ran").is_empty());
    scratch.drop().await;
}

#[tokio::test]
async fn stop_all_with_nothing_playing_succeeds_and_writes_nothing() {
    let scratch = Scratch::new("nothing").await;
    scratch.play(P_PORTE, SEANCE, PORTE).await;
    scratch.stopped(P_PORTE, EARLIER).await;
    scratch.play(P_VENT, BRUME_SEANCE, VENT).await;
    let before = scratch.playbacks().await;
    let (engine, app) = scratch.serve().await;

    let (status, answer) = stop_all(&app, json!({ "campaign": VALOMBRE })).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{answer}");
    assert_eq!(answer["result"]["status"], "applied", "{answer}");
    assert_eq!(answer["result"]["violations"], json!([]));
    assert!(
        engine.run_once().await.expect("the applier ran").is_empty(),
        "nothing queued"
    );
    assert_eq!(scratch.playbacks().await, before);
    scratch.drop().await;
}

#[tokio::test]
async fn stop_all_of_a_session_stops_that_session_s_playbacks_alone() {
    let scratch = Scratch::new("session").await;
    scratch.play(P_FOULE, SEANCE, FOULE).await;
    scratch.play(P_CRYPTE, CRYPTE, FOULE).await;
    let (engine, app) = scratch.serve().await;

    // The campaign as the command's target, as a Micro-UI names it.
    let body = json!({
        "dataCapability": "soundbox.stopAll@1",
        "payload": { "session": CRYPTE.to_uppercase() },
        "target": { "id": VALOMBRE },
    });
    let (status, answer) = call(&app, "/commands", Some(body)).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{answer}");
    assert_eq!(engine.run_once().await.expect("the applier ran").len(), 1);
    assert_eq!(result(&app, &answer).await["status"], "applied");
    assert!(scratch.stop_of(P_CRYPTE).await.is_some());
    assert_eq!(scratch.stop_of(P_FOULE).await, None);

    // A session of another campaign stops nothing of it.
    let (_, answer) = stop_all(
        &app,
        json!({ "campaign": VALOMBRE, "session": BRUME_SEANCE }),
    )
    .await;
    assert_eq!(answer["result"]["status"], "applied", "{answer}");
    assert!(engine.run_once().await.expect("the applier ran").is_empty());
    assert_eq!(scratch.stop_of(P_FOULE).await, None);
    scratch.drop().await;
}

#[tokio::test]
async fn stop_all_naming_no_campaign_or_no_session_is_refused_before_it_is_queued() {
    let scratch = Scratch::new("refused").await;
    scratch.play(P_FOULE, SEANCE, FOULE).await;
    let (engine, app) = scratch.serve().await;
    for payload in [
        json!({}),
        json!({ "campaign": "  " }),
        json!({ "campaign": "valombre" }),
        json!({ "campaign": VALOMBRE, "session": "la crypte" }),
        json!({ "campaign": VALOMBRE, "session": 1 }),
    ] {
        let (status, answer) = stop_all(&app, payload.clone()).await;
        assert_eq!(
            status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "{payload}: {answer}"
        );
        assert!(answer["error"].is_string(), "{answer}");
    }
    assert!(engine.run_once().await.expect("the applier ran").is_empty());
    assert_eq!(scratch.stop_of(P_FOULE).await, None);
    scratch.drop().await;
}

#[tokio::test]
async fn stop_all_under_the_same_key_again_is_replayed() {
    let scratch = Scratch::new("replay").await;
    scratch.play(P_FOULE, SEANCE, FOULE).await;
    scratch.play(P_PORTE, SEANCE, PORTE).await;
    let (engine, app) = scratch.serve().await;
    let body = json!({
        "dataCapability": "soundbox.stopAll@1",
        "payload": { "campaign": VALOMBRE },
        "idempotencyKey": "tout-arreter-1",
    });
    let (_, first) = call(&app, "/commands", Some(body.clone())).await;
    assert_eq!(engine.run_once().await.expect("the applier ran").len(), 2);
    let (status, again) = call(&app, "/commands", Some(body)).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{again}");
    assert_eq!(again["commandId"], first["commandId"], "{again}");
    assert_eq!(again["replayed"], true, "{again}");
    assert!(engine.run_once().await.expect("the applier ran").is_empty());
    scratch.drop().await;
}

#[tokio::test]
async fn a_stop_the_table_lost_is_written_again_at_the_next_start() {
    let scratch = Scratch::new("repair").await;
    scratch.play(P_FOULE, SEANCE, FOULE).await;
    let (engine, app) = scratch.serve().await;
    stop_all(&app, json!({ "campaign": VALOMBRE })).await;
    engine.run_once().await.expect("the applier ran");
    let stopped = scratch.stop_of(P_FOULE).await;
    assert!(stopped.is_some());
    scratch
        .client
        .batch_execute("UPDATE sound_playback SET stopped_at = NULL")
        .await
        .expect("the stop lost");
    drop((engine, app));
    let _ = scratch.serve().await;
    assert_eq!(scratch.stop_of(P_FOULE).await, stopped);
    scratch.drop().await;
}
