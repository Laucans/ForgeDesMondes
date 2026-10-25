//! The microphone's streams: one task per session whose microphone is on.
//!
//! [`Listening`] is what the server shares with the unit. It opens the
//! speech-to-text port's stream with the campaign's vocabulary, reads each
//! phrase heard against it ([`Vocabulary::detect`]) and submits one
//! `live.addSuggestion` per target named — through the DataGuard, whose
//! `Prepare` reads the session before it writes (live, microphone on, the
//! target of its campaign) and whose judge keeps the Suggestion's
//! invariants. The task's `JoinHandle` is kept: the microphone turned off,
//! or the server stopping, aborts it, and the stream it owns ends with it.
//!
//! The task starts once the command that turned the microphone on is
//! applied ([`Observer`] sees the write), never when it is only prepared: a
//! command the judge rejects listens to nothing.

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex, PoisonError, Weak};

use async_trait::async_trait;
use harness_dataguard::api::Submission;
use harness_dataguard::engine::Engine;
use harness_dataguard::guard::Caller;
use harness_dataguard::plugins::Project;
use harness_dataguard::store::Write;
use ports_speech_to_text::{SharedSpeechToText, SpeechToText};
use serde_json::{Value, json};
use tokio::sync::Notify;
use tokio::task::JoinHandle;

use crate::DATA_CAPABILITY;
use crate::detect::{Detection, Kind, Vocabulary};

/// The DataCapability that records a suggestion.
const ADD_SUGGESTION: &str = "live.addSuggestion";

/// The aggregate whose writes tell the microphone on or off.
const SESSION: &str = "Session/";

/// Where suggestions are submitted: the engine, in a run.
#[async_trait]
pub trait Submit: Send + Sync {
    /// Submits `submission`, or says why it could not be.
    async fn submit(&self, submission: Submission) -> Result<(), String>;
}

/// The engine a server planted, and the applier it wakes once a suggestion
/// is queued.
struct Engaged {
    engine: Weak<Engine>,
    wake: Arc<Notify>,
}

#[async_trait]
impl Submit for Engaged {
    async fn submit(&self, submission: Submission) -> Result<(), String> {
        let engine = self
            .engine
            .upgrade()
            .ok_or_else(|| "the engine is gone".to_string())?;
        engine
            .submit(submission)
            .await
            .map(|_| self.wake.notify_one())
            .map_err(|e| e.to_string())
    }
}

/// What a stream needs, staged when the command is prepared and taken when
/// it is applied.
pub struct Plan {
    /// What the campaign's microphone listens for.
    pub vocabulary: Vocabulary,
    /// The port that transcribes.
    pub speech: SharedSpeechToText,
    /// Who turned the microphone on: the suggestions it hears are theirs,
    /// with their rights, from this DataCapability.
    pub caller: Caller,
}

/// The running streams, by session, and where their suggestions go.
#[derive(Default)]
pub struct Listening {
    sink: Mutex<Option<Arc<dyn Submit>>>,
    streams: Mutex<HashMap<String, JoinHandle<()>>>,
}

impl Listening {
    /// A server with no stream, and nowhere yet to submit to.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Binds the engine the server planted: the suggestions are submitted to
    /// it, and `wake` — the applier's — is notified once each is queued.
    pub fn bind(&self, engine: &Arc<Engine>, wake: Arc<Notify>) {
        self.submitting(Arc::new(Engaged {
            engine: Arc::downgrade(engine),
            wake,
        }));
    }

    /// Binds another place to submit the suggestions to.
    pub fn submitting(&self, sink: Arc<dyn Submit>) {
        *self.sink.lock().unwrap_or_else(PoisonError::into_inner) = Some(sink);
    }

    /// Whether the session's stream is running.
    #[must_use]
    pub fn listens(&self, session: &str) -> bool {
        self.streams
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(session)
            .is_some_and(|task| !task.is_finished())
    }

    /// Ends every stream: the server is stopping.
    pub fn shutdown(&self) {
        let mut streams = self.streams.lock().unwrap_or_else(PoisonError::into_inner);
        for (_, task) in streams.drain() {
            task.abort();
        }
    }

    /// Opens the stream of `session` if none runs: its task is kept. Nothing
    /// starts without a sink to submit to, nor outside a Tokio runtime.
    fn start(&self, session: &str, plan: Plan) {
        let Some(sink) = self
            .sink
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
        else {
            eprintln!("microphone: no engine bound — session {session} listens to nothing");
            return;
        };
        let Ok(runtime) = tokio::runtime::Handle::try_current() else {
            eprintln!("microphone: no runtime — session {session} listens to nothing");
            return;
        };
        let mut streams = self.streams.lock().unwrap_or_else(PoisonError::into_inner);
        if streams.get(session).is_some_and(|task| !task.is_finished()) {
            return;
        }
        let task = runtime.spawn(listen(session.to_string(), plan, sink));
        streams.insert(session.to_string(), task);
    }

    /// Ends the stream of `session`, and its task with it.
    fn stop(&self, session: &str) {
        let task = self
            .streams
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(session);
        if let Some(task) = task {
            task.abort();
        }
    }
}

impl Drop for Listening {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// The task of a session: the port's stream, each phrase read against the
/// vocabulary, each target named submitted as a suggestion. It ends when it
/// is aborted — which drops the stream, and the port's task with it — or when
/// the stream ends by itself (the engine cannot start, fails, or stops): the
/// row says the microphone is on while nothing listens, so the microphone is
/// turned off, as its author would.
async fn listen(session: String, plan: Plan, sink: Arc<dyn Submit>) {
    let why = hear(&session, &plan, sink.as_ref()).await;
    eprintln!("microphone: session {session} listens to nothing: {why}");
    if let Err(why) = sink.submit(turn_off(&session, &plan.caller)).await {
        eprintln!("microphone: session {session}: {why}");
    }
}

/// Listens until the stream ends; why it did.
async fn hear(session: &str, plan: &Plan, sink: &dyn Submit) -> String {
    let mut stream = match plan.speech.listen(&plan.vocabulary.to_hear()).await {
        Ok(stream) => stream,
        Err(e) => return format!("it cannot listen: {e}"),
    };
    while let Some(heard) = stream.next().await {
        let phrase = match heard {
            Ok(transcript) => transcript.text,
            Err(e) => return format!("it failed: {e}"),
        };
        for detection in plan.vocabulary.detect(&phrase) {
            if let Err(why) = sink
                .submit(suggestion(session, &plan.caller, &detection))
                .await
            {
                eprintln!("microphone: session {session}: {why}");
            }
        }
    }
    "its stream ended".to_string()
}

/// The command that turns the microphone of `session` off, for `author`.
fn turn_off(session: &str, author: &Caller) -> Submission {
    Submission {
        data_capability: DATA_CAPABILITY.to_string(),
        payload: BTreeMap::from([("on".to_string(), json!(false))]),
        target_id: Some(session.to_string()),
        caller: Caller {
            from: DATA_CAPABILITY.to_string(),
            ..author.clone()
        },
        ..Submission::default()
    }
}

/// The suggestion `detection` makes in `session` for `author`, who turned the
/// microphone on: the words that named it are what the row keeps of the
/// microphone — text, never a recording.
fn suggestion(session: &str, author: &Caller, detection: &Detection) -> Submission {
    let target = match detection.kind {
        Kind::Entity => "entityId",
        Kind::Sound => "soundId",
    };
    let payload: BTreeMap<String, Value> = [
        ("sessionId", json!(session)),
        (target, json!(detection.id)),
        ("matchedText", json!(detection.via)),
    ]
    .into_iter()
    .map(|(field, value)| (field.to_string(), value))
    .collect();
    Submission {
        data_capability: ADD_SUGGESTION.to_string(),
        payload,
        caller: Caller {
            from: DATA_CAPABILITY.to_string(),
            ..author.clone()
        },
        ..Submission::default()
    }
}

/// What `Prepare` stages for the writes the [`Observer`] sees: the stream
/// of a session about to listen, and the server's [`Listening`].
#[derive(Default)]
pub struct Plans {
    staged: Mutex<HashMap<String, Plan>>,
    listening: Mutex<Option<Arc<Listening>>>,
}

impl Plans {
    /// Remembers the server's [`Listening`], and stages `plan` for the
    /// session when there is one: the microphone about to be turned on. A
    /// command that turns it off stages none and takes none away — the plan
    /// of a command queued before it is still that command's.
    pub fn stage(&self, session: &str, listening: &Arc<Listening>, plan: Option<Plan>) {
        *self
            .listening
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(Arc::clone(listening));
        if let Some(plan) = plan {
            self.staged
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .insert(session.to_string(), plan);
        }
    }

    fn listening(&self) -> Option<Arc<Listening>> {
        self.listening
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    fn take(&self, session: &str) -> Option<Plan> {
        self.staged
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(session)
    }
}

/// Sees every write of a session once applied: its microphone turned on
/// opens the stream the command staged, turned off ends it.
pub struct Observer(pub Arc<Plans>);

impl Project for Observer {
    fn observe(&self, write: &Write) {
        let Some(session) = write
            .partition
            .strip_prefix(SESSION)
            .filter(|_| !write.tombstone)
        else {
            return;
        };
        let Some(on) = write
            .values
            .get(aggregate_session::MICROPHONE_ON)
            .and_then(Value::as_bool)
        else {
            return;
        };
        let Some(listening) = self.0.listening() else {
            return;
        };
        if on {
            // A stream already runs: this write is not the one that opens a
            // stream, and the staged plan stays for the on that follows an off.
            if listening.listens(session) {
                return;
            }
            if let Some(plan) = self.0.take(session) {
                listening.start(session, plan);
            }
        } else {
            listening.stop(session);
        }
    }
}
