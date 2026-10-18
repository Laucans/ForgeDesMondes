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
//! nothing. A failed check panics with what it was about.

use crate::{Dictation, SpeechToText, SpeechToTextError, Vocabulary};

/// What the adapter's engine stand-in hears.
#[derive(Clone, Debug)]
pub struct Scenario {
    /// A non-empty dictation the stand-in understands.
    pub dictation: Dictation,
    /// A vocabulary of campaign names, aliases and sound names.
    pub vocabulary: Vocabulary,
    /// The text the stand-in answers to `dictation`.
    pub expected: String,
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
    for vocabulary in [&scenario.vocabulary, &Vocabulary::default()] {
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
