//! The `speech-to-text` port (ports.json) — the one seam to local
//! transcription.
//!
//! A DataCapability that takes dictated notes calls [`SpeechToText`] and never
//! knows which engine answers. The port is stateless: every call carries the
//! [`Dictation`] and the [`Vocabulary`] to listen for.
//!
//! - **Local, by construction.** A [`Dictation`] is bytes handed to the port
//!   and nothing else: no URL, no destination, no credential. The adapter
//!   runs the engine on the DM's machine; no type of this crate names a
//!   remote place audio could be sent to.
//! - **French.** The language is not a parameter: [`LANGUAGE`] is what every
//!   adapter listens for. What varies is the [`Vocabulary`], injected from the
//!   campaign's names, aliases and sound names.
//! - **Text only comes out.** A [`Transcript`] holds a `String`, so audio goes
//!   no further than the port (the `llm` port takes the text from there).
//! - **Journalled.** Every request a call accepts is recorded in the
//!   [`RequestJournal`] of [`SpeechToText::journal`]: the size of the audio —
//!   never the audio — and the vocabulary, unchanged.
//! - **[`fake`]** answers deterministically and opens no device and spawns no
//!   process: the tests, the CI, the rules' runtime and the mock-up run on it.
//! - **[`contract`]** is the suite every adapter passes, run against itself
//!   from its own tests.
//!
//! The adapters (the local Phonon sidecar) live under
//! `crates/ports/speech-to-text/<provider>/`; this crate holds no engine.

use std::fmt;
use std::future::Future;
use std::sync::{Arc, Mutex, PoisonError};

pub mod contract;
pub mod fake;

/// The language every adapter transcribes, as a BCP 47 tag.
pub const LANGUAGE: &str = "fr";

/// What the DM dictated: raw audio, as the capture hands it over.
///
/// The bytes are mono 16-bit little-endian PCM at 16 kHz. A dictation has no
/// field that could say where to send it; these do not compile:
///
/// ```compile_fail,E0560
/// use ports_speech_to_text::Dictation;
/// let _ = Dictation { audio: vec![0u8], destination: "https://example.org".to_owned() };
/// ```
///
/// ```compile_fail,E0560
/// use ports_speech_to_text::Transcript;
/// let _ = Transcript { text: String::new(), audio: vec![0u8] };
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Dictation {
    /// The audio to transcribe.
    pub audio: Vec<u8>,
}

impl Dictation {
    /// A dictation of `audio`.
    #[must_use]
    pub fn new(audio: impl Into<Vec<u8>>) -> Self {
        Self {
            audio: audio.into(),
        }
    }
}

/// What the engine should listen for: the words a French model alone would
/// get wrong. Injected by the caller from the campaign, handed to the engine
/// as it is.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Vocabulary {
    /// The names of the campaign's characters, places and events.
    pub campaign_names: Vec<String>,
    /// Their aliases.
    pub aliases: Vec<String>,
    /// The names of the sounds of the campaign's library.
    pub sound_names: Vec<String>,
}

/// What the engine heard: text, and nothing else.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Transcript {
    /// The transcribed French text.
    pub text: String,
}

impl Transcript {
    /// A transcript saying `text`.
    #[must_use]
    pub fn new(text: impl Into<String>) -> Self {
        Self { text: text.into() }
    }
}

/// Why a transcription failed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SpeechToTextError {
    /// The dictation holds no audio: nothing was asked of the engine.
    EmptyAudio,
    /// The engine could not answer; the reason is for the DM.
    Failed(String),
}

impl fmt::Display for SpeechToTextError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyAudio => f.write_str("aucun son à transcrire"),
            Self::Failed(reason) => f.write_str(reason),
        }
    }
}

impl std::error::Error for SpeechToTextError {}

/// One request the port accepted: how much audio, which vocabulary.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RequestRecord {
    /// Its place in the journal, from 1.
    pub sequence: u64,
    /// The size of the audio, in bytes; the audio itself is not kept.
    pub audio_bytes: usize,
    /// The vocabulary the engine was given, as it was passed.
    pub vocabulary: Vocabulary,
}

/// The requests a port accepted, in order. A handle: a clone shares the
/// journal, so the code that lists the requests holds the same one as the
/// port that records them.
#[derive(Clone, Debug, Default)]
pub struct RequestJournal {
    records: Arc<Mutex<Vec<RequestRecord>>>,
}

impl RequestJournal {
    /// An empty journal.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Appends a request; an adapter calls it for every request it accepts.
    pub fn record(&self, dictation: &Dictation, vocabulary: &Vocabulary) {
        let mut records = self.records.lock().unwrap_or_else(PoisonError::into_inner);
        let sequence = records.len() as u64 + 1;
        records.push(RequestRecord {
            sequence,
            audio_bytes: dictation.audio.len(),
            vocabulary: vocabulary.clone(),
        });
    }

    /// Every request accepted, oldest first.
    #[must_use]
    pub fn list(&self) -> Vec<RequestRecord> {
        self.records
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

/// The port: an engine that turns a French dictation into text.
///
/// An implementation transcribes locally, hands `vocabulary` to the engine
/// unchanged, journals every request it accepts (nothing for an empty
/// dictation: nothing is asked of the engine), and passes [`contract::run`].
pub trait SpeechToText: Send + Sync {
    /// Transcribes `dictation` in [`LANGUAGE`], listening for `vocabulary`.
    fn transcribe(
        &self,
        dictation: &Dictation,
        vocabulary: &Vocabulary,
    ) -> impl Future<Output = Result<Transcript, SpeechToTextError>> + Send;

    /// The journal of the requests this port accepted.
    fn journal(&self) -> &RequestJournal;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_journal_handle_shares_its_records_and_numbers_them_from_one() {
        let journal = RequestJournal::new();
        let listing = journal.clone();
        let vocabulary = Vocabulary {
            campaign_names: vec!["Élowen".into()],
            ..Vocabulary::default()
        };
        journal.record(&Dictation::new(vec![0u8; 4]), &vocabulary);
        journal.record(&Dictation::new(vec![0u8; 9]), &Vocabulary::default());
        let listed = listing.list();
        assert_eq!(
            listed
                .iter()
                .map(|r| (r.sequence, r.audio_bytes))
                .collect::<Vec<_>>(),
            [(1, 4), (2, 9)]
        );
        assert_eq!(listed[0].vocabulary, vocabulary);
    }

    #[test]
    fn the_errors_speak_to_the_dm() {
        assert_eq!(
            SpeechToTextError::EmptyAudio.to_string(),
            "aucun son à transcrire"
        );
        assert_eq!(
            SpeechToTextError::Failed("moteur absent".into()).to_string(),
            "moteur absent"
        );
    }
}
