//! `table.setTokenVisibility` through the wire, over the in-memory engine as
//! `plant()` binds it: a token hidden or revealed is its visibility alone,
//! on its own partition — its initiative entry keeps its rank, and so does
//! every other participant (`hidden-keeps-rank`). The instance's tokens, its
//! combat and their entries are written by test-only DataCapabilities, as
//! `battlemap_instance_invariants.rs` writes them: over the engine's memory
//! no table holds an instance to drop tokens on.

use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use dataguard_server::{Engine, MEMORY, Rows, planted, router};
use harness_dataguard::api::Submission;
use harness_dataguard::command::State;
use harness_dataguard::guard::Caller;
use harness_dataguard::manifest::DataCapability;
use serde_json::{Value, json};
use tower::ServiceExt;

const INSTANCE: &str = "b1000000-0000-4000-8000-000000000001";
/// Another scene's instance.
const AUTRE_INSTANCE: &str = "b1000000-0000-4000-8000-000000000002";
const ELOWEN: &str = "e1000000-0000-4000-8000-000000000001";
const KAEL: &str = "e1000000-0000-4000-8000-000000000003";
const VESNA: &str = "e1000000-0000-4000-8000-000000000004";
const JETON_ELOWEN: &str = "70000000-0000-4000-8000-000000000001";
const JETON_KAEL: &str = "70000000-0000-4000-8000-000000000002";
/// Chef bandit 1, the ambush.
const CHEF_BANDIT_1: &str = "70000000-0000-4000-8000-000000000003";
const COMBAT: &str = "c0000000-0000-4000-8000-000000000001";
const ENTREE_ELOWEN: &str = "1e000000-0000-4000-8000-000000000001";
const ENTREE_CHEF: &str = "1e000000-0000-4000-8000-000000000002";
const ENTREE_KAEL: &str = "1e000000-0000-4000-8000-000000000003";

/// The test-only DataCapabilities that set the table: their name, their
/// effect, the payload field naming the row.
const MANIFESTS: [(&str, &str, &str); 3] = [
    ("test.dropToken", "insert", "tokenId"),
    ("test.startCombat", "insert", "combatId"),
    ("test.addEntry", "insert", "initiativeEntryId"),
];

/// The engine as the server plants it, with the test-only DataCapabilities,
/// the rows it holds, and the wire it is served by.
async fn served() -> (Arc<Engine>, Rows, Router) {
    let planted = planted(MEMORY).await.expect("the in-memory engine");
    let mut engine = planted.engine;
    for (name, effect, id) in MANIFESTS {
        let manifest = json!({
            "dataCapability": name,
            "owner": "dataguard",
            "version": 1,
            "effect": effect,
            "target": { "aggregate": "BattlemapInstance", "id": format!("${id}") },
            "mode": "overwrite",
            "callableBy": ["*"],
        });
        engine = engine.with_data_capability(
            DataCapability::parse(&manifest.to_string()).expect("contract F"),
        );
    }
    let engine = Arc::new(engine);
    let app = router(
        engine.clone(),
        planted.rows.clone(),
        Arc::new(ports_llm::fake::FakeLlm::new()),
    );
    (engine, planted.rows, app)
}

/// A row written by a test-only DataCapability, applied.
async fn set(engine: &Engine, data_capability: &str, row: Value) {
    let Value::Object(payload) = row else {
        panic!("a payload is an object");
    };
    engine
        .submit(Submission {
            data_capability: data_capability.to_string(),
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

/// Runs the applier once: `count` commands, every one applied.
async fn applied(engine: &Engine, count: usize) {
    let results = engine.run_once().await.expect("the applier ran");
    assert_eq!(results.len(), count, "{results:?}");
    for result in &results {
        assert_eq!(result.status, State::Applied, "{result:?}");
    }
}

fn token(id: &str, character: &str, name: &str, visible: bool) -> Value {
    json!({
        "tokenId": id,
        "tokenInstanceId": INSTANCE,
        "characterId": character,
        "name": name,
        "q": 1,
        "r": 1,
        "visible": visible,
        "conditions": [],
    })
}

/// A PC's entry: no roll, the initiative the DM entered.
fn entry(id: &str, token: &str, initiative: i64, rank: i64) -> Value {
    json!({
        "initiativeEntryId": id,
        "initiativeEntryCombatId": COMBAT,
        "initiativeEntryTokenId": token,
        "roll": null,
        "initiative": initiative,
        "tiebreak": 0.5,
        "rank": rank,
    })
}

/// Elowen, Kael and Chef bandit 1 on the instance, the chief `hidden` or
/// not, and the combat: Elowen first, the chief second, Kael third.
async fn ambush(engine: &Engine, hidden: bool) {
    set(
        engine,
        "test.dropToken",
        token(JETON_ELOWEN, ELOWEN, "Elowen", true),
    )
    .await;
    set(
        engine,
        "test.dropToken",
        token(JETON_KAEL, KAEL, "Kael", true),
    )
    .await;
    set(
        engine,
        "test.dropToken",
        token(CHEF_BANDIT_1, VESNA, "Chef bandit 1", !hidden),
    )
    .await;
    set(
        engine,
        "test.startCombat",
        json!({
            "combatId": COMBAT,
            "combatInstanceId": INSTANCE,
            "round": 1,
            "currentEntryId": ENTREE_ELOWEN,
            "startedAt": "2026-10-20T20:05:00Z",
        }),
    )
    .await;
    for (id, token, initiative, rank) in [
        (ENTREE_ELOWEN, JETON_ELOWEN, 18, 1),
        (ENTREE_CHEF, CHEF_BANDIT_1, 15, 2),
        (ENTREE_KAEL, JETON_KAEL, 10, 3),
    ] {
        set(engine, "test.addEntry", entry(id, token, initiative, rank)).await;
    }
}

/// `POST /commands` of table.setTokenVisibility with `payload`, on `target`
/// when one is given, by the DM from the battlemap: its status and answer.
async fn command(app: &Router, target: Option<&str>, payload: Value) -> (StatusCode, Value) {
    command_by(app, target, payload, json!(["role:approver"])).await
}

/// [`command`], by a caller holding `roles`. The token's visibility is a
/// critical field of the aggregate: the DM, its approver, sends it applied.
async fn command_by(
    app: &Router,
    target: Option<&str>,
    payload: Value,
    roles: Value,
) -> (StatusCode, Value) {
    let mut body = json!({
        "dataCapability": "table.setTokenVisibility@1",
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

/// The row of `id`, as the engine holds it.
async fn row(rows: &Rows, id: &str) -> Value {
    json!(
        rows.projected(&format!("BattlemapInstance/{id}"))
            .await
            .expect("the row read")
    )
}

/// The ranks of the three entries, Elowen's, the chief's and Kael's.
async fn ranks(rows: &Rows) -> Vec<Value> {
    let mut ranks = Vec::new();
    for id in [ENTREE_ELOWEN, ENTREE_CHEF, ENTREE_KAEL] {
        ranks.push(row(rows, id).await["rank"].clone());
    }
    ranks
}

#[tokio::test]
async fn a_visible_token_submitted_with_visible_false_is_hidden_and_nothing_else_changes() {
    let (engine, rows, app) = served().await;
    ambush(&engine, false).await;
    let before = row(&rows, CHEF_BANDIT_1).await;
    let (status, body) = command(
        &app,
        Some(INSTANCE),
        json!({ "token": CHEF_BANDIT_1, "visible": false }),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    applied(&engine, 1).await;
    let mut hidden = before.clone();
    hidden["visible"] = json!(false);
    assert_eq!(row(&rows, CHEF_BANDIT_1).await, hidden);
    // The other tokens are as they were.
    assert_eq!(row(&rows, JETON_KAEL).await["visible"], true);
}

#[tokio::test]
async fn hidden_keeps_rank_a_hidden_participant_of_rank_2_revealed_moves_no_rank() {
    let (engine, rows, app) = served().await;
    ambush(&engine, true).await;
    let entries = [
        row(&rows, ENTREE_ELOWEN).await,
        row(&rows, ENTREE_CHEF).await,
        row(&rows, ENTREE_KAEL).await,
    ];
    assert_eq!(ranks(&rows).await, [json!(1), json!(2), json!(3)]);
    let (status, body) = command(
        &app,
        Some(INSTANCE),
        json!({ "token": CHEF_BANDIT_1, "visible": true }),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    applied(&engine, 1).await;
    assert_eq!(row(&rows, CHEF_BANDIT_1).await["visible"], true);
    // Its rank, and every other, unchanged: each entry is as it was.
    assert_eq!(ranks(&rows).await, [json!(1), json!(2), json!(3)]);
    for (id, before) in [ENTREE_ELOWEN, ENTREE_CHEF, ENTREE_KAEL]
        .into_iter()
        .zip(entries)
    {
        assert_eq!(row(&rows, id).await, before, "{id}");
    }
    // Hidden again, then revealed again: still second.
    for visible in [false, true] {
        let (status, body) = command(
            &app,
            Some(INSTANCE),
            json!({ "token": CHEF_BANDIT_1, "visible": visible }),
        )
        .await;
        assert_eq!(status, StatusCode::ACCEPTED, "{body}");
        applied(&engine, 1).await;
        assert_eq!(row(&rows, CHEF_BANDIT_1).await["visible"], visible);
        assert_eq!(ranks(&rows).await, [json!(1), json!(2), json!(3)]);
    }
}

#[tokio::test]
async fn the_token_may_be_the_target_itself() {
    let (engine, rows, app) = served().await;
    ambush(&engine, false).await;
    let (status, body) = command(&app, Some(CHEF_BANDIT_1), json!({ "visible": false })).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    applied(&engine, 1).await;
    assert_eq!(row(&rows, CHEF_BANDIT_1).await["visible"], false);
}

#[tokio::test]
async fn what_the_tables_could_never_hold_or_no_token_of_the_instance_is_refused_and_nothing_is_queued()
 {
    let (engine, rows, app) = served().await;
    ambush(&engine, false).await;
    let unknown = "70000000-0000-4000-8000-0000000000ff";
    for (target, payload, expected, error) in [
        (
            Some(INSTANCE),
            json!({ "token": CHEF_BANDIT_1 }),
            StatusCode::UNPROCESSABLE_ENTITY,
            "visible vaut true ou false",
        ),
        (
            Some(INSTANCE),
            json!({ "token": CHEF_BANDIT_1, "visible": "false" }),
            StatusCode::UNPROCESSABLE_ENTITY,
            "visible vaut true ou false",
        ),
        (
            Some(INSTANCE),
            json!({ "token": "chef-bandit-1", "visible": false }),
            StatusCode::UNPROCESSABLE_ENTITY,
            "token n'est pas un identifiant",
        ),
        (
            Some(INSTANCE),
            json!({ "token": unknown, "visible": false }),
            StatusCode::NOT_FOUND,
            "ce token n'est pas sur cette instance",
        ),
        // instance-isolated: the token stands on another instance.
        (
            Some(AUTRE_INSTANCE),
            json!({ "token": CHEF_BANDIT_1, "visible": false }),
            StatusCode::NOT_FOUND,
            "ce token n'est pas sur cette instance",
        ),
        // A combat's row is no token.
        (
            Some(INSTANCE),
            json!({ "token": COMBAT, "visible": false }),
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
    assert_eq!(row(&rows, CHEF_BANDIT_1).await["visible"], true);
}

#[tokio::test]
async fn a_payload_naming_other_fields_writes_the_visibility_alone() {
    let (engine, rows, app) = served().await;
    ambush(&engine, true).await;
    let before = row(&rows, CHEF_BANDIT_1).await;
    let (status, body) = command(
        &app,
        Some(INSTANCE),
        json!({ "token": CHEF_BANDIT_1, "visible": true, "rank": 1, "q": 9, "name": "Vesna" }),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    applied(&engine, 1).await;
    let mut revealed = before;
    revealed["visible"] = json!(true);
    assert_eq!(row(&rows, CHEF_BANDIT_1).await, revealed);
    assert_eq!(ranks(&rows).await, [json!(1), json!(2), json!(3)]);
}

#[tokio::test]
async fn a_caller_who_is_no_approver_has_the_visibility_reviewed_first() {
    let (engine, _, app) = served().await;
    ambush(&engine, true).await;
    let (status, body) = command_by(
        &app,
        Some(INSTANCE),
        json!({ "token": CHEF_BANDIT_1, "visible": true }),
        json!([]),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    assert_eq!(body["result"]["status"], "awaiting_review", "{body}");
    // Held until an approver decides: the applier runs nothing.
    applied(&engine, 0).await;
}
