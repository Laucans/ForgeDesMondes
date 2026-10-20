//! session.addScene through the wire protocol, over the in-memory engine as
//! the server plants it: a new scene is ranked after its session's last —
//! queued side by side, each after the other, none rejected — links the
//! places of its session's campaign and no other, and what the tables could
//! never hold, or a session nobody holds, is refused before it is queued.
//! The tables it reaches are `add_scene_projection.rs`'s.

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
const BRUME: &str = "c1000000-0000-4000-8000-000000000002";
/// Valombre's session 4.
const SEANCE: &str = "51000000-0000-4000-8000-000000000004";

/// A session written whole: a stand-in for session.createSession, which no
/// DataCapability of this branch is yet.
const CREATE_SESSION: &str = r#"{"dataCapability":"test.createSession","owner":"dataguard","version":1,"effect":"insert","target":{"aggregate":"Session","id":"$sessionId"},"mode":"overwrite","callableBy":["*"]}"#;

/// The engine as the server plants it, its rows and its wire protocol.
struct Served {
    engine: Arc<Engine>,
    rows: Rows,
    app: Router,
}

/// The DM, approver of their own campaign's critical fields — as the
/// AiSettings Micro-UI has them approve their settings.
fn dm() -> Value {
    json!({ "by": "dm", "from": "session.SceneBoard", "roles": ["role:approver"] })
}

fn applied(result: &CommandResult) {
    assert_eq!(result.status, State::Applied, "{result:?}");
}

/// The scene a submission's partition names.
fn partition_id(answer: &Value, aggregate: &str) -> String {
    answer["partition"]
        .as_str()
        .and_then(|p| p.strip_prefix(&format!("{aggregate}/")))
        .unwrap_or_else(|| panic!("a {aggregate} partition: {answer}"))
        .to_owned()
}

impl Served {
    /// Valombre's session 4, in preparation, with no scene.
    async fn new() -> Served {
        let Planted { engine, rows } = planted(MEMORY).await.expect("the in-memory engine");
        let engine = Arc::new(
            engine.with_data_capability(DataCapability::parse(CREATE_SESSION).expect("contract F")),
        );
        let app = router(
            engine.clone(),
            rows.clone(),
            Arc::new(ports_llm::fake::FakeLlm::new()),
        );
        let served = Served { engine, rows, app };
        let Value::Object(payload) = json!({
            "sessionId": SEANCE,
            "sessionCampaignId": VALOMBRE,
            "sessionPosition": 4,
            "sessionTitle": "Le gué",
            "sessionStatus": "prep",
            "microphoneOn": false,
        }) else {
            unreachable!()
        };
        served
            .engine
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
        applied(&served.settled().await);
        served
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

    /// `POST /commands` of session.addScene by the DM: the status and the
    /// answer.
    async fn add(&self, payload: Value, key: Option<&str>) -> (StatusCode, Value) {
        let mut body = json!({
            "dataCapability": "session.addScene@1",
            "payload": payload,
            "caller": dm(),
        });
        if let Some(key) = key {
            body["idempotencyKey"] = json!(key);
        }
        self.post("/commands", body).await
    }

    /// Adds a scene of `payload` to session 4: its id, and what the applier
    /// made of it.
    async fn scene(&self, payload: Value) -> (String, CommandResult) {
        let (status, answer) = self.add(payload, None).await;
        assert_eq!(status, StatusCode::ACCEPTED, "{answer}");
        (partition_id(&answer, "Session"), self.settled().await)
    }

    /// A place of `campaign`, saved by the DM: its id.
    async fn place(&self, campaign: &str, name: &str) -> String {
        let (status, answer) = self
            .post(
                "/commands",
                json!({
                    "dataCapability": "campaign.saveEntity@1",
                    "payload": { "campaign": campaign, "entity": { "kind": "lieu", "name": name } },
                    "caller": dm(),
                }),
            )
            .await;
        assert_eq!(status, StatusCode::ACCEPTED, "{answer}");
        applied(&self.settled().await);
        partition_id(&answer, "Entity")
    }

    /// One pass of the single applier, which settles one command.
    async fn settled(&self) -> CommandResult {
        let mut settled = self.engine.run_once().await.expect("the applier ran");
        assert_eq!(settled.len(), 1, "{settled:?}");
        settled.remove(0)
    }

    /// The row the engine holds for `scene`.
    async fn row(&self, scene: &str) -> Map<String, Value> {
        self.rows
            .projected(&format!("Session/{scene}"))
            .await
            .expect("the row")
            .into_iter()
            .collect()
    }
}

fn title(name: &str) -> Value {
    json!({ "session": SEANCE, "title": name })
}

#[tokio::test]
async fn a_scene_lands_after_the_session_s_last_so_the_ranks_stay_one_to_n() {
    let served = Served::new().await;
    let mut ranks = Vec::new();
    for name in ["La taverne", "La route"] {
        let (scene, result) = served.scene(title(name)).await;
        applied(&result);
        ranks.push(served.row(&scene).await["scenePosition"].clone());
    }
    let (scene, result) = served
        .scene(json!({ "session": SEANCE, "title": "Le gué", "notes": "Trois bandits sur la rive est." }))
        .await;
    applied(&result);
    let row = served.row(&scene).await;
    ranks.push(row["scenePosition"].clone());
    assert_eq!(ranks, [json!(1), json!(2), json!(3)]);
    assert_eq!(row["sceneId"], scene);
    assert_eq!(row["sceneSessionId"], SEANCE);
    assert_eq!(row["sceneTitle"], "Le gué");
    assert_eq!(row["notes"], "Trois bandits sur la rive est.");
}

#[tokio::test]
async fn scenes_queued_side_by_side_are_ranked_one_after_the_other_and_none_is_rejected() {
    let served = Served::new().await;
    let mut scenes = Vec::new();
    for name in ["La taverne", "La route", "Le gué"] {
        let (status, answer) = served.add(title(name), None).await;
        assert_eq!(status, StatusCode::ACCEPTED, "{answer}");
        scenes.push(partition_id(&answer, "Session"));
    }
    // Each was given rank 1 when it was submitted: they are settled when
    // applied.
    let results = served.engine.run_once().await.expect("the applier ran");
    assert_eq!(results.len(), 3, "{results:?}");
    results.iter().for_each(applied);
    let mut ranks = Vec::new();
    for scene in &scenes {
        ranks.push(served.row(scene).await["scenePosition"].clone());
    }
    ranks.sort_by_key(|rank| rank.as_u64());
    assert_eq!(ranks, [json!(1), json!(2), json!(3)]);
}

#[tokio::test]
async fn a_scene_links_the_places_of_its_session_s_campaign() {
    let served = Served::new().await;
    let taverne = served.place(VALOMBRE, "La Taverne du Sanglier").await;
    let (scene, result) = served
        .scene(json!({ "session": SEANCE, "title": "La taverne", "places": [taverne] }))
        .await;
    applied(&result);
    let row = served.row(&scene).await;
    assert_eq!(row["entityId"], json!([taverne]));
    assert_eq!(row["sceneEntitySceneId"], scene);
}

#[tokio::test]
async fn scene_links_same_campaign_rejects_a_place_of_another_campaign() {
    let served = Served::new().await;
    let taverne = served.place(VALOMBRE, "La Taverne du Sanglier").await;
    let phare = served.place(BRUME, "Le Phare").await;
    for places in [
        json!([phare]),
        json!([taverne, phare]),
        // An id nobody holds is no place of the campaign.
        json!(["e1000000-0000-4000-8000-0000000000ff"]),
        json!(["taverne-sanglier"]),
    ] {
        let (scene, result) = served
            .scene(json!({ "session": SEANCE, "title": "Ailleurs", "places": places }))
            .await;
        assert_eq!(result.status, State::Rejected, "{result:?}");
        assert_eq!(result.violations, ["scene-links-same-campaign"], "{places}");
        assert!(served.row(&scene).await.is_empty(), "{places}");
    }
    // Nothing rejected took a rank.
    let (scene, result) = served.scene(title("La taverne")).await;
    applied(&result);
    assert_eq!(served.row(&scene).await["scenePosition"], 1);
}

#[tokio::test]
async fn a_scene_of_a_session_nobody_holds_or_one_the_tables_could_never_hold_is_not_queued() {
    let served = Served::new().await;
    for (payload, status) in [
        (
            json!({ "session": "51000000-0000-4000-8000-0000000000ff", "title": "Le gué" }),
            StatusCode::NOT_FOUND,
        ),
        (
            json!({ "session": "seance-4", "title": "Le gué" }),
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
        (
            json!({ "title": "Le gué" }),
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
        (
            json!({ "session": SEANCE, "title": "  " }),
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
        (
            json!({ "session": SEANCE, "title": "Le gué", "places": "taverne" }),
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
    ] {
        let (answered, answer) = served.add(payload.clone(), None).await;
        assert_eq!(answered, status, "{payload}: {answer}");
        assert!(answer["error"].as_str().is_some_and(|e| !e.is_empty()));
    }
    let queued = served.engine.run_once().await.expect("the applier ran");
    assert!(queued.is_empty(), "{queued:?}");
}

#[tokio::test]
async fn the_same_idempotency_key_twice_adds_one_scene() {
    let served = Served::new().await;
    let (status, first) = served.add(title("Le gué"), Some("k-1")).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{first}");
    applied(&served.settled().await);
    let (status, again) = served.add(title("Le gué"), Some("k-1")).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{again}");
    assert_eq!(again["commandId"], first["commandId"], "{again}");
    // A replay is answered as the first, never prepared again: not even
    // refused for a session it names otherwise.
    let (status, replay) = served
        .add(
            json!({ "session": "51000000-0000-4000-8000-0000000000ff", "title": "Le gué" }),
            Some("k-1"),
        )
        .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{replay}");
    assert_eq!(replay["commandId"], first["commandId"], "{replay}");
    let queued = served.engine.run_once().await.expect("the applier ran");
    assert!(queued.is_empty(), "{queued:?}");
    // The next scene is the second.
    let (scene, result) = served.scene(title("La route")).await;
    applied(&result);
    assert_eq!(served.row(&scene).await["scenePosition"], 2);
}

#[tokio::test]
async fn a_scene_lands_first_in_the_session_a_oneshot_was_created_with() {
    let served = Served::new().await;
    let (status, answer) = served
        .post(
            "/commands",
            json!({
                "dataCapability": "campaign.createCampaign@1",
                "payload": { "name": "La Nuit du Bal", "type": "oneshot" },
                "caller": { "by": "dm", "from": "CampaignList", "roles": ["role:approver"] },
            }),
        )
        .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{answer}");
    applied(&served.settled().await);
    let campaign = partition_id(&answer, "Campaign");
    // The oneshot's session is written on its campaign's partition.
    let session = served
        .rows
        .projected(&format!("Campaign/{campaign}"))
        .await
        .expect("the row")["sessionId"]
        .as_str()
        .expect("a oneshot's session")
        .to_owned();
    let (scene, result) = served
        .scene(json!({ "session": session, "title": "Le bal" }))
        .await;
    applied(&result);
    let row = served.row(&scene).await;
    assert_eq!(row["sceneSessionId"], session);
    assert_eq!(row["scenePosition"], 1);
}
