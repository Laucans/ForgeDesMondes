//! The stream of phrases [`SpeechToText::listen`](crate::SpeechToText::listen)
//! opens: [`TranscriptStream`].
//!
//! The adapter listens (the microphone is its to open, never the caller's) and
//! the stream hands the caller what it heard as [`Transcript`]s, text and
//! nothing else. The task that listens is spawned by [`TranscriptStream::spawn`]
//! and owned by the stream, which keeps its `JoinHandle`: [`stop`] ends it and
//! waits for it, [`Drop`] aborts it, so no task outlives its stream.
//!
//! [`stop`]: TranscriptStream::stop

use std::future::Future;
use std::time::Duration;

use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;

use crate::{SpeechToTextError, Transcript};

/// How many phrases wait for the caller before the listening task does.
const BACKLOG: usize = 32;

/// How long [`TranscriptStream::stop`] lets the task end by itself, once told
/// to, before it aborts it.
const STOP_GRACE: Duration = Duration::from_secs(2);

/// What a stream yields: a phrase heard, or the reason listening failed (after
/// which the stream ends).
pub type Heard = Result<Transcript, SpeechToTextError>;

/// The phrases of a stream, as seen by the task that listens: where it puts
/// what it heard.
#[derive(Debug)]
pub struct Phrases(mpsc::Sender<Heard>);

impl Phrases {
    /// Hands the caller `transcript`. `false` once the caller is gone and
    /// nobody will read any more: the task should end.
    pub async fn say(&self, transcript: Transcript) -> bool {
        self.0.send(Ok(transcript)).await.is_ok()
    }

    /// Tells the caller listening failed. `false` once the caller is gone.
    pub async fn fail(&self, error: SpeechToTextError) -> bool {
        self.0.send(Err(error)).await.is_ok()
    }
}

/// The stop of a stream, as seen by the task that listens: it completes once
/// the caller asked to stop, or dropped the stream.
#[derive(Debug)]
pub struct StopSignal(watch::Receiver<bool>);

impl StopSignal {
    /// Completes when the task has to end.
    pub async fn requested(&mut self) {
        // An error is the stream dropped: the same answer.
        let _ = self.0.wait_for(|stop| *stop).await;
    }

    /// Runs `step`, unless the task has to end first: `None` then.
    pub async fn unless_stopped<F: Future>(&mut self, step: F) -> Option<F::Output> {
        tokio::select! {
            () = self.requested() => None,
            output = step => Some(output),
        }
    }
}

/// The phrases heard while the microphone listens, until stopped.
///
/// A stream has no field that could say where to send what it heard, and
/// [`Transcript`] holds a `String`: audio goes no further than the adapter.
/// These do not compile:
///
/// ```compile_fail,E0560
/// use ports_speech_to_text::Vocabulary;
/// let _ = Vocabulary { destination: "https://example.org".to_owned(), ..Vocabulary::default() };
/// ```
///
/// ```compile_fail,E0560
/// use ports_speech_to_text::TranscriptStream;
/// let _ = TranscriptStream { destination: "https://example.org".to_owned() };
/// ```
///
/// ```compile_fail,E0061
/// use ports_speech_to_text::{SpeechToText, Vocabulary, fake::FakeSpeechToText};
/// let _ = FakeSpeechToText::new("texte").listen(&Vocabulary::default(), "https://example.org");
/// ```
#[derive(Debug)]
pub struct TranscriptStream {
    heard: mpsc::Receiver<Heard>,
    stop: watch::Sender<bool>,
    /// `None` once stopped and awaited.
    task: Option<JoinHandle<()>>,
}

impl TranscriptStream {
    /// Spawns the task that listens, and returns the stream it feeds, so it is
    /// called within a Tokio runtime (it panics outside one). `listen`
    /// is handed where to put the phrases and the signal to end on; it ends
    /// when the signal completes, or when [`Phrases`] says nobody reads.
    /// Called within a Tokio runtime.
    pub fn spawn<F, Fut>(listen: F) -> Self
    where
        F: FnOnce(Phrases, StopSignal) -> Fut,
        Fut: Future<Output = ()> + Send + 'static,
    {
        let (sender, heard) = mpsc::channel(BACKLOG);
        let (stop, signal) = watch::channel(false);
        let task = tokio::spawn(listen(Phrases(sender), StopSignal(signal)));
        Self {
            heard,
            stop,
            task: Some(task),
        }
    }

    /// The next phrase heard, waiting for it. `None` once the stream is over:
    /// stopped, or ended by the engine after the last phrase and error it had.
    pub async fn next(&mut self) -> Option<Heard> {
        self.heard.recv().await
    }

    /// Stops listening and waits until the task is over: when this returns, no
    /// task of the stream is running and [`next`](Self::next) answers `None`.
    /// A phrase that was heard but not read is dropped. Stopping a stream
    /// already stopped does nothing.
    pub async fn stop(&mut self) {
        self.heard.close();
        self.stop.send_replace(true);
        // The handle stays in the stream until the task is over: a `stop`
        // cancelled midway leaves a stream that `Drop` still aborts.
        if let Some(task) = self.task.as_mut() {
            if tokio::time::timeout(STOP_GRACE, &mut *task).await.is_err() {
                task.abort();
                let _ = (&mut *task).await;
            }
            self.task = None;
        }
        while self.heard.try_recv().is_ok() {}
    }

    /// Whether the task that listens is over.
    #[must_use]
    pub fn is_finished(&self) -> bool {
        self.task.as_ref().is_none_or(JoinHandle::is_finished)
    }
}

impl Drop for TranscriptStream {
    fn drop(&mut self) {
        if let Some(task) = &self.task {
            task.abort();
        }
    }
}
