//! soundbox.addSound through the wire protocol, over the in-memory engine as
//! the server plants it: an addition is given its id, its tags normalised
//! (SoundTag@1) and its volume a fraction of 1, then judged by the Sound
//! aggregate's invariants and applied — or refused before it is queued when
//! its campaign is unknown. The table it reaches is
//! `add_sound_projection.rs`'s.

use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use dataguard_server::{Engine, MEMORY, Planted, Rows, planted, router};
use harness_dataguard::api::Submission;
use harness_dataguard::command::{CommandResult, State};
use harness_dataguard::guard::Caller;
use harness_dataguard::manifest::DataCapability;
use serde_json::{Map, Value, json};
use tower::ServiceExt;

const VALOMBRE: &str = "c1000000-0000-4000-8000-000000000001";
const NOWHERE: &str = "c1000000-0000-4000-8000-0000000000ff";

/// A campaign added, test-only: no DataCapability of this milestone writes
/// one, and the in-memory engine holds none of the fixtures'.
const ADD_CAMPAIGN: &str = r#"{"dataCapability":"test.addCampaign","owner":"dataguard","version":1,"effect":"insert","target":{"aggregate":"Campaign","id":"$id"},"mode":"overwrite","callableBy":["*"]}"#;

/// The DM, who approves their own library's critical fields (the sound's
/// id, campaign and file).
fn dm() -> Caller {
    Caller {
        by: "dm".to_string(),
        from: "SoundLibrary".to_string(),
        permissions: Vec::new(),
        roles: vec!["role:approver".to_string()],
    }
}

/// The engine as the server plants it, its rows and its wire protocol,
/// Valombre added.
struct Served {
    engine: Arc<Engine>,
    rows: Rows,
    app: Router,
}

async fn served() -> Served {
    let Planted { engine, rows } = planted(MEMORY).await.expect("the in-memory engine");
    let engine =
        engine.with_data_capability(DataCapability::parse(ADD_CAMPAIGN).expect("contract F"));
    let engine = Arc::new(engine);
    let app = router(
        engine.clone(),
        rows.clone(),
        Arc::new(ports_llm::fake::FakeLlm::new()),
    );
    let served = Served { engine, rows, app };
    let valombre = json!({ "id": VALOMBRE, "name": "Les Cendres de Valombre", "kind": "campagne" });
    let Value::Object(valombre) = valombre else {
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
            caller: dm(),
        })
        .await
        .expect("queued");
    applied(&served.settled().await);
    served
}

impl Served {
    /// `POST /commands` of soundbox.addSound by the DM's library: the status
    /// and the answer, as JSON.
    async fn add(&self, payload: Value) -> (StatusCode, Value) {
        let request = Request::post("/commands")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "dataCapability": "soundbox.addSound@1",
                    "payload": payload,
                    "caller": { "by": "dm", "from": "SoundLibrary", "roles": ["role:approver"] },
                })
                .to_string(),
            ))
            .expect("a request");
        let response = self.app.clone().oneshot(request).await.expect("an answer");
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("a body");
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    /// Adds `payload`: the sound's id, and what the applier made of it.
    async fn added(&self, payload: Value) -> (String, CommandResult) {
        let (status, answer) = self.add(payload).await;
        assert_eq!(status, StatusCode::ACCEPTED, "{answer}");
        let sound = answer["partition"]
            .as_str()
            .and_then(|p| p.strip_prefix("Sound/"))
            .unwrap_or_else(|| panic!("a Sound partition: {answer}"))
            .to_owned();
        (sound, self.settled().await)
    }

    /// One pass of the single applier, which settles one command.
    async fn settled(&self) -> CommandResult {
        let mut settled = self.engine.run_once().await.expect("the applier ran");
        assert_eq!(settled.len(), 1, "{settled:?}");
        settled.remove(0)
    }

    /// The row the engine holds for `sound`.
    async fn row(&self, sound: &str) -> Map<String, Value> {
        self.rows
            .projected(&format!("Sound/{sound}"))
            .await
            .expect("the row")
            .into_iter()
            .collect()
    }

    /// Nothing is left to apply: the queue is empty.
    async fn nothing_queued(&self) {
        let settled = self.engine.run_once().await.expect("the applier ran");
        assert!(settled.is_empty(), "{settled:?}");
    }
}

fn applied(result: &CommandResult) {
    assert_eq!(result.status, State::Applied, "{result:?}");
}

/// The crowd's murmur of the issue, its fields as `with` changes them.
fn crowd(with: Value) -> Value {
    let mut payload = json!({
        "campaign": VALOMBRE,
        "name": "Bruit de foule",
        "tags": ["Taverne"],
        "volume": 55,
        "file": "son.ogg",
    });
    for (field, value) in with.as_object().expect("an object") {
        payload[field] = value.clone();
    }
    payload
}

#[tokio::test]
async fn a_sound_is_added_with_its_tags_normalised_and_its_volume_a_fraction() {
    let served = served().await;
    let (sound, result) = served.added(crowd(json!({}))).await;
    applied(&result);
    let row = served.row(&sound).await;
    assert_eq!(row["id"], json!(sound));
    assert_eq!(row["campaignId"], json!(VALOMBRE));
    assert_eq!(row["name"], json!("Bruit de foule"));
    assert_eq!(row["filePath"], json!("son.ogg"));
    assert_eq!(row["tags"], json!(["taverne"]));
    assert_eq!(row["volume"], json!(0.55));
    assert!(
        row["createdAt"]
            .as_str()
            .is_some_and(|at| at.ends_with('Z'))
    );
}

#[tokio::test]
async fn tags_bois_and_bois_with_a_space_are_stored_as_one_tag() {
    let served = served().await;
    let (sound, result) = served
        .added(crowd(json!({ "tags": ["Bois", "bois "] })))
        .await;
    applied(&result);
    assert_eq!(served.row(&sound).await["tags"], json!(["bois"]));
}

#[tokio::test]
async fn each_addition_is_a_sound_of_its_own() {
    let served = served().await;
    let (first, result) = served.added(crowd(json!({}))).await;
    applied(&result);
    let (second, result) = served.added(crowd(json!({}))).await;
    applied(&result);
    assert_ne!(first, second);
    assert_eq!(served.row(&first).await["name"], json!("Bruit de foule"));
}

#[tokio::test]
async fn a_volume_of_120_is_rejected_by_sound_volume_range_and_nothing_is_written() {
    let served = served().await;
    let (sound, result) = served.added(crowd(json!({ "volume": 120 }))).await;
    assert_eq!(result.status, State::Rejected, "{result:?}");
    assert_eq!(result.violations, ["sound-volume-range"], "{result:?}");
    assert!(served.row(&sound).await.is_empty(), "the sound was written");
}

#[tokio::test]
async fn tags_that_are_no_list_are_rejected_by_tags_normalized() {
    let served = served().await;
    let (sound, result) = served.added(crowd(json!({ "tags": "Taverne" }))).await;
    assert_eq!(result.status, State::Rejected, "{result:?}");
    assert_eq!(result.violations, ["tags-normalized"], "{result:?}");
    assert!(served.row(&sound).await.is_empty(), "the sound was written");
}

#[tokio::test]
async fn a_sound_of_an_unknown_campaign_is_refused_404_and_never_queued() {
    let served = served().await;
    for campaign in [json!(NOWHERE), json!(null)] {
        let (status, answer) = served.add(crowd(json!({ "campaign": campaign }))).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{answer}");
        assert_eq!(answer["error"], json!("cette campagne n'existe pas"));
    }
    served.nothing_queued().await;
}

#[tokio::test]
async fn a_sound_with_no_name_or_no_file_is_refused_422_and_never_queued() {
    let served = served().await;
    for (with, why) in [
        (json!({ "name": " " }), "un son a besoin d'un nom"),
        (json!({ "file": null }), "un son a besoin de son fichier"),
    ] {
        let (status, answer) = served.add(crowd(with)).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{answer}");
        assert_eq!(answer["error"], json!(why));
    }
    served.nothing_queued().await;
}
