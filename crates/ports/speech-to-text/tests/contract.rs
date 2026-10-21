//! The contract suite, run against the fake — as an adapter crate runs it
//! against itself.

use ports_speech_to_text::contract::{Scenario, run};
use ports_speech_to_text::fake::FakeSpeechToText;
use ports_speech_to_text::{
    Dictation, RequestJournal, SharedSpeechToText, SpeechToText, SpeechToTextError, Transcript,
    Vocabulary,
};

const HEARD: &str = "Élowen entre dans la Tour d'Argent, le tonnerre gronde.";

fn vocabulary() -> Vocabulary {
    Vocabulary {
        campaign_names: vec!["Élowen".into(), "Tour d'Argent".into()],
        aliases: vec!["Élo".into(), "la Louve".into()],
        sound_names: vec!["tonnerre lointain".into()],
    }
}

fn scenario() -> Scenario {
    Scenario {
        dictation: Dictation::new(vec![7u8; 3200]),
        vocabulary: vocabulary(),
        expected: HEARD.into(),
    }
}

fn fake() -> FakeSpeechToText {
    FakeSpeechToText::new(HEARD)
}

#[tokio::test]
async fn the_fake_passes_the_contract() {
    run(&fake(), &scenario()).await;
}

#[tokio::test]
async fn the_contract_still_holds_on_a_fake_that_already_has_requests() {
    let fake = fake();
    run(&fake, &scenario()).await;
    run(&fake, &scenario()).await;
}

/// The handle a server shares with its units is the port behind it: it
/// passes the contract, and every clone of it reads the same journal.
#[tokio::test]
async fn the_shared_handle_passes_the_contract_and_shares_its_journal() {
    let shared = SharedSpeechToText::new(fake());
    run(&shared, &scenario()).await;
    let clone = shared.clone();
    assert!(!shared.journal().list().is_empty());
    assert_eq!(clone.journal().list(), shared.journal().list());
}

/// Behind the handle, a port that breaks the contract still breaks it.
#[tokio::test]
#[should_panic(expected = "the text heard")]
async fn the_shared_handle_hands_the_port_s_own_transcript_over() {
    let mut scenario = scenario();
    scenario.expected = "Autre chose".into();
    run(&SharedSpeechToText::new(fake()), &scenario).await;
}

#[tokio::test]
#[should_panic(expected = "the text heard")]
async fn the_contract_fails_a_port_that_hears_another_text() {
    let mut scenario = scenario();
    scenario.expected = "Autre chose".into();
    run(&fake(), &scenario).await;
}

#[tokio::test]
#[should_panic(expected = "the scenario's dictation holds no audio")]
async fn the_contract_refuses_a_scenario_without_audio() {
    let mut scenario = scenario();
    scenario.dictation = Dictation::new(Vec::new());
    run(&fake(), &scenario).await;
}

/// A port that behaves as the fake does, except for one fault.
enum Fault {
    /// Journals the vocabulary without its aliases.
    DropsAliases,
    /// Journals the campaign names in another order.
    ReordersNames,
    /// Journals a vocabulary of its own when it is given none.
    InventsTerms,
    /// Accepts an empty dictation and journals it.
    AcceptsSilence,
    /// Journals every request twice.
    JournalsTwice,
    /// Sorts the aliases, but only when they are the sole terms.
    SortsLoneAliases,
    /// Reverses the campaign names, but only when they are the sole terms.
    ReordersLoneNames,
    /// Drops the sound names, but only when they are the sole terms.
    DropsLoneSounds,
    /// Journals one byte less than the audio it was given.
    CountsOneByteShort,
}

struct Faulty {
    inner: FakeSpeechToText,
    fault: Fault,
    journal: RequestJournal,
}

impl Faulty {
    fn new(fault: Fault) -> Self {
        Self {
            inner: fake(),
            fault,
            journal: RequestJournal::new(),
        }
    }
}

impl SpeechToText for Faulty {
    async fn transcribe(
        &self,
        dictation: &Dictation,
        vocabulary: &Vocabulary,
    ) -> Result<Transcript, SpeechToTextError> {
        if dictation.audio.is_empty() && matches!(self.fault, Fault::AcceptsSilence) {
            self.journal.record(dictation, vocabulary);
            return Ok(Transcript::new(""));
        }
        let transcript = self.inner.transcribe(dictation, vocabulary).await?;
        let mut journalled = vocabulary.clone();
        match self.fault {
            Fault::DropsAliases => journalled.aliases.clear(),
            Fault::ReordersNames => journalled.campaign_names.reverse(),
            Fault::InventsTerms if *vocabulary == Vocabulary::default() => {
                journalled.sound_names.push("tonnerre".into());
            }
            Fault::SortsLoneAliases
                if journalled.campaign_names.is_empty() && journalled.sound_names.is_empty() =>
            {
                journalled.aliases.sort();
            }
            Fault::ReordersLoneNames
                if journalled.aliases.is_empty() && journalled.sound_names.is_empty() =>
            {
                journalled.campaign_names.reverse();
            }
            Fault::DropsLoneSounds
                if journalled.campaign_names.is_empty() && journalled.aliases.is_empty() =>
            {
                journalled.sound_names.clear();
            }
            _ => {}
        }
        let counted = if matches!(self.fault, Fault::CountsOneByteShort) {
            Dictation::new(&dictation.audio[..dictation.audio.len() - 1])
        } else {
            dictation.clone()
        };
        self.journal.record(&counted, &journalled);
        if matches!(self.fault, Fault::JournalsTwice) {
            self.journal.record(&counted, &journalled);
        }
        Ok(transcript)
    }

    fn journal(&self) -> &RequestJournal {
        &self.journal
    }
}

#[tokio::test]
#[should_panic(expected = "the vocabulary journalled")]
async fn the_contract_fails_a_port_that_drops_the_aliases() {
    run(&Faulty::new(Fault::DropsAliases), &scenario()).await;
}

#[tokio::test]
#[should_panic(expected = "the vocabulary journalled")]
async fn the_contract_fails_a_port_that_reorders_the_names() {
    run(&Faulty::new(Fault::ReordersNames), &scenario()).await;
}

#[tokio::test]
#[should_panic(expected = "the vocabulary journalled")]
async fn the_contract_fails_a_port_that_invents_terms() {
    run(&Faulty::new(Fault::InventsTerms), &scenario()).await;
}

#[tokio::test]
#[should_panic(expected = "an empty dictation")]
async fn the_contract_fails_a_port_that_accepts_silence() {
    run(&Faulty::new(Fault::AcceptsSilence), &scenario()).await;
}

#[tokio::test]
#[should_panic(expected = "the request journalled once")]
async fn the_contract_fails_a_port_that_journals_twice() {
    run(&Faulty::new(Fault::JournalsTwice), &scenario()).await;
}

#[tokio::test]
#[should_panic(expected = "the vocabulary journalled")]
async fn the_contract_fails_a_port_that_sorts_a_list_given_alone() {
    run(&Faulty::new(Fault::SortsLoneAliases), &scenario()).await;
}

#[tokio::test]
#[should_panic(expected = "the vocabulary journalled")]
async fn the_contract_fails_a_port_that_reorders_names_given_alone() {
    run(&Faulty::new(Fault::ReordersLoneNames), &scenario()).await;
}

#[tokio::test]
#[should_panic(expected = "the vocabulary journalled")]
async fn the_contract_fails_a_port_that_drops_sounds_given_alone() {
    run(&Faulty::new(Fault::DropsLoneSounds), &scenario()).await;
}

#[tokio::test]
#[should_panic(expected = "the size of the audio journalled")]
async fn the_contract_fails_a_port_that_journals_the_wrong_audio_size() {
    run(&Faulty::new(Fault::CountsOneByteShort), &scenario()).await;
}
