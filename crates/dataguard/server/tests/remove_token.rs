//! `table.removeToken` through the wire, over the in-memory engine as
//! `plant()` binds it: a token taken off the map is the deletion of its own
//! partition, which the engine keeps as a tombstone — no field written, no
//! other partition touched: the entries keep their ranks, the combat its turn,
//! the other tokens their rows. What the tables then do of it — the entry
//! cascaded, the places closed up — is `remove_token_projection.rs`'s. The
//! instance's tokens, its combat and their entries are written by test-only
//! DataCapabilities, as `battlemap_instance_invariants.rs` writes them.

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
/// Chef bandit 1, at rank 2, the one removed.
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

/// `POST /commands` of table.removeToken with `payload`, on `target`
/// when one is given, by the DM from the battlemap: its status and answer.
async fn command(app: &Router, target: Option<&str>, payload: Value) -> (StatusCode, Value) {
    command_by(app, target, payload, json!(["role:approver"])).await
}

/// [`command`], by a caller holding `roles`. The token's id is a critical
/// field of the aggregate: the DM, its approver, sends its removal applied.
async fn command_by(
    app: &Router,
    target: Option<&str>,
    payload: Value,
    roles: Value,
) -> (StatusCode, Value) {
    let mut body = json!({
        "dataCapability": "table.removeToken@1",
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

/// The three entries and the combat, as the engine holds them.
async fn order(rows: &Rows) -> Vec<Value> {
    let mut order = Vec::new();
    for id in [ENTREE_ELOWEN, ENTREE_CHEF, ENTREE_KAEL, COMBAT] {
        order.push(row(rows, id).await);
    }
    order
}

#[tokio::test]
async fn a_token_of_the_instance_is_removed_and_no_other_row_is_written() {
    let (engine, rows, app) = served().await;
    ambush(&engine, false).await;
    let order_before = order(&rows).await;
    let kael = row(&rows, JETON_KAEL).await;
    let (status, body) = command(&app, Some(INSTANCE), json!({ "token": CHEF_BANDIT_1 })).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    assert_eq!(
        body["partition"],
        format!("BattlemapInstance/{CHEF_BANDIT_1}"),
        "{body}"
    );
    assert_eq!(body["result"]["status"], "queued", "{body}");
    applied(&engine, 1).await;
    // The entries keep their ranks, the combat its turn: the order closes up
    // by itself, and a turn is table.nextTurn's.
    assert_eq!(order(&rows).await, order_before);
    assert_eq!(row(&rows, JETON_KAEL).await, kael);
}

#[tokio::test]
async fn the_token_may_be_the_target_itself() {
    let (engine, _, app) = served().await;
    ambush(&engine, false).await;
    let (status, body) = command(&app, Some(CHEF_BANDIT_1), json!({})).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    assert_eq!(
        body["partition"],
        format!("BattlemapInstance/{CHEF_BANDIT_1}"),
        "{body}"
    );
    applied(&engine, 1).await;
}

#[tokio::test]
async fn a_payload_naming_ranks_or_a_turn_writes_none_of_them() {
    let (engine, rows, app) = served().await;
    ambush(&engine, false).await;
    let order_before = order(&rows).await;
    let (status, body) = command(
        &app,
        Some(INSTANCE),
        json!({ "token": CHEF_BANDIT_1, "rank": 1, "currentEntryId": ENTREE_KAEL }),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    applied(&engine, 1).await;
    assert_eq!(order(&rows).await, order_before);
}

#[tokio::test]
async fn what_names_no_uuid_or_no_token_of_the_instance_is_refused_and_nothing_is_queued() {
    let (engine, rows, app) = served().await;
    ambush(&engine, false).await;
    let unknown = "70000000-0000-4000-8000-0000000000ff";
    for (target, payload, expected, error) in [
        (
            Some(INSTANCE),
            json!({ "token": "chef-bandit-1" }),
            StatusCode::UNPROCESSABLE_ENTITY,
            "token n'est pas un identifiant",
        ),
        (
            Some("taverne"),
            json!({ "token": CHEF_BANDIT_1 }),
            StatusCode::UNPROCESSABLE_ENTITY,
            "instance n'est pas un identifiant",
        ),
        (
            None,
            json!({}),
            StatusCode::UNPROCESSABLE_ENTITY,
            "token n'est pas un identifiant",
        ),
        (
            Some(INSTANCE),
            json!({ "token": unknown }),
            StatusCode::NOT_FOUND,
            "ce token n'est pas sur cette instance",
        ),
        // instance-isolated: the token stands on another instance.
        (
            Some(AUTRE_INSTANCE),
            json!({ "token": CHEF_BANDIT_1 }),
            StatusCode::NOT_FOUND,
            "ce token n'est pas sur cette instance",
        ),
        // An entry's row, or the combat's, is no token.
        (
            Some(INSTANCE),
            json!({ "token": ENTREE_CHEF }),
            StatusCode::NOT_FOUND,
            "ce token n'est pas sur cette instance",
        ),
        (
            Some(INSTANCE),
            json!({ "token": COMBAT }),
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
    assert_eq!(row(&rows, CHEF_BANDIT_1).await["tokenId"], CHEF_BANDIT_1);
}

#[tokio::test]
async fn a_caller_who_is_no_approver_has_the_removal_reviewed_first() {
    let (engine, _, app) = served().await;
    ambush(&engine, false).await;
    let (status, body) = command_by(
        &app,
        Some(INSTANCE),
        json!({ "token": CHEF_BANDIT_1 }),
        json!([]),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    assert_eq!(body["result"]["status"], "awaiting_review", "{body}");
    // Held until an approver decides: the applier runs nothing.
    applied(&engine, 0).await;
}

/// `POST /commands` of `data_capability` on the instance, by the DM.
async fn other(app: &Router, data_capability: &str, payload: Value) -> StatusCode {
    let body = json!({
        "dataCapability": data_capability,
        "payload": payload,
        "target": { "id": INSTANCE },
        "caller": { "by": "dm", "from": "table.Battlemap", "roles": ["role:approver"] },
    });
    let request = Request::post("/commands")
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    app.clone()
        .oneshot(request)
        .await
        .expect("an answer")
        .status()
}

#[tokio::test]
async fn a_token_whose_removal_is_queued_is_none_to_remove_again_nor_to_write() {
    let (engine, rows, app) = served().await;
    ambush(&engine, false).await;
    let (status, body) = command(&app, Some(INSTANCE), json!({ "token": CHEF_BANDIT_1 })).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    // Queued, not applied: the DM clicks again, or edits the token meanwhile.
    let (status, body) = command(&app, Some(INSTANCE), json!({ "token": CHEF_BANDIT_1 })).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    for (data_capability, payload) in [
        (
            "table.renameToken@1",
            json!({ "token": CHEF_BANDIT_1, "name": "Vesna" }),
        ),
        (
            "table.setTokenVisibility@1",
            json!({ "token": CHEF_BANDIT_1, "visible": false }),
        ),
        (
            "table.setTokenCondition@1",
            json!({ "token": CHEF_BANDIT_1, "state": "À terre", "on": true }),
        ),
    ] {
        assert_eq!(
            other(&app, data_capability, payload).await,
            StatusCode::NOT_FOUND,
            "{data_capability}"
        );
    }
    // The removal alone runs: nothing would land after its tombstone and lift it.
    applied(&engine, 1).await;
    // Another token is written as ever.
    assert_eq!(
        other(
            &app,
            "table.setTokenVisibility@1",
            json!({ "token": JETON_KAEL, "visible": false }),
        )
        .await,
        StatusCode::ACCEPTED
    );
    applied(&engine, 1).await;
    assert_eq!(row(&rows, JETON_KAEL).await["visible"], false);
}
