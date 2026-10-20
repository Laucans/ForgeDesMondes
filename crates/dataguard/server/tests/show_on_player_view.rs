//! live.showOnPlayerView, judged by the DataGuard as `plant()` binds it over
//! the in-memory store: what `prepare` makes of a send is written on its
//! session's diffusion row, base-coherent refuses an instance of another
//! session or a media that is no illustration, and two sends in a row are
//! both applied, in order. What the tables say of the rows a send names is
//! given here as `Found`; the tables' own answer is
//! `show_on_player_view_projection.rs`'s.

use std::collections::BTreeMap;
use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use dataguard_server::{Engine, MEMORY, Planted, Rows, planted, router};
use harness_dataguard::api::Submission;
use harness_dataguard::command::{CommandResult, State};
use harness_dataguard::guard::Caller;
use serde_json::{Value, json};
use show_on_player_view::{Found, NOT_AN_ILLUSTRATION, NOT_OF_THE_SESSION, prepare};
use tower::ServiceExt;

const VALOMBRE: &str = "c1000000-0000-4000-8000-000000000001";
/// Valombre's live session.
const SEANCE: &str = "51000000-0000-4000-8000-000000000001";
/// Valombre's session before it.
const SEANCE_1: &str = "51000000-0000-4000-8000-000000000000";
/// An illustration of Valombre.
const PORTRAIT: &str = "d1000000-0000-4000-8000-000000000002";
/// The tavern's battlemap instance, of the live session.
const INSTANCE: &str = "b1000000-0000-4000-8000-000000000001";
/// A battlemap instance of the session before.
const AUTRE: &str = "b1000000-0000-4000-8000-000000000002";

const NOW: &str = "2026-10-20T20:00:00.000Z";

fn wire(payload: Value) -> BTreeMap<String, Value> {
    let Value::Object(payload) = payload else {
        panic!("a payload is an object");
    };
    payload.into_iter().collect()
}

/// What the tables say when the instance sent is of `instance_session` and
/// the media sent is an illustration of the campaign or not.
fn found(instance_session: &str, illustration: bool) -> Found {
    Found {
        session: Some(SEANCE.to_string()),
        instance_session: Some(instance_session.to_string()),
        illustration,
        held: true,
    }
}

/// `payload`, prepared against `found`, submitted by the DM from the
/// Battlemap — the DM approves their own campaign's critical fields, the
/// session and the kind of its base: the warning it is told.
async fn submit(engine: &Engine, payload: Value, found: &Found) -> Option<&'static str> {
    let prepared = prepare(&wire(payload), found, NOW).expect("a send the table can hold");
    engine
        .submit(Submission {
            data_capability: "live.showOnPlayerView@1".to_string(),
            payload: prepared.payload,
            target_id: Some(prepared.target_id),
            based_on: None,
            idempotency_key: None,
            caller: Caller {
                by: "dm".to_string(),
                from: "Battlemap".to_string(),
                permissions: Vec::new(),
                roles: vec!["role:approver".to_string()],
            },
        })
        .await
        .expect("queued");
    prepared.warning
}

/// What the single applier made of what is queued, one command a partition.
async fn applied(engine: &Engine) -> Vec<CommandResult> {
    engine.run_once().await.expect("the applier ran")
}

/// The one result of a send just submitted.
async fn sent(
    engine: &Engine,
    payload: Value,
    found: &Found,
) -> (CommandResult, Option<&'static str>) {
    let warning = submit(engine, payload, found).await;
    let mut results = applied(engine).await;
    assert_eq!(results.len(), 1, "{results:?}");
    (results.remove(0), warning)
}

/// The engine as the server plants it, and its rows.
async fn served() -> (Engine, Rows) {
    let Planted { engine, rows } = planted(MEMORY).await.expect("the in-memory engine");
    (engine, rows)
}

/// The live session's diffusion row as the engine holds it.
async fn row(rows: &Rows) -> BTreeMap<String, Value> {
    rows.projected(&format!("Diffusion/{SEANCE}"))
        .await
        .expect("the row")
}

fn illustration() -> Value {
    json!({ "kind": "illustration", "media": PORTRAIT })
}

fn battlemap(instance: &str) -> Value {
    json!({ "kind": "battlemap", "instance": instance, "zoom": 1.5, "panX": -120, "panY": 40 })
}

#[tokio::test]
async fn the_first_send_of_an_illustration_makes_the_session_s_row() {
    let (engine, rows) = served().await;
    let (result, warning) = sent(&engine, illustration(), &found(SEANCE, true)).await;
    assert_eq!(result.status, State::Applied, "{result:?}");
    assert_eq!(warning, None);
    let row = row(&rows).await;
    assert_eq!(row["sessionId"], SEANCE);
    assert_eq!(row["baseKind"], "illustration");
    assert_eq!(row["baseMediaId"], PORTRAIT);
    assert_eq!(row["baseInstanceId"], Value::Null);
    assert_eq!(
        (&row["viewZoom"], &row["viewPanX"], &row["viewPanY"]),
        (&json!(1.0), &json!(0.0), &json!(0.0))
    );
    assert_eq!(row["updatedAt"], NOW);
}

#[tokio::test]
async fn a_battlemap_sent_after_an_illustration_replaces_the_base_on_the_same_row() {
    let (engine, rows) = served().await;
    sent(&engine, illustration(), &found(SEANCE, true)).await;
    let (result, _) = sent(&engine, battlemap(INSTANCE), &found(SEANCE, false)).await;
    assert_eq!(result.status, State::Applied, "{result:?}");
    let row = row(&rows).await;
    assert_eq!(row["sessionId"], SEANCE);
    assert_eq!(row["baseKind"], "battlemap");
    assert_eq!(row["baseInstanceId"], INSTANCE);
    assert_eq!(row["baseMediaId"], Value::Null);
    assert_eq!(
        (&row["viewZoom"], &row["viewPanX"], &row["viewPanY"]),
        (&json!(1.5), &json!(-120.0), &json!(40.0))
    );
}

#[tokio::test]
async fn base_coherent_rejects_an_instance_of_another_session_and_the_row_stays() {
    let (engine, rows) = served().await;
    sent(&engine, illustration(), &found(SEANCE, true)).await;
    let before = row(&rows).await;
    let (result, warning) = sent(&engine, battlemap(AUTRE), &found(SEANCE_1, false)).await;
    assert_eq!(result.status, State::Rejected, "{result:?}");
    assert_eq!(result.violations, ["base-coherent"], "{result:?}");
    assert_eq!(warning, Some(NOT_OF_THE_SESSION));
    assert_eq!(row(&rows).await, before);
}

#[tokio::test]
async fn base_coherent_rejects_a_media_that_is_no_illustration_of_the_campaign() {
    let (engine, rows) = served().await;
    let (result, warning) = sent(&engine, illustration(), &found(SEANCE, false)).await;
    assert_eq!(result.status, State::Rejected, "{result:?}");
    assert_eq!(result.violations, ["base-coherent"], "{result:?}");
    assert_eq!(warning, Some(NOT_AN_ILLUSTRATION));
    assert!(row(&rows).await.is_empty());
}

#[tokio::test]
async fn two_sends_in_a_row_are_both_queued_and_applied_in_order() {
    let (engine, rows) = served().await;
    submit(&engine, illustration(), &found(SEANCE, true)).await;
    submit(&engine, battlemap(INSTANCE), &found(SEANCE, false)).await;
    let mut results = applied(&engine).await;
    results.extend(applied(&engine).await);
    assert!(applied(&engine).await.is_empty());
    let statuses: Vec<&State> = results.iter().map(|r| &r.status).collect();
    assert_eq!(statuses, [&State::Applied, &State::Applied], "{results:?}");
    assert_eq!(row(&rows).await["baseKind"], "battlemap");
}

/// Over the engine's memory no table holds a live session: a send through
/// the wire protocol is refused, 422, saying why, and nothing is queued — as
/// is a send the table could never hold.
#[tokio::test]
async fn a_send_the_server_cannot_place_is_refused_before_it_is_queued() {
    let Planted { engine, rows } = planted(MEMORY).await.expect("the in-memory engine");
    let engine = Arc::new(engine);
    let app = router(
        engine.clone(),
        rows,
        Arc::new(ports_llm::fake::FakeLlm::new()),
    );
    for (payload, why) in [
        (
            json!({ "campaign": VALOMBRE, "kind": "illustration", "media": PORTRAIT }),
            "aucune séance n'est en cours",
        ),
        (illustration(), "media n'existe pas"),
        (
            json!({ "kind": "cover" }),
            "on envoie aux joueurs une battlemap ou une illustration",
        ),
        (
            json!({ "kind": "battlemap", "instance": INSTANCE, "zoom": 0 }),
            "le zoom est un nombre au-dessus de 0 et sous 1000",
        ),
    ] {
        let body = json!({
            "dataCapability": "live.showOnPlayerView@1",
            "target": { "aggregate": "Diffusion" },
            "payload": payload,
            "caller": { "by": "dm", "from": "Battlemap", "roles": ["role:approver"] },
        });
        let request = Request::post("/commands")
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .expect("a request");
        let response = app.clone().oneshot(request).await.expect("an answer");
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("a body");
        let answer: Value = serde_json::from_slice(&bytes).expect("JSON");
        assert_eq!(answer, json!({ "error": why }));
    }
    assert!(applied(&engine).await.is_empty());
}
