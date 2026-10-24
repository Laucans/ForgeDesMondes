//! ai.analyzeBrief through the engine, over its in-memory store, as the
//! server plants it with the Session aggregate — and with campaign.saveEntity
//! and the Entity aggregate, which an NPC's update is a follow-up of. The
//! brief is analysed before anything is queued; its command is the one
//! submitted and answered, the scenes', the entries' and the NPCs' saves
//! follow-ups the engine submits after it. A brief of no session, holding no
//! word, or that the `llm` port cannot analyse, is refused and nothing is
//! queued; an idempotency key answers the first analysis again. The tables
//! the analysis reaches are `tests/tables.rs`'.

use std::collections::BTreeMap;
use std::sync::Arc;

use async_trait::async_trait;
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
use ports_llm::{Llm, RequestKind, SharedLlm};
use ports_speech_to_text::SharedSpeechToText;
use ports_speech_to_text::fake::FakeSpeechToText;
use serde_json::{Value, json};

const VALOMBRE: &str = "c1000000-0000-4000-8000-000000000001";
const SEANCE: &str = "51000000-0000-4000-8000-000000000004";
/// A session the tables hold and the engine does not.
const FIXTURE: &str = "51000000-0000-4000-8000-000000000005";
const TAVERNE: &str = "e1000000-0000-4000-8000-000000000002";
/// An NPC the tables hold and the engine does not.
const GASPARD: &str = "e1000000-0000-4000-8000-000000000009";

/// The brief of R12: the tavern and Morlox the campaign holds, frère Odric
/// and Vesna it does not.
const BRIEF: &str = "On reprend à la Taverne du Sanglier. Morlox débarque trempé, il a vendu le plan du gué. \
Deuxième scène, sur la route : un pèlerin, frère Odric, les arrête. \
Troisième scène, au gué. Leur cheffe, Vesna, qu’on appelle la Balafrée, attend. Morlox arrive en courant.";

type Row = BTreeMap<String, Value>;

/// The tables, as a fake: each query answers what its table holds here.
struct Tables {
    sessions: Value,
    known: Value,
}

#[async_trait]
impl Rows for Tables {
    async fn rows(&self, sql: &str, parameters: &[Value]) -> harness_dataguard::Result<Value> {
        let asked = parameters[0].as_str().unwrap_or_default();
        Ok(if sql.contains("FROM session") {
            self.sessions
                .as_array()
                .into_iter()
                .flatten()
                .filter(|session| session["id"] == asked)
                .cloned()
                .collect()
        } else if sql.contains("FROM entity e") {
            assert_eq!(asked, VALOMBRE, "the session's campaign");
            self.known.clone()
        } else if sql.contains("FROM media") {
            json!([])
        } else if sql.contains("FROM entity_to_create") {
            json!([{ "type": "lieu", "name": "Le gué" }])
        } else if sql.contains("FROM dataguard_queue") {
            // The store's rows are applied ones: nothing queued on them.
            json!([])
        } else {
            panic!("an unknown query: {sql}")
        })
    }
}

/// The engine as the server plants these units, over a store holding
/// Valombre's session 4, and the llm port's fake, whose journal its handle
/// shares.
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
    let store = Arc::new(MemoryStore::default());
    written(
        &store,
        &format!("Session/{SEANCE}"),
        json!({ "sessionId": SEANCE, "sessionCampaignId": VALOMBRE, "sessionStatus": "prep" }),
    )
    .await;
    let llm = FakeLlm::new();
    let mut shared = Shared::default();
    shared.insert(Arc::new(SharedLlm::new(llm.clone())));
    shared.insert(Arc::new(SharedSpeechToText::new(FakeSpeechToText::new(
        "Morlox débarque.",
    ))));
    let engine = Engine::new(
        store.clone(),
        Arc::new(MemoryNotifier::default()),
        Arc::new(SystemClock),
    )
    .with_aggregate(Aggregate::parse(aggregate_session::DOCUMENT).expect("contract I"))
    .with_aggregate(Aggregate::parse(aggregate_entity::MANIFEST).expect("contract I"))
    .with_data_capability(DataCapability::parse(analyze_brief::MANIFEST).expect("contract F"))
    .with_data_capability(DataCapability::parse(save_entity::MANIFEST).expect("contract F"))
    .with_ids(Arc::new(RandomIds))
    .with_plugins(
        [
            aggregate_session::plugin(),
            aggregate_entity::plugin(),
            save_entity::plugin(),
            analyze_brief::plugin(),
        ]
        .into_iter()
        .collect(),
    )
    .with_shared(shared);
    engine.load().await.expect("the units fit");
    Served { engine, store, llm }
}

impl Served {
    /// The engine reading `tables`.
    fn reading(self, tables: Tables) -> Self {
        Self {
            engine: self.engine.with_rows(Arc::new(tables)),
            ..self
        }
    }

    /// Valombre's NPC Morlox, saved by the DM and applied: his id.
    async fn morlox(&self) -> String {
        let answer = self
            .submit(
                "campaign.saveEntity",
                json!({ "campaign": VALOMBRE, "entity": { "kind": "pnj", "name": "Morlox", "notes": "Doit 40 po à Vesna." } }),
                None,
            )
            .await
            .expect("Morlox saved");
        self.applied().await;
        answer.partition.trim_start_matches("Entity/").to_owned()
    }

    /// `data_capability` by the DM, approver of their own campaign: the
    /// answer, or why it was refused.
    async fn submit(
        &self,
        data_capability: &str,
        payload: Value,
        key: Option<&str>,
    ) -> Result<Submitted, Error> {
        let mut body = json!({
            "dataCapability": data_capability,
            "payload": payload,
            "caller": { "by": "dm", "from": "OralBrief", "roles": ["role:approver"] },
        });
        if let Some(key) = key {
            body["idempotencyKey"] = json!(key);
        }
        let submission: Submission = serde_json::from_value(body).expect("a submission");
        self.engine.submit(submission).await
    }

    /// ai.analyzeBrief of `payload`.
    async fn analysed(&self, payload: Value) -> Result<Submitted, Error> {
        self.submit("ai.analyzeBrief@1", payload, None).await
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

fn refused(result: Result<Submitted, Error>) -> (RefusalKind, String) {
    match result {
        Err(Error::Refused(refusal)) => (refusal.kind, refusal.message),
        other => panic!("refused, not {other:?}"),
    }
}

fn morlox_row(known: &str) -> Value {
    json!({ "id": known, "type": "pnj", "name": "Morlox", "aliases": [] })
}

#[tokio::test]
async fn the_brief_is_answered_its_scenes_and_entries_follow_and_morlox_is_saved_last() {
    let s = served().await;
    let morlox = s.morlox().await;
    let s = s.reading(Tables {
        sessions: json!([]),
        known: json!([
            { "id": TAVERNE, "type": "lieu", "name": "La Taverne du Sanglier", "aliases": [] },
            morlox_row(&morlox),
        ]),
    });
    let answer = s
        .analysed(json!({ "session": SEANCE, "text": BRIEF, "durationSeconds": 95 }))
        .await
        .expect("queued");
    assert!(
        answer.partition.starts_with("Session/"),
        "{}",
        answer.partition
    );
    let brief = answer.partition.trim_start_matches("Session/").to_owned();
    let then = follow_ups(&answer);
    let partitions: Vec<&str> = then.iter().map(|f| f.partition.as_str()).collect();
    // Three scenes, then the entries — frère Odric and Vesna — then Morlox's
    // save: the tavern, known, is no entry.
    assert_eq!(partitions.len(), 6, "{partitions:?}");
    assert!(partitions[..5].iter().all(|p| p.starts_with("Session/")));
    assert!(partitions[..5].iter().all(|p| **p > *answer.partition));
    assert!(
        partitions[..5].is_sorted(),
        "applied in the order submitted"
    );
    assert_eq!(partitions[5], format!("Entity/{morlox}"));

    let applied = s.applied().await;
    assert_eq!(applied.len(), 7, "{applied:?}");
    assert!(s.applied().await.is_empty());

    let row = s.row(&answer.partition).await;
    assert_eq!(row["oralBriefId"], json!(brief));
    assert_eq!(row["oralBriefSessionId"], json!(SEANCE));
    assert_eq!(row["transcript"], json!(BRIEF));
    assert_eq!(row["durationSeconds"], json!(95));
    assert!(row["recordedAt"].as_str().is_some_and(|t| t.ends_with('Z')));
    assert_eq!(row["analyzedAt"], row["recordedAt"]);

    let scenes = [
        s.row(partitions[0]).await,
        s.row(partitions[1]).await,
        s.row(partitions[2]).await,
    ];
    let titles: Vec<&str> = scenes
        .iter()
        .map(|scene| scene["sceneTitle"].as_str().expect("a title"))
        .collect();
    assert_eq!(
        titles,
        [
            "On reprend à la Taverne du Sanglier",
            "Sur la route",
            "Au gué"
        ]
    );
    assert_eq!(scenes[0]["entityId"], json!([TAVERNE, morlox]));
    assert_eq!(scenes[1]["entityId"], json!([]));
    assert_eq!(scenes[2]["entityId"], json!([morlox]));
    assert!(scenes.iter().all(|scene| scene["sceneSessionId"] == SEANCE));

    let entries = [s.row(partitions[3]).await, s.row(partitions[4]).await];
    let named: Vec<(&str, &str)> = entries
        .iter()
        .map(|e| (e["type"].as_str().unwrap(), e["name"].as_str().unwrap()))
        .collect();
    assert_eq!(
        named,
        [("pnj", "Frère Odric"), ("pnj", "Vesna la Balafrée")]
    );
    for (entry, scene) in entries.iter().zip([&scenes[1], &scenes[2]]) {
        assert_eq!(entry["entityToCreateStatus"], "a_creer");
        assert_eq!(entry["briefId"], json!(brief));
        assert_eq!(entry["entityToCreateCampaignId"], VALOMBRE);
        assert_eq!(entry["entityToCreateSessionId"], SEANCE);
        assert_eq!(entry["entityToCreateSceneId"], scene["sceneId"]);
        assert!(!entry.contains_key("createdEntityId"), "nothing is created");
    }

    let saved = s.row(&format!("Entity/{morlox}")).await;
    assert_eq!(saved["source"], "brief");
    assert_eq!(saved["versionNumber"], 2);
    assert_eq!(
        saved["diff"],
        json!([{
            "field": "notes",
            "old": "Doit 40 po à Vesna.",
            "new": "Doit 40 po à Vesna.\nMorlox débarque trempé, il a vendu le plan du gué\nMorlox arrive en courant",
        }])
    );
    assert_eq!(saved["name"], "Morlox");
    assert_eq!(saved["type"], "pnj");

    // Only the text went to the provider, as a brief.
    let records = s.llm.journal().list();
    assert_eq!(records.len(), 1);
    assert_eq!(
        (records[0].kind, records[0].text.as_str()),
        (RequestKind::Brief, BRIEF)
    );
}

#[tokio::test]
async fn an_npc_whose_notes_hold_what_the_brief_says_is_not_saved_again() {
    let s = served().await;
    let morlox = s.morlox().await;
    let s = s.reading(Tables {
        sessions: json!([]),
        known: json!([morlox_row(&morlox)]),
    });
    let text = "Morlox ment.";
    let first = s
        .analysed(json!({ "session": SEANCE, "text": text }))
        .await
        .expect("queued");
    assert_eq!(
        follow_ups(&first).last().unwrap().partition,
        format!("Entity/{morlox}")
    );
    s.applied().await;
    let again = s
        .analysed(json!({ "session": SEANCE, "text": text }))
        .await
        .expect("queued");
    let then = follow_ups(&again);
    assert_eq!(then.len(), 1, "the scene alone");
    assert!(then[0].partition.starts_with("Session/"));
    s.applied().await;
    assert_eq!(s.row(&format!("Entity/{morlox}")).await["versionNumber"], 2);
}

#[tokio::test]
async fn an_npc_the_engine_does_not_hold_is_linked_and_not_saved_and_the_dm_is_told() {
    let s = served().await.reading(Tables {
        sessions: json!([{ "id": FIXTURE, "campaign": VALOMBRE }]),
        known: json!([{ "id": GASPARD, "type": "pnj", "name": "Gaspard", "aliases": ["le tavernier"] }]),
    });
    // A session the tables hold alone: its campaign is theirs.
    let answer = s
        .analysed(json!({ "session": FIXTURE, "text": "Gaspard sert à boire." }))
        .await
        .expect("queued");
    assert_eq!(
        answer.warnings.first().map(String::as_str),
        Some(
            "« Gaspard » est lié à sa scène mais n'est pas mis à jour : sa fiche n'a jamais été enregistrée"
        )
    );
    let then = follow_ups(&answer);
    assert_eq!(then.len(), 1, "the scene alone");
    s.applied().await;
    assert_eq!(
        s.row(&then[0].partition).await["entityId"],
        json!([GASPARD])
    );
    assert!(s.row(&format!("Entity/{GASPARD}")).await.is_empty());
}

#[tokio::test]
async fn an_entry_the_list_awaits_already_is_not_added_again() {
    let s = served().await.reading(Tables {
        sessions: json!([]),
        known: json!([]),
    });
    let answer = s
        .analysed(
            json!({ "session": SEANCE, "text": "Ils passent le gué. Puis Le Gué et Morlox." }),
        )
        .await
        .expect("queued");
    let mut names = Vec::new();
    for follow in follow_ups(&answer) {
        if let Some(name) = s.row(&follow.partition).await.get("name") {
            names.push(name.clone());
        }
    }
    s.applied().await;
    let mut written = Vec::new();
    for follow in follow_ups(&answer) {
        if let Some(name) = s.row(&follow.partition).await.get("name") {
            written.push(name.clone());
        }
    }
    assert!(names.is_empty(), "nothing written before the applier runs");
    assert_eq!(written, [json!("Morlox")]);
}

#[tokio::test]
async fn a_brief_the_llm_cannot_analyse_writes_nothing_and_says_why() {
    let s = served().await;
    // The active provider at a URL of no known type.
    written(
        &s.store,
        "AiSettings/1",
        json!({ "mode": "api", "url": "https://inconnu.example", "generationModel": "g", "summaryModel": "s" }),
    )
    .await;
    let (kind, message) = refused(
        s.analysed(json!({ "session": SEANCE, "text": BRIEF }))
            .await,
    );
    assert_eq!(kind, RefusalKind::Unprocessable);
    assert!(
        message.starts_with("l’analyse du brief a échoué ("),
        "{message}"
    );
    assert!(
        s.engine.run_once().await.expect("ran").is_empty(),
        "nothing queued"
    );
}

#[tokio::test]
async fn a_brief_of_no_session_or_of_no_word_is_refused_and_nothing_is_queued() {
    let s = served().await.reading(Tables {
        sessions: json!([]),
        known: json!([]),
    });
    let unknown = "51000000-0000-4000-8000-0000000000ff";
    assert_eq!(
        refused(
            s.analysed(json!({ "session": unknown, "text": BRIEF }))
                .await
        ),
        (RefusalKind::NotFound, format!("aucune séance {unknown}"))
    );
    for payload in [
        json!({ "session": "s4", "text": BRIEF }),
        json!({ "text": BRIEF }),
        json!({ "session": SEANCE, "text": " " }),
    ] {
        assert_eq!(
            refused(s.analysed(payload.clone()).await).0,
            RefusalKind::Unprocessable,
            "{payload}"
        );
    }
    assert!(s.engine.run_once().await.expect("ran").is_empty());
    assert!(s.llm.journal().list().is_empty(), "nothing was sent");
}

#[tokio::test]
async fn a_dictated_brief_is_kept_as_its_transcript_never_its_audio() {
    let s = served().await;
    let answer = s
        .analysed(json!({ "session": SEANCE, "dictation": "AAEC" }))
        .await
        .expect("queued");
    s.applied().await;
    let row = s.row(&answer.partition).await;
    assert_eq!(row["transcript"], "Morlox débarque.");
    assert!(!row.contains_key("dictation"));
    assert!(
        row.values()
            .all(|value| !value.to_string().contains("AAEC"))
    );
}

#[tokio::test]
async fn an_idempotency_key_answers_the_first_analysis_again_with_no_follow_up() {
    let s = served().await;
    let first = s
        .submit(
            "ai.analyzeBrief",
            json!({ "session": SEANCE, "text": "Morlox. Deuxième scène, Vesna." }),
            Some("k1"),
        )
        .await
        .expect("queued");
    assert_eq!(follow_ups(&first).len(), 4, "two scenes, two entries");
    let again = s
        .submit(
            "ai.analyzeBrief",
            json!({ "session": SEANCE, "text": "Morlox. Deuxième scène, Vesna." }),
            Some("k1"),
        )
        .await
        .expect("replayed");
    assert!(again.replayed);
    assert_eq!(again.command_id, first.command_id);
    assert!(again.then.is_empty());
    assert_eq!(s.llm.journal().list().len(), 1, "analysed once");
}

#[tokio::test]
async fn an_empty_submission_on_a_row_is_no_follow_up_and_is_refused() {
    let s = served().await;
    let submission: Submission = serde_json::from_value(json!({
        "dataCapability": "ai.analyzeBrief",
        "payload": {},
        "targetId": SEANCE,
        "caller": { "by": "dm", "from": "OralBrief", "roles": ["role:approver"] },
    }))
    .expect("a submission");
    assert_eq!(
        refused(s.engine.submit(submission).await).0,
        RefusalKind::Unprocessable
    );
}
