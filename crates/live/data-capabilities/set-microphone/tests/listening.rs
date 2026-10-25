//! The microphone's streams, with no database: the write of a session seen once applied
//! opens the stream the command staged (microphone on) or ends it (microphone off), each
//! target a heard phrase names is submitted as a `live.addSuggestion` with the words that
//! named it, and nothing is listened to without an engine to submit to.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use async_trait::async_trait;
use harness_dataguard::api::Submission;
use harness_dataguard::guard::Caller;
use harness_dataguard::plugins::Project;
use harness_dataguard::store::Write;
use ports_speech_to_text::fake::FakeSpeechToText;
use ports_speech_to_text::{
    Dictation, RequestJournal, SharedSpeechToText, SpeechToText, SpeechToTextError, Transcript,
    TranscriptStream, Vocabulary as Heard,
};
use serde_json::{Value, json};

use set_microphone::detect::Vocabulary;
use set_microphone::listening::{Observer, Plan, Plans, Submit};
use set_microphone::{DATA_CAPABILITY, Listening};

const SESSION: &str = "5e000000-0000-4000-8000-000000000001";
const OTHER: &str = "5e000000-0000-4000-8000-000000000002";
const VESNA: &str = "e1000000-0000-4000-8000-000000000001";
const PORTE: &str = "50000000-0000-4000-8000-000000000001";

/// Where the suggestions go: kept, and failing the first `refusing` of them.
#[derive(Default)]
struct Recorder {
    kept: Mutex<Vec<Submission>>,
    refusing: Mutex<usize>,
}

impl Recorder {
    fn kept(&self) -> Vec<Submission> {
        self.kept
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

#[async_trait]
impl Submit for Recorder {
    async fn submit(&self, submission: Submission) -> Result<(), String> {
        let mut refusing = self.refusing.lock().unwrap_or_else(PoisonError::into_inner);
        if *refusing > 0 {
            *refusing -= 1;
            return Err("refused".to_string());
        }
        self.kept
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(submission);
        Ok(())
    }
}

/// The vocabulary of a campaign: Vesna by her alias, and a sound.
fn vocabulary() -> Vocabulary {
    Vocabulary::from_rows(&json!([
        { "term": "Vesna la Balafrée", "kind": "entity", "id": VESNA, "alias": false },
        { "term": "la Balafrée", "kind": "entity", "id": VESNA, "alias": true },
        { "term": "Porte qui grince", "kind": "sound", "id": PORTE, "alias": false },
    ]))
}

/// A plan listening to `phrases`, one every `every`.
fn plan(fake: &FakeSpeechToText, phrases: &[&str], every: Duration) -> Plan {
    Plan {
        vocabulary: vocabulary(),
        speech: SharedSpeechToText::new(fake.clone().listening(phrases.iter().copied(), every)),
        caller: Caller {
            by: "dm".to_string(),
            from: "SuggestionPill".to_string(),
            permissions: vec![],
            roles: vec!["role:approver".to_string()],
        },
    }
}

/// What a server has: a [`Listening`] submitting to a recorder, and the plans a
/// `Prepare` stages for the [`Observer`] to find.
fn server() -> (Arc<Listening>, Arc<Plans>, Arc<Recorder>) {
    let listening = Arc::new(Listening::new());
    let recorder = Arc::new(Recorder::default());
    listening.submitting(recorder.clone());
    (listening, Arc::new(Plans::default()), recorder)
}

/// The write of a session's row once applied, its microphone `on`.
fn written(partition: &str, on: Option<bool>) -> Write {
    let mut values: BTreeMap<String, Value> = BTreeMap::new();
    values.insert("sessionTitle".to_string(), json!("L’embuscade"));
    if let Some(on) = on {
        values.insert("microphoneOn".to_string(), json!(on));
    }
    Write {
        partition: partition.to_string(),
        values,
        tombstone: false,
    }
}

/// Waits until `done`, for two seconds at most.
async fn until(mut done: impl FnMut() -> bool) -> bool {
    for _ in 0..200 {
        if done() {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    done()
}

#[tokio::test]
async fn the_microphone_on_listens_and_each_target_named_becomes_a_suggestion() {
    let (listening, plans, recorder) = server();
    let fake = FakeSpeechToText::new("");
    plans.stage(
        SESSION,
        &listening,
        Some(plan(
            &fake,
            &[
                "Vous entrez dans la taverne.",
                "La Balafrée ? Jamais entendu parler.",
                "Derrière le comptoir, la porte qui grince.",
            ],
            Duration::from_millis(5),
        )),
    );
    Observer(plans).observe(&written(&format!("Session/{SESSION}"), Some(true)));
    assert!(
        until(|| recorder.kept().len() == 2).await,
        "{:?}",
        recorder.kept()
    );
    let kept = recorder.kept();
    let sent: Vec<Value> = kept
        .iter()
        .map(|s| json!({ "to": s.data_capability, "payload": s.payload, "from": s.caller.from }))
        .collect();
    // They are the DM's, who turned the microphone on, with their rights.
    for suggestion in &kept {
        assert_eq!(suggestion.caller.by, "dm");
        assert_eq!(suggestion.caller.roles, ["role:approver"]);
    }
    assert_eq!(
        sent,
        [
            json!({
                "to": "live.addSuggestion",
                "from": DATA_CAPABILITY,
                "payload": { "sessionId": SESSION, "entityId": VESNA, "matchedText": "la Balafrée" },
            }),
            json!({
                "to": "live.addSuggestion",
                "from": DATA_CAPABILITY,
                "payload": { "sessionId": SESSION, "soundId": PORTE, "matchedText": "Porte qui grince" },
            }),
        ]
    );
    // The port was asked once, for the campaign's vocabulary, and handed no audio.
    let journal = fake.journal().list();
    assert_eq!(journal.len(), 1);
    assert_eq!(journal[0].audio_bytes, 0);
    assert_eq!(
        journal[0].vocabulary,
        Heard {
            campaign_names: vec!["Vesna la Balafrée".into()],
            aliases: vec!["la Balafrée".into()],
            sound_names: vec!["Porte qui grince".into()],
        }
    );
    listening.shutdown();
}

#[tokio::test]
async fn the_microphone_off_ends_the_stream_and_its_task_and_nothing_more_is_suggested() {
    let (listening, plans, recorder) = server();
    let fake = FakeSpeechToText::new("");
    plans.stage(
        SESSION,
        &listening,
        Some(plan(
            &fake,
            &["La Balafrée", "La Balafrée"],
            Duration::from_millis(100),
        )),
    );
    let observer = Observer(plans);
    observer.observe(&written(&format!("Session/{SESSION}"), Some(true)));
    assert!(listening.listens(SESSION));
    assert!(until(|| recorder.kept().len() == 1).await);

    observer.observe(&written(&format!("Session/{SESSION}"), Some(false)));
    assert!(!listening.listens(SESSION), "the stream's task has ended");
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(
        recorder.kept().len(),
        1,
        "a phrase heard after is not suggested"
    );
}

#[tokio::test]
async fn a_microphone_already_on_keeps_its_stream_and_opens_no_second() {
    let (listening, plans, recorder) = server();
    let fake = FakeSpeechToText::new("");
    let every = Duration::from_millis(20);
    plans.stage(
        SESSION,
        &listening,
        Some(plan(&fake, &["La Balafrée"], every)),
    );
    let observer = Observer(Arc::clone(&plans));
    observer.observe(&written(&format!("Session/{SESSION}"), Some(true)));
    plans.stage(
        SESSION,
        &listening,
        Some(plan(&fake, &["La Balafrée"], every)),
    );
    observer.observe(&written(&format!("Session/{SESSION}"), Some(true)));
    assert!(until(|| !recorder.kept().is_empty()).await);
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(fake.journal().list().len(), 1, "one stream opened");
    assert_eq!(recorder.kept().len(), 1);
    listening.shutdown();
}

#[tokio::test]
async fn the_streams_are_by_session() {
    let (listening, plans, _recorder) = server();
    let fake = FakeSpeechToText::new("");
    let every = Duration::from_secs(60);
    plans.stage(SESSION, &listening, Some(plan(&fake, &["x"], every)));
    plans.stage(OTHER, &listening, Some(plan(&fake, &["x"], every)));
    let observer = Observer(plans);
    observer.observe(&written(&format!("Session/{SESSION}"), Some(true)));
    observer.observe(&written(&format!("Session/{OTHER}"), Some(true)));
    assert!(listening.listens(SESSION) && listening.listens(OTHER));
    observer.observe(&written(&format!("Session/{SESSION}"), Some(false)));
    assert!(!listening.listens(SESSION));
    assert!(listening.listens(OTHER), "the other session still listens");
    listening.shutdown();
    assert!(
        !listening.listens(OTHER),
        "the server stopping ends every stream"
    );
}

#[tokio::test]
async fn a_write_that_is_not_a_microphone_turned_on_listens_to_nothing() {
    let (listening, plans, _recorder) = server();
    let fake = FakeSpeechToText::new("");
    let every = Duration::from_secs(60);
    let observer = Observer(Arc::clone(&plans));

    // A write that carries no microphone: a scene, a title.
    plans.stage(SESSION, &listening, Some(plan(&fake, &["x"], every)));
    observer.observe(&written(&format!("Session/{SESSION}"), None));
    // A row that is no session's.
    observer.observe(&written(&format!("Scene/{SESSION}"), Some(true)));
    // A tombstone.
    let mut tombstone = written(&format!("Session/{SESSION}"), Some(true));
    tombstone.tombstone = true;
    observer.observe(&tombstone);
    // A session nothing staged.
    observer.observe(&written(&format!("Session/{OTHER}"), Some(true)));
    assert!(!listening.listens(SESSION) && !listening.listens(OTHER));
    assert!(fake.journal().list().is_empty(), "the port was never asked");

    // What was staged is still there for the write that turns the microphone on.
    observer.observe(&written(&format!("Session/{SESSION}"), Some(true)));
    assert!(listening.listens(SESSION));
    listening.shutdown();
}

#[tokio::test]
async fn an_off_queued_around_an_on_does_not_take_its_plan_away() {
    // Off, then on: the off is applied first and the on still finds its plan.
    let (listening, plans, _recorder) = server();
    let fake = FakeSpeechToText::new("");
    let every = Duration::from_secs(60);
    plans.stage(SESSION, &listening, None);
    plans.stage(SESSION, &listening, Some(plan(&fake, &["x"], every)));
    let observer = Observer(Arc::clone(&plans));
    observer.observe(&written(&format!("Session/{SESSION}"), Some(false)));
    observer.observe(&written(&format!("Session/{SESSION}"), Some(true)));
    assert!(listening.listens(SESSION), "the on listens");
    observer.observe(&written(&format!("Session/{SESSION}"), Some(false)));
    assert!(!listening.listens(SESSION));

    // On, then an off prepared before the on is applied: the plan is still the on's.
    plans.stage(SESSION, &listening, Some(plan(&fake, &["x"], every)));
    plans.stage(SESSION, &listening, None);
    observer.observe(&written(&format!("Session/{SESSION}"), Some(true)));
    assert!(
        listening.listens(SESSION),
        "the off prepared after did not take the plan"
    );
    listening.shutdown();
}

#[tokio::test]
async fn a_write_of_another_command_while_listening_leaves_the_next_on_its_plan() {
    // Listening; an unrelated write (microphone still on) is applied, then an
    // off, then an on whose plan was staged before them all.
    let (listening, plans, _recorder) = server();
    let fake = FakeSpeechToText::new("");
    let every = Duration::from_secs(60);
    plans.stage(SESSION, &listening, Some(plan(&fake, &["x"], every)));
    let observer = Observer(Arc::clone(&plans));
    observer.observe(&written(&format!("Session/{SESSION}"), Some(true)));
    assert!(listening.listens(SESSION));

    plans.stage(SESSION, &listening, Some(plan(&fake, &["x"], every)));
    observer.observe(&written(&format!("Session/{SESSION}"), Some(true)));
    observer.observe(&written(&format!("Session/{SESSION}"), Some(false)));
    assert!(!listening.listens(SESSION));
    observer.observe(&written(&format!("Session/{SESSION}"), Some(true)));
    assert!(
        listening.listens(SESSION),
        "the on found its plan: the row says on and a stream listens"
    );
    listening.shutdown();
}

#[tokio::test]
async fn without_an_engine_to_submit_to_nothing_listens() {
    let listening = Arc::new(Listening::new());
    let plans = Arc::new(Plans::default());
    let fake = FakeSpeechToText::new("");
    plans.stage(
        SESSION,
        &listening,
        Some(plan(&fake, &["x"], Duration::from_secs(60))),
    );
    Observer(plans).observe(&written(&format!("Session/{SESSION}"), Some(true)));
    assert!(!listening.listens(SESSION));
    assert!(
        fake.journal().list().is_empty(),
        "the microphone was not opened"
    );
}

#[tokio::test]
async fn a_suggestion_refused_does_not_end_the_stream() {
    let (listening, plans, recorder) = server();
    *recorder
        .refusing
        .lock()
        .unwrap_or_else(PoisonError::into_inner) = 1;
    let fake = FakeSpeechToText::new("");
    plans.stage(
        SESSION,
        &listening,
        Some(plan(
            &fake,
            &["La Balafrée", "La Balafrée"],
            Duration::from_millis(5),
        )),
    );
    Observer(plans).observe(&written(&format!("Session/{SESSION}"), Some(true)));
    assert!(
        until(|| recorder.kept().len() == 1).await,
        "the second phrase is heard"
    );
    listening.shutdown();
}

/// A port whose microphone cannot be opened, or that fails after a phrase.
struct Broken {
    fails_after_a_phrase: bool,
    journal: RequestJournal,
}

impl SpeechToText for Broken {
    async fn transcribe(
        &self,
        _dictation: &Dictation,
        _vocabulary: &Heard,
    ) -> Result<Transcript, SpeechToTextError> {
        Err(SpeechToTextError::EmptyAudio)
    }

    async fn listen(&self, _vocabulary: &Heard) -> Result<TranscriptStream, SpeechToTextError> {
        if !self.fails_after_a_phrase {
            return Err(SpeechToTextError::Failed("no microphone".to_string()));
        }
        Ok(TranscriptStream::spawn(|phrases, mut stop| async move {
            phrases.say(Transcript::new("La Balafrée")).await;
            phrases
                .fail(SpeechToTextError::Failed("the engine died".to_string()))
                .await;
            stop.requested().await;
        }))
    }

    fn journal(&self) -> &RequestJournal {
        &self.journal
    }
}

#[tokio::test]
async fn a_port_that_cannot_listen_leaves_no_task_and_one_that_fails_ends_it_and_turns_the_microphone_off()
 {
    for fails_after_a_phrase in [false, true] {
        let (listening, plans, recorder) = server();
        let speech = SharedSpeechToText::new(Broken {
            fails_after_a_phrase,
            journal: RequestJournal::new(),
        });
        plans.stage(
            SESSION,
            &listening,
            Some(Plan {
                vocabulary: vocabulary(),
                speech,
                caller: Caller {
                    by: "dm".to_string(),
                    roles: vec!["role:approver".to_string()],
                    ..Caller::default()
                },
            }),
        );
        Observer(plans).observe(&written(&format!("Session/{SESSION}"), Some(true)));
        assert!(
            until(|| !listening.listens(SESSION)).await,
            "the task ends (fails after a phrase: {fails_after_a_phrase})"
        );
        // What was heard before the failure is suggested; then the microphone, which
        // the row still says is on, is turned off, for the DM who turned it on.
        let expected = 1 + usize::from(fails_after_a_phrase);
        assert!(
            until(|| recorder.kept().len() == expected).await,
            "{:?}",
            recorder.kept()
        );
        let kept = recorder.kept();
        let last = kept.last().expect("the microphone turned off");
        assert_eq!(last.data_capability, DATA_CAPABILITY);
        assert_eq!(
            last.payload,
            BTreeMap::from([("on".to_string(), json!(false))])
        );
        assert_eq!(last.target_id.as_deref(), Some(SESSION));
        assert_eq!(last.caller.from, DATA_CAPABILITY);
        assert_eq!(last.caller.by, "dm");
        assert_eq!(last.caller.roles, ["role:approver"]);
        assert_eq!(
            kept.iter()
                .filter(|s| s.data_capability == "live.addSuggestion")
                .count(),
            usize::from(fails_after_a_phrase)
        );
    }
}

/// A port whose stream tells when its task is gone.
struct Watched {
    gone: Arc<std::sync::atomic::AtomicBool>,
    journal: RequestJournal,
}

/// Sets the flag when dropped: the task that held it ended.
struct Gone(Arc<std::sync::atomic::AtomicBool>);

impl Drop for Gone {
    fn drop(&mut self) {
        self.0.store(true, std::sync::atomic::Ordering::SeqCst);
    }
}

impl SpeechToText for Watched {
    async fn transcribe(
        &self,
        _dictation: &Dictation,
        _vocabulary: &Heard,
    ) -> Result<Transcript, SpeechToTextError> {
        Err(SpeechToTextError::EmptyAudio)
    }

    async fn listen(&self, _vocabulary: &Heard) -> Result<TranscriptStream, SpeechToTextError> {
        let guard = Gone(Arc::clone(&self.gone));
        Ok(TranscriptStream::spawn(|phrases, mut stop| async move {
            // A microphone that hears nothing keeps its stream open until it is stopped.
            let _open = phrases;
            let _guard = guard;
            stop.requested().await;
        }))
    }

    fn journal(&self) -> &RequestJournal {
        &self.journal
    }
}

#[tokio::test]
async fn the_server_going_away_ends_the_streams_and_the_ports_tasks_with_them() {
    let gone = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let (listening, plans, _recorder) = server();
    plans.stage(
        SESSION,
        &listening,
        Some(Plan {
            vocabulary: vocabulary(),
            speech: SharedSpeechToText::new(Watched {
                gone: Arc::clone(&gone),
                journal: RequestJournal::new(),
            }),
            caller: Caller::default(),
        }),
    );
    let observer = Observer(Arc::clone(&plans));
    observer.observe(&written(&format!("Session/{SESSION}"), Some(true)));
    assert!(listening.listens(SESSION));
    tokio::task::yield_now().await;
    assert!(
        !gone.load(std::sync::atomic::Ordering::SeqCst),
        "the port's task runs"
    );
    // Nobody holds the streams any more.
    drop(observer);
    drop(plans);
    drop(listening);
    assert!(
        until(|| gone.load(std::sync::atomic::Ordering::SeqCst)).await,
        "the port's task is gone"
    );
}
