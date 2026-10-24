//! The contract suite of the `speech-to-text` port: what every adapter does
//! the same.
//!
//! An adapter crate runs it against itself from a test of its own, on
//! whatever stands in for its engine (a stub program):
//!
//! ```ignore
//! #[tokio::test]
//! async fn the_adapter_passes_the_contract() {
//!     let scenario = Scenario {
//!         dictation: Dictation::new(vec![1u8; 3200]),
//!         vocabulary: Vocabulary { /* ... */ ..Vocabulary::default() },
//!         expected: "…".to_owned(),
//!         heard: vec!["…".to_owned()],
//!     };
//!     ports_speech_to_text::contract::run(&MyAdapter::new(), &scenario).await;
//! }
//! ```
//!
//! It holds the adapter to: a dictation is answered by the text the engine
//! heard; the vocabulary reaches the journal as it was passed — same terms,
//! same order, nothing added or dropped — whether it is full or empty; the
//! journal lists each request in order with the size of its audio; and an
//! empty dictation fails with [`SpeechToTextError::EmptyAudio`] and journals
//! nothing.
//!
//! And a stream: [`SpeechToText::listen`] yields the phrases the engine heard,
//! in order, and stays open, silent, once they are said; its vocabulary
//! reaches the journal as it was passed — whole, empty and each list alone —
//! with no audio byte; and stopping it ends it, leaving no task running.
//!
//! A failed check panics with what it was about.

use std::time::Duration;

use crate::{Dictation, SpeechToText, SpeechToTextError, Transcript, TranscriptStream, Vocabulary};

/// How long a stream is watched to see it does not end by itself.
const WATCH: Duration = Duration::from_millis(100);

/// How long a stream has to say a phrase its stand-in has to say.
const PATIENCE: Duration = Duration::from_secs(5);

/// What the adapter's engine stand-in hears.
#[derive(Clone, Debug)]
pub struct Scenario {
    /// A non-empty dictation the stand-in understands.
    pub dictation: Dictation,
    /// A vocabulary of campaign names, aliases and sound names.
    pub vocabulary: Vocabulary,
    /// The text the stand-in answers to `dictation`.
    pub expected: String,
    /// The phrases the stand-in yields to a stream, in order. At least one.
    pub heard: Vec<String>,
}

/// Runs the whole suite on `port`.
///
/// # Panics
///
/// When the adapter breaks the contract.
pub async fn run<P: SpeechToText>(port: &P, scenario: &Scenario) {
    assert!(
        !scenario.dictation.audio.is_empty(),
        "the scenario's dictation holds no audio"
    );
    a_dictation_is_answered_by_the_text_heard(port, scenario).await;
    the_vocabulary_reaches_the_journal_unchanged(port, scenario).await;
    the_journal_lists_each_request_in_order(port, scenario).await;
    an_empty_dictation_fails_and_asks_nothing(port, scenario).await;
    assert!(
        !scenario.heard.is_empty(),
        "the scenario's stream hears nothing"
    );
    a_stream_yields_the_phrases_heard_in_order(port, scenario).await;
    a_stream_stays_open_until_it_is_stopped(port, scenario).await;
    a_stopped_stream_leaves_no_task_running(port, scenario).await;
    a_stream_journals_the_vocabulary_unchanged_and_no_audio(port, scenario).await;
}

async fn a_dictation_is_answered_by_the_text_heard<P: SpeechToText>(port: &P, scenario: &Scenario) {
    let transcript = port
        .transcribe(&scenario.dictation, &scenario.vocabulary)
        .await
        .unwrap_or_else(|error| panic!("the dictation failed: {error}"));
    assert_eq!(transcript.text, scenario.expected, "the text heard");
}

async fn the_vocabulary_reaches_the_journal_unchanged<P: SpeechToText>(
    port: &P,
    scenario: &Scenario,
) {
    let Vocabulary {
        campaign_names,
        aliases,
        sound_names,
    } = scenario.vocabulary.clone();
    // Whole, empty, and each list alone: a port that sorts, dedups or drops a
    // list only when the others are empty is caught too.
    let vocabularies = [
        scenario.vocabulary.clone(),
        Vocabulary::default(),
        Vocabulary {
            campaign_names,
            ..Vocabulary::default()
        },
        Vocabulary {
            aliases,
            ..Vocabulary::default()
        },
        Vocabulary {
            sound_names,
            ..Vocabulary::default()
        },
    ];
    for vocabulary in &vocabularies {
        let before = port.journal().list().len();
        port.transcribe(&scenario.dictation, vocabulary)
            .await
            .unwrap_or_else(|error| panic!("the dictation failed: {error}"));
        let journal = port.journal().list();
        assert_eq!(journal.len(), before + 1, "the request journalled once");
        let last = journal.last().expect("a request was journalled");
        assert_eq!(&last.vocabulary, vocabulary, "the vocabulary journalled");
    }
}

async fn the_journal_lists_each_request_in_order<P: SpeechToText>(port: &P, scenario: &Scenario) {
    let journal = port.journal().list();
    assert!(!journal.is_empty(), "the journal of the requests accepted");
    for (index, record) in journal.iter().enumerate() {
        assert_eq!(
            record.sequence,
            index as u64 + 1,
            "the sequence of the request journalled"
        );
    }
    let before = journal.len();
    let longer = Dictation::new([scenario.dictation.audio.clone(), vec![0u8; 2]].concat());
    port.transcribe(&longer, &scenario.vocabulary)
        .await
        .unwrap_or_else(|error| panic!("the dictation failed: {error}"));
    let journal = port.journal().list();
    assert_eq!(journal.len(), before + 1, "the request journalled once");
    let last = journal.last().expect("a request was journalled");
    assert_eq!(
        last.audio_bytes,
        longer.audio.len(),
        "the size of the audio journalled"
    );
    assert_eq!(
        last.sequence,
        before as u64 + 1,
        "the sequence of the request journalled"
    );
}

async fn an_empty_dictation_fails_and_asks_nothing<P: SpeechToText>(port: &P, scenario: &Scenario) {
    let before = port.journal().list().len();
    let result = port
        .transcribe(&Dictation::new(Vec::new()), &scenario.vocabulary)
        .await;
    assert_eq!(
        result,
        Err(SpeechToTextError::EmptyAudio),
        "an empty dictation"
    );
    assert_eq!(
        port.journal().list().len(),
        before,
        "a request was journalled though no audio was sent"
    );
}

async fn open<P: SpeechToText>(port: &P, vocabulary: &Vocabulary) -> TranscriptStream {
    port.listen(vocabulary)
        .await
        .unwrap_or_else(|error| panic!("the stream failed to open: {error}"))
}

/// The next phrase of `stream`, which has one to say.
async fn hear(stream: &mut TranscriptStream) -> Transcript {
    tokio::time::timeout(PATIENCE, stream.next())
        .await
        .expect("a phrase is heard in time")
        .expect("the stream ended before every phrase was heard")
        .unwrap_or_else(|error| panic!("the stream failed: {error}"))
}

async fn a_stream_yields_the_phrases_heard_in_order<P: SpeechToText>(
    port: &P,
    scenario: &Scenario,
) {
    let mut stream = open(port, &scenario.vocabulary).await;
    let mut heard = Vec::new();
    for _ in &scenario.heard {
        heard.push(hear(&mut stream).await.text);
    }
    assert_eq!(heard, scenario.heard, "the phrases heard, in order");
    stream.stop().await;
}

async fn a_stream_stays_open_until_it_is_stopped<P: SpeechToText>(port: &P, scenario: &Scenario) {
    let mut stream = open(port, &scenario.vocabulary).await;
    for _ in &scenario.heard {
        hear(&mut stream).await;
    }
    let after = tokio::time::timeout(WATCH, stream.next()).await;
    assert!(after.is_err(), "the stream stays open until it is stopped");
    stream.stop().await;
}

async fn a_stopped_stream_leaves_no_task_running<P: SpeechToText>(port: &P, scenario: &Scenario) {
    let mut stream = open(port, &scenario.vocabulary).await;
    hear(&mut stream).await;
    stream.stop().await;
    assert!(stream.is_finished(), "the task of a stopped stream is over");
    assert!(
        stream.next().await.is_none(),
        "a stopped stream yields nothing more"
    );
    // Stopping again changes nothing.
    stream.stop().await;
    assert!(stream.is_finished(), "the task of a stopped stream is over");
}

async fn a_stream_journals_the_vocabulary_unchanged_and_no_audio<P: SpeechToText>(
    port: &P,
    scenario: &Scenario,
) {
    let Vocabulary {
        campaign_names,
        aliases,
        sound_names,
    } = scenario.vocabulary.clone();
    let vocabularies = [
        scenario.vocabulary.clone(),
        Vocabulary::default(),
        Vocabulary {
            campaign_names,
            ..Vocabulary::default()
        },
        Vocabulary {
            aliases,
            ..Vocabulary::default()
        },
        Vocabulary {
            sound_names,
            ..Vocabulary::default()
        },
    ];
    for vocabulary in &vocabularies {
        let before = port.journal().list().len();
        let mut stream = open(port, vocabulary).await;
        stream.stop().await;
        let journal = port.journal().list();
        assert_eq!(journal.len(), before + 1, "the stream journalled once");
        let last = journal.last().expect("a stream was journalled");
        assert_eq!(
            &last.vocabulary, vocabulary,
            "the vocabulary journalled for a stream"
        );
        assert_eq!(last.audio_bytes, 0, "no audio journalled for a stream");
        assert_eq!(
            last.sequence,
            before as u64 + 1,
            "the sequence of the stream journalled"
        );
    }
}
