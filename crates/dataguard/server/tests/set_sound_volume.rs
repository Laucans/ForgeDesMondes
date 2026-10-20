//! soundbox.setSoundVolume through the wire protocol, over the in-memory
//! engine as the server plants it: one sound's slider, in percent, is made
//! the fraction it writes on that sound's partition — one soundbox.addSound
//! added — judged by `sound-volume-range` and applied, and nothing else of
//! the sound, nor any other sound, changes. The `sound` table it reaches is
//! `set_sound_volume_projection.rs`'s.

use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use dataguard_server::{Engine, MEMORY, Planted, Reads, Rows, planted, router_reading};
use harness_dataguard::api::Submission;
use harness_dataguard::command::{CommandResult, State};
use harness_dataguard::guard::Caller;
use harness_dataguard::manifest::DataCapability;
use serde_json::{Map, Value, json};
use tower::ServiceExt;

const VALOMBRE: &str = "c1000000-0000-4000-8000-000000000001";

/// A campaign added, test-only: no DataCapability of this milestone writes
/// one, and the in-memory engine holds none of the fixtures'.
const ADD_CAMPAIGN: &str = r#"{"dataCapability":"test.addCampaign","owner":"dataguard","version":1,"effect":"insert","target":{"aggregate":"Campaign","id":"$id"},"mode":"overwrite","callableBy":["*"]}"#;

/// The engine as the server plants it, its rows and its wire protocol,
/// Valombre added.
struct Served {
    engine: Arc<Engine>,
    rows: Rows,
    app: Router,
}

async fn served() -> Served {
    let Planted { engine, rows } = planted(MEMORY).await.expect("the in-memory engine");
    let engine = Arc::new(
        engine.with_data_capability(DataCapability::parse(ADD_CAMPAIGN).expect("contract F")),
    );
    let app = app(&engine, &rows, Reads::none());
    let served = Served { engine, rows, app };
    let Value::Object(valombre) =
        json!({ "id": VALOMBRE, "name": "Les Cendres de Valombre", "kind": "campagne" })
    else {
        unreachable!("an object");
    };
    served
        .engine
        .submit(Submission {
            data_capability: "test.addCampaign".to_string(),
            payload: valombre.into_iter().collect(),
            target_id: None,
            based_on: None,
            idempotency_key: None,
            caller: Caller {
                by: "dm".to_string(),
                from: "SoundLibrary".to_string(),
                permissions: Vec::new(),
                roles: vec!["role:approver".to_string()],
            },
        })
        .await
        .expect("queued");
    applied(&served.settled().await);
    served
}

/// The wire protocol over `engine`, its ids named on the wire as `reads`
/// names them.
fn app(engine: &Arc<Engine>, rows: &Rows, reads: Reads) -> Router {
    router_reading(
        engine.clone(),
        rows.clone(),
        Arc::new(ports_llm::fake::FakeLlm::new()),
        Arc::new(reads),
    )
}

/// The DM's live soundbox calling as `by`.
fn caller(by: &str) -> Value {
    json!({ "by": by, "from": "LiveSoundbox", "roles": ["role:approver"] })
}

impl Served {
    /// The sound `name` of the soundbox at 80 %, added by soundbox.addSound
    /// and applied: its id.
    async fn sound(&self, name: &str) -> String {
        let (status, answer) = post(
            &self.app,
            json!({
                "dataCapability": "soundbox.addSound@1",
                "payload": {
                    "campaign": VALOMBRE,
                    "name": name,
                    "tags": ["bois"],
                    "volume": 80,
                    "file": "sons/porte.ogg",
                },
                "caller": { "by": "dm", "from": "SoundLibrary", "roles": ["role:approver"] },
            }),
        )
        .await;
        assert_eq!(status, StatusCode::ACCEPTED, "{answer}");
        applied(&self.settled().await);
        answer["partition"]
            .as_str()
            .and_then(|p| p.strip_prefix("Sound/"))
            .unwrap_or_else(|| panic!("a Sound partition: {answer}"))
            .to_owned()
    }

    /// `POST /commands` of soundbox.setSoundVolume with `payload` on
    /// `target`, as the slider sends it, by `by`.
    async fn slide(&self, by: &str, payload: Value, target: Option<&str>) -> (StatusCode, Value) {
        slide(&self.app, by, payload, target).await
    }

    /// One pass of the single applier, which settles one command.
    async fn settled(&self) -> CommandResult {
        let mut settled = self.engine.run_once().await.expect("the applier ran");
        assert_eq!(settled.len(), 1, "{settled:?}");
        settled.remove(0)
    }

    /// The row of `sound` as the engine holds it, the commands queued there
    /// applied.
    async fn row(&self, sound: &str) -> Map<String, Value> {
        self.rows
            .projected(&format!("Sound/{sound}"))
            .await
            .expect("the row")
            .into_iter()
            .collect()
    }

    /// Whether the applier finds nothing queued.
    async fn nothing_queued(&self) -> bool {
        self.engine
            .run_once()
            .await
            .expect("the applier ran")
            .is_empty()
    }
}

async fn slide(
    app: &Router,
    by: &str,
    payload: Value,
    target: Option<&str>,
) -> (StatusCode, Value) {
    let mut body = json!({
        "dataCapability": "soundbox.setSoundVolume@1",
        "payload": payload,
        "caller": caller(by),
    });
    if let Some(target) = target {
        body["target"] = json!({ "aggregate": "Sound", "id": target });
    }
    post(app, body).await
}

async fn post(app: &Router, body: Value) -> (StatusCode, Value) {
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

fn applied(result: &CommandResult) {
    assert_eq!(result.status, State::Applied, "{result:?}");
}

#[tokio::test]
async fn a_sound_at_80_set_to_40_stores_0_4_and_no_other_field_changes() {
    let s = served().await;
    let door = s.sound("Porte").await;
    let crowd = s.sound("Foule").await;
    let (before, other) = (s.row(&door).await, s.row(&crowd).await);
    assert_eq!(before["volume"], 0.8);
    let (status, answer) = s.slide("dm", json!({ "volume": 40 }), Some(&door)).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{answer}");
    assert_eq!(answer["partition"], format!("Sound/{door}"));
    applied(&s.settled().await);
    let mut expected = before;
    expected.insert("volume".into(), json!(0.4));
    assert_eq!(s.row(&door).await, expected);
    // Only that sound gets quieter.
    assert_eq!(s.row(&crowd).await, other);
}

#[tokio::test]
async fn sound_volume_range_rejects_a_volume_of_101_and_nothing_changes() {
    let s = served().await;
    let door = s.sound("Porte").await;
    let before = s.row(&door).await;
    for volume in [json!(101), json!(-1), Value::Null, json!("fort")] {
        let (status, answer) = s
            .slide("dm", json!({ "volume": volume }), Some(&door))
            .await;
        assert_eq!(status, StatusCode::ACCEPTED, "{volume}: {answer}");
        let result = s.settled().await;
        assert_eq!(result.status, State::Rejected, "{volume}: {result:?}");
        assert_eq!(result.violations, ["sound-volume-range"], "{volume}");
        assert_eq!(s.row(&door).await, before, "{volume}");
    }
}

#[tokio::test]
async fn two_moves_of_one_slider_in_flight_queue_and_the_last_one_stays_unconfirmed() {
    let s = served().await;
    let door = s.sound("Porte").await;
    let (_, first) = s.slide("dm", json!({ "volume": 60 }), Some(&door)).await;
    let (status, second) = s.slide("dm-2", json!({ "volume": 20 }), Some(&door)).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{second}");
    assert_eq!(first["partition"], second["partition"]);
    // Never refused, never parked for a confirmation: a slider overwrites.
    for answer in [&first, &second] {
        assert_eq!(answer["result"]["status"], "queued", "{answer}");
    }
    let settled = s.engine.run_once().await.expect("the applier ran");
    assert_eq!(settled.len(), 2, "{settled:?}");
    settled.iter().for_each(applied);
    assert_eq!(s.row(&door).await["volume"], 0.2);
}

#[tokio::test]
async fn a_sound_named_in_capitals_or_by_the_soundbox_s_slug_is_the_same_sound() {
    let s = served().await;
    let door = s.sound("Porte").await;
    let (status, answer) = s
        .slide("dm", json!({ "volume": 55 }), Some(&door.to_uppercase()))
        .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{answer}");
    applied(&s.settled().await);
    assert_eq!(s.row(&door).await["volume"], 0.55);
    // The fixtures' reader names the sound by its slug, and so does its slider.
    let named = app(
        &s.engine,
        &s.rows,
        Reads::none().named([("snd-porte".to_string(), door.clone())]),
    );
    for (slug, volume) in [(" snd-porte ", 25), ("SND-Porte", 35)] {
        let (status, answer) = slide(&named, "dm", json!({ "volume": volume }), Some(slug)).await;
        assert_eq!(status, StatusCode::ACCEPTED, "{slug}: {answer}");
        assert_eq!(answer["partition"], format!("Sound/{door}"), "{slug}");
        applied(&s.settled().await);
        assert_eq!(
            s.row(&door).await["volume"],
            f64::from(volume) / 100.0,
            "{slug}"
        );
    }
}

#[tokio::test]
async fn a_sound_the_engine_does_not_hold_is_refused_404_and_nothing_is_queued() {
    let s = served().await;
    s.sound("Porte").await;
    let other = "50000000-0000-4000-8000-0000000000ff";
    for target in [Some(other), Some("  "), None] {
        let (status, answer) = s.slide("dm", json!({ "volume": 40 }), target).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{target:?}: {answer}");
        assert_eq!(answer, json!({ "error": "ce son n'existe pas" }));
    }
    assert!(s.nothing_queued().await);
    assert!(s.row(other).await.is_empty());
}
