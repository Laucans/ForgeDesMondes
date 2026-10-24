//! ai.proposeCharacter through the engine, over its in-memory store, as the
//! server plants it with the Entity aggregate, campaign.saveEntity and
//! campaign.restoreEntityVersion: a proposal is made before anything is
//! queued, judged, queued on the element and applied with the version of
//! source `ia` it adds — which a restoration undoes exactly. A proposal that
//! does not fill the sheet is refused and nothing is queued. The tables it
//! reaches are the Entity aggregate's `tests/projection.rs`.

use std::sync::Arc;

use aggregate_entity::History;
use harness_dataguard::Error;
use harness_dataguard::aggregate::Aggregate;
use harness_dataguard::api::Submission;
use harness_dataguard::command::{CommandResult, State};
use harness_dataguard::engine::Engine;
use harness_dataguard::manifest::DataCapability;
use harness_dataguard::memory::{MemoryNotifier, MemoryStore};
use harness_dataguard::plugins::{RefusalKind, Shared};
use harness_dataguard::postgres::SystemClock;
use harness_dataguard::store::{RandomIds, Store};
use ports_llm::fake::FakeLlm;
use ports_llm::{Llm, SharedLlm};
use ports_speech_to_text::SharedSpeechToText;
use ports_speech_to_text::fake::FakeSpeechToText;
use serde_json::{Map, Value, json};

const VALOMBRE: &str = "c1000000-0000-4000-8000-000000000001";

const PROMPT: &str = "Une ancienne sergente de la garde, balafrée, qui commande une bande de \
coupe-jarrets au gué.";

/// What the fake provider answers, in the editor's shape.
fn sergente() -> Value {
    json!({
        "background": "Renvoyée de la garde pour avoir refusé de brûler un village.",
        "behavior": "Parle peu, d’une voix basse.",
        "stats": { "FOR": 14, "DEX": 16, "CON": 14, "INT": 14, "SAG": 11, "CHA": 15 },
        "ca": 15,
        "pvMax": 65,
        "features": [{ "name": "Parade", "text": "Réaction : +2 à la CA." }]
    })
}

/// What became of a submission: queued, or refused before it was — as the
/// server answers it, 202, 404 or 422.
#[derive(Debug, PartialEq, Eq)]
enum Status {
    Accepted,
    NotFound,
    Unprocessable,
}

/// The engine as the server plants these DataCapabilities and their
/// aggregate, with the llm port's fake, whose journal its handle shares.
struct Served {
    engine: Engine,
    store: Arc<MemoryStore>,
    llm: FakeLlm,
}

async fn served(llm: FakeLlm) -> Served {
    let store = Arc::new(MemoryStore::default());
    let mut shared = Shared::default();
    shared.insert(Arc::new(SharedLlm::new(llm.clone())));
    shared.insert(Arc::new(SharedSpeechToText::new(FakeSpeechToText::new(
        PROMPT,
    ))));
    let engine = Engine::new(
        store.clone(),
        Arc::new(MemoryNotifier::default()),
        Arc::new(SystemClock),
    )
    .with_aggregate(Aggregate::parse(aggregate_entity::MANIFEST).expect("contract I"))
    .with_data_capability(DataCapability::parse(save_entity::MANIFEST).expect("contract F"))
    .with_data_capability(
        DataCapability::parse(restore_entity_version::MANIFEST).expect("contract F"),
    )
    .with_data_capability(DataCapability::parse(propose_character::MANIFEST).expect("contract F"))
    .with_ids(Arc::new(RandomIds))
    .with_plugins(
        [
            aggregate_entity::keeping(Arc::new(History::default())),
            save_entity::plugin(),
            restore_entity_version::plugin(),
            propose_character::plugin(),
        ]
        .into_iter()
        .collect(),
    )
    .with_shared(shared);
    engine.load().await.expect("the units fit");
    Served { engine, store, llm }
}

impl Served {
    /// `data_capability` on `target` by the DM, approver of their own
    /// campaign: what became of it, and the answer.
    async fn post(
        &self,
        data_capability: &str,
        payload: Value,
        target: Option<&str>,
        key: Option<&str>,
    ) -> (Status, Value) {
        let mut body = json!({
            "dataCapability": data_capability,
            "payload": payload,
            "caller": { "by": "dm", "from": "campaign.EntityEditor", "roles": ["role:approver"] },
        });
        if let Some(target) = target {
            body["targetId"] = json!(target);
        }
        if let Some(key) = key {
            body["idempotencyKey"] = json!(key);
        }
        let submission: Submission = serde_json::from_value(body).expect("a submission");
        match self.engine.submit(submission).await {
            Ok(submitted) => (
                Status::Accepted,
                serde_json::to_value(submitted).expect("JSON"),
            ),
            Err(Error::Refused(refusal)) => (
                match refusal.kind {
                    RefusalKind::NotFound => Status::NotFound,
                    RefusalKind::Unprocessable => Status::Unprocessable,
                },
                json!({ "error": refusal.message }),
            ),
            Err(e) => panic!("{e}"),
        }
    }

    /// One pass of the single applier: what it settled.
    async fn settled(&self) -> Vec<CommandResult> {
        self.engine.run_once().await.expect("the applier ran")
    }

    /// `data_capability` on `target`, queued with no warning and applied.
    async fn applied(&self, data_capability: &str, payload: Value, target: Option<&str>) -> Value {
        let (status, answer) = self.post(data_capability, payload, target, None).await;
        assert_eq!(status, Status::Accepted, "{answer}");
        assert_eq!(answer["warnings"], json!([]), "{answer}");
        let settled = self.settled().await;
        assert_eq!(settled.len(), 1, "{settled:?}");
        assert_eq!(settled[0].status, State::Applied, "{settled:?}");
        answer
    }

    /// An element of Valombre saved by the DM: its id.
    async fn saved(&self, entity: Value) -> String {
        let answer = self
            .applied(
                "campaign.saveEntity@1",
                json!({ "campaign": VALOMBRE, "entity": entity }),
                None,
            )
            .await;
        let partition = answer["partition"].as_str().expect("a partition");
        partition.trim_start_matches("Entity/").to_owned()
    }

    /// The element's row, as the engine holds it.
    async fn row(&self, id: &str) -> Map<String, Value> {
        self.store
            .row(&format!("Entity/{id}"))
            .await
            .expect("the row")
            .into_iter()
            .collect()
    }
}

/// What the DM sees of a row: its snapshot's fields and its aliases, an
/// absent field and a `null` being the same (EntityVersion@1).
fn sheet(row: &Map<String, Value>) -> Map<String, Value> {
    row["snapshot"]
        .as_object()
        .expect("a snapshot")
        .iter()
        .filter(|(_, value)| !value.is_null())
        .map(|(field, value)| (field.clone(), value.clone()))
        .collect()
}

#[tokio::test]
async fn a_proposal_fills_the_sheet_at_once_as_a_version_from_the_ia() {
    let s = served(FakeLlm::new().with_character("", sergente())).await;
    let npc = s
        .saved(json!({ "kind": "pnj", "name": "La sergente", "prompt": PROMPT }))
        .await;
    s.applied(
        "ai.proposeCharacter@1",
        json!({ "prompt": PROMPT }),
        Some(&npc),
    )
    .await;
    let row = s.row(&npc).await;
    assert_eq!(
        row["background"],
        json!("Renvoyée de la garde pour avoir refusé de brûler un village.")
    );
    assert_eq!(row["behavior"], json!("Parle peu, d’une voix basse."));
    for (field, score) in [
        ("strength", 14),
        ("dexterity", 16),
        ("constitution", 14),
        ("intelligence", 14),
        ("wisdom", 11),
        ("charisma", 15),
        ("armorClass", 15),
        ("hpMax", 65),
    ] {
        assert_eq!(row[field], json!(score), "{field}");
    }
    assert_eq!(
        row["actions"],
        json!([{ "name": "Parade", "description": "Réaction : +2 à la CA." }])
    );
    assert!(!row.contains_key("dexterityModifier"));
    assert_eq!(row["source"], json!("ia"));
    assert_eq!(row["versionNumber"], json!(2));
    assert_eq!(row["restoredFromVersionId"], Value::Null);
    assert_eq!(row["snapshot"]["hpMax"], json!(65));
    let diff = row["diff"].as_array().expect("a diff");
    assert!(!diff.is_empty());
    for change in diff {
        for key in ["field", "old", "new"] {
            assert!(change.get(key).is_some(), "{change}");
        }
    }
    // The element is untouched beyond its sheet.
    assert_eq!(row["name"], json!("La sergente"));
    assert_eq!(row["tokenColor"], json!("accent"));
}

#[tokio::test]
async fn undoing_the_proposal_or_going_back_to_the_version_before_restores_the_sheet_exactly() {
    let s = served(FakeLlm::new().with_character("", sergente())).await;
    let npc = s
        .saved(json!({ "kind": "pnj", "name": "La sergente", "prompt": PROMPT }))
        .await;
    let before = sheet(&s.row(&npc).await);
    s.applied(
        "ai.proposeCharacter@1",
        json!({ "prompt": PROMPT }),
        Some(&npc),
    )
    .await;
    let proposed = s.row(&npc).await;
    assert_ne!(sheet(&proposed), before);
    let every: Vec<usize> = (0..proposed["diff"].as_array().expect("a diff").len()).collect();
    s.applied(
        "campaign.restoreEntityVersion@1",
        json!({ "version": 1, "changes": every }),
        Some(&npc),
    )
    .await;
    assert_eq!(sheet(&s.row(&npc).await), before);

    s.applied("ai.proposeCharacter@1", json!({}), Some(&npc))
        .await;
    let again = s.row(&npc).await;
    assert_eq!(again["versionNumber"], json!(4));
    assert_eq!(
        again["restoredFromVersionId"],
        Value::Null,
        "a proposal after a restoration restores nothing"
    );
    s.applied(
        "campaign.restoreEntityVersion@1",
        json!({ "version": 1 }),
        Some(&npc),
    )
    .await;
    assert_eq!(sheet(&s.row(&npc).await), before);
}

#[tokio::test]
async fn a_dictated_prompt_is_transcribed_and_kept_as_the_sheets_prompt() {
    let s = served(FakeLlm::new().with_character("", sergente())).await;
    let npc = s
        .saved(json!({ "kind": "pnj", "name": "La sergente" }))
        .await;
    s.applied(
        "ai.proposeCharacter@1",
        json!({ "audio": "UklGRgECAw==" }),
        Some(&npc),
    )
    .await;
    assert_eq!(s.row(&npc).await["initialPrompt"], json!(PROMPT));
    let sent = s.llm.journal().list();
    assert_eq!(sent.len(), 1);
    assert!(!sent[0].text.contains("UklGRgECAw=="));
}

#[tokio::test]
async fn the_element_may_be_named_by_its_entity_id_instead_of_the_target() {
    let s = served(FakeLlm::new().with_character("", sergente())).await;
    let npc = s
        .saved(json!({ "kind": "pnj", "name": "La sergente", "prompt": PROMPT }))
        .await;
    let answer = s
        .applied("ai.proposeCharacter@1", json!({ "entityId": npc }), None)
        .await;
    assert_eq!(answer["partition"], json!(format!("Entity/{npc}")));
    assert_eq!(s.row(&npc).await["source"], json!("ia"));
}

#[tokio::test]
async fn an_incomplete_proposal_is_refused_naming_the_invariant_and_nothing_is_queued() {
    let mut partial = sergente();
    partial.as_object_mut().expect("an object").remove("ca");
    let s = served(FakeLlm::new().with_character("", partial)).await;
    let npc = s
        .saved(json!({ "kind": "pnj", "name": "La sergente", "prompt": PROMPT }))
        .await;
    let before = s.row(&npc).await;
    let (status, answer) = s
        .post(
            "ai.proposeCharacter@1",
            json!({ "prompt": PROMPT }),
            Some(&npc),
            None,
        )
        .await;
    assert_eq!(status, Status::Unprocessable);
    assert_eq!(
        answer["error"],
        json!("proposal-fills-sheet-fields : la proposition ne remplit pas armorClass")
    );
    assert!(s.settled().await.is_empty(), "nothing queued");
    assert_eq!(s.row(&npc).await, before, "nothing written");
}

#[tokio::test]
async fn an_unknown_element_is_not_found_and_a_place_unprocessable() {
    let s = served(FakeLlm::new().with_character("", sergente())).await;
    let (status, _) = s
        .post(
            "ai.proposeCharacter@1",
            json!({ "prompt": PROMPT }),
            Some("e1000000-0000-4000-8000-0000000000ff"),
            None,
        )
        .await;
    assert_eq!(status, Status::NotFound);
    let (status, _) = s
        .post(
            "ai.proposeCharacter@1",
            json!({ "prompt": PROMPT }),
            None,
            None,
        )
        .await;
    assert_eq!(status, Status::NotFound);
    let place = s.saved(json!({ "kind": "lieu", "name": "Le gué" })).await;
    let (status, answer) = s
        .post(
            "ai.proposeCharacter@1",
            json!({ "prompt": PROMPT }),
            Some(&place),
            None,
        )
        .await;
    assert_eq!(status, Status::Unprocessable);
    assert_eq!(
        answer["error"],
        json!("seul un PJ ou un PNJ a une fiche à proposer")
    );
    assert!(s.llm.journal().list().is_empty(), "nothing sent");
}

#[tokio::test]
async fn an_idempotency_key_answers_the_first_proposal_again_without_asking_the_provider() {
    let s = served(FakeLlm::new().with_character("", sergente())).await;
    let npc = s
        .saved(json!({ "kind": "pnj", "name": "La sergente", "prompt": PROMPT }))
        .await;
    let (_, first) = s
        .post(
            "ai.proposeCharacter@1",
            json!({ "prompt": PROMPT }),
            Some(&npc),
            Some("k1"),
        )
        .await;
    s.settled().await;
    let (status, again) = s
        .post(
            "ai.proposeCharacter@1",
            json!({ "prompt": PROMPT }),
            Some(&npc),
            Some("k1"),
        )
        .await;
    assert_eq!(status, Status::Accepted);
    assert_eq!(again["commandId"], first["commandId"]);
    assert_eq!(s.llm.journal().list().len(), 1);
}

#[tokio::test]
async fn a_proposal_queued_behind_a_save_is_never_refused_and_applies_after_it() {
    let s = served(FakeLlm::new().with_character("", sergente())).await;
    let npc = s
        .saved(json!({ "kind": "pnj", "name": "La sergente", "prompt": PROMPT }))
        .await;
    let (status, _) = s
        .post(
            "campaign.saveEntity@1",
            json!({ "campaign": VALOMBRE, "entity": { "id": npc, "notes": "au gué" } }),
            None,
            None,
        )
        .await;
    assert_eq!(status, Status::Accepted);
    let (status, answer) = s
        .post(
            "ai.proposeCharacter@1",
            json!({ "prompt": PROMPT }),
            Some(&npc),
            None,
        )
        .await;
    assert_eq!(status, Status::Accepted, "{answer}");
    assert_eq!(answer["result"]["status"], json!("queued"), "{answer}");
    for _ in 0..2 {
        let settled = s.settled().await;
        assert!(
            settled.iter().all(|r| r.status == State::Applied),
            "{settled:?}"
        );
    }
    let row = s.row(&npc).await;
    assert_eq!(row["notes"], json!("au gué"));
    assert_eq!(row["source"], json!("ia"));
    assert_eq!(row["versionNumber"], json!(3));
}

#[tokio::test]
async fn a_pcs_proposal_writes_its_story_and_leaves_its_statblock_as_it_is() {
    let s = served(FakeLlm::new().with_character("", sergente())).await;
    let pc = s
        .saved(json!({
            "kind": "pj", "name": "Elowen", "stats": { "DEX": 18 }, "ca": 16, "pvMax": 44
        }))
        .await;
    s.applied(
        "ai.proposeCharacter@1",
        json!({ "prompt": "Une rôdeuse elfe." }),
        Some(&pc),
    )
    .await;
    let row = s.row(&pc).await;
    assert_eq!(row["source"], json!("ia"));
    assert_eq!(row["initialPrompt"], json!("Une rôdeuse elfe."));
    assert_eq!(row["behavior"], json!("Parle peu, d’une voix basse."));
    assert_eq!(row["dexterity"], json!(18));
    assert_eq!(row["armorClass"], json!(16));
    assert_eq!(row["hpMax"], json!(44));
    assert_eq!(row["actions"], json!([]));
    let changed: Vec<&str> = row["diff"]
        .as_array()
        .expect("a diff")
        .iter()
        .filter_map(|change| change["field"].as_str())
        .collect();
    assert_eq!(changed, ["background", "behavior", "initialPrompt"]);
}
