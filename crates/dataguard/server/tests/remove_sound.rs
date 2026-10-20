//! soundbox.removeSound through the wire protocol, over the in-memory engine
//! as the server plants it: a removal is checked against the sound's row —
//! one soundbox.addSound added — queued on the sound's partition and applied
//! as the engine's tombstone. The `sound` table it reaches, and the readers
//! that no longer see it, are `remove_sound_projection.rs`'s.

use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use dataguard_server::{Engine, MEMORY, Planted, Rows, planted, router};
use harness_dataguard::api::Submission;
use harness_dataguard::command::{CommandResult, State};
use harness_dataguard::guard::Caller;
use harness_dataguard::manifest::DataCapability;
use serde_json::{Value, json};
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
    let app = router(
        engine.clone(),
        rows.clone(),
        Arc::new(ports_llm::fake::FakeLlm::new()),
    );
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

/// The DM's library calling as `by`, approver of their own sounds' critical
/// fields.
fn caller(by: &str) -> Value {
    json!({ "by": by, "from": "SoundLibrary", "roles": ["role:approver"] })
}

impl Served {
    /// The sound `name`, added by soundbox.addSound and queued: its id.
    async fn queue_sound(&self, name: &str) -> String {
        let (status, answer) = self
            .post(json!({
                "dataCapability": "soundbox.addSound@1",
                "payload": { "campaign": VALOMBRE, "name": name, "file": "sons/porte.ogg" },
                "caller": caller("dm"),
            }))
            .await;
        assert_eq!(status, StatusCode::ACCEPTED, "{answer}");
        answer["partition"]
            .as_str()
            .and_then(|p| p.strip_prefix("Sound/"))
            .unwrap_or_else(|| panic!("a Sound partition: {answer}"))
            .to_owned()
    }

    /// The creaking door of the soundbox, added and applied: its id.
    async fn door(&self) -> String {
        let door = self.queue_sound("Porte").await;
        applied(&self.settled().await);
        door
    }

    /// `POST /commands` of soundbox.removeSound on `target` by `caller`: the
    /// status, and the answer as JSON.
    async fn remove(&self, target: Option<&str>, caller: Value) -> (StatusCode, Value) {
        let mut body = json!({
            "dataCapability": "soundbox.removeSound@1",
            "payload": {},
            "caller": caller,
        });
        if let Some(target) = target {
            body["targetId"] = json!(target);
        }
        self.post(body).await
    }

    /// `POST /commands` of `body`: the status, and the answer as JSON.
    async fn post(&self, body: Value) -> (StatusCode, Value) {
        self.post_to("/commands", body).await
    }

    async fn post_to(&self, uri: &str, body: Value) -> (StatusCode, Value) {
        let request = Request::post(uri)
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
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

    /// One pass of the single applier, which settles one command.
    async fn settled(&self) -> CommandResult {
        let mut settled = self.engine.run_once().await.expect("the applier ran");
        assert_eq!(settled.len(), 1, "{settled:?}");
        settled.remove(0)
    }

    /// Whether the applier finds nothing to settle.
    async fn idle(&self) -> bool {
        self.engine
            .run_once()
            .await
            .expect("the applier ran")
            .is_empty()
    }
}

fn applied(result: &CommandResult) {
    assert_eq!(result.status, State::Applied, "{result:?}");
}

#[tokio::test]
async fn a_sound_removed_is_queued_on_its_partition_and_applied() {
    let s = served().await;
    let door = s.door().await;
    let (status, answer) = s.remove(Some(&door), caller("dm")).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{answer}");
    assert_eq!(answer["partition"], format!("Sound/{door}"));
    assert_eq!(answer["result"]["status"], "queued", "{answer}");
    let settled = s.settled().await;
    applied(&settled);
    assert_eq!(settled.command_id, answer["commandId"], "{settled:?}");
}

#[tokio::test]
async fn the_sound_named_as_its_target_s_id_in_capitals_is_the_same_sound() {
    let s = served().await;
    let door = s.door().await;
    let (status, answer) = s
        .post(json!({
            "dataCapability": "soundbox.removeSound@1",
            "payload": {},
            "target": { "aggregate": "Sound", "id": door.to_uppercase() },
            "caller": caller("dm"),
        }))
        .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{answer}");
    assert_eq!(answer["partition"], format!("Sound/{door}"));
    applied(&s.settled().await);
}

#[tokio::test]
async fn a_removal_of_a_sound_the_engine_does_not_hold_is_refused_404_and_not_queued() {
    let s = served().await;
    s.door().await;
    let other = "50000000-0000-4000-8000-0000000000ff";
    for target in [Some(other), Some("  "), None] {
        let (status, answer) = s.remove(target, caller("dm")).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{target:?}: {answer}");
        assert_eq!(answer["error"], "ce son n'existe pas");
    }
    assert!(s.idle().await);
    assert!(
        s.rows
            .projected(&format!("Sound/{other}"))
            .await
            .expect("the row")
            .is_empty()
    );
}

#[tokio::test]
async fn a_removal_behind_the_sound_s_addition_in_flight_waits_for_its_author() {
    let s = served().await;
    let crowd = s.queue_sound("Foule").await;
    let (status, removal) = s.remove(Some(&crowd), caller("dm")).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{removal}");
    assert_eq!(removal["partition"], format!("Sound/{crowd}"));
    // The addition declares the sound ahead of it: never rejected, the
    // removal keeps its place and its author confirms it.
    assert_eq!(
        removal["result"]["status"], "awaiting_confirmation",
        "{removal}"
    );
    applied(&s.settled().await);
    let command = removal["commandId"].as_str().expect("an id");
    let (status, confirmed) = s
        .post_to(
            &format!("/commands/{command}/confirm"),
            json!({ "by": "dm" }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{confirmed}");
    let settled = s.settled().await;
    applied(&settled);
    assert_eq!(settled.command_id, command);
}

#[tokio::test]
async fn a_removal_behind_an_edit_in_flight_keeps_its_place_and_is_applied_after_it() {
    let s = served().await;
    let door = s.door().await;
    let (status, edit) = s
        .post(json!({
            "dataCapability": "soundbox.updateSound@1",
            "payload": { "name": "Porte qui grince" },
            "targetId": door,
            "caller": caller("dm-2"),
        }))
        .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{edit}");
    let (status, removal) = s.remove(Some(&door), caller("dm")).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{removal}");
    assert_eq!(edit["partition"], removal["partition"]);
    // The edit changes the name, the removal the sound itself: nothing it
    // overwrites, so it queues behind the edit, never rejected.
    assert_eq!(removal["result"]["status"], "queued", "{removal}");
    // One pass applies the partition's queue in its order: the edit, then
    // the removal.
    let settled = s.engine.run_once().await.expect("the applier ran");
    settled.iter().for_each(applied);
    let order: Vec<&str> = settled.iter().map(|r| r.command_id.as_str()).collect();
    assert_eq!(
        order,
        [&edit["commandId"], &removal["commandId"]],
        "{settled:?}"
    );
}

#[tokio::test]
async fn a_removal_by_a_caller_who_is_no_approver_waits_for_its_review() {
    let s = served().await;
    let door = s.door().await;
    // Sound.id is critical on the aggregate: removing a sound is reviewed.
    let (status, answer) = s
        .remove(
            Some(&door),
            json!({ "by": "dm", "from": "SoundLibrary", "roles": [] }),
        )
        .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{answer}");
    assert_eq!(answer["result"]["status"], "awaiting_review", "{answer}");
    assert!(s.idle().await);
}

#[tokio::test]
async fn a_sound_whose_removal_is_queued_is_none_to_remove_or_edit_again() {
    let s = served().await;
    let door = s.door().await;
    let (status, answer) = s.remove(Some(&door), caller("dm")).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{answer}");
    // Applied after the tombstone, an edit would bring the sound back.
    for (data_capability, payload) in [
        ("soundbox.removeSound@1", json!({})),
        (
            "soundbox.updateSound@1",
            json!({ "name": "Porte qui grince" }),
        ),
        ("soundbox.setSoundVolume@1", json!({ "volume": 40 })),
    ] {
        let (status, answer) = s
            .post(json!({
                "dataCapability": data_capability,
                "payload": payload,
                "targetId": door,
                "caller": caller("dm-2"),
            }))
            .await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{data_capability}: {answer}");
        assert_eq!(answer["error"], "ce son n'existe pas");
    }
    applied(&s.settled().await);
    assert!(s.idle().await);
}
