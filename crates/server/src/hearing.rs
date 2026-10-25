//! The `speech-to-text` port's fake, made to hear the brief of the OralBrief
//! fixture where the shell had no microphone to offer.
//!
//! A browser without a microphone hands the shell's *no-device take*: the
//! bytes of the Micro-UI's id (`OralBrief`), never empty so the port is always
//! asked. The fake of `ports-speech-to-text` hears one text whatever the
//! audio; this server tells two takes apart — that one hears the brief, any
//! other audio hears the draft of notes — and keeps one journal for both.

use ports_speech_to_text::fake::FakeSpeechToText;
use ports_speech_to_text::{
    Dictation, RequestJournal, SpeechToText, SpeechToTextError, Transcript, Vocabulary,
};

/// The audio the shell hands when no microphone answers: the id of the
/// Micro-UI that asked, as bytes.
pub const ORAL_BRIEF_TAKE: &[u8] = b"OralBrief";

/// Two [`FakeSpeechToText`], the first for the brief's take and the second for
/// any other audio, recording in the one journal of this port.
pub struct Hearing {
    brief: FakeSpeechToText,
    other: FakeSpeechToText,
    journal: RequestJournal,
}

impl Hearing {
    /// A port that hears `brief` in the take of [`ORAL_BRIEF_TAKE`] and
    /// `other` in any other audio.
    #[must_use]
    pub fn new(brief: impl Into<String>, other: impl Into<String>) -> Self {
        Self {
            brief: FakeSpeechToText::new(brief),
            other: FakeSpeechToText::new(other),
            journal: RequestJournal::new(),
        }
    }
}

impl SpeechToText for Hearing {
    async fn transcribe(
        &self,
        dictation: &Dictation,
        vocabulary: &Vocabulary,
    ) -> Result<Transcript, SpeechToTextError> {
        let fake = if dictation.audio == ORAL_BRIEF_TAKE {
            &self.brief
        } else {
            &self.other
        };
        let heard = fake.transcribe(dictation, vocabulary).await?;
        self.journal.record(dictation, vocabulary);
        Ok(heard)
    }

    fn journal(&self) -> &RequestJournal {
        &self.journal
    }
}

#[cfg(test)]
mod tests {
    use ports_speech_to_text::contract;

    use super::*;

    #[tokio::test]
    async fn an_empty_take_is_refused_and_not_journalled() {
        let port = Hearing::new("le brief", "les notes");
        let vocabulary = Vocabulary::default();
        let refused = port
            .transcribe(&Dictation::new(Vec::new()), &vocabulary)
            .await;
        assert_eq!(refused, Err(SpeechToTextError::EmptyAudio));
        assert!(port.journal().list().is_empty());
    }

    #[tokio::test]
    async fn the_journal_records_what_was_asked_of_either_fake() {
        let port = Hearing::new("le brief", "les notes");
        let vocabulary = Vocabulary {
            campaign_names: vec!["Morlox".into()],
            ..Vocabulary::default()
        };
        port.transcribe(&Dictation::new(ORAL_BRIEF_TAKE), &vocabulary)
            .await
            .expect("heard");
        port.transcribe(&Dictation::new(vec![0_u8; 3]), &Vocabulary::default())
            .await
            .expect("heard");
        let listed = port.journal().list();
        assert_eq!(
            listed
                .iter()
                .map(|r| (r.sequence, r.audio_bytes))
                .collect::<Vec<_>>(),
            [(1, ORAL_BRIEF_TAKE.len()), (2, 3)]
        );
        assert_eq!(listed[0].vocabulary, vocabulary);
    }

    #[tokio::test]
    async fn it_passes_the_contract_of_the_port() {
        let scenario = contract::Scenario {
            dictation: Dictation::new(ORAL_BRIEF_TAKE),
            vocabulary: Vocabulary {
                campaign_names: vec!["Morlox".into(), "Valombre".into()],
                aliases: vec!["Le Balafré".into()],
                sound_names: vec!["Orage".into()],
            },
            expected: "le brief".to_owned(),
        };
        contract::run(&Hearing::new("le brief", "les notes"), &scenario).await;
    }
}
