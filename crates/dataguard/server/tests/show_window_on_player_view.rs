//! live.showWindowOnPlayerView, judged by the DataGuard as `plant()` binds
//! it over the in-memory store: what `prepare` makes of an opened window is
//! written on its session's diffusion row — made when there was none, its
//! window replaced otherwise, its base kept — by the DM with no approval,
//! since a window touches no critical field; a media that is no illustration
//! is rejected under window-proportional. The live session and the media the
//! tables give are given here; the tables' own answer is
//! `show_window_on_player_view_projection.rs`'s. Over memory, which has no
//! table, the wire protocol refuses every window before it is queued.

use std::collections::BTreeMap;
use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use dataguard_server::{Engine, MEMORY, Planted, Rows, planted, router};
use harness_dataguard::api::Submission;
use harness_dataguard::command::{CommandResult, State};
use harness_dataguard::guard::Caller;
use serde_json::{Value, json};
use show_window_on_player_view::{Found, Named, WIDTH, X, Y, prepare};
use tower::ServiceExt;

const VALOMBRE: &str = "c1000000-0000-4000-8000-000000000001";
/// Valombre's live session.
const SEANCE: &str = "51000000-0000-4000-8000-000000000002";
/// The tavern's battlemap instance, of the live session.
const INSTANCE: &str = "b1000000-0000-4000-8000-000000000002";
/// An illustration of Valombre, 800 × 1200.
const PORTRAIT: &str = "d1000000-0000-4000-8000-000000000002";
/// Another illustration of Valombre.
const BLASON: &str = "d1000000-0000-4000-8000-000000000003";
/// Valombre's battlemap: a media, but no illustration.
const CARTE: &str = "d1000000-0000-4000-8000-000000000001";

const BEFORE: &str = "2026-10-20T19:59:00.000Z";
const NOW: &str = "2026-10-20T20:00:00.000Z";

fn object(value: Value) -> BTreeMap<String, Value> {
    let Value::Object(value) = value else {
        panic!("an object");
    };
    value.into_iter().collect()
}

/// `state` written on the live session's row by the DM from the table
/// sheet, with no role: no approval is asked of them.
async fn submit(engine: &Engine, state: BTreeMap<String, Value>, roles: &[&str]) {
    engine
        .submit(Submission {
            data_capability: "live.showWindowOnPlayerView@1".to_string(),
            payload: state,
            target_id: Some(SEANCE.to_string()),
            based_on: None,
            idempotency_key: None,
            caller: Caller {
                by: "dm".to_string(),
                from: "TableSheet".to_string(),
                permissions: Vec::new(),
                roles: roles.iter().map(|role| (*role).to_string()).collect(),
            },
        })
        .await
        .expect("queued");
}

/// What the single applier made of what is queued, one command a partition.
async fn applied(engine: &Engine) -> Vec<CommandResult> {
    engine.run_once().await.expect("the applier ran")
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

/// What the tables say of `media`: of Valombre, whose live session is
/// `SEANCE`; an illustration or not.
fn found(illustration: bool) -> Found {
    Found {
        session: Some(SEANCE.to_string()),
        held: true,
        illustration,
        base: None,
    }
}

/// Opens `media` in the window of the live session as it stands: what the
/// applier made of it.
async fn opened(engine: &Engine, rows: &Rows, media: &str, illustration: bool) -> CommandResult {
    let named = Named {
        campaign: VALOMBRE.to_string(),
        media: media.to_string(),
    };
    let prepared = prepare(&named, &found(illustration), &row(rows).await, NOW).expect("prepared");
    assert_eq!(prepared.target_id, SEANCE);
    submit(engine, prepared.payload, &[]).await;
    let mut results = applied(engine).await;
    assert_eq!(results.len(), 1, "{results:?}");
    results.remove(0)
}

/// The row of a session showing the tavern's battlemap instance, framed —
/// as a send to the player view leaves it, approved by the DM.
async fn showing_a_battlemap(engine: &Engine) {
    submit(
        engine,
        object(json!({
            "sessionId": SEANCE,
            "baseKind": "battlemap",
            "baseMediaId": null,
            "baseInstanceId": INSTANCE,
            "viewZoom": 1.5,
            "viewPanX": -120.0,
            "viewPanY": 40.0,
            "updatedAt": BEFORE,
        })),
        &["role:approver"],
    )
    .await;
    let results = applied(engine).await;
    assert!(
        results.len() == 1 && results[0].status == State::Applied,
        "{results:?}"
    );
}

#[tokio::test]
async fn an_illustration_opens_over_the_battlemap_and_the_base_is_unchanged() {
    let (engine, rows) = served().await;
    showing_a_battlemap(&engine).await;
    let before = row(&rows).await;
    let result = opened(&engine, &rows, PORTRAIT, true).await;
    // No critical field is touched: the DM's window needs no approval.
    assert_eq!(result.status, State::Applied, "{result:?}");
    let row = row(&rows).await;
    // window-fields-together: the media, x, y and the width all set.
    assert_eq!(row["windowMediaId"], PORTRAIT);
    assert_eq!(row["windowX"], X);
    assert_eq!(row["windowY"], Y);
    assert_eq!(row["windowWidth"], WIDTH);
    assert_eq!(row["updatedAt"], NOW);
    // window-proportional: the height is never stored.
    assert!(!row.contains_key("windowHeight"));
    // The base and the framing are as they were.
    for field in [
        "sessionId",
        "baseKind",
        "baseMediaId",
        "baseInstanceId",
        "viewZoom",
        "viewPanX",
        "viewPanY",
    ] {
        assert_eq!(row[field], before[field], "{field}");
    }
}

#[tokio::test]
async fn a_session_with_no_diffusion_row_gets_one_showing_the_cover_under_the_window() {
    let (engine, rows) = served().await;
    assert!(row(&rows).await.is_empty(), "no row before");
    let result = opened(&engine, &rows, PORTRAIT, true).await;
    assert_eq!(result.status, State::Applied, "{result:?}");
    assert_eq!(
        row(&rows).await,
        object(json!({
            "sessionId": SEANCE,
            "baseKind": "cover",
            "baseMediaId": null,
            "baseInstanceId": null,
            "windowMediaId": PORTRAIT,
            "windowX": X,
            "windowY": Y,
            "windowWidth": WIDTH,
            "updatedAt": NOW,
        }))
    );
}

#[tokio::test]
async fn another_illustration_replaces_the_window_s_content_on_the_same_row() {
    let (engine, rows) = served().await;
    showing_a_battlemap(&engine).await;
    opened(&engine, &rows, PORTRAIT, true).await;
    let result = opened(&engine, &rows, BLASON, true).await;
    assert_eq!(result.status, State::Applied, "{result:?}");
    let row = row(&rows).await;
    assert_eq!(row["windowMediaId"], BLASON);
    assert_eq!(row["windowWidth"], WIDTH);
    assert_eq!(row["baseInstanceId"], INSTANCE);
}

#[tokio::test]
async fn a_media_that_is_no_illustration_is_rejected_under_window_proportional_and_the_row_stays() {
    let (engine, rows) = served().await;
    showing_a_battlemap(&engine).await;
    opened(&engine, &rows, PORTRAIT, true).await;
    let before = row(&rows).await;
    let result = opened(&engine, &rows, CARTE, false).await;
    assert_eq!(result.status, State::Rejected, "{result:?}");
    assert_eq!(result.violations, vec!["window-proportional".to_string()]);
    assert_eq!(row(&rows).await, before);
}

#[tokio::test]
async fn a_window_with_no_live_session_s_row_and_no_illustration_makes_no_row() {
    let (engine, rows) = served().await;
    let result = opened(&engine, &rows, CARTE, false).await;
    assert_eq!(result.status, State::Rejected, "{result:?}");
    assert_eq!(result.violations, vec!["window-proportional".to_string()]);
    assert!(row(&rows).await.is_empty());
}

/// `POST /commands` of live.showWindowOnPlayerView with `payload` by the DM,
/// over the engine's memory: the status and the answer.
async fn post(engine: Engine, rows: Rows, payload: Value) -> (StatusCode, Value) {
    let app = router(
        Arc::new(engine),
        rows,
        Arc::new(ports_llm::fake::FakeLlm::new()),
    );
    let body = json!({
        "dataCapability": "live.showWindowOnPlayerView@1",
        "target": { "aggregate": "Diffusion" },
        "payload": payload,
        "caller": { "by": "dm", "from": "TableSheet", "roles": [] },
    });
    let request = Request::post("/commands")
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .expect("a request");
    let response = app.oneshot(request).await.expect("an answer");
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("a body");
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

#[tokio::test]
async fn the_wire_protocol_refuses_before_queueing_what_names_no_live_session() {
    for (payload, said) in [
        (json!({ "media": PORTRAIT }), "campaign manque"),
        (json!({ "campaign": VALOMBRE }), "media manque"),
        (
            json!({ "campaign": VALOMBRE, "media": "portrait" }),
            "campaign ou media n'est pas un identifiant",
        ),
        // The engine's memory holds no table, so no live session.
        (
            json!({ "campaign": VALOMBRE, "media": PORTRAIT }),
            "aucune séance n'est en cours",
        ),
    ] {
        let (engine, rows) = served().await;
        let probe = rows.clone();
        let (status, answer) = post(engine, rows, payload.clone()).await;
        assert_eq!(
            status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "{payload}: {answer}"
        );
        assert_eq!(answer["error"], said, "{payload}");
        assert!(row(&probe).await.is_empty(), "{payload}: nothing queued");
    }
}
