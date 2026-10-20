//! media.setMediaPlace through the wire protocol, over the in-memory engine
//! as the server plants it: a change of place is judged against the media
//! and the place as the engine holds them — `media-place-is-lieu` before it
//! is queued — and writes the media's `placeId` alone. The table it reaches
//! is `set_media_place_projection.rs`'s.

use std::collections::BTreeMap;
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
const BRUME: &str = "c1000000-0000-4000-8000-000000000002";
const CARTE: &str = "a1000000-0000-4000-8000-000000000001";
const NOWHERE: &str = "a1000000-0000-4000-8000-0000000000ff";

/// A media recorded, test-only: `media.uploadMedia` is its own task.
const ADD_MEDIA: &str = r#"{"dataCapability":"test.addMedia","owner":"dataguard","version":1,"effect":"insert","target":{"aggregate":"Media","id":"$id"},"callableBy":["*"]}"#;

/// The engine as the server plants it, its rows and its wire protocol.
struct Served {
    engine: Arc<Engine>,
    rows: Rows,
    app: Router,
}

/// Valombre's battlemap of the ford, `CARTE`, recorded, and its elements:
/// the Taverne (a lieu), Morlox (an NPC), and Brume's lighthouse (a lieu of
/// another campaign) — their ids.
struct Valombre {
    served: Served,
    taverne: String,
    moulin: String,
    morlox: String,
    phare: String,
}

async fn valombre() -> Valombre {
    let Planted { engine, rows } = planted(MEMORY).await.expect("the in-memory engine");
    let engine = Arc::new(
        engine.with_data_capability(DataCapability::parse(ADD_MEDIA).expect("contract F")),
    );
    let app = router(
        engine.clone(),
        rows.clone(),
        Arc::new(ports_llm::fake::FakeLlm::new()),
    );
    let served = Served { engine, rows, app };
    let taverne = served
        .save(VALOMBRE, "lieu", "La Taverne du Sanglier")
        .await;
    let moulin = served.save(VALOMBRE, "lieu", "Le Moulin").await;
    let morlox = served.save(VALOMBRE, "pnj", "Morlox").await;
    let phare = served.save(BRUME, "lieu", "Le Phare").await;
    let submitted = served
        .engine
        .submit(Submission {
            data_capability: "test.addMedia".to_string(),
            payload: json!({
                "id": CARTE,
                "campaignId": VALOMBRE,
                "kind": "battlemap",
                "title": "Le Gué des Corbeaux",
                "filePath": "media/gue.png",
                "widthPx": 960,
                "heightPx": 640,
                "hexSizePx": 30,
                "offsetX": 12,
                "offsetY": 8,
                "orientation": "pointe",
                "color": "#202020",
                "opacity": 0.4,
            })
            .as_object()
            .expect("an object")
            .clone()
            .into_iter()
            .collect(),
            target_id: None,
            based_on: None,
            idempotency_key: None,
            caller: Caller {
                by: "dm".to_string(),
                from: "test".to_string(),
                permissions: Vec::new(),
                roles: vec!["role:approver".to_string()],
            },
        })
        .await
        .expect("queued");
    assert_eq!(submitted.partition, format!("Media/{CARTE}"));
    let added = served.settled().await;
    assert_eq!(added.status, State::Applied, "{added:?}");
    Valombre {
        served,
        taverne,
        moulin,
        morlox,
        phare,
    }
}

impl Served {
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

    /// Saves an element of `campaign`, of `kind`, through
    /// campaign.saveEntity: its id.
    async fn save(&self, campaign: &str, kind: &str, name: &str) -> String {
        let (status, answer) = self
            .post(
                "/commands",
                json!({
                    "dataCapability": "campaign.saveEntity@1",
                    "payload": { "campaign": campaign, "entity": { "kind": kind, "name": name } },
                    "caller": { "by": "dm", "from": "campaign.EntityEditor", "roles": ["role:approver"] },
                }),
            )
            .await;
        assert_eq!(status, StatusCode::ACCEPTED, "{answer}");
        let saved = self.settled().await;
        assert_eq!(saved.status, State::Applied, "{saved:?}");
        answer["partition"]
            .as_str()
            .and_then(|p| p.strip_prefix("Entity/"))
            .expect("an Entity partition")
            .to_owned()
    }

    /// `POST /commands` of media.setMediaPlace with `payload`, on `target`
    /// when it names one, as the MediaLibrary card calls it.
    async fn place(
        &self,
        target: Option<&str>,
        payload: Value,
        key: Option<&str>,
    ) -> (StatusCode, Value) {
        let mut body = json!({
            "dataCapability": "media.setMediaPlace@1",
            "payload": payload,
            "caller": { "by": "dm", "from": "media.MediaLibrary", "roles": [] },
        });
        if let Some(target) = target {
            body["targetId"] = json!(target);
        }
        if let Some(key) = key {
            body["idempotencyKey"] = json!(key);
        }
        self.post("/commands", body).await
    }

    /// Sets the place of `CARTE` to `place` and applies it: the result.
    async fn placed(&self, place: Value) -> CommandResult {
        let (status, answer) = self
            .place(Some(CARTE), json!({ "place": place }), None)
            .await;
        assert_eq!(status, StatusCode::ACCEPTED, "{answer}");
        assert_eq!(answer["partition"], format!("Media/{CARTE}"), "{answer}");
        self.settled().await
    }

    /// The battlemap's row as the engine holds it.
    async fn carte(&self) -> BTreeMap<String, Value> {
        self.rows
            .projected(&format!("Media/{CARTE}"))
            .await
            .expect("the row")
    }
}

#[tokio::test]
async fn a_lieu_of_the_campaign_becomes_the_media_s_place_and_nothing_else_changes() {
    let valombre = valombre().await;
    let served = &valombre.served;
    let before = served.carte().await;
    let applied = served.placed(json!(valombre.taverne)).await;
    assert_eq!(applied.status, State::Applied, "{applied:?}");
    assert!(applied.violations.is_empty(), "{applied:?}");
    let mut after = served.carte().await;
    assert_eq!(after.remove("placeId"), Some(json!(valombre.taverne)));
    assert_eq!(after, before, "no other field changed");

    // Changed for another lieu of the campaign.
    let applied = served.placed(json!(valombre.moulin)).await;
    assert_eq!(applied.status, State::Applied, "{applied:?}");
    assert_eq!(served.carte().await["placeId"], json!(valombre.moulin));
}

#[tokio::test]
async fn a_null_place_detaches_the_media_which_stays_valid() {
    let valombre = valombre().await;
    let served = &valombre.served;
    served.placed(json!(valombre.taverne)).await;
    let applied = served.placed(Value::Null).await;
    assert_eq!(applied.status, State::Applied, "{applied:?}");
    assert!(applied.violations.is_empty(), "{applied:?}");
    let row = served.carte().await;
    assert_eq!(row["placeId"], Value::Null);
    assert_eq!(row["kind"], json!("battlemap"), "{row:?}");
}

#[tokio::test]
async fn the_payload_s_media_names_it_when_the_command_names_no_target() {
    let valombre = valombre().await;
    let served = &valombre.served;
    let (status, answer) = served
        .place(
            None,
            json!({ "media": CARTE, "place": valombre.taverne }),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{answer}");
    assert_eq!(answer["partition"], format!("Media/{CARTE}"), "{answer}");
    assert_eq!(served.settled().await.status, State::Applied);
    assert_eq!(served.carte().await["placeId"], json!(valombre.taverne));
}

#[tokio::test]
async fn a_place_that_is_no_lieu_of_the_campaign_is_refused_naming_media_place_is_lieu() {
    let valombre = valombre().await;
    let served = &valombre.served;
    served.placed(json!(valombre.taverne)).await;
    let before = served.carte().await;
    for (place, why) in [
        (valombre.morlox.as_str(), "an NPC"),
        (valombre.phare.as_str(), "a lieu of another campaign"),
        (NOWHERE, "no element at all"),
    ] {
        let (status, answer) = served
            .place(Some(CARTE), json!({ "place": place }), None)
            .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{why}: {answer}");
        let error = answer["error"].as_str().expect("an error");
        assert!(error.starts_with("media-place-is-lieu"), "{why}: {error}");
        assert!(
            served
                .engine
                .run_once()
                .await
                .expect("the applier ran")
                .is_empty(),
            "{why}: nothing was queued"
        );
        assert_eq!(served.carte().await, before, "{why}: nothing changed");
    }
}

#[tokio::test]
async fn a_media_there_is_not_or_a_malformed_request_is_refused_and_not_queued() {
    let valombre = valombre().await;
    let served = &valombre.served;
    let (status, answer) = served
        .place(Some(NOWHERE), json!({ "place": valombre.taverne }), None)
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{answer}");
    let (status, answer) = served
        .place(Some("bm-gue"), json!({ "place": valombre.taverne }), None)
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{answer}");
    for payload in [
        json!({ "place": valombre.taverne, "title": "Autre" }),
        json!({}),
        json!({ "place": "la-taverne" }),
    ] {
        let (status, answer) = served.place(Some(CARTE), payload.clone(), None).await;
        assert_eq!(
            status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "{payload}: {answer}"
        );
    }
    let (status, answer) = served
        .place(None, json!({ "place": valombre.taverne }), None)
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{answer}");
    assert!(
        served
            .engine
            .run_once()
            .await
            .expect("the applier ran")
            .is_empty()
    );
    assert!(!served.carte().await.contains_key("placeId"));
}

#[tokio::test]
async fn a_command_replayed_with_its_idempotency_key_is_applied_once() {
    let valombre = valombre().await;
    let served = &valombre.served;
    let payload = json!({ "place": valombre.taverne });
    let (status, first) = served
        .place(Some(CARTE), payload.clone(), Some("attach-1"))
        .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{first}");
    assert_eq!(served.settled().await.status, State::Applied);
    let once = served.carte().await;
    assert_eq!(once["placeId"], json!(valombre.taverne));

    let (status, again) = served.place(Some(CARTE), payload, Some("attach-1")).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{again}");
    assert_eq!(again["replayed"], json!(true), "{again}");
    assert_eq!(again["commandId"], first["commandId"]);
    assert_eq!(again["result"]["status"], json!("applied"), "{again}");
    assert!(
        served
            .engine
            .run_once()
            .await
            .expect("the applier ran")
            .is_empty(),
        "the replay queued nothing"
    );
    assert_eq!(served.carte().await, once);
}

#[tokio::test]
async fn a_replay_is_answered_as_it_was_queued_though_its_place_is_retyped_since() {
    let valombre = valombre().await;
    let served = &valombre.served;
    let payload = json!({ "place": valombre.moulin });
    let (status, first) = served
        .place(Some(CARTE), payload.clone(), Some("attach-2"))
        .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{first}");
    assert_eq!(served.settled().await.status, State::Applied);

    // The Moulin is no lieu any more.
    let (status, answer) = served
        .post(
            "/commands",
            json!({
                "dataCapability": "campaign.saveEntity@1",
                "payload": {
                    "campaign": VALOMBRE,
                    "entity": { "id": valombre.moulin, "kind": "pnj", "name": "Le Meunier" },
                },
                "caller": { "by": "dm", "from": "campaign.EntityEditor", "roles": ["role:approver"] },
            }),
        )
        .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{answer}");
    assert_eq!(served.settled().await.status, State::Applied);
    let (status, fresh) = served.place(Some(CARTE), payload.clone(), None).await;
    assert_eq!(
        status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "a new command is judged against what the Moulin is now: {fresh}"
    );

    let (status, again) = served.place(Some(CARTE), payload, Some("attach-2")).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{again}");
    assert_eq!(again["replayed"], json!(true), "{again}");
    assert_eq!(again["commandId"], first["commandId"]);
}
