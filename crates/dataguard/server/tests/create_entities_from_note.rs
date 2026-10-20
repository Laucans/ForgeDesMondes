//! campaign.createEntitiesFromNote through the wire protocol, over the
//! in-memory engine: the proposals an analysis of Valombre's notes made are
//! validated in one command. Each accepted proposal is checked and becomes an
//! element of its type, name and excerpt with its first version — a PC or an
//! NPC with its sheet and token colour — and records it; the others are
//! unchecked and become nothing. A proposal that created its element, or
//! recognises one the campaign holds, never makes another. A validation of
//! no campaign or no proposal is refused, as is one of another campaign's
//! proposal, and nothing is queued; an idempotency key answers the first
//! validation again.

use std::collections::BTreeMap;
use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use dataguard_server::{Engine, MEMORY, Planted, Reads, Rows, planted, router_reading};
use harness_dataguard::command::State;
use harness_dataguard::manifest::DataCapability;
use ports_llm::fake::FakeLlm;
use ports_speech_to_text::fake::FakeSpeechToText;
use serde_json::{Value, json};
use tower::ServiceExt;

/// A campaign written: the Campaign aggregate's partition the notes need.
const ADD_CAMPAIGN: &str = r#"{"dataCapability":"test.addCampaign","owner":"dataguard","version":1,"effect":"insert","target":{"aggregate":"Campaign","id":"$id"},"mode":"overwrite","callableBy":["*"]}"#;

const VALOMBRE: &str = "c1000000-0000-4000-8000-000000000001";
const BRUMES: &str = "c1000000-0000-4000-8000-000000000002";

/// The DM, approver of their own campaign: the critical fields' review is
/// not what these tests judge.
fn caller() -> Value {
    json!({ "by": "dm", "from": "NoteImport", "roles": ["role:approver"] })
}

fn draft() -> String {
    let fixture: Value =
        serde_json::from_str(include_str!("../../../../mock/fixtures/NoteImport.json"))
            .expect("the fixture is JSON");
    fixture["draft"].as_str().expect("a draft").to_owned()
}

type Fields = BTreeMap<String, Value>;

/// The engine over memory, holding Valombre and Brumes, its rows, and its
/// wire protocol.
struct Served {
    engine: Arc<Engine>,
    rows: Rows,
    app: Router,
}

impl Served {
    async fn new() -> Served {
        let Planted { engine, rows } = planted(MEMORY).await.expect("the in-memory engine");
        let engine = Arc::new(
            engine.with_data_capability(DataCapability::parse(ADD_CAMPAIGN).expect("contract F")),
        );
        let app = router_reading(
            engine.clone(),
            rows.clone(),
            Arc::new(FakeLlm::new()),
            Arc::new(FakeSpeechToText::new(String::new())),
            Arc::new(Reads::open(MEMORY).await.expect("no database")),
        );
        let served = Served { engine, rows, app };
        for (id, name) in [(VALOMBRE, "Les Cendres de Valombre"), (BRUMES, "Brumes")] {
            let (status, answer) = served
                .post(json!({
                    "dataCapability": "test.addCampaign@1",
                    "payload": { "id": id, "name": name, "kind": "campagne" },
                    "target": { "id": id },
                    "caller": caller(),
                }))
                .await;
            assert_eq!(status, StatusCode::ACCEPTED, "{answer}");
        }
        served.applied().await;
        served
    }

    async fn post(&self, body: Value) -> (StatusCode, Value) {
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

    /// Runs the applier once and requires every command it ran applied:
    /// their ids, in the order applied.
    async fn applied(&self) -> Vec<String> {
        let applied = self.engine.run_once().await.expect("the applier ran");
        assert!(
            applied.iter().all(|r| r.status == State::Applied),
            "{applied:?}"
        );
        applied.into_iter().map(|r| r.command_id).collect()
    }

    async fn row(&self, partition: &str) -> Fields {
        self.rows.projected(partition).await.expect("a row")
    }

    /// Valombre's notes analysed, applied: the note's id, and its proposals'
    /// rows by id.
    async fn analysed(&self, campaign: &str) -> (String, BTreeMap<String, Fields>) {
        let (status, answer) = self
            .post(json!({
                "dataCapability": "ai.extractFromNotes@1",
                "payload": { "campaign": campaign, "text": draft() },
                "caller": caller(),
            }))
            .await;
        assert_eq!(status, StatusCode::ACCEPTED, "{answer}");
        self.applied().await;
        let note = answer["partition"]
            .as_str()
            .and_then(|p| p.strip_prefix("NoteImport/"))
            .expect("the note's partition")
            .to_owned();
        let mut proposals = BTreeMap::new();
        for id in self.rows.proposals(&note) {
            let row = self.row(&format!("NoteImport/{id}")).await;
            proposals.insert(id, row);
        }
        assert!(proposals.len() >= 3, "{proposals:?}");
        (note, proposals)
    }

    async fn validate(&self, payload: Value) -> (StatusCode, Value) {
        self.post(validation(payload)).await
    }

    /// An element saved by the DM, applied: its id.
    async fn saved(&self, kind: &str, name: &str, aliases: &[&str]) -> String {
        let (status, answer) = self
            .post(json!({
                "dataCapability": "campaign.saveEntity@1",
                "payload": { "campaign": VALOMBRE, "entity": { "kind": kind, "name": name, "aliases": aliases } },
                "caller": caller(),
            }))
            .await;
        assert_eq!(status, StatusCode::ACCEPTED, "{answer}");
        self.applied().await;
        answer["partition"]
            .as_str()
            .and_then(|p| p.strip_prefix("Entity/"))
            .expect("the element's partition")
            .to_owned()
    }
}

fn validation(payload: Value) -> Value {
    json!({ "dataCapability": "campaign.createEntitiesFromNote@1", "payload": payload, "caller": caller() })
}

/// The first proposal of `kind`, by id.
fn first<'a>(proposals: &'a BTreeMap<String, Fields>, kind: &str) -> &'a str {
    proposals
        .iter()
        .find(|(_, row)| row["type"] == kind)
        .map(|(id, _)| id.as_str())
        .unwrap_or_else(|| panic!("no {kind} proposed: {proposals:?}"))
}

#[tokio::test]
async fn each_accepted_proposal_becomes_one_element_with_its_first_version_and_the_others_none() {
    let s = Served::new().await;
    let (note, proposals) = s.analysed(VALOMBRE).await;
    let pnj = first(&proposals, "pnj");
    let lieu = first(&proposals, "lieu");
    let (status, answer) = s
        .validate(json!({ "campaign": VALOMBRE, "accepted": [pnj, lieu] }))
        .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{answer}");
    let applied = s.applied().await;
    // Two elements, then every proposal checked or unchecked — the answered
    // one last.
    assert_eq!(applied.len(), 2 + proposals.len(), "{applied:?}");
    assert_eq!(
        applied.last().map(String::as_str),
        answer["commandId"].as_str()
    );
    assert!(
        s.engine
            .run_once()
            .await
            .expect("the applier ran")
            .is_empty()
    );

    for (id, before) in &proposals {
        let row = s.row(&format!("NoteImport/{id}")).await;
        assert_eq!(row["noteId"], json!(note));
        let accepted = id == pnj || id == lieu;
        assert_eq!(row["checked"], json!(accepted), "{}", before["name"]);
        let Some(element) = row.get("createdEntityId").and_then(Value::as_str) else {
            assert!(!accepted, "{} created nothing", before["name"]);
            continue;
        };
        assert!(accepted, "{} was refused", before["name"]);
        let entity = s.row(&format!("Entity/{element}")).await;
        assert_eq!(entity["entityId"], json!(element));
        assert_eq!(entity["entityCampaignId"], json!(VALOMBRE));
        assert_eq!(entity["type"], before["type"]);
        assert_eq!(entity["name"], before["name"]);
        assert_eq!(entity["description"], before["excerpt"]);
        assert_eq!(entity["versionNumber"], 1);
        assert_eq!(entity["source"], "dm");
        assert_eq!(entity["entityVersionEntityId"], json!(element));
        let mut snapshot = json!({
            "type": before["type"],
            "name": before["name"],
            "description": before["excerpt"],
        });
        if id == pnj {
            // A sheet, with a token colour to fall back on.
            assert_eq!(entity["characterEntityId"], json!(element));
            assert_eq!(entity["tokenColor"], "accent");
            for (field, value) in [
                ("tokenColor", json!("accent")),
                ("spells", json!([])),
                ("actions", json!([])),
            ] {
                snapshot[field] = value;
            }
        } else {
            assert!(entity.get("characterEntityId").is_none_or(Value::is_null));
            assert!(entity.get("tokenColor").is_none_or(Value::is_null));
        }
        assert_eq!(entity["snapshot"], snapshot);
        let fields: Vec<&str> = entity["diff"]
            .as_array()
            .expect("a diff")
            .iter()
            .map(|change| {
                assert!(change["old"].is_null(), "{change}");
                assert_eq!(
                    change["new"],
                    snapshot[change["field"].as_str().expect("a field")]
                );
                change["field"].as_str().expect("a field")
            })
            .collect();
        let mut expected: Vec<&str> = snapshot
            .as_object()
            .expect("an object")
            .keys()
            .map(String::as_str)
            .collect();
        expected.sort_unstable();
        assert_eq!(fields, expected);
    }
}

#[tokio::test]
async fn validating_again_creates_no_second_element() {
    let s = Served::new().await;
    let (_, proposals) = s.analysed(VALOMBRE).await;
    let pnj = first(&proposals, "pnj");
    let (status, _) = s
        .validate(json!({ "campaign": VALOMBRE, "accepted": [pnj] }))
        .await;
    assert_eq!(status, StatusCode::ACCEPTED);
    s.applied().await;
    let created = s.row(&format!("NoteImport/{pnj}")).await["createdEntityId"].clone();
    let element = s
        .row(&format!("Entity/{}", created.as_str().expect("an element")))
        .await;

    let (status, answer) = s
        .validate(json!({ "campaign": VALOMBRE, "accepted": [pnj, pnj] }))
        .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{answer}");
    // The accepted proposal checked again, and nothing else: the others are
    // unchecked already.
    assert_eq!(s.applied().await.len(), 1);
    let row = s.row(&format!("NoteImport/{pnj}")).await;
    assert_eq!(row["createdEntityId"], created);
    assert_eq!(row["checked"], true);
    assert_eq!(
        s.row(&format!("Entity/{}", created.as_str().expect("an element")))
            .await,
        element
    );
}

#[tokio::test]
async fn a_proposal_recognising_an_element_of_the_campaign_creates_none_even_accepted() {
    let s = Served::new().await;
    let gaspard = s.saved("pnj", "Gaspard Grosmollet", &["Gaspard"]).await;
    let (_, proposals) = s.analysed(VALOMBRE).await;
    let (recognising, _) = proposals
        .iter()
        .find(|(_, row)| row.get("existingEntityId") == Some(&json!(gaspard)))
        .expect("a proposal recognising Gaspard");
    let (status, answer) = s
        .validate(json!({ "campaign": VALOMBRE, "accepted": [recognising] }))
        .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{answer}");
    // Every proposal checked or unchecked, and no element.
    assert!(s.applied().await.len() <= proposals.len());
    let row = s.row(&format!("NoteImport/{recognising}")).await;
    assert_eq!(row["checked"], true);
    assert!(row.get("createdEntityId").is_none_or(Value::is_null));
}

#[tokio::test]
async fn an_element_the_dataguard_rejects_is_told_and_its_proposal_records_nothing() {
    let s = Served::new().await;
    let (_, proposals) = s.analysed(VALOMBRE).await;
    let pnj = first(&proposals, "pnj");
    let lieu = first(&proposals, "lieu");
    // Since the analysis, another element answers to the NPC's name.
    let name = proposals[pnj]["name"].as_str().expect("a name").to_owned();
    s.saved("pnj", "Un autre", &[&name]).await;
    let before = s.row(&format!("NoteImport/{pnj}")).await;

    let (status, answer) = s
        .validate(json!({ "campaign": VALOMBRE, "accepted": [pnj, lieu] }))
        .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{answer}");
    assert_eq!(
        answer["warnings"],
        json!([format!(
            "{name} : invariant(s) at risk: alias-unique-in-campaign"
        )]),
        "{answer}"
    );
    // The element is rejected once applied, and the rest applied.
    let ran = s.engine.run_once().await.expect("the applier ran");
    let rejected: Vec<_> = ran.iter().filter(|r| r.status == State::Rejected).collect();
    assert_eq!(rejected.len(), 1, "{ran:?}");
    assert_eq!(rejected[0].violations, ["alias-unique-in-campaign"]);
    assert!(
        ran.iter()
            .all(|r| r.status == State::Applied || r.status == State::Rejected),
        "{ran:?}"
    );
    assert_eq!(s.row(&format!("NoteImport/{pnj}")).await, before);
    let place = s.row(&format!("NoteImport/{lieu}")).await;
    assert!(place["createdEntityId"].is_string(), "{place:?}");
}

#[tokio::test]
async fn when_every_element_accepted_is_rejected_the_last_rejection_is_answered() {
    let s = Served::new().await;
    let (_, proposals) = s.analysed(VALOMBRE).await;
    let pnj = first(&proposals, "pnj");
    let name = proposals[pnj]["name"].as_str().expect("a name").to_owned();
    // Every other proposal unchecked already: the validation of the NPC alone
    // has nothing else to write.
    let (status, _) = s
        .validate(json!({ "campaign": VALOMBRE, "accepted": [first(&proposals, "lieu")] }))
        .await;
    assert_eq!(status, StatusCode::ACCEPTED);
    s.applied().await;
    s.saved("lieu", "Ailleurs", &[&name]).await;

    let (status, answer) = s
        .validate(json!({ "campaign": VALOMBRE, "accepted": [pnj] }))
        .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{answer}");
    assert!(
        answer["partition"]
            .as_str()
            .is_some_and(|p| p.starts_with("Entity/")),
        "{answer}"
    );
    let ran = s.engine.run_once().await.expect("the applier ran");
    assert_eq!(ran.len(), 1, "{ran:?}");
    assert_eq!(
        ran[0].command_id,
        answer["commandId"].as_str().expect("a command")
    );
    assert_eq!(ran[0].status, State::Rejected, "{ran:?}");
    assert_eq!(ran[0].violations, ["alias-unique-in-campaign"]);
    let row = s.row(&format!("NoteImport/{pnj}")).await;
    assert!(
        row.get("createdEntityId").is_none_or(Value::is_null),
        "{row:?}"
    );
}

#[tokio::test]
async fn a_validation_of_no_campaign_or_no_proposal_is_422_and_nothing_is_queued() {
    let s = Served::new().await;
    let (_, proposals) = s.analysed(VALOMBRE).await;
    let pnj = first(&proposals, "pnj");
    for (payload, why) in [
        (
            json!({ "accepted": [pnj] }),
            "une validation nomme sa campagne",
        ),
        (
            json!({ "campaign": "  ", "accepted": [pnj] }),
            "une validation nomme sa campagne",
        ),
        (
            json!({ "campaign": VALOMBRE }),
            "une validation nomme les propositions acceptées",
        ),
        (
            json!({ "campaign": VALOMBRE, "accepted": [] }),
            "une validation nomme les propositions acceptées",
        ),
        (
            json!({ "campaign": VALOMBRE, "accepted": [pnj, 3] }),
            "une validation nomme les propositions acceptées",
        ),
    ] {
        let (status, answer) = s.validate(payload.clone()).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{payload}");
        assert_eq!(answer["error"], why, "{payload}");
    }
    assert!(
        s.engine
            .run_once()
            .await
            .expect("the applier ran")
            .is_empty()
    );
}

#[tokio::test]
async fn a_proposal_of_another_campaign_or_of_none_is_404_and_nothing_is_queued() {
    let s = Served::new().await;
    let (note, proposals) = s.analysed(BRUMES).await;
    let pnj = first(&proposals, "pnj");
    for unknown in [pnj, note.as_str(), "f1000000-0000-4000-8000-0000000000ff"] {
        let (status, answer) = s
            .validate(json!({ "campaign": VALOMBRE, "accepted": [unknown] }))
            .await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{unknown}");
        assert_eq!(
            answer["error"],
            format!("aucune proposition {unknown} dans cette campagne")
        );
    }
    assert!(
        s.engine
            .run_once()
            .await
            .expect("the applier ran")
            .is_empty()
    );
    // Brumes' own validation goes through, its campaign in another case: the
    // element is of the campaign its note is of.
    let (status, answer) = s
        .validate(json!({ "campaign": BRUMES.to_uppercase(), "accepted": [pnj] }))
        .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{answer}");
    s.applied().await;
    let element = s.row(&format!("NoteImport/{pnj}")).await["createdEntityId"].clone();
    let entity = s
        .row(&format!("Entity/{}", element.as_str().expect("an element")))
        .await;
    assert_eq!(entity["entityCampaignId"], json!(BRUMES));
}

#[tokio::test]
async fn the_same_idempotency_key_answers_the_first_validation_again() {
    let s = Served::new().await;
    let (_, proposals) = s.analysed(VALOMBRE).await;
    let pnj = first(&proposals, "pnj");
    let mut body = validation(json!({ "campaign": VALOMBRE, "accepted": [pnj] }));
    body["idempotencyKey"] = json!("valider-1");
    let (status, first_answer) = s.post(body.clone()).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{first_answer}");
    s.applied().await;
    let created = s.row(&format!("NoteImport/{pnj}")).await["createdEntityId"].clone();

    let (status, again) = s.post(body).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{again}");
    assert_eq!(again["commandId"], first_answer["commandId"]);
    assert!(
        s.engine
            .run_once()
            .await
            .expect("the applier ran")
            .is_empty()
    );
    assert_eq!(
        s.row(&format!("NoteImport/{pnj}")).await["createdEntityId"],
        created
    );
}

#[tokio::test]
async fn an_element_left_for_review_is_recorded_on_its_proposal() {
    let s = Served::new().await;
    let (_, proposals) = s.analysed(VALOMBRE).await;
    let pnj = first(&proposals, "pnj");
    let mut body = validation(json!({ "campaign": VALOMBRE, "accepted": [pnj.to_uppercase()] }));
    // A DM who is no approver: what touches a critical field waits for one.
    body["caller"]["roles"] = json!([]);
    let (status, answer) = s.post(body).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{answer}");
    let warnings = answer["warnings"].as_array().expect("warnings");
    assert!(!warnings.is_empty(), "{answer}");
    assert!(
        warnings
            .iter()
            .all(|w| !w.as_str().is_some_and(|w| w.contains("at risk"))),
        "{answer}"
    );
    let row = s.row(&format!("NoteImport/{pnj}")).await;
    let element = row["createdEntityId"]
        .as_str()
        .expect("the element recorded");
    assert_eq!(
        s.row(&format!("Entity/{element}")).await["name"],
        proposals[pnj]["name"]
    );
}
