//! campaign.createEntitiesFromNote through the engine, over its in-memory
//! store, as the server plants it with the NoteImport and Entity aggregates:
//! the proposals an analysis of Valombre's notes made are validated in one
//! submission. The last proposal's command is the one submitted and
//! answered; the engine then submits its follow-ups — each element's save
//! (`campaign.saveEntity`), then every other proposal's command. Each
//! accepted proposal is checked and becomes an element of its type, name and
//! excerpt with its first version — a PC or an NPC with its sheet and token
//! colour — and records it; the others are unchecked and become nothing. A
//! proposal that created its element, or recognises one the campaign holds,
//! never makes another. A validation of no campaign or no proposal is
//! refused, as is one of another campaign's proposal, and nothing is
//! queued; an idempotency key answers the first validation again. The
//! tables a validation reaches are the NoteImport aggregate's
//! `tests/projection.rs`.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use create_entities_from_note::Fresh;
use harness_dataguard::Error;
use harness_dataguard::aggregate::Aggregate;
use harness_dataguard::api::{FollowUp, Submission, Submitted};
use harness_dataguard::command::{CommandResult, State};
use harness_dataguard::engine::Engine;
use harness_dataguard::manifest::DataCapability;
use harness_dataguard::memory::{MemoryNotifier, MemoryStore};
use harness_dataguard::plugins::{RefusalKind, Shared};
use harness_dataguard::postgres::SystemClock;
use harness_dataguard::reads::Rows;
use harness_dataguard::store::{RandomIds, Store, Write};
use ports_llm::SharedLlm;
use ports_llm::fake::FakeLlm;
use ports_speech_to_text::SharedSpeechToText;
use ports_speech_to_text::fake::FakeSpeechToText;
use serde_json::{Value, json};

const VALOMBRE: &str = "c1000000-0000-4000-8000-000000000001";
const BRUMES: &str = "c1000000-0000-4000-8000-000000000002";

/// The draft of notes the mock-up pastes, as mock/fixtures/NoteImport.json
/// holds it: a copy, the crate reading nothing outside its folder.
const DRAFT: &str = "Valombre — notes en vrac après la séance 2.\n\
Le moulin a brûlé dans la nuit, le meunier a disparu. Gaspard (le tavernier du Sanglier) dit avoir vu des cavaliers en cape grise.\n\
Mère Ombeline, l’herboriste aveugle à l’orée du bois de Corbière, a soigné les apprentis. Elle sent la fumée sur les gens.\n\
Aerin, le frère d’Elowen, aurait été vu avec ces cavaliers.\n\
La Tour d’Ébène : ruine au nord du mont Pourpre, son sceau est un œil fermé.\n\
À creuser : le péage illégal au gué, 5 po par tête.";

type Fields = BTreeMap<String, Value>;

/// The DM, approver of their own campaign: the critical fields' review is
/// not what these tests judge.
fn caller() -> Value {
    json!({ "by": "dm", "from": "NoteImport", "roles": ["role:approver"] })
}

/// A row written straight into the store, as an applied command leaves it.
async fn written(store: &MemoryStore, partition: &str, values: Fields) {
    let write = Write {
        partition: partition.to_owned(),
        values,
        tombstone: false,
    };
    let result = CommandResult {
        command_id: format!("c_{partition}"),
        status: State::Applied,
        data_version: None,
        violations: Vec::new(),
        review_id: None,
        attachment: None,
    };
    store.apply(&write, &result).await.expect("written");
}

fn fields(value: Value) -> Fields {
    serde_json::from_value(value).expect("an object")
}

/// The engine over memory, holding Valombre and Brumes, as the server plants
/// the analysis, the validation, the save of an element and their
/// aggregates.
struct Served {
    engine: Engine,
    store: Arc<MemoryStore>,
}

/// The tables, as a fake: the partitions whose row the engine deleted.
#[derive(Default)]
struct Tombstones(Mutex<BTreeSet<String>>);

#[async_trait]
impl Rows for Tombstones {
    async fn rows(&self, _sql: &str, parameters: &[Value]) -> harness_dataguard::Result<Value> {
        let partition = parameters[0].as_str().expect("a partition");
        let deleted = self.0.lock().expect("the tombstones").contains(partition);
        Ok(if deleted {
            json!([{ "partition": partition }])
        } else {
            json!([])
        })
    }
}

impl Served {
    async fn new() -> Served {
        Self::reading(None).await
    }

    /// [`Served::new`], the Prepares reading `tables` when there are some.
    async fn reading(tables: Option<Arc<Tombstones>>) -> Served {
        let store = Arc::new(MemoryStore::default());
        for (id, name) in [(VALOMBRE, "Les Cendres de Valombre"), (BRUMES, "Brumes")] {
            written(
                &store,
                &format!("Campaign/{id}"),
                fields(json!({ "id": id, "name": name })),
            )
            .await;
        }
        let mut shared = Shared::default();
        shared.insert(Arc::new(SharedLlm::new(FakeLlm::new())));
        shared.insert(Arc::new(SharedSpeechToText::new(FakeSpeechToText::new(""))));
        let mut engine = Engine::new(
            store.clone(),
            Arc::new(MemoryNotifier::default()),
            Arc::new(SystemClock),
        )
        .with_ids(Arc::new(RandomIds));
        for aggregate in [aggregate_note_import::MANIFEST, aggregate_entity::MANIFEST] {
            engine = engine.with_aggregate(Aggregate::parse(aggregate).expect("contract I"));
        }
        for manifest in [
            extract_from_notes::MANIFEST,
            create_entities_from_note::MANIFEST,
            save_entity::MANIFEST,
        ] {
            engine =
                engine.with_data_capability(DataCapability::parse(manifest).expect("contract F"));
        }
        let engine = engine
            .with_plugins(
                [
                    aggregate_note_import::plugin(),
                    aggregate_entity::plugin(),
                    extract_from_notes::plugin(),
                    create_entities_from_note::plugin(),
                    save_entity::plugin(),
                ]
                .into_iter()
                .collect(),
            )
            .with_shared(shared);
        let engine = match tables {
            Some(tables) => engine.with_rows(tables),
            None => engine,
        };
        engine.load().await.expect("the units fit");
        Served { engine, store }
    }

    async fn post(&self, body: Value) -> Result<Submitted, Error> {
        let submission: Submission = serde_json::from_value(body).expect("a submission");
        self.engine.submit(submission).await
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
        self.store.row(partition).await.expect("a row")
    }

    /// Valombre's — or another campaign's — notes analysed, applied: the
    /// note's id, and its proposals' rows by id.
    async fn analysed(&self, campaign: &str) -> (String, BTreeMap<String, Fields>) {
        let answer = self
            .post(json!({
                "dataCapability": "ai.extractFromNotes@1",
                "payload": { "campaign": campaign, "text": DRAFT },
                "caller": caller(),
            }))
            .await
            .expect("queued");
        self.applied().await;
        let note = answer
            .partition
            .strip_prefix("NoteImport/")
            .expect("the note's partition")
            .to_owned();
        let mut proposals = BTreeMap::new();
        for follow_up in submitted(&answer) {
            let id = follow_up.partition.trim_start_matches("NoteImport/");
            proposals.insert(id.to_owned(), self.row(&follow_up.partition).await);
        }
        assert!(proposals.len() >= 3, "{proposals:?}");
        (note, proposals)
    }

    async fn validate(&self, payload: Value) -> Result<Submitted, Error> {
        self.post(validation(payload)).await
    }

    /// An element saved by the DM, applied: its id.
    async fn saved(&self, kind: &str, name: &str, aliases: &[&str]) -> String {
        let answer = self
            .post(json!({
                "dataCapability": "campaign.saveEntity@1",
                "payload": { "campaign": VALOMBRE, "entity": { "kind": kind, "name": name, "aliases": aliases } },
                "caller": caller(),
            }))
            .await
            .expect("queued");
        self.applied().await;
        answer
            .partition
            .strip_prefix("Entity/")
            .expect("the element's partition")
            .to_owned()
    }
}

fn validation(payload: Value) -> Value {
    json!({ "dataCapability": "campaign.createEntitiesFromNote@1", "payload": payload, "caller": caller() })
}

/// The follow-ups submitted, each queued.
fn submitted(answer: &Submitted) -> Vec<&Submitted> {
    answer
        .then
        .iter()
        .map(|follow_up| match follow_up {
            FollowUp::Submitted(submitted) => submitted.as_ref(),
            FollowUp::Refused { error, .. } => panic!("a follow-up refused: {error}"),
        })
        .collect()
}

/// The first proposal of `kind`, by id.
fn first<'a>(proposals: &'a BTreeMap<String, Fields>, kind: &str) -> &'a str {
    proposals
        .iter()
        .find(|(_, row)| row["type"] == kind)
        .map(|(id, _)| id.as_str())
        .unwrap_or_else(|| panic!("no {kind} proposed: {proposals:?}"))
}

fn refused(result: Result<Submitted, Error>) -> (RefusalKind, String) {
    match result {
        Err(Error::Refused(refusal)) => (refusal.kind, refusal.message),
        other => panic!("refused, not {other:?}"),
    }
}

#[tokio::test]
async fn each_accepted_proposal_becomes_one_element_with_its_first_version_and_the_others_none() {
    let s = Served::new().await;
    let (note, proposals) = s.analysed(VALOMBRE).await;
    let pnj = first(&proposals, "pnj");
    let lieu = first(&proposals, "lieu");
    let answer = s
        .validate(json!({ "campaign": VALOMBRE, "accepted": [pnj, lieu] }))
        .await
        .expect("queued");
    // The last proposal's command is the one submitted; the two elements'
    // saves follow it, then every other proposal's.
    let last = proposals.keys().next_back().expect("a proposal");
    assert_eq!(answer.partition, format!("NoteImport/{last}"));
    let partitions: Vec<&str> = submitted(&answer)
        .iter()
        .map(|follow_up| follow_up.partition.as_str())
        .collect();
    assert_eq!(partitions.len(), 1 + proposals.len(), "{partitions:?}");
    assert!(partitions[..2].iter().all(|p| p.starts_with("Entity/")));
    let applied = s.applied().await;
    assert_eq!(applied.len(), 2 + proposals.len(), "{applied:?}");
    assert_eq!(
        applied.last(),
        Some(&answer.command_id),
        "the answered last"
    );
    assert!(s.applied().await.is_empty());

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
        assert_eq!(element, Fresh::of(id).entity);
        let entity = s.row(&format!("Entity/{element}")).await;
        assert_eq!(entity["entityId"], json!(element));
        assert_eq!(entity["entityCampaignId"], json!(VALOMBRE));
        assert_eq!(entity["type"], before["type"]);
        assert_eq!(entity["name"], before["name"]);
        assert_eq!(entity["description"], before["excerpt"]);
        assert_eq!(entity["versionNumber"], 1);
        assert_eq!(entity["source"], "dm");
        assert_eq!(entity["entityVersionEntityId"], json!(element));
        if id == pnj {
            // A sheet, with a token colour to fall back on.
            assert_eq!(entity["characterEntityId"], json!(element));
            assert_eq!(entity["tokenColor"], "accent");
            assert_eq!(entity["snapshot"]["tokenColor"], "accent");
        } else {
            assert!(entity.get("characterEntityId").is_none_or(Value::is_null));
            assert!(entity.get("tokenColor").is_none_or(Value::is_null));
        }
        assert_eq!(entity["snapshot"]["name"], before["name"]);
        let diff = entity["diff"].as_array().expect("a diff");
        assert!(!diff.is_empty());
        assert!(
            diff.iter().all(|change| change["old"].is_null()),
            "{diff:?}"
        );
    }
}

#[tokio::test]
async fn validating_again_creates_no_second_element() {
    let s = Served::new().await;
    let (_, proposals) = s.analysed(VALOMBRE).await;
    let pnj = first(&proposals, "pnj");
    s.validate(json!({ "campaign": VALOMBRE, "accepted": [pnj] }))
        .await
        .expect("queued");
    s.applied().await;
    let created = s.row(&format!("NoteImport/{pnj}")).await["createdEntityId"].clone();
    let partition = format!("Entity/{}", created.as_str().expect("an element"));
    let element = s.row(&partition).await;

    let answer = s
        .validate(json!({ "campaign": VALOMBRE, "accepted": [pnj, pnj] }))
        .await
        .expect("queued");
    // The accepted proposal checked again, and nothing else: the others are
    // unchecked already.
    assert!(answer.then.is_empty(), "{answer:?}");
    assert_eq!(s.applied().await.len(), 1);
    let row = s.row(&format!("NoteImport/{pnj}")).await;
    assert_eq!(row["createdEntityId"], created);
    assert_eq!(row["checked"], true);
    assert_eq!(s.row(&partition).await, element);
}

#[tokio::test]
async fn a_validation_retried_after_a_failure_half_way_finds_its_element_and_makes_no_second() {
    let s = Served::new().await;
    let (_, proposals) = s.analysed(VALOMBRE).await;
    let pnj = first(&proposals, "pnj");
    // The failure: the element was queued, applied, and its proposal never
    // recorded it. It is queued as the validation saves it, under the id
    // its proposal gives it.
    let element = Fresh::of(pnj).entity;
    s.post(json!({
        "dataCapability": "campaign.saveEntity@1",
        "payload": { "campaign": VALOMBRE, "entity": { "id": element, "kind": "pnj", "name": proposals[pnj]["name"] } },
        "targetId": element,
        "caller": caller(),
    }))
    .await
    .expect("the element is queued");
    s.applied().await;
    let before = s.row(&format!("Entity/{element}")).await;
    assert_eq!(before["name"], proposals[pnj]["name"]);

    let answer = s
        .validate(json!({ "campaign": VALOMBRE, "accepted": [pnj] }))
        .await
        .expect("queued");
    // Only the proposals are written: the element is not submitted again.
    assert!(
        submitted(&answer)
            .iter()
            .all(|follow_up| follow_up.partition.starts_with("NoteImport/"))
    );
    assert_eq!(s.applied().await.len(), proposals.len());
    let row = s.row(&format!("NoteImport/{pnj}")).await;
    assert_eq!(row["createdEntityId"], json!(element));
    assert_eq!(row["checked"], true);
    assert_eq!(s.row(&format!("Entity/{element}")).await, before);
}

#[tokio::test]
async fn a_proposal_recording_an_element_the_engine_does_not_hold_makes_it_again() {
    let s = Served::new().await;
    let (_, proposals) = s.analysed(VALOMBRE).await;
    let pnj = first(&proposals, "pnj");
    // What a validation leaves when its element is rejected once applied:
    // the proposal's stamp, applied, and no element behind it.
    let element = Fresh::of(pnj).entity;
    let mut stamped = proposals[pnj].clone();
    stamped.insert("checked".into(), json!(true));
    stamped.insert("createdEntityId".into(), json!(element));
    written(&s.store, &format!("NoteImport/{pnj}"), stamped).await;
    assert!(s.row(&format!("Entity/{element}")).await.is_empty());

    s.validate(json!({ "campaign": VALOMBRE, "accepted": [pnj] }))
        .await
        .expect("queued");
    s.applied().await;
    let made = s.row(&format!("Entity/{element}")).await;
    assert_eq!(made["name"], proposals[pnj]["name"]);
    let row = s.row(&format!("NoteImport/{pnj}")).await;
    assert_eq!(row["createdEntityId"], json!(element));
    assert_eq!(row["checked"], true);
}

#[tokio::test]
async fn an_element_the_dm_deleted_is_never_made_again_and_its_proposal_keeps_it() {
    let tombstones = Arc::new(Tombstones::default());
    let s = Served::reading(Some(tombstones.clone())).await;
    let (_, proposals) = s.analysed(VALOMBRE).await;
    let pnj = first(&proposals, "pnj");
    let lieu = first(&proposals, "lieu");
    // Made, recorded, then deleted by the DM: the engine reads its row as
    // empty, and its tables hold its tombstone.
    let deleted = Fresh::of(pnj).entity;
    let mut stamped = proposals[pnj].clone();
    stamped.insert("createdEntityId".into(), json!(deleted));
    written(&s.store, &format!("NoteImport/{pnj}"), stamped).await;
    tombstones
        .0
        .lock()
        .expect("the tombstones")
        .insert(format!("Entity/{deleted}"));

    let answer = s
        .validate(json!({ "campaign": VALOMBRE, "accepted": [pnj, lieu] }))
        .await
        .expect("queued");
    let saves: Vec<String> = submitted(&answer)
        .iter()
        .filter(|follow_up| follow_up.partition.starts_with("Entity/"))
        .map(|follow_up| follow_up.partition.clone())
        .collect();
    assert_eq!(saves, [format!("Entity/{}", Fresh::of(lieu).entity)]);
    s.applied().await;
    assert!(s.row(&format!("Entity/{deleted}")).await.is_empty());
    let row = s.row(&format!("NoteImport/{pnj}")).await;
    assert_eq!(row["createdEntityId"], json!(deleted));
    assert_eq!(row["checked"], true);
}

#[tokio::test]
async fn a_proposal_recognising_an_element_of_the_campaign_creates_none_even_accepted() {
    let s = Served::new().await;
    let gaspard = s.saved("pnj", "Gaspard Grosmollet", &["Gaspard"]).await;
    let (_, proposals) = s.analysed(VALOMBRE).await;
    // Over memory the analysis recognises nothing: the proposal is made one
    // that recognises Gaspard, as an analysis over the tables makes it.
    let (recognising, row) = proposals.iter().next().expect("a proposal");
    let mut row = row.clone();
    row.insert("existingEntityId".into(), json!(gaspard));
    written(&s.store, &format!("NoteImport/{recognising}"), row).await;

    let answer = s
        .validate(json!({ "campaign": VALOMBRE, "accepted": [recognising] }))
        .await
        .expect("queued");
    assert!(
        submitted(&answer)
            .iter()
            .all(|follow_up| follow_up.partition.starts_with("NoteImport/")),
        "no element"
    );
    s.applied().await;
    let row = s.row(&format!("NoteImport/{recognising}")).await;
    assert_eq!(row["checked"], true);
    assert!(row.get("createdEntityId").is_none_or(Value::is_null));
}

#[tokio::test]
async fn an_element_the_dataguard_rejects_is_told_and_validated_again_it_is_made_again() {
    let s = Served::new().await;
    let (_, proposals) = s.analysed(VALOMBRE).await;
    let pnj = first(&proposals, "pnj");
    let lieu = first(&proposals, "lieu");
    // Since the analysis, another element answers to the NPC's name.
    let name = proposals[pnj]["name"].as_str().expect("a name").to_owned();
    let other = s.saved("pnj", "Un autre", &[&name]).await;
    let element = Fresh::of(pnj).entity;

    let answer = s
        .validate(json!({ "campaign": VALOMBRE, "accepted": [pnj, lieu] }))
        .await
        .expect("queued");
    // The save of the element is told at risk, among the follow-ups.
    let save = submitted(&answer)
        .into_iter()
        .find(|follow_up| follow_up.partition == format!("Entity/{element}"))
        .expect("the element's save");
    assert!(
        save.warnings
            .iter()
            .any(|w| w == "invariant(s) at risk: alias-unique-in-campaign"),
        "{:?}",
        save.warnings
    );
    // The element is rejected once applied, and the rest applied.
    let ran = s.engine.run_once().await.expect("the applier ran");
    let rejected: Vec<_> = ran.iter().filter(|r| r.status == State::Rejected).collect();
    assert_eq!(rejected.len(), 1, "{ran:?}");
    assert_eq!(rejected[0].command_id, save.command_id);
    assert_eq!(rejected[0].violations, ["alias-unique-in-campaign"]);
    assert!(
        ran.iter()
            .all(|r| r.status == State::Applied || r.status == State::Rejected),
        "{ran:?}"
    );
    // Its proposal records an element the engine does not hold.
    assert_eq!(
        s.row(&format!("NoteImport/{pnj}")).await["createdEntityId"],
        json!(element)
    );
    assert!(s.row(&format!("Entity/{element}")).await.is_empty());
    let place = s.row(&format!("NoteImport/{lieu}")).await;
    assert!(place["createdEntityId"].is_string(), "{place:?}");

    // Once the other element lets its alias go, a validation makes it.
    s.post(json!({
        "dataCapability": "campaign.saveEntity@1",
        "payload": { "campaign": VALOMBRE, "entity": { "id": other, "kind": "pnj", "name": "Un autre", "aliases": [] } },
        "targetId": other,
        "caller": caller(),
    }))
    .await
    .expect("queued");
    s.applied().await;
    s.validate(json!({ "campaign": VALOMBRE, "accepted": [pnj] }))
        .await
        .expect("queued");
    s.applied().await;
    assert_eq!(
        s.row(&format!("Entity/{element}")).await["name"],
        json!(name)
    );
}

#[tokio::test]
async fn a_validation_of_no_campaign_or_no_proposal_is_unprocessable_and_nothing_is_queued() {
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
        // What a follow-up carries, never announced.
        (json!({}), "une validation nomme sa campagne"),
    ] {
        let (kind, message) = refused(s.validate(payload.clone()).await);
        assert_eq!(kind, RefusalKind::Unprocessable, "{payload}");
        assert_eq!(message, why, "{payload}");
    }
    assert!(s.engine.run_once().await.expect("ran").is_empty());
}

#[tokio::test]
async fn a_proposal_of_another_campaign_or_of_none_is_not_found_and_nothing_is_queued() {
    let s = Served::new().await;
    let (note, proposals) = s.analysed(BRUMES).await;
    let pnj = first(&proposals, "pnj");
    for unknown in [pnj, note.as_str(), "f1000000-0000-4000-8000-0000000000ff"] {
        let (kind, message) = refused(
            s.validate(json!({ "campaign": VALOMBRE, "accepted": [unknown] }))
                .await,
        );
        assert_eq!(kind, RefusalKind::NotFound, "{unknown}");
        assert_eq!(
            message,
            format!("aucune proposition {unknown} dans cette campagne")
        );
    }
    assert!(s.engine.run_once().await.expect("ran").is_empty());
    // Brumes' own validation goes through, its campaign in another case: the
    // element is of the campaign its note is of.
    s.validate(json!({ "campaign": BRUMES.to_uppercase(), "accepted": [pnj] }))
        .await
        .expect("queued");
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
    let first_answer = s.post(body.clone()).await.expect("queued");
    s.applied().await;
    let created = s.row(&format!("NoteImport/{pnj}")).await["createdEntityId"].clone();

    let again = s.post(body).await.expect("replayed");
    assert!(again.replayed);
    assert_eq!(again.command_id, first_answer.command_id);
    assert!(again.then.is_empty(), "a replay submits no follow-up");
    assert!(s.engine.run_once().await.expect("ran").is_empty());
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
    let answer = s.post(body).await.expect("queued");
    let element = Fresh::of(pnj).entity;
    let follow_ups = submitted(&answer);
    let save = follow_ups
        .iter()
        .find(|follow_up| follow_up.partition == format!("Entity/{element}"))
        .expect("the element's save");
    assert!(
        save.warnings.iter().any(|w| w.starts_with("review ")),
        "{:?}",
        save.warnings
    );
    let warnings = answer
        .warnings
        .iter()
        .chain(follow_ups.iter().flat_map(|f| f.warnings.iter()));
    assert!(warnings.into_iter().all(|w| !w.contains("at risk")));
    let stamp = std::iter::once(&answer)
        .chain(follow_ups.iter().copied())
        .find(|s| s.partition == format!("NoteImport/{pnj}"))
        .expect("the proposal's command");
    assert!(
        matches!(stamp.result.status, State::Queued | State::AwaitingReview),
        "{stamp:?}"
    );
}
