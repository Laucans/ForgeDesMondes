//! The fake of the `speech-to-text` port: it answers the transcript it was
//! configured with whatever the audio, opens no device, spawns no process, and
//! journals every request it accepts with the vocabulary it was given.

use crate::{Dictation, RequestJournal, SpeechToText, SpeechToTextError, Transcript, Vocabulary};

/// The recording fake. A clone shares the journal, as every handle of it does.
#[derive(Clone, Debug)]
pub struct FakeSpeechToText {
    transcript: Transcript,
    journal: RequestJournal,
}

impl FakeSpeechToText {
    /// A fake that hears `text` in every dictation.
    #[must_use]
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            transcript: Transcript::new(text),
            journal: RequestJournal::new(),
        }
    }
}

impl SpeechToText for FakeSpeechToText {
    async fn transcribe(
        &self,
        dictation: &Dictation,
        vocabulary: &Vocabulary,
    ) -> Result<Transcript, SpeechToTextError> {
        if dictation.audio.is_empty() {
            return Err(SpeechToTextError::EmptyAudio);
        }
        self.journal.record(dictation, vocabulary);
        Ok(self.transcript.clone())
    }

    fn journal(&self) -> &RequestJournal {
        &self.journal
    }
}
