//! The stream of the `speech-to-text` port: what it hands the caller, and the
//! task behind it, which never outlives it.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use ports_speech_to_text::{SpeechToTextError, Transcript, TranscriptStream};
use tokio::sync::oneshot;

/// Tells when the future that holds it is dropped: ended, or aborted.
struct Dropped(Option<oneshot::Sender<()>>);

impl Drop for Dropped {
    fn drop(&mut self) {
        if let Some(sender) = self.0.take() {
            let _ = sender.send(());
        }
    }
}

fn watched() -> (Dropped, oneshot::Receiver<()>) {
    let (sender, receiver) = oneshot::channel();
    (Dropped(Some(sender)), receiver)
}

/// Waits for the future that held the guard to be dropped.
async fn is_dropped(dropped: oneshot::Receiver<()>) {
    tokio::time::timeout(Duration::from_secs(10), dropped)
        .await
        .expect("the task was dropped in time")
        .expect("the guard told it was dropped");
}

#[tokio::test]
async fn it_yields_what_the_task_says_then_ends_with_the_task() {
    let mut stream = TranscriptStream::spawn(|phrases, _| async move {
        assert!(phrases.say(Transcript::new("un")).await);
        assert!(phrases.say(Transcript::new("deux")).await);
    });
    assert_eq!(stream.next().await, Some(Ok(Transcript::new("un"))));
    assert_eq!(stream.next().await, Some(Ok(Transcript::new("deux"))));
    assert_eq!(stream.next().await, None);
    assert!(stream.is_finished());
}

#[tokio::test]
async fn it_yields_the_failure_the_task_reports() {
    let mut stream = TranscriptStream::spawn(|phrases, _| async move {
        assert!(phrases.say(Transcript::new("un")).await);
        assert!(phrases.fail(SpeechToTextError::Failed("boom".into())).await);
    });
    assert_eq!(stream.next().await, Some(Ok(Transcript::new("un"))));
    assert_eq!(
        stream.next().await,
        Some(Err(SpeechToTextError::Failed("boom".into())))
    );
    assert_eq!(stream.next().await, None);
}

#[tokio::test]
async fn stopping_lets_the_task_end_by_itself_and_waits_for_it() {
    let tidied = Arc::new(AtomicBool::new(false));
    let flag = tidied.clone();
    let mut stream = TranscriptStream::spawn(|_, mut stop| async move {
        stop.requested().await;
        // Work after the signal, across a yield: an abort would cut it short.
        tokio::task::yield_now().await;
        flag.store(true, Ordering::SeqCst);
    });
    assert!(!stream.is_finished());
    stream.stop().await;
    assert!(tidied.load(Ordering::SeqCst), "the task ended by itself");
    assert!(stream.is_finished());
    assert_eq!(stream.next().await, None);
}

/// Time is paused: the grace the task is given elapses at once.
#[tokio::test(start_paused = true)]
async fn stopping_aborts_a_task_that_ignores_the_signal() {
    let (guard, dropped) = watched();
    let mut stream = TranscriptStream::spawn(|_, _| async move {
        let _guard = guard;
        std::future::pending::<()>().await;
    });
    tokio::task::yield_now().await;
    assert!(!stream.is_finished());
    stream.stop().await;
    assert!(stream.is_finished());
    is_dropped(dropped).await;
}

/// A `stop` given up on midway leaves the task to the stream, which still
/// aborts it when dropped.
#[tokio::test(start_paused = true)]
async fn a_stop_given_up_on_midway_leaves_the_task_to_the_stream() {
    let (guard, dropped) = watched();
    let mut stream = TranscriptStream::spawn(|_, _| async move {
        let _guard = guard;
        std::future::pending::<()>().await;
    });
    tokio::task::yield_now().await;
    let given_up = tokio::time::timeout(Duration::from_millis(10), stream.stop()).await;
    assert!(given_up.is_err());
    assert!(!stream.is_finished());
    drop(stream);
    is_dropped(dropped).await;
}

#[tokio::test]
async fn stopping_drops_the_phrases_nobody_read_and_can_be_repeated() {
    let mut stream = TranscriptStream::spawn(|phrases, mut stop| async move {
        phrases.say(Transcript::new("un")).await;
        phrases.say(Transcript::new("deux")).await;
        stop.requested().await;
    });
    tokio::task::yield_now().await;
    stream.stop().await;
    assert_eq!(stream.next().await, None);
    stream.stop().await;
    assert!(stream.is_finished());
    assert_eq!(stream.next().await, None);
}

#[tokio::test]
async fn once_stopped_the_task_is_told_nobody_reads() {
    let (told, answer) = oneshot::channel();
    let mut stream = TranscriptStream::spawn(|phrases, mut stop| async move {
        stop.requested().await;
        let said = phrases.say(Transcript::new("trop tard")).await;
        let failed = phrases.fail(SpeechToTextError::EmptyAudio).await;
        let _ = told.send((said, failed));
    });
    stream.stop().await;
    assert_eq!(answer.await.unwrap(), (false, false));
}

#[tokio::test]
async fn while_it_is_read_the_task_is_told_somebody_reads() {
    let (told, answer) = oneshot::channel();
    let mut stream = TranscriptStream::spawn(|phrases, _| async move {
        let said = phrases.say(Transcript::new("un")).await;
        let failed = phrases.fail(SpeechToTextError::EmptyAudio).await;
        let _ = told.send((said, failed));
    });
    assert!(stream.next().await.is_some());
    assert!(stream.next().await.is_some());
    assert_eq!(answer.await.unwrap(), (true, true));
}

#[tokio::test]
async fn dropping_the_stream_aborts_its_task() {
    let (guard, dropped) = watched();
    let stream = TranscriptStream::spawn(|_, _| async move {
        let _guard = guard;
        std::future::pending::<()>().await;
    });
    tokio::task::yield_now().await;
    drop(stream);
    is_dropped(dropped).await;
}

#[tokio::test]
async fn dropping_a_stopped_stream_is_harmless() {
    let mut stream = TranscriptStream::spawn(|_, mut stop| async move { stop.requested().await });
    stream.stop().await;
    drop(stream);
}

#[tokio::test]
async fn a_step_runs_unless_the_task_has_to_end_first() {
    let (told, answer) = oneshot::channel();
    let (go, ready) = oneshot::channel::<()>();
    let mut stream = TranscriptStream::spawn(|_, mut stop| async move {
        let first = stop.unless_stopped(async { 7 }).await;
        let _ = go.send(());
        stop.requested().await;
        let second = stop.unless_stopped(std::future::pending::<u8>()).await;
        let _ = told.send((first, second));
    });
    ready.await.unwrap();
    stream.stop().await;
    assert_eq!(answer.await.unwrap(), (Some(7), None));
}
