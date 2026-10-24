//! ai.createFromBrief through the engine, over its in-memory store, as the
//! server plants it with the Session and Entity aggregates and the save of
//! an element (`campaign.saveEntity`): an entry « à créer » of Valombre's
//! fourth session is made an element. The entry's command is the one
//! submitted and answered; the engine then submits its follow-ups — the
//! element's save, then the scene's link. A pnj gets its sheet and its first
//! version, a place or an event its description and no sheet; the entry
//! turns « creee » naming its element, and keeps the DM's refinement. The
//! llm port, scripted here, is asked with the excerpt and the refinement.
//! An entry « creee », a failure of the port or an answer that fills no
//! sheet are refused, and nothing is queued.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use create_from_brief::{CREEE, element_id};
use harness_dataguard::Error;
use harness_dataguard::aggregate::Aggregate;
use harness_dataguard::api::{FollowUp, Submission, Submitted};
use harness_dataguard::command::{CommandResult, Entry, State};
use harness_dataguard::engine::Engine;
use harness_dataguard::manifest::DataCapability;
use harness_dataguard::memory::{MemoryNotifier, MemoryStore};
use harness_dataguard::plugins::{RefusalKind, Shared};
use harness_dataguard::postgres::SystemClock;
use harness_dataguard::queue::Hold;
use harness_dataguard::reads::Rows;
use harness_dataguard::store::{Collision, RandomIds, Store, Write};
use ports_llm::{
    ConnectionReport, GenerateAnswer, GenerateRequest, Llm, LlmError, ProviderConfig,
    RequestJournal, SharedLlm,
};
use ports_speech_to_text::SharedSpeechToText;
use ports_speech_to_text::fake::FakeSpeechToText;
use serde_json::{Value, json};
use tokio::sync::Notify;

const VALOMBRE: &str = "c1000000-0000-4000-8000-000000000001";
const BRUMES: &str = "c1000000-0000-4000-8000-000000000002";
const SESSION: &str = "5e000000-0000-4000-8000-000000000004";
const SCENE: &str = "5c000000-0000-4000-8000-000000000001";
const GASPARD: &str = "e0000000-0000-4000-8000-000000000001";
const ODRIC: &str = "7c000000-0000-4000-8000-000000000001";
const GUE: &str = "7c000000-0000-4000-8000-000000000002";
const MOULIN: &str = "7c000000-0000-4000-8000-000000000003";

/// The excerpt of the brief the mock-up's ToCreateList shows for Frère Odric.
const EXCERPT: &str = "un pèlerin, frère Odric, les arrête à la sortie du bourg pour les prévenir que le gué n’est plus sûr";

/// Three sentences the DM adds, as the mock-up's fixture dictates them.
const REFINED: &str = "Il a une soixantaine d’années et il boite depuis une chute dans la Corbière. En réalité c’est l’ancien meunier, qui a survécu à l’incendie. Il veut qu’on retrouve sa fille, enlevée par les bandits.";

type Fields = BTreeMap<String, Value>;

fn fields(value: Value) -> Fields {
    serde_json::from_value(value).expect("an object")
}

/// The DM, approver of their own campaign: the critical fields' review is
/// not what these tests judge.
fn caller() -> Value {
    json!({ "by": "dm", "from": "ToCreateList", "roles": ["role:approver"] })
}

/// The text after `label` in `prompt`, up to the end of its paragraph.
fn after<'a>(prompt: &'a str, label: &str) -> Option<&'a str> {
    let start = prompt.find(label)? + label.len();
    let rest = &prompt[start..];
    Some(rest.split("\n\n").next().unwrap_or(rest))
}

/// What the scripted provider answers a prompt: the proposal its request
/// asks for, its description from the excerpt and an NPC's background from
/// the refinement — a reader that keeps the words it is given.
fn echoed(prompt: &str) -> Option<String> {
    let excerpt = after(prompt, "Extrait du brief : ").unwrap_or_default();
    let refined = after(prompt, "Précisions du DM : ").unwrap_or("rien de plus");
    let description = format!("Selon le brief : {excerpt}");
    if !prompt.starts_with("Propose le PNJ") {
        return Some(json!({ "description": description }).to_string());
    }
    Some(format!(
        "Voici :\n```json\n{}\n```",
        json!({
            "description": description,
            "background": format!("Ce que le DM ajoute : {refined}"),
            "behavior": "Parle lentement, bénit les gens sans qu’on lui demande.",
            "stats": { "FOR": 13, "DEX": 8, "CON": 11, "INT": 10, "SAG": 14, "CHA": 11 },
            "ca": 10,
            "pvMax": 13,
            "actions": [{ "name": "Bâton ferré", "description": "+3 pour toucher. Touché : 4 (1d6 + 1)." }],
        })
    ))
}

/// A provider that answers no statblock.
fn sheetless(_prompt: &str) -> Option<String> {
    Some(json!({ "description": "Un pèlerin.", "background": "Un passé." }).to_string())
}

/// A provider that cannot be reached.
fn offline(_prompt: &str) -> Option<String> {
    None
}

/// The llm port, scripted: it answers each prompt as `answer` says, a
/// failure when it says none, and keeps the prompts it was sent.
#[derive(Clone)]
struct Scripted {
    answer: fn(&str) -> Option<String>,
    asked: Arc<Mutex<Vec<String>>>,
    journal: RequestJournal,
    gate: Option<Arc<Gate>>,
}

impl Scripted {
    fn new(answer: fn(&str) -> Option<String>) -> Self {
        Self {
            answer,
            asked: Arc::default(),
            journal: RequestJournal::new(),
            gate: None,
        }
    }

    fn asked(&self) -> Vec<String> {
        self.asked.lock().expect("the prompts").clone()
    }
}

impl Llm for Scripted {
    async fn test_connection(&self, _config: &ProviderConfig) -> ConnectionReport {
        ConnectionReport {
            ok: false,
            provider: None,
            latency_ms: 0,
            message: "jamais testé".to_owned(),
        }
    }

    async fn generate(
        &self,
        _config: &ProviderConfig,
        request: &GenerateRequest,
    ) -> Result<GenerateAnswer, LlmError> {
        self.asked
            .lock()
            .expect("the prompts")
            .push(request.text.clone());
        if let Some(gate) = &self.gate {
            gate.arm();
        }
        match (self.answer)(&request.text) {
            Some(text) => Ok(GenerateAnswer {
                text,
                model: "scripted".to_owned(),
            }),
            None => Err(LlmError::Failed("hors ligne".to_owned())),
        }
    }

    fn journal(&self) -> &RequestJournal {
        &self.journal
    }
}

/// A gate on one partition of the store: armed by the provider's first
/// answer, it holds the next read of that partition until it is opened —
/// the moment between a `Prepare` returning and the engine queueing.
struct Gate {
    partition: String,
    state: Mutex<(bool, bool)>,
    open: Notify,
}

impl Gate {
    fn on(partition: &str) -> Self {
        Self {
            partition: partition.to_owned(),
            state: Mutex::default(),
            open: Notify::new(),
        }
    }

    /// Armed once, by the first answer.
    fn arm(&self) {
        let mut state = self.state.lock().expect("the gate");
        if !state.0 {
            *state = (true, false);
        }
    }

    fn open(&self) {
        self.open.notify_one();
    }

    fn was_closed(&self) -> bool {
        self.state.lock().expect("the gate").1
    }

    /// Whether this read of `partition` is the one held.
    fn closes(&self, partition: &str) -> bool {
        let mut state = self.state.lock().expect("the gate");
        let closes = state.0 && !state.1 && partition == self.partition;
        state.1 |= closes;
        closes
    }
}

/// The in-memory store, its reads behind a [`Gate`].
struct Gated {
    store: Arc<MemoryStore>,
    gate: Arc<Gate>,
}

#[async_trait]
impl Store for Gated {
    async fn pending(&self) -> harness_dataguard::Result<Vec<Entry>> {
        self.store.pending().await
    }

    async fn holds(&self) -> harness_dataguard::Result<Vec<Hold>> {
        self.store.holds().await
    }

    async fn save(&self, entry: &Entry) -> harness_dataguard::Result<()> {
        self.store.save(entry).await
    }

    async fn save_hold(&self, hold: &Hold, alive: bool) -> harness_dataguard::Result<()> {
        self.store.save_hold(hold, alive).await
    }

    async fn data_version(&self) -> harness_dataguard::Result<u64> {
        self.store.data_version().await
    }

    async fn row(&self, partition: &str) -> harness_dataguard::Result<Fields> {
        if self.gate.closes(partition) {
            self.gate.open.notified().await;
        }
        self.store.row(partition).await
    }

    async fn collision(&self, entry: &Entry) -> harness_dataguard::Result<Collision> {
        self.store.collision(entry).await
    }

    async fn incoming(&self, partition: &str) -> harness_dataguard::Result<Vec<(String, u64)>> {
        self.store.incoming(partition).await
    }

    async fn apply(&self, write: &Write, result: &CommandResult) -> harness_dataguard::Result<u64> {
        self.store.apply(write, result).await
    }

    async fn journal(&self, result: &CommandResult) -> harness_dataguard::Result<()> {
        Store::journal(self.store.as_ref(), result).await
    }

    async fn result(&self, command: &str) -> harness_dataguard::Result<Option<CommandResult>> {
        self.store.result(command).await
    }

    async fn by_idempotency_key(
        &self,
        data_capability: &str,
        key: &str,
    ) -> harness_dataguard::Result<Option<String>> {
        self.store.by_idempotency_key(data_capability, key).await
    }
}

/// The tables, as a fake: the rows each query answers, by the table it
/// reads and the id it is asked for.
#[derive(Default)]
struct Tables(BTreeMap<(&'static str, String), Value>);

impl Tables {
    fn with(mut self, table: &'static str, id: &str, row: Value) -> Self {
        self.0.insert((table, id.to_owned()), row);
        self
    }
}

#[async_trait]
impl Rows for Tables {
    async fn rows(&self, sql: &str, parameters: &[Value]) -> harness_dataguard::Result<Value> {
        let id = parameters[0].as_str().expect("an id").to_owned();
        let table = ["entity_to_create", "scene", "session", "entity"]
            .into_iter()
            .find(|table| sql.contains(&format!("FROM {table} ")))
            .expect("a table read");
        Ok(match self.0.get(&(table, id)) {
            Some(row) => json!([row]),
            None => json!([]),
        })
    }
}

/// The engine over memory, as the server plants the creation, the save of
/// an element and their aggregates, holding Valombre's fourth session, its
/// first scene — which links Gaspard — and its three entries « à créer ».
struct Served {
    engine: Engine,
    store: Arc<MemoryStore>,
    llm: Scripted,
}

/// A row written straight into the store, as an applied command leaves it.
async fn written(store: &MemoryStore, partition: &str, values: Value) {
    let write = Write {
        partition: partition.to_owned(),
        values: fields(values),
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

/// An entry « à créer » of the session, of `kind` and `name`, in the scene.
fn entry(kind: &str, name: &str, excerpt: &str) -> Value {
    json!({
        "entityToCreateCampaignId": VALOMBRE,
        "entityToCreateSessionId": SESSION,
        "entityToCreateSceneId": SCENE,
        "type": kind,
        "name": name,
        "excerpt": excerpt,
        "entityToCreateStatus": "a_creer",
    })
}

impl Served {
    async fn new(answer: fn(&str) -> Option<String>) -> Served {
        Self::reading(answer, None).await
    }

    /// [`Served::new`], the Prepare reading `tables` when there are some.
    async fn reading(answer: fn(&str) -> Option<String>, tables: Option<Tables>) -> Served {
        Self::built(answer, tables, None).await
    }

    /// [`Served::new`], its store's reads behind `gate`, which the
    /// provider's first answer arms.
    async fn gated(answer: fn(&str) -> Option<String>, gate: Arc<Gate>) -> Served {
        Self::built(answer, None, Some(gate)).await
    }

    async fn built(
        answer: fn(&str) -> Option<String>,
        tables: Option<Tables>,
        gate: Option<Arc<Gate>>,
    ) -> Served {
        let store = Arc::new(MemoryStore::default());
        for (partition, row) in [
            (
                format!("Session/{SESSION}"),
                json!({ "sessionId": SESSION, "sessionCampaignId": VALOMBRE, "sessionTitle": "Séance 4 — L’embuscade du Gué", "sessionStatus": "prep" }),
            ),
            (
                format!("Session/{SCENE}"),
                json!({ "sceneId": SCENE, "sceneSessionId": SESSION, "sceneTitle": "Le bourg", "notes": "La sortie du bourg.", "entityId": [GASPARD] }),
            ),
            (
                format!("Session/{ODRIC}"),
                entry("pnj", "Frère Odric", EXCERPT),
            ),
            (
                format!("Session/{GUE}"),
                entry(
                    "lieu",
                    "Le Gué des Corbeaux",
                    "le gué des Corbeaux, là où la Corbière s’élargit",
                ),
            ),
            (
                format!("Session/{MOULIN}"),
                entry(
                    "evenement",
                    "L’incendie du moulin",
                    "le moulin a brûlé dans la nuit",
                ),
            ),
        ] {
            written(&store, &partition, row).await;
        }
        let mut llm = Scripted::new(answer);
        llm.gate.clone_from(&gate);
        let held: Arc<dyn Store> = match gate {
            Some(gate) => Arc::new(Gated {
                store: store.clone(),
                gate,
            }),
            None => store.clone(),
        };
        let mut shared = Shared::default();
        shared.insert(Arc::new(SharedLlm::new(llm.clone())));
        shared.insert(Arc::new(SharedSpeechToText::new(FakeSpeechToText::new(
            REFINED,
        ))));
        let mut engine = Engine::new(
            held,
            Arc::new(MemoryNotifier::default()),
            Arc::new(SystemClock),
        )
        .with_ids(Arc::new(RandomIds));
        for aggregate in [aggregate_session::DOCUMENT, aggregate_entity::MANIFEST] {
            engine = engine.with_aggregate(Aggregate::parse(aggregate).expect("contract I"));
        }
        for manifest in [create_from_brief::MANIFEST, save_entity::MANIFEST] {
            engine =
                engine.with_data_capability(DataCapability::parse(manifest).expect("contract F"));
        }
        let engine = engine
            .with_plugins(
                [
                    aggregate_session::plugin(),
                    aggregate_entity::plugin(),
                    create_from_brief::plugin(),
                    save_entity::plugin(),
                ]
                .into_iter()
                .collect(),
            )
            .with_shared(shared);
        let engine = match tables {
            Some(tables) => engine.with_rows(Arc::new(tables)),
            None => engine,
        };
        engine.load().await.expect("the units fit");
        Served { engine, store, llm }
    }

    async fn create(&self, payload: Value) -> Result<Submitted, Error> {
        self.post(payload, None).await
    }

    async fn post(&self, payload: Value, key: Option<&str>) -> Result<Submitted, Error> {
        let submission: Submission = serde_json::from_value(json!({
            "dataCapability": "ai.createFromBrief@1",
            "payload": payload,
            "caller": caller(),
            "idempotencyKey": key,
        }))
        .expect("a submission");
        self.engine.submit(submission).await
    }

    /// Runs the applier once and requires every command it ran applied:
    /// how many.
    async fn applied(&self) -> usize {
        let applied = self.engine.run_once().await.expect("the applier ran");
        assert!(
            applied.iter().all(|r| r.status == State::Applied),
            "{applied:?}"
        );
        applied.len()
    }

    async fn row(&self, partition: &str) -> Fields {
        self.store.row(partition).await.expect("a row")
    }

    /// The element `entry` becomes, its row.
    async fn element(&self, entry: &str) -> Fields {
        self.row(&format!("Entity/{}", element_id(entry))).await
    }
}

/// The follow-ups submitted, each queued: their partitions.
fn follow_ups(answer: &Submitted) -> Vec<String> {
    answer
        .then
        .iter()
        .map(|follow_up| match follow_up {
            FollowUp::Submitted(submitted) => submitted.partition.clone(),
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

#[tokio::test]
async fn a_pnj_is_created_from_its_excerpt_with_its_sheet_and_first_version() {
    let served = Served::new(echoed).await;
    let answer = served
        .create(json!({ "entry": ODRIC }))
        .await
        .expect("queued");
    let element = element_id(ODRIC);
    assert_eq!(answer.partition, format!("Session/{ODRIC}"));
    assert_eq!(
        follow_ups(&answer),
        [format!("Entity/{element}"), format!("Session/{SCENE}")]
    );
    assert_eq!(served.applied().await, 3);

    let made = served.element(ODRIC).await;
    assert_eq!(made["type"], "pnj");
    assert_eq!(made["name"], "Frère Odric");
    assert_eq!(made["entityCampaignId"], VALOMBRE);
    assert_eq!(made["characterEntityId"], element.as_str());
    assert_eq!(made["description"], format!("Selon le brief : {EXCERPT}"));
    assert_eq!(made["initialPrompt"], EXCERPT);
    assert_eq!(made["background"], "Ce que le DM ajoute : rien de plus");
    assert!(made["behavior"].as_str().is_some_and(|b| !b.is_empty()));
    for (field, score) in [
        ("strength", 13),
        ("dexterity", 8),
        ("constitution", 11),
        ("intelligence", 10),
        ("wisdom", 14),
        ("charisma", 11),
        ("armorClass", 10),
        ("hpMax", 13),
    ] {
        assert_eq!(made[field], score, "{field}");
    }
    assert_eq!(made["actions"][0]["name"], "Bâton ferré");
    // default-token: an NPC without a colour takes the accent.
    assert_eq!(made["tokenColor"], "accent");
    // every-change-versioned: its first version (EntityVersion@1).
    assert_eq!(made["versionNumber"], 1);

    // to-create-leaves-once-created: « creee » naming its element.
    let entry = served.row(&format!("Session/{ODRIC}")).await;
    assert_eq!(entry["entityToCreateStatus"], CREEE);
    assert_eq!(entry["createdEntityId"], element.as_str());
    assert!(!entry.contains_key("refinementText"), "{entry:?}");

    // From the brief alone: the excerpt is sent, no refinement.
    let asked = served.llm.asked();
    assert_eq!(asked.len(), 1);
    assert!(asked[0].contains(EXCERPT), "{}", asked[0]);
    assert!(!asked[0].contains("Précisions du DM"), "{}", asked[0]);
}

#[tokio::test]
async fn a_refined_pnj_keeps_the_refinement_and_its_proposal_holds_both() {
    let served = Served::new(echoed).await;
    served
        .create(json!({ "entry": ODRIC, "refinement": format!("  {REFINED}\n") }))
        .await
        .expect("queued");
    served.applied().await;

    let entry = served.row(&format!("Session/{ODRIC}")).await;
    assert_eq!(entry["refinementText"], REFINED);
    assert_eq!(entry["entityToCreateStatus"], CREEE);

    let asked = served.llm.asked();
    assert!(asked[0].contains(EXCERPT), "{}", asked[0]);
    assert!(asked[0].contains(REFINED), "{}", asked[0]);

    let made = served.element(ODRIC).await;
    let description = made["description"].as_str().expect("a description");
    let background = made["background"].as_str().expect("a background");
    assert!(description.contains("frère Odric"), "{description}");
    assert!(background.contains("ancien meunier"), "{background}");
    assert_eq!(made["initialPrompt"], format!("{EXCERPT}\n\n{REFINED}"));
}

#[tokio::test]
async fn a_dictated_refinement_is_transcribed_and_only_its_text_is_kept() {
    let served = Served::new(echoed).await;
    served
        .create(json!({ "entry": ODRIC, "audio": "AAEC" }))
        .await
        .expect("queued");
    served.applied().await;
    let entry = served.row(&format!("Session/{ODRIC}")).await;
    assert_eq!(entry["refinementText"], REFINED);
    assert!(
        entry.values().all(|value| value != "AAEC"),
        "no audio kept: {entry:?}"
    );
    assert!(served.llm.asked()[0].contains(REFINED));
}

#[tokio::test]
async fn a_typed_refinement_is_preferred_to_a_dictation() {
    let served = Served::new(echoed).await;
    served
        .create(json!({ "entry": ODRIC, "refinement": "Il ment.", "audio": "AAEC" }))
        .await
        .expect("queued");
    served.applied().await;
    let entry = served.row(&format!("Session/{ODRIC}")).await;
    assert_eq!(entry["refinementText"], "Il ment.");
}

#[tokio::test]
async fn a_dictation_that_is_no_base64_is_refused() {
    let served = Served::new(echoed).await;
    let (kind, message) = refused(
        served
            .create(json!({ "entry": ODRIC, "audio": "pas du base64 !" }))
            .await,
    );
    assert_eq!(kind, RefusalKind::Unprocessable);
    assert!(message.contains("base64"), "{message}");
    assert!(served.llm.asked().is_empty());
}

#[tokio::test]
async fn the_element_is_linked_to_its_scene_at_the_next_position() {
    let served = Served::new(echoed).await;
    served
        .create(json!({ "entry": ODRIC }))
        .await
        .expect("queued");
    served.applied().await;
    let scene = served.row(&format!("Session/{SCENE}")).await;
    assert_eq!(scene["entityId"], json!([GASPARD, element_id(ODRIC)]));
    // Its title and notes kept.
    assert_eq!(scene["sceneTitle"], "Le bourg");
    assert_eq!(scene["notes"], "La sortie du bourg.");
    assert_eq!(scene["sceneSessionId"], SESSION);
}

#[tokio::test]
async fn an_entry_already_created_is_refused_and_no_second_element_is_made() {
    let served = Served::new(echoed).await;
    served
        .create(json!({ "entry": ODRIC }))
        .await
        .expect("queued");
    served.applied().await;
    let made = served.element(ODRIC).await;

    let (kind, message) = refused(served.create(json!({ "entry": ODRIC })).await);
    assert_eq!(kind, RefusalKind::Unprocessable);
    assert!(
        message.contains("to-create-leaves-once-created"),
        "{message}"
    );
    // Nothing queued: the applier finds nothing to run.
    assert_eq!(served.applied().await, 0);
    assert_eq!(served.element(ODRIC).await, made);
    assert_eq!(served.llm.asked().len(), 1, "the provider asked once");
}

#[tokio::test]
async fn an_entry_whose_element_never_existed_is_created_again_under_the_same_id() {
    let served = Served::new(echoed).await;
    // « creee », naming the element whose save was rejected.
    let mut stale = entry("pnj", "Frère Odric", EXCERPT);
    stale["entityToCreateStatus"] = json!(CREEE);
    stale["createdEntityId"] = json!(element_id(ODRIC));
    written(&served.store, &format!("Session/{ODRIC}"), stale).await;
    served
        .create(json!({ "entry": ODRIC }))
        .await
        .expect("queued");
    served.applied().await;
    assert_eq!(served.element(ODRIC).await["name"], "Frère Odric");
}

#[tokio::test]
async fn an_entry_created_with_no_element_named_is_refused() {
    let served = Served::new(echoed).await;
    let mut created = entry("pnj", "Frère Odric", EXCERPT);
    created["entityToCreateStatus"] = json!(CREEE);
    written(&served.store, &format!("Session/{ODRIC}"), created).await;
    let (kind, _) = refused(served.create(json!({ "entry": ODRIC })).await);
    assert_eq!(kind, RefusalKind::Unprocessable);
    assert!(served.llm.asked().is_empty());
}

#[tokio::test]
async fn a_place_or_an_event_is_created_with_its_description_and_no_sheet() {
    let served = Served::new(echoed).await;
    for (id, kind, excerpt) in [
        (GUE, "lieu", "le gué des Corbeaux"),
        (MOULIN, "evenement", "le moulin a brûlé"),
    ] {
        served.create(json!({ "entry": id })).await.expect("queued");
        served.applied().await;
        let made = served.element(id).await;
        assert_eq!(made["type"], kind);
        let description = made["description"].as_str().expect("a description");
        assert!(description.contains(excerpt), "{description}");
        // character-is-pj-or-pnj: no character row.
        for field in ["characterEntityId", "background", "strength", "tokenColor"] {
            assert!(
                made.get(field).is_none_or(Value::is_null),
                "{field}: {made:?}"
            );
        }
        assert_eq!(made["versionNumber"], 1);
        let entry = served.row(&format!("Session/{id}")).await;
        assert_eq!(entry["entityToCreateStatus"], CREEE);
    }
    let asked = served.llm.asked();
    assert!(asked[0].starts_with("Propose le lieu"), "{}", asked[0]);
    assert!(asked[1].starts_with("Propose l’événement"), "{}", asked[1]);
}

#[tokio::test]
async fn when_the_llm_port_fails_nothing_is_written_and_the_entry_stays_todo() {
    let served = Served::new(offline).await;
    let before = served.row(&format!("Session/{ODRIC}")).await;
    let (kind, message) = refused(served.create(json!({ "entry": ODRIC })).await);
    assert_eq!(kind, RefusalKind::Unprocessable);
    assert!(message.contains("hors ligne"), "{message}");
    assert_eq!(served.applied().await, 0);
    assert_eq!(served.row(&format!("Session/{ODRIC}")).await, before);
    assert!(served.element(ODRIC).await.is_empty());
}

#[tokio::test]
async fn a_proposal_that_fills_no_statblock_is_refused_and_nothing_written() {
    let served = Served::new(sheetless).await;
    let (kind, message) = refused(served.create(json!({ "entry": ODRIC })).await);
    assert_eq!(kind, RefusalKind::Unprocessable);
    assert!(message.contains("behavior"), "{message}");
    assert!(message.contains("armorClass"), "{message}");
    assert_eq!(served.applied().await, 0);
    assert!(served.element(ODRIC).await.is_empty());
}

#[tokio::test]
async fn an_entry_of_no_uuid_or_of_none_known_is_refused() {
    let served = Served::new(echoed).await;
    let (kind, message) = refused(served.create(json!({ "entry": "odric" })).await);
    assert_eq!(kind, RefusalKind::Unprocessable);
    assert!(message.starts_with("entry"), "{message}");
    let (kind, _) = refused(served.create(json!({})).await);
    assert_eq!(kind, RefusalKind::Unprocessable);
    // A session's row is no entry.
    let (kind, _) = refused(served.create(json!({ "entry": SESSION })).await);
    assert_eq!(kind, RefusalKind::NotFound);
    let (kind, _) = refused(
        served
            .create(json!({ "entry": "7c000000-0000-4000-8000-0000000000ff" }))
            .await,
    );
    assert_eq!(kind, RefusalKind::NotFound);
    assert!(served.llm.asked().is_empty());
}

#[tokio::test]
async fn an_entry_named_in_capitals_is_the_same_entry() {
    let served = Served::new(echoed).await;
    let answer = served
        .create(json!({ "entry": ODRIC.to_ascii_uppercase() }))
        .await
        .expect("queued");
    assert_eq!(answer.partition, format!("Session/{ODRIC}"));
}

#[tokio::test]
async fn an_entry_whose_session_is_of_another_campaign_is_refused() {
    let served = Served::new(echoed).await;
    let mut stray = entry("pnj", "Frère Odric", EXCERPT);
    stray["entityToCreateCampaignId"] = json!(BRUMES);
    written(&served.store, &format!("Session/{ODRIC}"), stray).await;
    let (kind, message) = refused(served.create(json!({ "entry": ODRIC })).await);
    assert_eq!(kind, RefusalKind::Unprocessable);
    assert!(message.contains("scene-links-same-campaign"), "{message}");
    assert!(served.llm.asked().is_empty());
}

#[tokio::test]
async fn an_entry_whose_scene_is_gone_is_created_unlinked_and_says_so() {
    let served = Served::new(echoed).await;
    let mut unscened = entry("pnj", "Frère Odric", EXCERPT);
    unscened["entityToCreateSceneId"] = json!("5c000000-0000-4000-8000-0000000000ff");
    written(&served.store, &format!("Session/{ODRIC}"), unscened).await;
    let answer = served
        .create(json!({ "entry": ODRIC }))
        .await
        .expect("queued");
    assert_eq!(follow_ups(&answer).len(), 1, "the save alone");
    assert!(
        answer
            .warnings
            .iter()
            .any(|w| w.contains("n'y est pas lié")),
        "{:?}",
        answer.warnings
    );
    served.applied().await;
    assert_eq!(served.element(ODRIC).await["name"], "Frère Odric");
}

#[tokio::test]
async fn an_entry_of_no_scene_is_created_with_no_link_and_no_warning() {
    let served = Served::new(echoed).await;
    let mut unscened = entry("pnj", "Frère Odric", EXCERPT);
    unscened["entityToCreateSceneId"] = Value::Null;
    written(&served.store, &format!("Session/{ODRIC}"), unscened).await;
    let answer = served
        .create(json!({ "entry": ODRIC }))
        .await
        .expect("queued");
    assert_eq!(follow_ups(&answer).len(), 1);
    assert!(answer.warnings.is_empty(), "{:?}", answer.warnings);
}

#[tokio::test]
async fn an_idempotency_key_answers_the_first_creation_again() {
    let served = Served::new(echoed).await;
    let first = served
        .post(json!({ "entry": ODRIC }), Some("k1"))
        .await
        .expect("queued");
    served.applied().await;
    let again = served
        .post(json!({ "entry": ODRIC }), Some("k1"))
        .await
        .expect("replayed");
    assert!(again.replayed);
    assert_eq!(again.command_id, first.command_id);
    assert_eq!(served.llm.asked().len(), 1);
}

#[tokio::test]
async fn the_tables_entry_and_scene_are_read_under_the_engines_rows() {
    let served = Served::reading(
        echoed,
        Some(
            Tables::default()
                .with(
                    "entity_to_create",
                    "7c000000-0000-4000-8000-0000000000aa",
                    entry("lieu", "La Tour d’Ébène", "une ruine au nord du mont Pourpre"),
                )
                .with(
                    "scene",
                    SCENE,
                    json!({ "sceneId": SCENE, "sceneSessionId": SESSION, "sceneTitle": "Le bourg", "notes": null, "entityId": [GASPARD] }),
                ),
        ),
    )
    .await;
    let tour = "7c000000-0000-4000-8000-0000000000aa";
    let answer = served
        .create(json!({ "entry": tour }))
        .await
        .expect("queued");
    assert_eq!(answer.partition, format!("Session/{tour}"));
    served.applied().await;
    assert_eq!(served.element(tour).await["type"], "lieu");
    let entry = served.row(&format!("Session/{tour}")).await;
    assert_eq!(entry["entityToCreateStatus"], CREEE);
    // The engine's scene row lies over the table's: its notes kept.
    let scene = served.row(&format!("Session/{SCENE}")).await;
    assert_eq!(scene["notes"], "La sortie du bourg.");
    assert_eq!(scene["entityId"], json!([GASPARD, element_id(tour)]));
}

#[tokio::test]
async fn an_entry_whose_element_the_tables_hold_is_refused() {
    // The element the entry names is in the tables alone, as a fixture
    // seeds it: the engine holds no row of it.
    let served = Served::reading(
        echoed,
        Some(Tables::default().with("entity", GASPARD, json!({ "held": 1 }))),
    )
    .await;
    let mut created = entry("pnj", "Frère Odric", EXCERPT);
    created["entityToCreateStatus"] = json!(CREEE);
    created["createdEntityId"] = json!(GASPARD);
    written(&served.store, &format!("Session/{ODRIC}"), created).await;
    let (kind, _) = refused(served.create(json!({ "entry": ODRIC })).await);
    assert_eq!(kind, RefusalKind::Unprocessable);
    assert!(served.llm.asked().is_empty());
}

#[tokio::test]
async fn a_session_of_another_campaign_in_the_tables_is_refused() {
    let served = Served::reading(
        echoed,
        Some(Tables::default().with(
            "session",
            "5e000000-0000-4000-8000-0000000000bb",
            json!({ "sessionCampaignId": BRUMES }),
        )),
    )
    .await;
    let mut stray = entry("pnj", "Frère Odric", EXCERPT);
    stray["entityToCreateSessionId"] = json!("5e000000-0000-4000-8000-0000000000bb");
    written(&served.store, &format!("Session/{ODRIC}"), stray).await;
    let (kind, message) = refused(served.create(json!({ "entry": ODRIC })).await);
    assert_eq!(kind, RefusalKind::Unprocessable);
    assert!(message.contains("scene-links-same-campaign"), "{message}");
}

#[tokio::test]
async fn two_entries_of_one_scene_created_side_by_side_are_both_linked_in_order() {
    let served = Served::new(echoed).await;
    // Neither applied before the other is submitted: the second link reads
    // the scene as the first left it queued.
    served
        .create(json!({ "entry": ODRIC }))
        .await
        .expect("queued");
    served
        .create(json!({ "entry": GUE }))
        .await
        .expect("queued");
    served.applied().await;
    let scene = served.row(&format!("Session/{SCENE}")).await;
    assert_eq!(
        scene["entityId"],
        json!([GASPARD, element_id(ODRIC), element_id(GUE)])
    );
}

/// A link submitted as the follow-up is: naming the entry, targeting a row.
async fn link(served: &Served, entry: &str, scene: &str) -> Result<Submitted, Error> {
    let submission: Submission = serde_json::from_value(json!({
        "dataCapability": "ai.createFromBrief@1",
        "payload": { "entry": entry },
        "targetId": scene,
        "caller": caller(),
    }))
    .expect("a submission");
    served.engine.submit(submission).await
}

#[tokio::test]
async fn a_link_of_an_entry_not_created_or_of_another_scene_is_refused() {
    let served = Served::new(echoed).await;
    let (kind, message) = refused(link(&served, ODRIC, SCENE).await);
    assert_eq!(kind, RefusalKind::Unprocessable);
    assert!(message.contains("entrée créée"), "{message}");

    served
        .create(json!({ "entry": ODRIC }))
        .await
        .expect("queued");
    served.applied().await;
    let (kind, _) = refused(link(&served, ODRIC, SESSION).await);
    assert_eq!(kind, RefusalKind::Unprocessable);
    let (kind, message) = refused(link(&served, ODRIC, BRUMES).await);
    assert_eq!(kind, RefusalKind::Unprocessable);
    assert!(message.contains("ni sa scène"), "{message}");
    // Its own scene, linked again: refused, the one link stays.
    let (kind, message) = refused(link(&served, ODRIC, &SCENE.to_ascii_uppercase()).await);
    assert_eq!(kind, RefusalKind::Unprocessable);
    assert!(message.contains("une fois"), "{message}");
    assert_eq!(served.applied().await, 0);
    let scene = served.row(&format!("Session/{SCENE}")).await;
    assert_eq!(scene["entityId"], json!([GASPARD, element_id(ODRIC)]));
    assert!(served.llm.asked().len() == 1, "a link asks no provider");
}

#[tokio::test]
async fn an_element_the_dm_removed_from_its_scene_is_never_linked_again() {
    let served = Served::new(echoed).await;
    served
        .create(json!({ "entry": ODRIC }))
        .await
        .expect("queued");
    served.applied().await;
    // The DM takes the element out of the scene.
    let mut scene = served.row(&format!("Session/{SCENE}")).await;
    scene.insert("entityId".to_owned(), json!([GASPARD]));
    written(&served.store, &format!("Session/{SCENE}"), json!(scene)).await;
    let (kind, message) = refused(link(&served, ODRIC, SCENE).await);
    assert_eq!(kind, RefusalKind::Unprocessable);
    assert!(message.contains("une fois"), "{message}");
    assert_eq!(served.applied().await, 0);
    let scene = served.row(&format!("Session/{SCENE}")).await;
    assert_eq!(scene["entityId"], json!([GASPARD]));
}

#[tokio::test]
async fn a_link_of_an_entry_created_elsewhere_is_refused() {
    // « creee » with its element, by no creation of this engine: no link
    // is owed.
    let served = Served::new(echoed).await;
    let mut created = entry("pnj", "Frère Odric", EXCERPT);
    created["entityToCreateStatus"] = json!(CREEE);
    created["createdEntityId"] = json!(element_id(ODRIC));
    written(&served.store, &format!("Session/{ODRIC}"), created).await;
    let (kind, message) = refused(link(&served, ODRIC, SCENE).await);
    assert_eq!(kind, RefusalKind::Unprocessable);
    assert!(message.contains("une fois"), "{message}");
}

/// The command the ToCreateList sends (`commandBody`): the entry in the
/// payload, the entry's session as the target, the DM with no role.
async fn from_the_list(
    served: &Served,
    entry: &str,
    refinement: Option<&str>,
) -> Result<Submitted, Error> {
    let mut payload = json!({ "entry": entry });
    if let Some(refinement) = refinement {
        payload["refinement"] = json!(refinement);
    }
    let submission: Submission = serde_json::from_value(json!({
        "dataCapability": "ai.createFromBrief@1",
        "payload": payload,
        "target": { "aggregate": "Session", "id": SESSION },
        "caller": { "by": "dm", "from": "ToCreateList", "roles": [] },
    }))
    .expect("a submission");
    served.engine.submit(submission).await
}

/// Every review `answer` and its follow-ups wait on, approved by an
/// approver who is not the DM: the entry's status and the element's type
/// are critical fields, and the list's DM holds no role.
async fn approved(served: &Served, answer: &Submitted) {
    let followed = answer.then.iter().filter_map(|follow_up| match follow_up {
        FollowUp::Submitted(submitted) => Some(submitted.as_ref()),
        FollowUp::Refused { .. } => None,
    });
    for submitted in std::iter::once(answer).chain(followed) {
        assert_eq!(submitted.result.status, State::AwaitingReview);
        let review = submitted.result.review_id.as_deref().expect("a review");
        served
            .engine
            .decide(review, "approbateur", &["role:approver".to_owned()], true)
            .await
            .expect("approved");
    }
}

#[tokio::test]
async fn the_list_s_command_targeting_the_entry_s_session_creates_and_links() {
    let served = Served::new(echoed).await;
    let answer = from_the_list(&served, ODRIC, Some("Il boite."))
        .await
        .expect("queued");
    let element = element_id(ODRIC);
    assert_eq!(answer.partition, format!("Session/{ODRIC}"));
    assert_eq!(
        follow_ups(&answer),
        [format!("Entity/{element}"), format!("Session/{SCENE}")]
    );
    approved(&served, &answer).await;
    assert_eq!(served.applied().await, 3);
    let made = served.element(ODRIC).await;
    assert_eq!(made["name"], "Frère Odric");
    assert_eq!(made["versionNumber"], 1);
    let entry = served.row(&format!("Session/{ODRIC}")).await;
    assert_eq!(entry["entityToCreateStatus"], CREEE);
    assert_eq!(entry["createdEntityId"], element.as_str());
    assert_eq!(entry["refinementText"], "Il boite.");
    let scene = served.row(&format!("Session/{SCENE}")).await;
    assert_eq!(scene["entityId"], json!([GASPARD, element]));
    // The session itself is untouched.
    let session = served.row(&format!("Session/{SESSION}")).await;
    assert!(!session.contains_key("entityToCreateStatus"), "{session:?}");
}

#[tokio::test]
async fn a_command_targeting_a_session_not_the_entry_s_is_refused() {
    let served = Served::new(echoed).await;
    let submission: Submission = serde_json::from_value(json!({
        "dataCapability": "ai.createFromBrief@1",
        "payload": { "entry": ODRIC },
        "target": { "aggregate": "Session", "id": "5e000000-0000-4000-8000-0000000000bb" },
        "caller": caller(),
    }))
    .expect("a submission");
    let (kind, message) = refused(served.engine.submit(submission).await);
    assert_eq!(kind, RefusalKind::Unprocessable);
    assert!(message.contains("ni sa scène"), "{message}");
    assert!(served.llm.asked().is_empty());
    assert!(served.element(ODRIC).await.is_empty());
}

#[tokio::test]
async fn a_double_click_makes_one_element_and_asks_the_provider_once() {
    // The first submission is held right after its Prepare returned, before
    // the engine queues its stamp; the second comes then.
    let gate = Arc::new(Gate::on(&format!("Session/{ODRIC}")));
    let served = Served::gated(echoed, gate.clone()).await;
    let (first, second) = tokio::join!(from_the_list(&served, ODRIC, None), async {
        let second = from_the_list(&served, ODRIC, None).await;
        gate.open();
        second
    });
    let first = first.expect("queued");
    let (kind, message) = refused(second);
    assert_eq!(kind, RefusalKind::Unprocessable);
    assert!(message.contains("en cours de création"), "{message}");
    assert!(gate.was_closed(), "the first was held when the second came");
    approved(&served, &first).await;
    assert_eq!(served.applied().await, 3);
    assert_eq!(served.llm.asked().len(), 1, "the provider asked once");
    let scene = served.row(&format!("Session/{SCENE}")).await;
    assert_eq!(scene["entityId"], json!([GASPARD, element_id(ODRIC)]));
}

#[tokio::test]
async fn a_link_to_a_scene_gone_is_not_found() {
    let served = Served::new(echoed).await;
    let gone = "5c000000-0000-4000-8000-0000000000ff";
    let mut created = entry("pnj", "Frère Odric", EXCERPT);
    created["entityToCreateSceneId"] = json!(gone);
    created["entityToCreateStatus"] = json!(CREEE);
    created["createdEntityId"] = json!(element_id(ODRIC));
    written(&served.store, &format!("Session/{ODRIC}"), created).await;
    let (kind, _) = refused(link(&served, ODRIC, gone).await);
    assert_eq!(kind, RefusalKind::NotFound);
}
