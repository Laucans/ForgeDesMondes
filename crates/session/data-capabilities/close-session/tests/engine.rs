//! session.closeSession as the engine runs it: its `Prepare` ([`plugin`])
//! over the in-memory store, beside its target aggregate's judge, the `llm`
//! port shared as the server shares it.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use close_session::{NO_SUMMARY_PROVIDER, plugin};
use harness_dataguard::Error;
use harness_dataguard::aggregate::Aggregate;
use harness_dataguard::api::{Submission, Submitted};
use harness_dataguard::command::{CommandResult, State};
use harness_dataguard::engine::Engine;
use harness_dataguard::guard::Caller;
use harness_dataguard::manifest::DataCapability;
use harness_dataguard::memory::{FixedClock, MemoryNotifier, MemoryStore};
use harness_dataguard::plugins::{RefusalKind, Shared};
use ports_llm::fake::FakeLlm;
use ports_llm::{
    ConnectionReport, GenerateAnswer, GenerateRequest, Llm, LlmError, Provider, ProviderConfig,
    RequestJournal, RequestKind, SharedLlm,
};
use serde_json::{Value, json};
use tokio::sync::Notify;

const SEANCE: &str = "51000000-0000-4000-8000-000000000004";

/// Now, as the engine's clock tells it: a millisecond into a second.
const NOW_MILLIS: u64 = 1_792_231_200_123;

/// A journal line written on a session's row: what a closed session no
/// longer takes.
const JOURNAL_LINE: &str = r#"{
    "dataCapability": "test.writeJournalLine",
    "owner": "dataguard",
    "version": 1,
    "effect": "update",
    "target": { "aggregate": "Session" },
    "touches": ["Session.spokenAt", "Session.text"],
    "payload": {},
    "mode": "overwrite",
    "invariants": ["status-lifecycle"],
    "permissions": [],
    "callableBy": ["*"],
    "idempotencyKey": "optional"
}"#;

fn object(value: Value) -> BTreeMap<String, Value> {
    value
        .as_object()
        .expect("an object")
        .clone()
        .into_iter()
        .collect()
}

fn partition(id: &str) -> String {
    format!("Session/{id}")
}

/// A session in `status`, its microphone on.
fn session(status: &str) -> BTreeMap<String, Value> {
    object(json!({
        "sessionId": SEANCE,
        "sessionTitle": "L'embuscade du Gué",
        "sessionStatus": status,
        "microphoneOn": true,
    }))
}

/// The home server's endpoint, as a save of the settings leaves it.
fn home_saved() -> BTreeMap<String, Value> {
    object(json!({
        "mode": "api",
        "url": "http://192.168.1.40:4096",
        "detectedApiType": "opencode",
        "apiKey": null,
        "binary": null,
        "generationModel": "qwen2.5-32b-instruct",
        "summaryModel": "qwen2.5-14b-instruct",
    }))
}

fn aggregate() -> Aggregate {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../aggregates/Session/aggregate.json");
    Aggregate::parse(&std::fs::read_to_string(path).expect("Session's aggregate.json"))
        .expect("contract I")
}

/// A provider that answers once the test lets it, and tells when a
/// request reaches it.
#[derive(Clone, Default)]
struct Gated {
    fake: FakeLlm,
    reached: Arc<Notify>,
    release: Arc<Notify>,
}

impl Llm for Gated {
    async fn test_connection(&self, config: &ProviderConfig) -> ConnectionReport {
        self.fake.test_connection(config).await
    }

    async fn generate(
        &self,
        config: &ProviderConfig,
        request: &GenerateRequest,
    ) -> Result<GenerateAnswer, LlmError> {
        self.reached.notify_one();
        self.release.notified().await;
        self.fake.generate(config, request).await
    }

    fn journal(&self) -> &RequestJournal {
        self.fake.journal()
    }
}

/// The engine over `store`, which the test reads, `llm` shared when there
/// is one; a clone of the fake shares its journal.
async fn engine<L: Llm + 'static>(
    store: MemoryStore,
    llm: Option<L>,
) -> Result<(Engine, Arc<MemoryStore>), Error> {
    let store = Arc::new(store);
    let mut shared = Shared::default();
    if let Some(llm) = llm {
        shared.insert(Arc::new(SharedLlm::new(llm)));
    }
    let engine = Engine::new(
        store.clone(),
        Arc::new(MemoryNotifier::default()),
        Arc::new(FixedClock::at_millis(NOW_MILLIS)),
    )
    .with_aggregate(aggregate())
    .with_data_capability(
        DataCapability::parse(include_str!("../data-capability.json")).expect("contract F"),
    )
    .with_data_capability(DataCapability::parse(JOURNAL_LINE).expect("contract F"))
    .with_plugins(
        [aggregate_session::plugin(), plugin()]
            .into_iter()
            .collect(),
    )
    .with_shared(shared);
    engine.load().await?;
    Ok((engine, store))
}

async fn submit(
    engine: &Engine,
    data_capability: &str,
    payload: Value,
    target: Option<&str>,
) -> Result<Submitted, Error> {
    engine
        .submit(Submission {
            data_capability: data_capability.to_string(),
            payload: object(payload),
            target_id: target.map(str::to_owned),
            based_on: None,
            idempotency_key: None,
            caller: Caller {
                by: "dm".to_string(),
                from: "LiveSessionBar".to_string(),
                permissions: Vec::new(),
                roles: vec!["role:approver".to_string()],
            },
        })
        .await
}

/// session.closeSession of `target`, as the Micro-UI sends it.
async fn close(engine: &Engine, target: Option<&str>) -> Result<Submitted, Error> {
    submit(engine, "session.closeSession@1", json!({}), target).await
}

async fn applied(engine: &Engine) -> Vec<CommandResult> {
    engine.run_once().await.expect("the applier ran")
}

fn refused(result: Result<Submitted, Error>, kind: RefusalKind) -> String {
    match result {
        Err(Error::Refused(refusal)) if refusal.kind == kind => refusal.message,
        other => panic!("refused as {kind:?}: {other:?}"),
    }
}

#[tokio::test]
async fn close_session_a_live_session_is_closed_with_its_summary_and_the_summary_model() {
    let llm = FakeLlm::new();
    let store = MemoryStore::default()
        .with_row(&partition(SEANCE), session("live"))
        .with_row("AiSettings/1", home_saved());
    let (engine, store) = engine(store, Some(llm.clone())).await.expect("loaded");
    let submitted = close(&engine, Some(SEANCE)).await.expect("queued");
    assert_eq!(submitted.partition, partition(SEANCE));
    assert!(submitted.warnings.iter().all(|w| !w.starts_with("résumé")));
    let results = applied(&engine).await;
    assert_eq!(results.len(), 1, "{results:?}");
    assert_eq!(results[0].status, State::Applied, "{results:?}");
    let now = store.row_now(&partition(SEANCE));
    assert_eq!(now["sessionStatus"], "closed");
    assert_eq!(now["closedAt"], "2026-10-17T10:00:00.123Z");
    assert_eq!(now["microphoneOn"], false);
    assert_eq!(now["summaryModel"], "qwen2.5-14b-instruct");
    assert!(
        now["summary"]
            .as_str()
            .is_some_and(|summary| !summary.trim().is_empty()),
        "{now:?}"
    );
    // The rest of the session's row as it was.
    assert_eq!(now["sessionTitle"], "L'embuscade du Gué");
    // text-only-no-audio: one Journal request, to the active provider.
    let sent = llm.journal().list();
    assert_eq!(sent.len(), 1, "{sent:?}");
    assert_eq!(sent[0].kind, RequestKind::Journal);
    assert_eq!(sent[0].provider, Provider::OpencodeServer);
}

#[tokio::test]
async fn close_session_with_no_settings_saved_over_memory_closes_without_a_summary() {
    let store = MemoryStore::default().with_row(&partition(SEANCE), session("live"));
    let (engine, store) = engine(store, Some(FakeLlm::new())).await.expect("loaded");
    let submitted = close(&engine, Some(SEANCE)).await.expect("queued");
    assert!(
        submitted.warnings.iter().any(|w| w == NO_SUMMARY_PROVIDER),
        "{:?}",
        submitted.warnings
    );
    assert_eq!(applied(&engine).await[0].status, State::Applied);
    let now = store.row_now(&partition(SEANCE));
    assert_eq!(now["sessionStatus"], "closed");
    assert_eq!(now["microphoneOn"], false);
    assert!(!now.contains_key("summary"), "{now:?}");
}

#[tokio::test]
async fn status_lifecycle_a_session_in_prep_or_closed_is_refused_and_nothing_is_written() {
    for status in ["prep", "closed"] {
        let llm = FakeLlm::new();
        let store = MemoryStore::default()
            .with_row(&partition(SEANCE), session(status))
            .with_row("AiSettings/1", home_saved());
        let (engine, store) = engine(store, Some(llm.clone())).await.expect("loaded");
        let message = refused(
            close(&engine, Some(SEANCE)).await,
            RefusalKind::Unprocessable,
        );
        assert!(message.starts_with("status-lifecycle"), "{message}");
        assert!(message.contains(status), "{message}");
        assert!(applied(&engine).await.is_empty());
        assert_eq!(store.row_now(&partition(SEANCE)), session(status));
        assert!(llm.journal().list().is_empty());
    }
}

#[tokio::test]
async fn status_lifecycle_a_second_close_queued_behind_the_first_is_refused() {
    let llm = FakeLlm::new();
    let store = MemoryStore::default()
        .with_row(&partition(SEANCE), session("live"))
        .with_row("AiSettings/1", home_saved());
    let (engine, _) = engine(store, Some(llm.clone())).await.expect("loaded");
    close(&engine, Some(SEANCE)).await.expect("queued");
    let message = refused(
        close(&engine, Some(SEANCE)).await,
        RefusalKind::Unprocessable,
    );
    assert!(message.starts_with("status-lifecycle"), "{message}");
    assert_eq!(llm.journal().list().len(), 1);
}

#[tokio::test]
async fn a_close_names_the_session_it_closes() {
    let (engine, _) = engine(MemoryStore::default(), Some(FakeLlm::new()))
        .await
        .expect("loaded");
    let message = refused(close(&engine, Some(SEANCE)).await, RefusalKind::NotFound);
    assert!(message.contains(SEANCE), "{message}");
    refused(close(&engine, None).await, RefusalKind::Unprocessable);
}

#[tokio::test]
async fn status_lifecycle_a_closed_session_takes_no_more_journal_lines() {
    let store = MemoryStore::default().with_row(&partition(SEANCE), session("live"));
    let (engine, store) = engine(store, Some(FakeLlm::new())).await.expect("loaded");
    close(&engine, Some(SEANCE)).await.expect("queued");
    applied(&engine).await;
    submit(
        &engine,
        "test.writeJournalLine@1",
        json!({ "spokenAt": "2026-10-17T22:41:00Z", "text": "Encore une chose…" }),
        Some(SEANCE),
    )
    .await
    .expect("queued");
    let results = applied(&engine).await;
    assert_eq!(results[0].status, State::Rejected, "{results:?}");
    assert_eq!(results[0].violations, ["status-lifecycle"]);
    assert!(!store.row_now(&partition(SEANCE)).contains_key("text"));
}

#[tokio::test]
async fn the_engine_refuses_to_load_it_without_the_llm_port() {
    assert!(matches!(
        engine::<FakeLlm>(MemoryStore::default(), None).await,
        Err(Error::Misconfigured(_))
    ));
}

#[tokio::test]
async fn status_lifecycle_a_close_while_the_first_is_being_summarised_is_refused() {
    let llm = Gated::default();
    let store = MemoryStore::default()
        .with_row(&partition(SEANCE), session("live"))
        .with_row("AiSettings/1", home_saved());
    let (engine, store) = engine(store, Some(llm.clone())).await.expect("loaded");
    // The second close is sent while the first waits on the provider —
    // or, if the first never reaches it, after a while all the same.
    let second = async {
        let reached = tokio::time::timeout(Duration::from_secs(5), llm.reached.notified()).await;
        let second = close(&engine, Some(SEANCE)).await;
        llm.release.notify_one();
        (reached.is_ok(), second)
    };
    let (first, (reached, second)) = tokio::join!(close(&engine, Some(SEANCE)), second);
    assert!(reached, "the first close never reached the provider");
    first.expect("queued");
    let message = refused(second, RefusalKind::Unprocessable);
    assert!(message.starts_with("status-lifecycle"), "{message}");
    assert!(message.contains("en train"), "{message}");
    assert_eq!(applied(&engine).await.len(), 1);
    assert_eq!(store.row_now(&partition(SEANCE))["sessionStatus"], "closed");
    assert_eq!(llm.journal().list().len(), 1);
}

#[tokio::test]
async fn a_close_withdrawn_before_it_is_applied_leaves_the_session_closable_again() {
    let store = MemoryStore::default().with_row(&partition(SEANCE), session("live"));
    let (engine, store) = engine(store, Some(FakeLlm::new())).await.expect("loaded");
    let first = close(&engine, Some(SEANCE)).await.expect("queued");
    engine
        .cancel(&first.command_id, "dm")
        .await
        .expect("cancelled");
    close(&engine, Some(SEANCE)).await.expect("queued again");
    assert_eq!(applied(&engine).await.len(), 1);
    assert_eq!(store.row_now(&partition(SEANCE))["sessionStatus"], "closed");
}
