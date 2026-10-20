//! `table.changeHitPoints` through the wire, over the in-memory engine as
//! `plant()` binds it: a change of HP is the step of one token's current HP
//! alone, on its own partition — the other tokens of the same character keep
//! theirs (`hp-change-one-token`) — cut so an NPC's HP land between 0 and its
//! HP max, and refused on a PC's token, which tracks none
//! (`pj-hp-not-tracked`). The instance's tokens are written by a test-only
//! DataCapability, as `battlemap_instance_invariants.rs` writes them: over
//! the engine's memory no table holds an instance to drop tokens on.

use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use dataguard_server::{Engine, MEMORY, Rows, planted, router};
use harness_dataguard::api::Submission;
use harness_dataguard::command::{CommandResult, State};
use harness_dataguard::guard::Caller;
use harness_dataguard::manifest::DataCapability;
use serde_json::{Value, json};
use tower::ServiceExt;

const INSTANCE: &str = "b1000000-0000-4000-8000-000000000001";
/// Another scene's instance.
const AUTRE_INSTANCE: &str = "b1000000-0000-4000-8000-000000000002";
const ELOWEN: &str = "e1000000-0000-4000-8000-000000000001";
const BANDIT: &str = "e1000000-0000-4000-8000-000000000003";
const JETON_ELOWEN: &str = "70000000-0000-4000-8000-000000000001";
const BANDIT_1: &str = "70000000-0000-4000-8000-000000000002";
const BANDIT_2: &str = "70000000-0000-4000-8000-000000000003";

/// The engine as the server plants it, with the test-only DataCapability
/// that drops a token, the rows it holds, and the wire it is served by.
async fn served() -> (Arc<Engine>, Rows, Router) {
    let planted = planted(MEMORY).await.expect("the in-memory engine");
    let manifest = json!({
        "dataCapability": "test.dropToken",
        "owner": "dataguard",
        "version": 1,
        "effect": "insert",
        "target": { "aggregate": "BattlemapInstance", "id": "$tokenId" },
        "mode": "overwrite",
        "callableBy": ["*"],
    });
    let engine =
        Arc::new(planted.engine.with_data_capability(
            DataCapability::parse(&manifest.to_string()).expect("contract F"),
        ));
    let app = router(
        engine.clone(),
        planted.rows.clone(),
        Arc::new(ports_llm::fake::FakeLlm::new()),
    );
    (engine, planted.rows, app)
}

/// Runs the applier once: its results.
async fn run(engine: &Engine) -> Vec<CommandResult> {
    engine.run_once().await.expect("the applier ran")
}

/// Runs the applier once: `count` commands, every one applied.
async fn applied(engine: &Engine, count: usize) {
    let results = run(engine).await;
    assert_eq!(results.len(), count, "{results:?}");
    for result in &results {
        assert_eq!(result.status, State::Applied, "{result:?}");
    }
}

/// A token of `character` on the instance, at `hp` of `hp_max` — none for a
/// PC — dropped and applied.
async fn drop_token(
    engine: &Engine,
    id: &str,
    character: &str,
    name: &str,
    hp: Option<(i64, i64)>,
) {
    let (current, max) = hp.map_or((Value::Null, Value::Null), |(current, max)| {
        (json!(current), json!(max))
    });
    let Value::Object(payload) = json!({
        "tokenId": id,
        "tokenInstanceId": INSTANCE,
        "characterId": character,
        "name": name,
        "q": 1,
        "r": 1,
        "visible": true,
        "hpCurrent": current,
        "hpMax": max,
        "conditions": [],
    }) else {
        unreachable!("a payload is an object");
    };
    engine
        .submit(Submission {
            data_capability: "test.dropToken".to_string(),
            payload: payload.into_iter().collect(),
            target_id: None,
            based_on: None,
            idempotency_key: None,
            caller: Caller {
                by: "dm".to_string(),
                from: "table.battlemap".to_string(),
                permissions: Vec::new(),
                roles: vec!["role:approver".to_string()],
            },
        })
        .await
        .expect("queued");
    applied(engine, 1).await;
}

/// Elowen, a PC, and Bandit 1 and Bandit 2, two NPCs of the same character,
/// the first at `first` of 20 HP, the second untouched at 20.
async fn tavern(engine: &Engine, first: i64) {
    drop_token(engine, JETON_ELOWEN, ELOWEN, "Elowen", None).await;
    drop_token(engine, BANDIT_1, BANDIT, "Bandit 1", Some((first, 20))).await;
    drop_token(engine, BANDIT_2, BANDIT, "Bandit 2", Some((20, 20))).await;
}

/// `POST /commands` of table.changeHitPoints with `payload`, on `target`
/// when one is given, by the DM from the battlemap: its status and answer.
async fn command(app: &Router, target: Option<&str>, payload: Value) -> (StatusCode, Value) {
    command_by(app, target, payload, json!(["role:approver"])).await
}

/// [`command`], by a caller holding `roles`. A token's current HP are a
/// critical field of the aggregate: the DM, its approver, sends them applied.
async fn command_by(
    app: &Router,
    target: Option<&str>,
    payload: Value,
    roles: Value,
) -> (StatusCode, Value) {
    let mut body = json!({
        "dataCapability": "table.changeHitPoints@1",
        "payload": payload,
        "caller": { "by": "dm", "from": "table.Battlemap", "roles": roles },
    });
    if let Some(target) = target {
        body["target"] = json!({ "id": target });
    }
    let request = Request::post("/commands")
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
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

/// `delta` on Bandit 1, on the instance, accepted.
async fn hit(app: &Router, delta: i64) {
    let (status, body) = command(
        app,
        Some(INSTANCE),
        json!({ "token": BANDIT_1, "delta": delta }),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
}

/// The row of `id`, as the engine holds it.
async fn row(rows: &Rows, id: &str) -> Value {
    json!(
        rows.projected(&format!("BattlemapInstance/{id}"))
            .await
            .expect("the row read")
    )
}

#[tokio::test]
async fn hp_change_one_token_an_npc_at_20_of_20_sent_minus_7_is_at_13_and_no_other_token_changed() {
    let (engine, rows, app) = served().await;
    tavern(&engine, 20).await;
    let before = row(&rows, BANDIT_1).await;
    let others = [row(&rows, BANDIT_2).await, row(&rows, JETON_ELOWEN).await];
    hit(&app, -7).await;
    applied(&engine, 1).await;
    let mut wounded = before;
    wounded["hpCurrent"] = json!(13);
    assert_eq!(row(&rows, BANDIT_1).await, wounded);
    // The other bandit, of the same character, and the PC, as they were.
    assert_eq!(
        [row(&rows, BANDIT_2).await, row(&rows, JETON_ELOWEN).await],
        others
    );
}

#[tokio::test]
async fn pj_hp_not_tracked_an_npc_at_3_sent_minus_10_is_at_0_and_one_at_13_sent_plus_50_at_20() {
    let (engine, rows, app) = served().await;
    tavern(&engine, 3).await;
    hit(&app, -10).await;
    applied(&engine, 1).await;
    assert_eq!(row(&rows, BANDIT_1).await["hpCurrent"], 0);
    // Healed back to 13, then far beyond its max.
    hit(&app, 13).await;
    applied(&engine, 1).await;
    assert_eq!(row(&rows, BANDIT_1).await["hpCurrent"], 13);
    hit(&app, 50).await;
    applied(&engine, 1).await;
    assert_eq!(row(&rows, BANDIT_1).await["hpCurrent"], 20);
}

#[tokio::test]
async fn blows_queued_before_the_applier_runs_are_each_cut_against_the_hp_the_others_leave() {
    let (engine, rows, app) = served().await;
    tavern(&engine, 5).await;
    // 5 - 3 = 2, then 2 - 3 cut at 0: both applied, none below.
    hit(&app, -3).await;
    hit(&app, -3).await;
    applied(&engine, 2).await;
    assert_eq!(row(&rows, BANDIT_1).await["hpCurrent"], 0);
    // 0 + 15 = 15, 15 - 4 = 11, then 11 + 20 cut at 20.
    hit(&app, 15).await;
    hit(&app, -4).await;
    hit(&app, 20).await;
    assert_eq!(row(&rows, BANDIT_1).await["hpCurrent"], 20);
    applied(&engine, 3).await;
    assert_eq!(row(&rows, BANDIT_1).await["hpCurrent"], 20);
}

#[tokio::test]
async fn the_token_may_be_the_target_itself() {
    let (engine, rows, app) = served().await;
    tavern(&engine, 20).await;
    let (status, body) = command(&app, Some(BANDIT_1), json!({ "delta": -6 })).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    applied(&engine, 1).await;
    assert_eq!(row(&rows, BANDIT_1).await["hpCurrent"], 14);
}

#[tokio::test]
async fn pj_hp_not_tracked_a_change_of_a_pc_s_hp_is_rejected_and_its_hp_stay_untracked() {
    let (engine, rows, app) = served().await;
    tavern(&engine, 20).await;
    let before = row(&rows, JETON_ELOWEN).await;
    for delta in [-5, 0, 5] {
        let (status, body) = command(
            &app,
            Some(INSTANCE),
            json!({ "token": JETON_ELOWEN, "delta": delta }),
        )
        .await;
        assert_eq!(status, StatusCode::ACCEPTED, "{body}");
        let results = run(&engine).await;
        assert_eq!(results.len(), 1, "{results:?}");
        assert_eq!(results[0].status, State::Rejected, "{results:?}");
        assert_eq!(results[0].violations, ["pj-hp-not-tracked"]);
        assert_eq!(row(&rows, JETON_ELOWEN).await, before);
    }
}

#[tokio::test]
async fn what_the_tables_could_never_hold_or_no_token_of_the_instance_is_refused_and_nothing_is_queued()
 {
    let (engine, rows, app) = served().await;
    tavern(&engine, 20).await;
    let unknown = "70000000-0000-4000-8000-0000000000ff";
    for (target, payload, expected, error) in [
        (
            Some(INSTANCE),
            json!({ "token": BANDIT_1 }),
            StatusCode::UNPROCESSABLE_ENTITY,
            "delta est un nombre entier de PV",
        ),
        (
            Some(INSTANCE),
            json!({ "token": BANDIT_1, "delta": "-7" }),
            StatusCode::UNPROCESSABLE_ENTITY,
            "delta est un nombre entier de PV",
        ),
        (
            Some(INSTANCE),
            json!({ "token": BANDIT_1, "delta": -2.5 }),
            StatusCode::UNPROCESSABLE_ENTITY,
            "delta est un nombre entier de PV",
        ),
        (
            Some(INSTANCE),
            json!({ "token": "bandit-1", "delta": -7 }),
            StatusCode::UNPROCESSABLE_ENTITY,
            "token n'est pas un identifiant",
        ),
        (
            Some(INSTANCE),
            json!({ "token": unknown, "delta": -7 }),
            StatusCode::NOT_FOUND,
            "ce token n'est pas sur cette instance",
        ),
        // instance-isolated: the token stands on another instance.
        (
            Some(AUTRE_INSTANCE),
            json!({ "token": BANDIT_1, "delta": -7 }),
            StatusCode::NOT_FOUND,
            "ce token n'est pas sur cette instance",
        ),
    ] {
        let (status, body) = command(&app, target, payload.clone()).await;
        assert_eq!(status, expected, "{payload} {body}");
        assert_eq!(body, json!({ "error": error }), "{payload}");
    }
    // Nothing was queued: the applier finds nothing to run.
    applied(&engine, 0).await;
    assert_eq!(row(&rows, BANDIT_1).await["hpCurrent"], 20);
}

#[tokio::test]
async fn a_payload_naming_other_fields_steps_the_current_hp_alone() {
    let (engine, rows, app) = served().await;
    tavern(&engine, 20).await;
    let before = row(&rows, BANDIT_1).await;
    let (status, body) = command(
        &app,
        Some(INSTANCE),
        json!({ "token": BANDIT_1, "delta": -1, "hpMax": 99, "dead": true, "name": "Vesna" }),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    applied(&engine, 1).await;
    let mut wounded = before;
    wounded["hpCurrent"] = json!(19);
    assert_eq!(row(&rows, BANDIT_1).await, wounded);
}

#[tokio::test]
async fn a_caller_who_is_no_approver_has_the_change_reviewed_first() {
    let (engine, _, app) = served().await;
    tavern(&engine, 20).await;
    let (status, body) = command_by(
        &app,
        Some(INSTANCE),
        json!({ "token": BANDIT_1, "delta": -7 }),
        json!([]),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    assert_eq!(body["result"]["status"], "awaiting_review", "{body}");
    // Held until an approver decides: the applier runs nothing.
    applied(&engine, 0).await;
}
