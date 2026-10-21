//! ai.extractFromNotes through the engine, over its in-memory store, as the
//! server plants it with the NoteImport aggregate: the notes are analysed
//! before anything is queued; the note's command is the one submitted and
//! answered, each proposal's a follow-up the engine submits after it, and
//! the note is applied after its proposals. Notes of no campaign, or holding
//! no word, are refused and nothing is queued; an idempotency key answers
//! the first analysis again. The tables an analysis reaches, and the
//! elements it recognises there, are the NoteImport aggregate's
//! `tests/projection.rs`.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
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
use ports_llm::fake::FakeLlm;
use ports_llm::{Llm, SharedLlm};
use ports_speech_to_text::SharedSpeechToText;
use ports_speech_to_text::fake::FakeSpeechToText;
use serde_json::{Value, json};

const VALOMBRE: &str = "c1000000-0000-4000-8000-000000000001";
const GASPARD: &str = "e1000000-0000-4000-8000-000000000001";

/// The draft of notes the mock-up pastes, as mock/fixtures/NoteImport.json
/// holds it: a copy, the crate reading nothing outside its folder.
const DRAFT: &str = "Valombre — notes en vrac après la séance 2.\n\
Le moulin a brûlé dans la nuit, le meunier a disparu. Gaspard (le tavernier du Sanglier) dit avoir vu des cavaliers en cape grise.\n\
Mère Ombeline, l’herboriste aveugle à l’orée du bois de Corbière, a soigné les apprentis. Elle sent la fumée sur les gens.\n\
Aerin, le frère d’Elowen, aurait été vu avec ces cavaliers.\n\
La Tour d’Ébène : ruine au nord du mont Pourpre, son sceau est un œil fermé.\n\
À creuser : le péage illégal au gué, 5 po par tête.";

type Row = BTreeMap<String, Value>;

/// The engine as the server plants this DataCapability and its aggregate,
/// over a store holding Valombre, and the llm port's fake, whose journal its
/// handle shares.
struct Served {
    engine: Engine,
    store: Arc<MemoryStore>,
    llm: FakeLlm,
}

/// A row written straight into the store, as an applied command leaves it.
async fn written(store: &MemoryStore, partition: &str, values: Value) {
    let write = Write {
        partition: partition.to_owned(),
        values: serde_json::from_value(values).expect("an object"),
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

async fn served() -> Served {
    serving(None).await
}

/// The tables, as a fake: every query answers `rows`, and what it was asked
/// with is kept.
struct Tables {
    rows: Value,
    asked: Mutex<Vec<Vec<Value>>>,
}

#[async_trait]
impl Rows for Tables {
    async fn rows(&self, _sql: &str, parameters: &[Value]) -> harness_dataguard::Result<Value> {
        self.asked
            .lock()
            .expect("the questions")
            .push(parameters.to_vec());
        Ok(self.rows.clone())
    }
}

/// [`served`], the Prepares reading `tables` when there are some.
async fn serving(tables: Option<Arc<Tables>>) -> Served {
    let store = Arc::new(MemoryStore::default());
    written(
        &store,
        &format!("Campaign/{VALOMBRE}"),
        json!({ "id": VALOMBRE, "name": "Les Cendres de Valombre" }),
    )
    .await;
    let llm = FakeLlm::new();
    let mut shared = Shared::default();
    shared.insert(Arc::new(SharedLlm::new(llm.clone())));
    shared.insert(Arc::new(SharedSpeechToText::new(FakeSpeechToText::new(
        DRAFT,
    ))));
    let engine = Engine::new(
        store.clone(),
        Arc::new(MemoryNotifier::default()),
        Arc::new(SystemClock),
    )
    .with_aggregate(Aggregate::parse(aggregate_note_import::MANIFEST).expect("contract I"))
    .with_data_capability(DataCapability::parse(extract_from_notes::MANIFEST).expect("contract F"))
    .with_ids(Arc::new(RandomIds))
    .with_plugins(
        [
            aggregate_note_import::plugin(),
            extract_from_notes::plugin(),
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
    Served { engine, store, llm }
}

impl Served {
    /// ai.extractFromNotes by the DM, approver of their own campaign: the
    /// answer, or why it was refused.
    async fn submitted(&self, payload: Value, key: Option<&str>) -> Result<Submitted, Error> {
        let mut body = json!({
            "dataCapability": "ai.extractFromNotes@1",
            "payload": payload,
            "caller": { "by": "dm", "from": "NoteImport", "roles": ["role:approver"] },
        });
        if let Some(key) = key {
            body["idempotencyKey"] = json!(key);
        }
        let submission: Submission = serde_json::from_value(body).expect("a submission");
        self.engine.submit(submission).await
    }

    /// The row the engine holds for `partition`.
    async fn row(&self, partition: &str) -> Row {
        self.store.row(partition).await.expect("the row")
    }

    /// One pass of the applier: what it ran, all applied.
    async fn applied(&self) -> Vec<String> {
        let ran = self.engine.run_once().await.expect("the applier ran");
        assert!(ran.iter().all(|r| r.status == State::Applied), "{ran:?}");
        ran.into_iter().map(|r| r.command_id).collect()
    }
}

/// The follow-ups submitted, each queued.
fn follow_ups(answer: &Submitted) -> Vec<&Submitted> {
    answer
        .then
        .iter()
        .map(|follow_up| match follow_up {
            FollowUp::Submitted(submitted) => submitted.as_ref(),
            FollowUp::Refused { error, .. } => panic!("a follow-up refused: {error}"),
        })
        .collect()
}

fn refused(result: Result<Submitted, Error>) -> RefusalKind {
    match result {
        Err(Error::Refused(refusal)) => refusal.kind,
        other => panic!("refused, not {other:?}"),
    }
}

#[tokio::test]
async fn the_proposals_follow_the_note_are_applied_before_it_and_the_note_keeps_the_raw_text() {
    let s = served().await;
    let answer = s
        .submitted(json!({ "campaign": VALOMBRE, "text": DRAFT }), None)
        .await
        .expect("queued");
    let note = answer.partition.clone();
    assert!(note.starts_with("NoteImport/"), "{note}");
    assert!(answer.warnings.is_empty(), "{:?}", answer.warnings);
    let proposals = follow_ups(&answer);
    assert!(proposals.len() >= 3, "{proposals:?}");
    for proposal in &proposals {
        assert!(proposal.partition.starts_with("NoteImport/"));
        assert!(proposal.partition < note, "the note takes the greatest id");
        assert!(proposal.warnings.is_empty(), "{:?}", proposal.warnings);
    }

    let applied = s.applied().await;
    assert_eq!(applied.len(), proposals.len() + 1, "{applied:?}");
    assert_eq!(applied.last(), Some(&answer.command_id), "the note last");
    assert!(s.applied().await.is_empty());

    let id = note.trim_start_matches("NoteImport/");
    let row = s.row(&note).await;
    assert_eq!(row["importedNoteId"], json!(id));
    assert_eq!(row["campaignId"], json!(VALOMBRE));
    assert_eq!(row["rawText"], json!(DRAFT));
    assert!(row["analyzedAt"].as_str().is_some_and(|t| t.ends_with('Z')));
    assert!(
        !row.contains_key("createdEntityId"),
        "no element is created"
    );
    let mut names = Vec::new();
    for proposal in &proposals {
        let row = s.row(&proposal.partition).await;
        assert_eq!(row["noteId"], json!(id));
        assert_eq!(
            row["noteProposalId"].as_str(),
            proposal.partition.strip_prefix("NoteImport/")
        );
        // Over the engine's memory, no element is known: each is checked.
        assert_eq!(row["existingEntityId"], Value::Null);
        assert_eq!(row["checked"], json!(true));
        names.push(row["name"].as_str().expect("a name").to_owned());
    }
    assert!(names.iter().any(|n| n == "Mère Ombeline"), "{names:?}");
}

#[tokio::test]
async fn an_element_the_tables_hold_by_a_name_or_an_alias_is_proposed_unchecked() {
    let tables = Arc::new(Tables {
        rows: json!([{ "id": GASPARD, "name": "Gaspard Grosmollet", "aliases": ["Gaspard"] }]),
        asked: Mutex::default(),
    });
    let s = serving(Some(tables.clone())).await;
    let answer = s
        .submitted(json!({ "campaign": VALOMBRE, "text": DRAFT }), None)
        .await
        .expect("queued");
    assert_eq!(
        *tables.asked.lock().expect("the questions"),
        [vec![json!(VALOMBRE)]]
    );
    s.applied().await;
    let mut gaspard = None;
    for proposal in follow_ups(&answer) {
        let row = s.row(&proposal.partition).await;
        if row["name"] == "Gaspard" {
            gaspard = Some(row);
        } else {
            assert_eq!(row["checked"], json!(true), "{row:?}");
        }
    }
    let gaspard = gaspard.expect("Gaspard proposed");
    assert_eq!(gaspard["existingEntityId"], json!(GASPARD));
    assert_eq!(gaspard["checked"], json!(false));
}

#[tokio::test]
async fn a_dictation_is_heard_through_the_port_and_only_its_transcript_is_stored() {
    let s = served().await;
    let answer = s
        .submitted(
            json!({ "campaign": VALOMBRE, "dictation": STANDARD.encode([3_u8; 64]) }),
            None,
        )
        .await
        .expect("queued");
    assert!(answer.then.len() >= 3, "{answer:?}");
    s.applied().await;
    let row = s.row(&answer.partition).await;
    assert_eq!(row["rawText"], json!(DRAFT));
    assert!(!row.contains_key("dictation"), "the audio is not stored");
}

#[tokio::test]
async fn when_the_llm_port_fails_the_note_is_kept_alone_and_the_dm_is_told() {
    let s = served().await;
    // The AI settings make active a provider the fake does not answer.
    written(
        &s.store,
        "AiSettings/1",
        json!({ "mode": "api", "url": "https://inconnu.example.org" }),
    )
    .await;
    let answer = s
        .submitted(json!({ "campaign": VALOMBRE, "text": DRAFT }), None)
        .await
        .expect("queued");
    assert!(answer.then.is_empty(), "{answer:?}");
    assert_eq!(answer.warnings.len(), 1, "{:?}", answer.warnings);
    assert!(answer.warnings[0].contains("la note est gardée"));
    assert_eq!(s.applied().await, std::slice::from_ref(&answer.command_id));
    let row = s.row(&answer.partition).await;
    assert_eq!(row["rawText"], json!(DRAFT));
    assert_eq!(row["analyzedAt"], Value::Null);
}

#[tokio::test]
async fn notes_of_a_campaign_the_engine_does_not_hold_are_not_found_and_nothing_is_queued() {
    let s = served().await;
    let unknown = "c1000000-0000-4000-8000-0000000000ff";
    let answer = s
        .submitted(json!({ "campaign": unknown, "text": DRAFT }), None)
        .await;
    match answer {
        Err(Error::Refused(refusal)) => {
            assert_eq!(refusal.kind, RefusalKind::NotFound);
            assert_eq!(refusal.message, format!("aucune campagne {unknown}"));
        }
        other => panic!("refused, not {other:?}"),
    }
    assert!(s.llm.journal().list().is_empty(), "nothing is analysed");
    assert!(s.applied().await.is_empty());
}

#[tokio::test]
async fn notes_that_name_no_campaign_or_hold_no_word_are_unprocessable_and_nothing_is_queued() {
    let s = served().await;
    for payload in [
        json!({ "text": DRAFT }),
        json!({ "campaign": "valombre", "text": DRAFT }),
        json!({ "campaign": VALOMBRE, "text": "   " }),
        json!({ "campaign": VALOMBRE, "dictation": "pas du base64 !" }),
        // What a follow-up carries, never announced.
        json!({}),
    ] {
        let kind = refused(s.submitted(payload.clone(), None).await);
        assert_eq!(kind, RefusalKind::Unprocessable, "{payload}");
    }
    assert!(s.applied().await.is_empty());
}

#[tokio::test]
async fn a_row_no_analysis_announced_is_no_follow_up() {
    let s = served().await;
    let answer = s
        .submitted(json!({ "campaign": VALOMBRE, "text": DRAFT }), None)
        .await
        .expect("queued");
    // Its follow-ups were taken back: the same row, named again with an
    // empty payload, is an analysis of nothing.
    let proposal = follow_ups(&answer)[0].partition.clone();
    let again = Submission {
        data_capability: "ai.extractFromNotes@1".to_owned(),
        target_id: proposal.strip_prefix("NoteImport/").map(str::to_owned),
        caller: serde_json::from_value(json!({ "by": "dm", "from": "NoteImport" }))
            .expect("a caller"),
        ..Submission::default()
    };
    let kind = refused(s.engine.submit(again).await);
    assert_eq!(kind, RefusalKind::Unprocessable);
}

#[tokio::test]
async fn the_same_idempotency_key_answers_the_first_analysis_again() {
    let s = served().await;
    let payload = json!({ "campaign": VALOMBRE, "text": DRAFT });
    let first = s
        .submitted(payload.clone(), Some("import-1"))
        .await
        .expect("queued");
    let again = s
        .submitted(payload.clone(), Some("import-1"))
        .await
        .expect("replayed");
    assert!(again.replayed);
    assert_eq!(again.command_id, first.command_id);
    assert!(again.then.is_empty(), "a replay submits no follow-up");
    assert_eq!(
        s.llm.journal().list().len(),
        1,
        "the notes are analysed once"
    );
    let applied = s.applied().await;
    assert_eq!(applied.len(), first.then.len() + 1);
    let other = s.submitted(payload, None).await.expect("queued");
    assert_eq!(
        s.applied().await.len(),
        other.then.len() + 1,
        "a second analysis, without the key"
    );
}
