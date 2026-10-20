//! soundbox.updateSound through the wire protocol, over the in-memory engine
//! as the server plants it: an edit is prepared against the sound's row —
//! one soundbox.addSound added — judged by the Sound aggregate's invariants,
//! queued on the sound's partition and applied. The `sound` table it
//! reaches is `update_sound_projection.rs`'s.

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

/// The DM's library calling as `by`: the DM approves their own sounds'
/// critical fields, which these tests do not judge.
fn caller(by: &str) -> Value {
    json!({ "by": by, "from": "SoundLibrary", "roles": ["role:approver"] })
}

impl Served {
    /// The creaking door of the soundbox, added by soundbox.addSound and
    /// applied: its id.
    async fn door(&self) -> String {
        let (status, answer) = self
            .post(
                "/commands",
                json!({
                    "dataCapability": "soundbox.addSound@1",
                    "payload": {
                        "campaign": VALOMBRE,
                        "name": "Porte",
                        "tags": ["bois"],
                        "volume": 80,
                        "file": "sons/porte.ogg",
                    },
                    "caller": caller("dm"),
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

    /// `POST /commands` of soundbox.updateSound on `target` by `by`: the
    /// status, and the answer as JSON.
    async fn submit(&self, by: &str, payload: Value, target: Option<&str>) -> (StatusCode, Value) {
        let mut body = json!({
            "dataCapability": "soundbox.updateSound@1",
            "payload": payload,
            "caller": caller(by),
        });
        if let Some(target) = target {
            body["targetId"] = json!(target);
        }
        self.post("/commands", body).await
    }

    /// An edit of `sound` by the DM, queued: its answer.
    async fn edit(&self, sound: &str, payload: Value) -> Value {
        let (status, answer) = self.submit("dm", payload, Some(sound)).await;
        assert_eq!(status, StatusCode::ACCEPTED, "{answer}");
        answer
    }

    async fn post(&self, uri: &str, body: Value) -> (StatusCode, Value) {
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
}

fn applied(result: &CommandResult) {
    assert_eq!(result.status, State::Applied, "{result:?}");
}

fn rejected_by(result: &CommandResult, ids: &[&str]) {
    assert_eq!(result.status, State::Rejected, "{result:?}");
    assert_eq!(result.violations, ids, "{result:?}");
}

#[tokio::test]
async fn a_sound_is_renamed_and_retagged_its_tags_normalised_and_the_rest_kept() {
    let s = served().await;
    let door = s.door().await;
    let before = s.row(&door).await;
    let answer = s
        .edit(
            &door,
            json!({ "name": "Porte qui grince", "tags": ["Porte", "porte"] }),
        )
        .await;
    assert_eq!(answer["partition"], format!("Sound/{door}"));
    applied(&s.settled().await);
    let mut expected = before;
    expected.insert("name".into(), json!("Porte qui grince"));
    expected.insert("tags".into(), json!(["porte"]));
    assert_eq!(s.row(&door).await, expected);
}

#[tokio::test]
async fn a_sound_is_pointed_to_another_file_and_its_volume_given_in_percent() {
    let s = served().await;
    let door = s.door().await;
    s.edit(&door, json!({ "file": " sons/porte-2.ogg ", "volume": 30 }))
        .await;
    applied(&s.settled().await);
    let row = s.row(&door).await;
    assert_eq!(row["filePath"], "sons/porte-2.ogg");
    assert_eq!(row["volume"], 0.3);
    assert_eq!(row["name"], "Porte");
    assert_eq!(row["tags"], json!(["bois"]));
}

#[tokio::test]
async fn a_volume_of_minus_5_is_rejected_by_sound_volume_range_and_nothing_changes() {
    let s = served().await;
    let door = s.door().await;
    let before = s.row(&door).await;
    s.edit(&door, json!({ "name": "Porte qui grince", "volume": -5 }))
        .await;
    rejected_by(&s.settled().await, &["sound-volume-range"]);
    assert_eq!(s.row(&door).await, before);
}

#[tokio::test]
async fn tags_that_are_no_list_of_texts_are_rejected_by_tags_normalized() {
    let s = served().await;
    let door = s.door().await;
    let before = s.row(&door).await;
    s.edit(&door, json!({ "tags": ["Porte", 3] })).await;
    rejected_by(&s.settled().await, &["tags-normalized"]);
    assert_eq!(s.row(&door).await, before);
}

#[tokio::test]
async fn two_edits_of_one_sound_in_flight_queue_and_the_second_s_author_confirms_the_overwrite() {
    let s = served().await;
    let door = s.door().await;
    let first = s.edit(&door, json!({ "name": "Porte qui grince" })).await;
    let (status, second) = s
        .submit("dm-2", json!({ "name": "Porte qui claque" }), Some(&door))
        .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{second}");
    assert_eq!(first["partition"], second["partition"]);
    // The second is not refused: it keeps its place, and its author confirms
    // it overwrites the first.
    assert_eq!(first["result"]["status"], "queued", "{first}");
    assert_eq!(
        second["result"]["status"], "awaiting_confirmation",
        "{second}"
    );
    // The first is applied in its turn, the second waiting for its author.
    let settled = s.settled().await;
    applied(&settled);
    assert_eq!(settled.command_id, first["commandId"], "{settled:?}");
    let command = second["commandId"].as_str().expect("an id");
    let (status, confirmed) = s
        .post(
            &format!("/commands/{command}/confirm"),
            json!({ "by": "dm-2" }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{confirmed}");
    applied(&s.settled().await);
    assert_eq!(s.row(&door).await["name"], "Porte qui claque");
}

#[tokio::test]
async fn an_edit_queued_behind_the_sound_s_addition_is_accepted_and_applied_once_confirmed() {
    let s = served().await;
    let (status, added) = s
        .post(
            "/commands",
            json!({
                "dataCapability": "soundbox.addSound@1",
                "payload": { "campaign": VALOMBRE, "name": "Foule", "file": "sons/foule.ogg" },
                "caller": caller("dm"),
            }),
        )
        .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{added}");
    let crowd = added["partition"]
        .as_str()
        .and_then(|p| p.strip_prefix("Sound/"))
        .expect("a Sound partition");
    let edit = s.edit(crowd, json!({ "volume": 40 })).await;
    assert_eq!(edit["partition"], added["partition"]);
    // The addition declares the volume ahead of it: its author confirms.
    assert_eq!(edit["result"]["status"], "awaiting_confirmation", "{edit}");
    let command = edit["commandId"].as_str().expect("an id");
    let (status, confirmed) = s
        .post(
            &format!("/commands/{command}/confirm"),
            json!({ "by": "dm" }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{confirmed}");
    let mut settled = s.engine.run_once().await.expect("the applier ran");
    settled.extend(s.engine.run_once().await.expect("the applier ran"));
    assert_eq!(settled.len(), 2, "{settled:?}");
    settled.iter().for_each(applied);
    assert_eq!(s.row(crowd).await["volume"], 0.4);
}

#[tokio::test]
async fn an_edit_of_a_sound_the_engine_does_not_hold_is_refused_404_and_not_queued() {
    let s = served().await;
    s.door().await;
    let other = "50000000-0000-4000-8000-0000000000ff";
    for target in [Some(other), None] {
        let (status, answer) = s
            .submit("dm", json!({ "name": "Porte qui grince" }), target)
            .await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{answer}");
        assert_eq!(answer["error"], "ce son n'existe pas");
    }
    assert!(
        s.engine
            .run_once()
            .await
            .expect("the applier ran")
            .is_empty()
    );
    assert!(s.row(other).await.is_empty());
}

#[tokio::test]
async fn the_sound_named_as_its_target_s_id_in_capitals_is_the_same_sound() {
    let s = served().await;
    let door = s.door().await;
    let body = json!({
        "dataCapability": "soundbox.updateSound@1",
        "payload": { "volume": 55 },
        "target": { "aggregate": "Sound", "id": door.to_uppercase() },
        "caller": caller("dm"),
    });
    let (status, answer) = s.post("/commands", body).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{answer}");
    applied(&s.settled().await);
    assert_eq!(s.row(&door).await["volume"], 0.55);
}

#[tokio::test]
async fn a_blank_name_or_file_or_nothing_to_change_is_refused_422_and_not_queued() {
    let s = served().await;
    let door = s.door().await;
    let before = s.row(&door).await;
    for (payload, error) in [
        (json!({ "name": "  " }), "un son a besoin d'un nom"),
        (json!({ "file": "" }), "un son a besoin de son fichier"),
        (json!({}), "rien à modifier sur ce son"),
        (json!({ "playing": true }), "rien à modifier sur ce son"),
    ] {
        let (status, answer) = s.submit("dm", payload.clone(), Some(&door)).await;
        assert_eq!(
            status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "{payload}: {answer}"
        );
        assert_eq!(answer["error"], error, "{payload}");
    }
    assert!(
        s.engine
            .run_once()
            .await
            .expect("the applier ran")
            .is_empty()
    );
    assert_eq!(s.row(&door).await, before);
}
