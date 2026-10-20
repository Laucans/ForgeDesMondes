//! table.setTokenCondition through the wire protocol, over the in-memory
//! engine as the server plants it: a badge set is prepared against the
//! token's row, queued on the token's own partition and judged there — its
//! conditions alone change, never its hit points nor the other tokens of its
//! character (`instance-isolated`), and a read after holds them
//! (`state-persisted-reset`). The tokens are dropped by a test-only
//! DataCapability: the product's drop is table.placeTokens'. The table it
//! reaches is `set_token_condition_projection.rs`'s.

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

const INSTANCE: &str = "b1000000-0000-4000-8000-000000000001";
const AUTRE_INSTANCE: &str = "b1000000-0000-4000-8000-000000000002";
const BANDIT: &str = "e1000000-0000-4000-8000-0000000000b1";
const BANDITS: [&str; 3] = [
    "70000000-0000-4000-8000-000000000001",
    "70000000-0000-4000-8000-000000000002",
    "70000000-0000-4000-8000-000000000003",
];
/// A token of the ambush's instance.
const AILLEURS: &str = "70000000-0000-4000-8000-000000000004";

/// The test-only drop: a token inserted whole on its own partition.
const DROP_TOKEN: &str = r#"{"dataCapability":"test.dropToken","owner":"dataguard","version":1,"effect":"insert","target":{"aggregate":"BattlemapInstance","id":"$tokenId"},"mode":"overwrite","callableBy":["*"]}"#;

/// The engine as the server plants it, its rows and its wire protocol.
struct Served {
    engine: Arc<Engine>,
    rows: Rows,
    app: Router,
}

/// The server, Bandit 1 to 3 dropped on the tavern's instance and one token
/// on the ambush's.
async fn tavern() -> Served {
    let Planted { engine, rows } = planted(MEMORY).await.expect("the in-memory engine");
    let engine = Arc::new(
        engine.with_data_capability(DataCapability::parse(DROP_TOKEN).expect("contract F")),
    );
    let app = router(
        engine.clone(),
        rows.clone(),
        Arc::new(ports_llm::fake::FakeLlm::new()),
    );
    let served = Served { engine, rows, app };
    for (number, id) in (1..).zip(BANDITS) {
        served.drop_token(id, INSTANCE, number).await;
    }
    served.drop_token(AILLEURS, AUTRE_INSTANCE, 4).await;
    served
}

impl Served {
    /// Bandit `number` dropped on `instance` under `id`, applied.
    async fn drop_token(&self, id: &str, instance: &str, number: i64) {
        let Value::Object(payload) = json!({
            "tokenId": id,
            "tokenInstanceId": instance,
            "characterId": BANDIT,
            "number": number,
            "name": format!("Bandit {number}"),
            "q": 5,
            "r": 5,
            "visible": true,
            "hpCurrent": 11,
            "hpMax": 11,
            "conditions": [],
        }) else {
            unreachable!("a payload is an object");
        };
        self.engine
            .submit(Submission {
                data_capability: "test.dropToken".to_string(),
                payload: payload.into_iter().collect(),
                target_id: None,
                based_on: None,
                idempotency_key: None,
                caller: Caller {
                    by: "dm".to_string(),
                    from: "table.Battlemap".to_string(),
                    permissions: Vec::new(),
                    roles: vec!["role:approver".to_string()],
                },
            })
            .await
            .expect("queued");
        assert_eq!(self.settled().await.status, State::Applied);
    }

    /// `POST /commands` of table.setTokenCondition with `payload`, on
    /// `target`.
    async fn mark(&self, target: Option<&str>, payload: Value) -> (StatusCode, Value) {
        let mut body = json!({
            "dataCapability": "table.setTokenCondition@1",
            "payload": payload,
            "caller": { "by": "dm", "from": "table.Battlemap" },
        });
        if let Some(target) = target {
            body["target"] = json!({ "id": target });
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

    /// Whether nothing is queued.
    async fn idle(&self) -> bool {
        self.engine
            .run_once()
            .await
            .expect("the applier ran")
            .is_empty()
    }

    /// The token `id` as the engine holds it.
    async fn token(&self, id: &str) -> Value {
        let row = self
            .rows
            .projected(&format!("BattlemapInstance/{id}"))
            .await
            .expect("the token's row");
        Value::Object(row.into_iter().collect())
    }

    async fn tokens(&self) -> Vec<Value> {
        let mut tokens = Vec::new();
        for id in BANDITS.into_iter().chain([AILLEURS]) {
            tokens.push(self.token(id).await);
        }
        tokens
    }
}

#[tokio::test]
async fn poisoning_bandit_2_leaves_bandit_1_and_3_as_they_were_instance_isolated() {
    let served = tavern().await;
    let before = served.tokens().await;
    let (status, answer) = served
        .mark(
            Some(INSTANCE),
            json!({ "token": BANDITS[1], "state": " Empoisonné ", "on": true }),
        )
        .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{answer}");
    assert_eq!(
        answer["partition"],
        format!("BattlemapInstance/{}", BANDITS[1])
    );
    let result = served.settled().await;
    assert_eq!(result.status, State::Applied, "{result:?}");

    let after = served.tokens().await;
    let mut expected = before.clone();
    expected[1]["conditions"] = json!(["Empoisonné"]);
    assert_eq!(after, expected);
}

#[tokio::test]
async fn a_badge_set_is_still_there_when_the_token_is_read_again_state_persisted_reset() {
    let served = tavern().await;
    for (state, on) in [
        ("Empoisonné", true),
        ("À terre", true),
        ("Empoisonné", false),
    ] {
        let (status, answer) = served
            .mark(
                Some(INSTANCE),
                json!({ "token": BANDITS[1], "state": state, "on": on }),
            )
            .await;
        assert_eq!(status, StatusCode::ACCEPTED, "{answer}");
        assert_eq!(served.settled().await.status, State::Applied);
    }
    assert_eq!(
        served.token(BANDITS[1]).await["conditions"],
        json!(["À terre"])
    );
    assert_eq!(served.token(BANDITS[0]).await["conditions"], json!([]));
}

#[tokio::test]
async fn a_token_targeted_itself_is_marked_too() {
    let served = tavern().await;
    let (status, answer) = served
        .mark(Some(BANDITS[2]), json!({ "state": "Effrayé", "on": true }))
        .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{answer}");
    assert_eq!(served.settled().await.status, State::Applied);
    assert_eq!(
        served.token(BANDITS[2]).await["conditions"],
        json!(["Effrayé"])
    );
}

#[tokio::test]
async fn a_badge_with_no_state_or_no_switch_is_rejected_and_nothing_is_queued() {
    let served = tavern().await;
    let before = served.tokens().await;
    for (payload, error) in [
        (
            json!({ "token": BANDITS[1], "state": "  ", "on": true }),
            "un état a un nom",
        ),
        (
            json!({ "token": BANDITS[1], "on": true }),
            "un état a un nom",
        ),
        (
            json!({ "token": BANDITS[1], "state": "Empoisonné" }),
            "on vaut true ou false",
        ),
        (
            json!({ "token": BANDITS[1], "state": "Empoisonné", "on": "oui" }),
            "on vaut true ou false",
        ),
    ] {
        let (status, answer) = served.mark(Some(INSTANCE), payload.clone()).await;
        assert_eq!(
            status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "{payload}: {answer}"
        );
        assert_eq!(answer["error"], error);
    }
    assert!(served.idle().await);
    assert_eq!(served.tokens().await, before);
}

#[tokio::test]
async fn a_token_that_is_not_on_the_targeted_instance_is_not_found() {
    let served = tavern().await;
    let before = served.tokens().await;
    for token in [
        // A token of another instance.
        AILLEURS,
        // A token the engine does not hold.
        "70000000-0000-4000-8000-0000000000ff",
        // The instance itself, which is no token.
        INSTANCE,
    ] {
        let (status, answer) = served
            .mark(
                Some(INSTANCE),
                json!({ "token": token, "state": "Empoisonné", "on": true }),
            )
            .await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{token}: {answer}");
    }
    let (status, _) = served
        .mark(
            Some(INSTANCE),
            json!({ "token": "bandit-2", "state": "Empoisonné", "on": true }),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(served.idle().await);
    assert_eq!(served.tokens().await, before);
}
