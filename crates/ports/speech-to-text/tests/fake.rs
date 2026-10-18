//! What the fake promises beyond the contract: it is deterministic, it
//! records what it was asked, and its journal is the one a DataCapability
//! lists.

use ports_speech_to_text::fake::FakeSpeechToText;
use ports_speech_to_text::{
    Dictation, LANGUAGE, SpeechToText, SpeechToTextError, Transcript, Vocabulary,
};

fn vocabulary() -> Vocabulary {
    Vocabulary {
        campaign_names: vec!["Élowen".into(), "Valdris".into()],
        aliases: vec!["la Louve".into()],
        sound_names: vec!["orage".into(), "taverne animée".into()],
    }
}

#[tokio::test]
async fn it_answers_the_configured_text_whatever_the_audio() {
    let fake = FakeSpeechToText::new("Bonjour à tous");
    let first = fake
        .transcribe(&Dictation::new(vec![1u8; 10]), &vocabulary())
        .await;
    let second = fake
        .transcribe(&Dictation::new(vec![9u8; 500]), &Vocabulary::default())
        .await;
    assert_eq!(first, Ok(Transcript::new("Bonjour à tous")));
    assert_eq!(first, second);
}

#[tokio::test]
async fn it_records_the_request_and_the_vocabulary_unchanged() {
    let fake = FakeSpeechToText::new("texte");
    let vocabulary = vocabulary();
    fake.transcribe(&Dictation::new(vec![0u8; 64]), &vocabulary)
        .await
        .unwrap();
    let journal = fake.journal().list();
    assert_eq!(journal.len(), 1);
    assert_eq!(journal[0].sequence, 1);
    assert_eq!(journal[0].audio_bytes, 64);
    assert_eq!(journal[0].vocabulary, vocabulary);
}

#[tokio::test]
async fn an_empty_dictation_fails_and_records_nothing() {
    let fake = FakeSpeechToText::new("texte");
    assert_eq!(
        fake.transcribe(&Dictation::new(Vec::new()), &vocabulary())
            .await,
        Err(SpeechToTextError::EmptyAudio)
    );
    assert!(fake.journal().list().is_empty());
}

#[tokio::test]
async fn the_journal_a_capability_holds_is_the_one_the_port_writes() {
    let fake = FakeSpeechToText::new("texte");
    let listing = fake.journal().clone();
    fake.transcribe(&Dictation::new(vec![0u8; 2]), &Vocabulary::default())
        .await
        .unwrap();
    assert_eq!(listing.list().len(), 1);
}

#[test]
fn the_port_transcribes_french() {
    assert_eq!(LANGUAGE, "fr");
}
