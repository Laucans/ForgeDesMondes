//! session.createSession through the wire protocol, over the in-memory engine
//! as the server plants it: a new session is prepared at the position after
//! its campaign's last, judged against the PCs and the sessions the engine
//! holds of its campaign (`present-only-pj`, `oneshot-single-session`), and
//! applied on its own partition. The tables it reaches are
//! `create_session_projection.rs`'s.

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

/// A campaign written under the id its payload names: no DataCapability of
/// the product creates one yet.
const CREATE_CAMPAIGN: &str = r#"{"dataCapability":"test.createCampaign","owner":"dataguard","version":1,"effect":"insert","target":{"aggregate":"Campaign","id":"$id"},"mode":"overwrite","callableBy":["*"]}"#;

const VALOMBRE: &str = "c1000000-0000-4000-8000-000000000001";
const BRUME: &str = "c1000000-0000-4000-8000-000000000002";
const ONESHOT: &str = "c1000000-0000-4000-8000-000000000003";

/// The engine as the server plants it, its rows and its wire protocol.
struct Served {
    engine: Arc<Engine>,
    rows: Rows,
    app: Router,
}

async fn served() -> Served {
    let Planted { engine, rows } = planted(MEMORY).await.expect("the in-memory engine");
    let engine = Arc::new(
        engine.with_data_capability(DataCapability::parse(CREATE_CAMPAIGN).expect("contract F")),
    );
    let app = router(
        engine.clone(),
        rows.clone(),
        Arc::new(ports_llm::fake::FakeLlm::new()),
    );
    Served { engine, rows, app }
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

    /// Every command the single applier settles in one pass.
    async fn settle(&self) -> Vec<CommandResult> {
        self.engine.run_once().await.expect("the applier ran")
    }

    /// One pass of the single applier, which settles one command.
    async fn settled(&self) -> CommandResult {
        let mut settled = self.settle().await;
        assert_eq!(settled.len(), 1, "{settled:?}");
        settled.remove(0)
    }

    /// A campaign of `kind`, written as the tables would hold it.
    async fn campaign(&self, id: &str, kind: &str) {
        let payload = json!({ "id": id, "name": format!("La campagne {id}"), "kind": kind });
        let Value::Object(payload) = payload else {
            unreachable!()
        };
        self.engine
            .submit(Submission {
                data_capability: "test.createCampaign".to_string(),
                payload: payload.into_iter().collect(),
                target_id: None,
                based_on: None,
                idempotency_key: None,
                caller: Caller {
                    by: "dm".to_string(),
                    from: "campaign.home".to_string(),
                    permissions: Vec::new(),
                    roles: vec!["role:approver".to_string()],
                },
            })
            .await
            .expect("queued");
        applied(&self.settled().await);
    }

    /// A character of `campaign`, `pj` or `pnj`, saved by the DM: its id.
    async fn character(&self, campaign: &str, kind: &str, name: &str) -> String {
        let body = json!({
            "dataCapability": "campaign.saveEntity@1",
            "payload": { "campaign": campaign, "entity": { "kind": kind, "name": name } },
            "caller": { "by": "dm", "from": "campaign.EntityEditor", "roles": ["role:approver"] },
        });
        let (status, answer) = self.post("/commands", body).await;
        assert_eq!(status, StatusCode::ACCEPTED, "{answer}");
        applied(&self.settled().await);
        answer["partition"]
            .as_str()
            .and_then(|p| p.strip_prefix("Entity/"))
            .unwrap_or_else(|| panic!("an Entity partition: {answer}"))
            .to_owned()
    }

    /// `POST /commands` of session.createSession by SessionList — the DM
    /// approves their own campaign's critical fields, as `save_entity.rs`
    /// has them approve an element's: the status and the answer.
    async fn submit(&self, payload: Value, key: Option<&str>) -> (StatusCode, Value) {
        let mut body = json!({
            "dataCapability": "session.createSession@1",
            "payload": payload,
            "caller": { "by": "dm", "from": "SessionList", "roles": ["role:approver"] },
        });
        if let Some(key) = key {
            body["idempotencyKey"] = json!(key);
        }
        self.post("/commands", body).await
    }

    /// A new session of `payload`: its id, and what the applier made of it.
    async fn create(&self, payload: Value) -> (String, CommandResult) {
        let (status, answer) = self.submit(payload, None).await;
        assert_eq!(status, StatusCode::ACCEPTED, "{answer}");
        (session(&answer), self.settled().await)
    }

    /// The row the engine holds for `session`.
    async fn row(&self, session: &str) -> Map<String, Value> {
        self.rows
            .projected(&format!("Session/{session}"))
            .await
            .expect("the row")
            .into_iter()
            .collect()
    }
}

/// The session a submission's partition names.
fn session(answer: &Value) -> String {
    answer["partition"]
        .as_str()
        .and_then(|p| p.strip_prefix("Session/"))
        .unwrap_or_else(|| panic!("a Session partition: {answer}"))
        .to_owned()
}

fn applied(result: &CommandResult) {
    assert_eq!(result.status, State::Applied, "{result:?}");
}

fn rejected_by(result: &CommandResult, ids: &[&str]) {
    assert_eq!(result.status, State::Rejected, "{result:?}");
    assert_eq!(result.violations, ids, "{result:?}");
}

fn titled(campaign: &str, title: &str, present: &[&str]) -> Value {
    json!({ "campaign": campaign, "title": title, "present": present })
}

#[tokio::test]
async fn the_fourth_session_of_a_campaign_is_at_position_4_in_prep_with_its_two_pcs() {
    let served = served().await;
    served.campaign(VALOMBRE, "campagne").await;
    let elowen = served.character(VALOMBRE, "pj", "Elowen").await;
    let kael = served.character(VALOMBRE, "pj", "Kael").await;
    let sirielle = served.character(VALOMBRE, "pj", "Sirielle").await;
    for title in ["L'arrivée", "La forêt", "Le marais"] {
        let (_, result) = served.create(titled(VALOMBRE, title, &[&sirielle])).await;
        applied(&result);
    }
    let (fourth, result) = served
        .create(json!({
            "campaign": VALOMBRE,
            "title": "  La tour  ",
            "plannedOn": "2026-11-07",
            "present": [elowen, kael, elowen.to_uppercase()],
        }))
        .await;
    applied(&result);
    let row = served.row(&fourth).await;
    assert_eq!(row["sessionId"], json!(fourth));
    assert_eq!(row["sessionCampaignId"], json!(VALOMBRE));
    assert_eq!(row["sessionPosition"], json!(4));
    assert_eq!(row["sessionTitle"], json!("La tour"));
    assert_eq!(row["plannedOn"], json!("2026-11-07"));
    assert_eq!(row["sessionStatus"], json!("prep"));
    assert_eq!(row["microphoneOn"], json!(false));
    // Exactly the two ticked, each once.
    assert_eq!(
        row["sessionPresentCharacterCharacterId"],
        json!([elowen, kael])
    );
    assert_eq!(row["sessionPresentCharacterSessionId"], json!(fourth));
}

#[tokio::test]
async fn sessions_queued_side_by_side_take_the_positions_after_one_another() {
    let served = served().await;
    served.campaign(VALOMBRE, "campagne").await;
    let mut sessions = Vec::new();
    for title in ["Une", "Deux"] {
        let (status, answer) = served.submit(titled(VALOMBRE, title, &[]), None).await;
        assert_eq!(status, StatusCode::ACCEPTED, "{answer}");
        sessions.push(session(&answer));
    }
    // Both prepared at position 1, before either was applied.
    for result in served.settle().await {
        applied(&result);
    }
    let mut positions = Vec::new();
    for id in &sessions {
        positions.push(served.row(id).await["sessionPosition"].clone());
    }
    positions.sort_by_key(|p| p.as_u64());
    assert_eq!(positions, [json!(1), json!(2)]);
}

#[tokio::test]
async fn an_npc_or_a_pc_of_another_campaign_present_rejects_the_session_under_present_only_pj() {
    let served = served().await;
    served.campaign(VALOMBRE, "campagne").await;
    served.campaign(BRUME, "campagne").await;
    let elowen = served.character(VALOMBRE, "pj", "Elowen").await;
    let morlox = served.character(VALOMBRE, "pnj", "Morlox").await;
    let stranger = served.character(BRUME, "pj", "Vesna").await;
    for intruder in [&morlox, &stranger, &"elowen".to_owned()] {
        let (id, result) = served
            .create(titled(VALOMBRE, "La tour", &[&elowen, intruder]))
            .await;
        rejected_by(&result, &["present-only-pj"]);
        // No session is created.
        assert!(served.row(&id).await.is_empty(), "{id}");
    }
    let (_, result) = served.create(titled(VALOMBRE, "Seule", &[&elowen])).await;
    applied(&result);
}

#[tokio::test]
async fn a_oneshot_that_has_its_session_rejects_a_second_under_oneshot_single_session() {
    let served = served().await;
    served.campaign(ONESHOT, "oneshot").await;
    let (first, result) = served.create(titled(ONESHOT, "La nuit", &[])).await;
    applied(&result);
    let (second, result) = served.create(titled(ONESHOT, "Encore", &[])).await;
    rejected_by(&result, &["oneshot-single-session"]);
    assert!(served.row(&second).await.is_empty());
    assert_eq!(served.row(&first).await["sessionPosition"], json!(1));
    // A campagne takes as many as it is given.
    served.campaign(VALOMBRE, "campagne").await;
    for title in ["Une", "Deux"] {
        let (_, result) = served.create(titled(VALOMBRE, title, &[])).await;
        applied(&result);
    }
}

#[tokio::test]
async fn the_same_idempotency_key_twice_is_one_session() {
    let served = served().await;
    served.campaign(VALOMBRE, "campagne").await;
    let payload = titled(VALOMBRE, "La tour", &[]);
    let (status, first) = served.submit(payload.clone(), Some("k-1")).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{first}");
    applied(&served.settled().await);
    let (status, again) = served.submit(payload, Some("k-1")).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{again}");
    assert_eq!(again["commandId"], first["commandId"], "{again}");
    assert!(served.settle().await.is_empty(), "nothing more to apply");
    // The next session is the second, not the third.
    let (next, result) = served.create(titled(VALOMBRE, "Après", &[])).await;
    applied(&result);
    assert_eq!(served.row(&next).await["sessionPosition"], json!(2));
}

#[tokio::test]
async fn a_session_the_tables_could_never_hold_is_refused_422_and_not_queued() {
    let served = served().await;
    for payload in [
        json!({ "title": "Sans campagne" }),
        json!({ "campaign": "cendres-valombre", "title": "Par son nom" }),
        json!({ "campaign": VALOMBRE }),
        json!({ "campaign": VALOMBRE, "title": "   " }),
        json!({ "campaign": VALOMBRE, "title": "Un jour", "plannedOn": "2026-02-30" }),
        json!({ "campaign": VALOMBRE, "title": "Des PJ", "present": "elowen" }),
        json!({ "campaign": VALOMBRE, "title": "Des PJ", "present": [7] }),
        json!({ "campaign": VALOMBRE, "title": "Nul\u{0}" }),
    ] {
        let (status, answer) = served.submit(payload.clone(), None).await;
        assert_eq!(
            status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "{payload} {answer}"
        );
        assert!(answer["error"].is_string(), "{answer}");
    }
    assert!(served.settle().await.is_empty(), "nothing was queued");
}
