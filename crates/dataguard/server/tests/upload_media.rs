//! media.uploadMedia through the wire protocol, over the in-memory engine as
//! the server plants it: an upload is prepared against its place, queued on
//! its new media's partition through the DataQueue, judged by the DataGuard
//! and applied — a battlemap with its default grid. The tables it reaches are
//! `upload_media_projection.rs`'s.

use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use dataguard_server::{Engine, MEMORY, Planted, Rows, planted, router};
use harness_dataguard::command::{CommandResult, State};
use serde_json::{Map, Value, json};
use tower::ServiceExt;

const VALOMBRE: &str = "c1000000-0000-4000-8000-000000000001";
const BRUME: &str = "c1000000-0000-4000-8000-000000000002";

/// The engine as the server plants it, its rows and its wire protocol.
struct Served {
    engine: Arc<Engine>,
    rows: Rows,
    app: Router,
}

async fn served() -> Served {
    let Planted { engine, rows } = planted(MEMORY).await.expect("the in-memory engine");
    let engine = Arc::new(engine);
    let app = router(
        engine.clone(),
        rows.clone(),
        Arc::new(ports_llm::fake::FakeLlm::new()),
    );
    Served { engine, rows, app }
}

impl Served {
    /// `POST /commands` of `data_capability` by the DM, approver of their
    /// own campaign's critical fields: the status and the answer.
    async fn command(
        &self,
        data_capability: &str,
        payload: Value,
        key: Option<&str>,
    ) -> (StatusCode, Value) {
        let mut body = json!({
            "dataCapability": data_capability,
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

    /// Uploads `payload`: the media's partition, and what the applier made
    /// of the upload.
    async fn upload(&self, payload: Value) -> (String, CommandResult) {
        let (status, answer) = self.command("media.uploadMedia@1", payload, None).await;
        assert_eq!(status, StatusCode::ACCEPTED, "{answer}");
        let partition = answer["partition"]
            .as_str()
            .filter(|p| p.starts_with("Media/"))
            .unwrap_or_else(|| panic!("a Media partition: {answer}"))
            .to_owned();
        (partition, self.settled().await)
    }

    /// A new element of `campaign`, saved by the DM: its id.
    async fn element(&self, campaign: &str, kind: &str, name: &str) -> String {
        let payload = json!({ "campaign": campaign, "entity": { "kind": kind, "name": name } });
        let (status, answer) = self.command("campaign.saveEntity@1", payload, None).await;
        assert_eq!(status, StatusCode::ACCEPTED, "{answer}");
        applied(&self.settled().await);
        answer["partition"]
            .as_str()
            .and_then(|p| p.strip_prefix("Entity/"))
            .expect("an Entity partition")
            .to_owned()
    }

    /// The row the engine holds on `partition`.
    async fn row(&self, partition: &str) -> Map<String, Value> {
        self.rows
            .projected(partition)
            .await
            .expect("the row")
            .into_iter()
            .collect()
    }

    /// Whether the queue holds nothing to apply.
    async fn nothing_queued(&self) -> bool {
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

fn upload(kind: &str, title: &str) -> Value {
    json!({
        "campaign": VALOMBRE,
        "kind": kind,
        "title": title,
        "file": "/Users/dm/cartes/taverne.png",
        "width": 2048,
        "height": 1536,
    })
}

/// The grid's fields, as the aggregate names them.
const GRID: [&str; 7] = [
    "mediaId",
    "hexSizePx",
    "offsetX",
    "offsetY",
    "orientation",
    "color",
    "opacity",
];

#[tokio::test]
async fn a_battlemap_is_applied_with_its_default_grid() {
    let served = served().await;
    let (partition, result) = served.upload(upload("battlemap", "La Taverne")).await;
    applied(&result);
    let row = served.row(&partition).await;
    let id = partition.strip_prefix("Media/").expect("its id");
    assert_eq!(row["id"], id);
    assert_eq!(row["mediaId"], id);
    assert_eq!(row["kind"], "battlemap");
    assert_eq!(row["campaignId"], VALOMBRE);
    assert_eq!(row["filePath"], "/Users/dm/cartes/taverne.png");
    assert_eq!(
        (row["widthPx"].clone(), row["heightPx"].clone()),
        (json!(2048), json!(1536))
    );
    // grid-values-range: a hex above 0, an opacity from 0 to 1.
    assert!(
        row["hexSizePx"].as_f64().is_some_and(|size| size > 0.0),
        "{row:?}"
    );
    assert!(
        row["opacity"]
            .as_f64()
            .is_some_and(|opacity| (0.0..=1.0).contains(&opacity)),
        "{row:?}"
    );
    assert_eq!(row["placeId"], Value::Null);
}

#[tokio::test]
async fn an_illustration_is_applied_with_no_grid() {
    let served = served().await;
    let (partition, result) = served.upload(upload("illustration", "Elowen")).await;
    applied(&result);
    let row = served.row(&partition).await;
    assert_eq!(row["kind"], "illustration");
    for field in GRID {
        assert!(
            row.get(field).is_none_or(Value::is_null),
            "{field}: {row:?}"
        );
    }
}

#[tokio::test]
async fn a_lieu_of_the_campaign_is_the_media_s_place_named_in_either_case() {
    let served = served().await;
    let taverne = served
        .element(VALOMBRE, "lieu", "La Taverne du Sanglier")
        .await;
    for place in [taverne.clone(), taverne.to_uppercase()] {
        let mut payload = upload("battlemap", "La Taverne");
        payload["place"] = json!(place);
        let (partition, result) = served.upload(payload).await;
        applied(&result);
        assert_eq!(served.row(&partition).await["placeId"], place);
    }
}

#[tokio::test]
async fn media_place_is_lieu_refuses_a_non_lieu_another_campaign_s_lieu_or_no_element() {
    let served = served().await;
    let morlox = served.element(VALOMBRE, "pnj", "Morlox").await;
    let phare = served.element(BRUME, "lieu", "Le Phare").await;
    let nobody = "e1000000-0000-4000-8000-0000000000ff".to_owned();
    for place in [morlox, phare, nobody] {
        let mut payload = upload("illustration", "Elowen");
        payload["place"] = json!(place);
        let (status, answer) = served.command("media.uploadMedia@1", payload, None).await;
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
        // Not queued: nothing is written.
        assert!(served.nothing_queued().await, "{place}");
    }
}

#[tokio::test]
async fn an_upload_the_tables_could_never_hold_is_refused_and_not_queued() {
    let served = served().await;
    let (status, answer) = served
        .command("media.uploadMedia@1", upload("carte", "La Taverne"), None)
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{answer}");
    assert!(served.nothing_queued().await);
}

#[tokio::test]
async fn the_same_upload_twice_under_one_idempotency_key_is_one_media() {
    let served = served().await;
    let payload = upload("battlemap", "La Taverne");
    let (_, first) = served
        .command("media.uploadMedia@1", payload.clone(), Some("upload-1"))
        .await;
    let (_, second) = served
        .command("media.uploadMedia@1", payload, Some("upload-1"))
        .await;
    assert_eq!(second["replayed"], true, "{second}");
    assert_eq!(second["commandId"], first["commandId"]);
    assert_eq!(second["partition"], first["partition"]);
    applied(&served.settled().await);
    assert!(served.nothing_queued().await);
}

#[tokio::test]
async fn two_uploads_are_two_media() {
    let served = served().await;
    let (one, _) = served.upload(upload("battlemap", "La Taverne")).await;
    let (other, _) = served.upload(upload("battlemap", "La Taverne")).await;
    assert_ne!(one, other);
}

#[tokio::test]
async fn a_replay_is_answered_as_the_upload_was_queued_though_its_place_was_retyped_since() {
    let served = served().await;
    let taverne = served
        .element(VALOMBRE, "lieu", "La Taverne du Sanglier")
        .await;
    let mut payload = upload("battlemap", "La Taverne");
    payload["place"] = json!(taverne);
    let (_, first) = served
        .command("media.uploadMedia@1", payload.clone(), Some("upload-1"))
        .await;
    applied(&served.settled().await);
    // The tavern is an NPC now: a new upload naming it would be refused.
    let retyped = json!({
        "campaign": VALOMBRE,
        "entity": { "id": taverne, "kind": "pnj", "name": "La Taverne du Sanglier" },
    });
    let (status, answer) = served.command("campaign.saveEntity@1", retyped, None).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{answer}");
    applied(&served.settled().await);

    let (status, second) = served
        .command("media.uploadMedia@1", payload, Some("upload-1"))
        .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{second}");
    assert_eq!(second["replayed"], true, "{second}");
    assert_eq!(second["commandId"], first["commandId"]);
    assert!(served.nothing_queued().await);
}
