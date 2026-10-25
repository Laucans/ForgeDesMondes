//! The fake of the `speech-to-text` port: it answers the transcript it was
//! configured with whatever the audio, replays the phrases it was configured
//! with when it listens, opens no device, spawns no process, and journals every
//! request it accepts with the vocabulary it was given.

use std::time::Duration;

use crate::stream::{Phrases, StopSignal};
use crate::{
    Dictation, RequestJournal, SpeechToText, SpeechToTextError, Transcript, TranscriptStream,
    Vocabulary,
};

/// The recording fake. A clone shares the journal, as every handle of it does.
#[derive(Clone, Debug)]
pub struct FakeSpeechToText {
    transcript: Transcript,
    journal: RequestJournal,
    phrases: Vec<Transcript>,
    interval: Duration,
}

impl FakeSpeechToText {
    /// A fake that hears `text` in every dictation, and nothing when it
    /// listens (see [`listening`](Self::listening)).
    #[must_use]
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            transcript: Transcript::new(text),
            journal: RequestJournal::new(),
            phrases: Vec::new(),
            interval: Duration::ZERO,
        }
    }

    /// The same fake, which hears `phrases` in order when it listens, one every
    /// `interval` (the first `interval` after the stream opens). Once the last
    /// is heard the stream stays open and silent, as a microphone does, until
    /// it is stopped.
    #[must_use]
    pub fn listening<I, S>(mut self, phrases: I, interval: Duration) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.phrases = phrases.into_iter().map(Transcript::new).collect();
        self.interval = interval;
        self
    }
}

/// The listening task of the fake: its phrases on the clock, then silence.
async fn replay(
    phrases: Vec<Transcript>,
    interval: Duration,
    sender: Phrases,
    mut stop: StopSignal,
) {
    for phrase in phrases {
        let heard = stop
            .unless_stopped(async {
                tokio::time::sleep(interval).await;
                sender.say(phrase).await
            })
            .await;
        if heard != Some(true) {
            return;
        }
    }
    stop.requested().await;
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

    async fn listen(&self, vocabulary: &Vocabulary) -> Result<TranscriptStream, SpeechToTextError> {
        self.journal.record_listen(vocabulary);
        let (phrases, interval) = (self.phrases.clone(), self.interval);
        Ok(TranscriptStream::spawn(move |sender, stop| {
            replay(phrases, interval, sender, stop)
        }))
    }

    fn journal(&self) -> &RequestJournal {
        &self.journal
    }
}
