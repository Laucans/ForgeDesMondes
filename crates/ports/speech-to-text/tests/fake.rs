//! What the fake promises beyond the contract: it is deterministic, it
//! records what it was asked, and its journal is the one a DataCapability
//! lists.

use std::time::Duration;

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

const PACE: Duration = Duration::from_millis(250);

fn phrases() -> [&'static str; 3] {
    ["Élowen entre.", "Le tonnerre gronde.", "La Louve hurle."]
}

/// Time is paused: the runtime advances it by the sleeps alone, so the pace
/// is measured exactly.
#[tokio::test(start_paused = true)]
async fn it_listens_to_its_phrases_in_order_at_its_pace() {
    let fake = FakeSpeechToText::new("texte").listening(phrases(), PACE);
    let mut stream = fake.listen(&vocabulary()).await.unwrap();
    let opened = tokio::time::Instant::now();
    for (index, phrase) in phrases().iter().enumerate() {
        let heard = stream.next().await.unwrap().unwrap();
        assert_eq!(heard, Transcript::new(*phrase));
        assert_eq!(
            opened.elapsed(),
            PACE * (index as u32 + 1),
            "phrase {index}"
        );
    }
    stream.stop().await;
}

#[tokio::test(start_paused = true)]
async fn once_its_phrases_are_said_the_stream_stays_open_and_silent() {
    let fake = FakeSpeechToText::new("texte").listening(["Un seul."], PACE);
    let mut stream = fake.listen(&Vocabulary::default()).await.unwrap();
    assert_eq!(stream.next().await, Some(Ok(Transcript::new("Un seul."))));
    assert!(
        tokio::time::timeout(PACE * 100, stream.next())
            .await
            .is_err()
    );
    assert!(!stream.is_finished());
    stream.stop().await;
    assert!(stream.is_finished());
    assert_eq!(stream.next().await, None);
}

#[tokio::test(start_paused = true)]
async fn a_fake_given_no_phrases_listens_in_silence_until_stopped() {
    let fake = FakeSpeechToText::new("texte");
    let mut stream = fake.listen(&vocabulary()).await.unwrap();
    assert!(
        tokio::time::timeout(Duration::from_secs(60), stream.next())
            .await
            .is_err()
    );
    stream.stop().await;
    assert!(stream.is_finished());
}

#[tokio::test(start_paused = true)]
async fn a_stream_stopped_between_two_phrases_never_hears_the_next() {
    let fake = FakeSpeechToText::new("texte").listening(phrases(), PACE);
    let mut stream = fake.listen(&vocabulary()).await.unwrap();
    assert_eq!(
        stream.next().await,
        Some(Ok(Transcript::new("Élowen entre.")))
    );
    stream.stop().await;
    assert!(stream.is_finished());
    assert_eq!(stream.next().await, None);
}

#[tokio::test]
async fn its_phrases_come_with_no_pace_when_the_interval_is_zero() {
    let fake = FakeSpeechToText::new("texte").listening(phrases(), Duration::ZERO);
    let mut stream = fake.listen(&vocabulary()).await.unwrap();
    for phrase in phrases() {
        assert_eq!(stream.next().await, Some(Ok(Transcript::new(phrase))));
    }
    stream.stop().await;
}

#[tokio::test]
async fn a_stream_is_journalled_with_the_vocabulary_unchanged_and_no_audio() {
    let fake = FakeSpeechToText::new("texte").listening(phrases(), PACE);
    let vocabulary = vocabulary();
    let mut stream = fake.listen(&vocabulary).await.unwrap();
    let journal = fake.journal().list();
    assert_eq!(journal.len(), 1);
    assert_eq!(journal[0].sequence, 1);
    assert_eq!(journal[0].audio_bytes, 0);
    assert_eq!(journal[0].vocabulary, vocabulary);
    stream.stop().await;
}

#[tokio::test]
async fn a_dropped_stream_leaves_the_fake_usable() {
    let fake = FakeSpeechToText::new("texte").listening(phrases(), PACE);
    drop(fake.listen(&vocabulary()).await.unwrap());
    let mut again = fake.listen(&vocabulary()).await.unwrap();
    again.stop().await;
    assert_eq!(fake.journal().list().len(), 2);
}
