//! `analyse` and `Analysis::commands`: the DM's notes — pasted, or dictated
//! and transcribed — analysed by the llm port's fake into proposals, the
//! elements the campaign knows recognised (AliasMatch@1), and the commands
//! they are submitted as. Nothing here names an element to create.

use std::collections::BTreeMap;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use extract_from_notes::{Analysis, Known, Proposal, Refusal, TYPES, analyse, names};
use ports_llm::fake::FakeLlm;
use ports_llm::{ApiType, Llm, Models, ProviderConfig, RequestKind};
use ports_speech_to_text::fake::FakeSpeechToText;
use ports_speech_to_text::{
    Dictation, RequestJournal, SpeechToText, SpeechToTextError, Transcript, Vocabulary,
};
use serde_json::{Value, json};

const VALOMBRE: &str = "c1000000-0000-4000-8000-000000000001";
const GASPARD: &str = "e1000000-0000-4000-8000-000000000001";
const TOUR: &str = "e1000000-0000-4000-8000-000000000002";
const NOW: &str = "2026-10-20T10:00:00.000Z";

/// The draft of notes the mock-up pastes, as mock/fixtures/NoteImport.json
/// holds it: a copy, the crate reading nothing outside its folder.
const DRAFT: &str = "Valombre — notes en vrac après la séance 2.\n\
Le moulin a brûlé dans la nuit, le meunier a disparu. Gaspard (le tavernier du Sanglier) dit avoir vu des cavaliers en cape grise.\n\
Mère Ombeline, l’herboriste aveugle à l’orée du bois de Corbière, a soigné les apprentis. Elle sent la fumée sur les gens.\n\
Aerin, le frère d’Elowen, aurait été vu avec ces cavaliers.\n\
La Tour d’Ébène : ruine au nord du mont Pourpre, son sceau est un œil fermé.\n\
À creuser : le péage illégal au gué, 5 po par tête.";

fn draft() -> String {
    DRAFT.to_owned()
}

/// The cloud API, which the fake recognises by its URL alone.
fn cloud() -> ProviderConfig {
    ProviderConfig::Api {
        url: "https://api.anthropic.com/v1".to_owned(),
        api_type: Some(ApiType::Cloud),
        key: None,
        models: Models {
            generation: "claude-sonnet-5-5".to_owned(),
            summary: "claude-haiku-4-5-20251001".to_owned(),
        },
    }
}

/// Valombre's elements: Gaspard, known by an alias, and the tower, by its
/// name written otherwise.
fn valombre() -> Vec<Known> {
    vec![
        Known {
            id: GASPARD.to_owned(),
            name: "Gaspard Grosmollet".to_owned(),
            aliases: vec!["le tavernier".to_owned(), "  GASPARD ".to_owned()],
        },
        Known {
            id: TOUR.to_owned(),
            name: "la tour d’ebene".to_owned(),
            aliases: Vec::new(),
        },
    ]
}

fn wire(payload: Value) -> BTreeMap<String, Value> {
    let Value::Object(payload) = payload else {
        panic!("a payload is an object");
    };
    payload.into_iter().collect()
}

/// A dictation the fake transcribes as `heard`, and the analysis of it.
async fn dictated(
    heard: &str,
    payload: Value,
) -> (Result<Analysis, Refusal>, FakeSpeechToText, FakeLlm) {
    let speech = FakeSpeechToText::new(heard);
    let llm = FakeLlm::new();
    let analysis = analyse(&llm, &cloud(), &speech, &wire(payload), &valombre()).await;
    (analysis, speech, llm)
}

async fn pasted(payload: Value) -> (Result<Analysis, Refusal>, FakeLlm) {
    let (analysis, speech, llm) = dictated("", payload).await;
    assert!(speech.journal().list().is_empty(), "nothing was dictated");
    (analysis, llm)
}

fn named<'a>(analysis: &'a Analysis, name: &str) -> &'a Proposal {
    analysis
        .proposals
        .iter()
        .find(|proposal| proposal.name == name)
        .unwrap_or_else(|| panic!("{name} is not proposed: {:?}", analysis.proposals))
}

#[test]
fn it_names_ai_extract_from_notes_with_or_without_its_version() {
    assert!(names("ai.extractFromNotes"));
    assert!(names("ai.extractFromNotes@1"));
    assert!(!names("campaign.createEntitiesFromNote"));
}

#[tokio::test]
async fn pasted_notes_are_kept_whole_and_proposed_with_type_name_and_excerpt() {
    let (analysis, llm) = pasted(json!({ "campaign": VALOMBRE, "text": draft() })).await;
    let analysis = analysis.expect("analysed");
    assert_eq!(analysis.campaign, VALOMBRE);
    assert_eq!(analysis.text, draft());
    assert_eq!(analysis.failure, None);
    assert!(analysis.proposals.len() >= 2, "{:?}", analysis.proposals);
    for proposal in &analysis.proposals {
        assert!(TYPES.contains(&proposal.kind), "{proposal:?}");
        assert!(!proposal.name.is_empty(), "{proposal:?}");
        assert!(draft().contains(&proposal.excerpt), "{proposal:?}");
    }
    let ombeline = named(&analysis, "Mère Ombeline");
    assert_eq!(ombeline.kind, "pnj");
    assert!(
        ombeline
            .excerpt
            .starts_with("Mère Ombeline, l’herboriste aveugle")
    );
    // The notes, and nothing else, were sent: once, as a Notes request.
    let sent = llm.journal().list();
    assert_eq!(sent.len(), 1, "{sent:?}");
    assert_eq!(sent[0].kind, RequestKind::Notes);
    assert_eq!(sent[0].text, draft());
}

#[tokio::test]
async fn an_element_known_by_its_name_or_an_alias_is_proposed_unchecked() {
    let (analysis, _) = pasted(json!({ "campaign": VALOMBRE, "text": draft() })).await;
    let analysis = analysis.expect("analysed");
    // « Gaspard » is an alias of Gaspard Grosmollet, written otherwise.
    let gaspard = named(&analysis, "Gaspard");
    assert_eq!(gaspard.existing.as_deref(), Some(GASPARD));
    assert!(!gaspard.checked());
    // « La Tour d’Ébène » is the tower's name, its case and accents aside.
    let tour = named(&analysis, "La Tour d’Ébène");
    assert_eq!(tour.existing.as_deref(), Some(TOUR));
    assert!(!tour.checked());
}

#[tokio::test]
async fn an_element_the_campaign_does_not_know_is_proposed_checked() {
    let (analysis, _) = pasted(json!({ "campaign": VALOMBRE, "text": draft() })).await;
    let analysis = analysis.expect("analysed");
    for name in ["Mère Ombeline", "bois de Corbière", "Aerin"] {
        let proposal = named(&analysis, name);
        assert_eq!(proposal.existing, None, "{name}");
        assert!(proposal.checked(), "{name}");
    }
}

#[tokio::test]
async fn a_dictation_is_transcribed_locally_and_only_its_text_goes_further() {
    let audio = vec![7_u8; 32];
    let (analysis, speech, llm) = dictated(
        &draft(),
        json!({ "campaign": VALOMBRE, "dictation": STANDARD.encode(&audio) }),
    )
    .await;
    let analysis = analysis.expect("analysed");
    assert_eq!(analysis.text, draft());
    assert!(analysis.proposals.len() >= 2);
    // The port heard the audio, listening for the campaign's names and aliases.
    let heard = speech.journal().list();
    assert_eq!(heard.len(), 1, "{heard:?}");
    assert_eq!(heard[0].audio_bytes, audio.len());
    assert_eq!(
        heard[0].vocabulary,
        Vocabulary {
            campaign_names: vec!["Gaspard Grosmollet".into(), "la tour d’ebene".into()],
            aliases: vec!["le tavernier".into(), "  GASPARD ".into()],
            sound_names: Vec::new(),
        }
    );
    // The llm port was sent the transcript.
    let sent = llm.journal().list();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].text, draft());
}

#[tokio::test]
async fn a_typed_text_is_analysed_and_a_dictation_beside_it_is_not_heard() {
    let (analysis, speech, _) = dictated(
        "autre chose",
        json!({ "campaign": VALOMBRE, "text": draft(), "dictation": STANDARD.encode([1_u8; 4]) }),
    )
    .await;
    assert_eq!(analysis.expect("analysed").text, draft());
    assert!(speech.journal().list().is_empty());
}

#[tokio::test]
async fn notes_that_name_no_campaign_or_hold_no_word_are_refused() {
    for payload in [
        json!({ "text": draft() }),
        json!({ "campaign": "cendres-valombre", "text": draft() }),
    ] {
        assert_eq!(pasted(payload).await.0, Err(Refusal::NoCampaign));
    }
    for payload in [
        json!({ "campaign": VALOMBRE }),
        json!({ "campaign": VALOMBRE, "text": "  \n " }),
        json!({ "campaign": VALOMBRE, "text": "", "dictation": "" }),
    ] {
        let (analysis, llm) = pasted(payload.clone()).await;
        assert_eq!(analysis, Err(Refusal::NoNotes), "{payload}");
        assert!(llm.journal().list().is_empty(), "nothing is sent");
    }
}

#[tokio::test]
async fn a_dictation_that_is_no_base64_or_says_nothing_is_refused() {
    let (analysis, speech, llm) = dictated(
        &draft(),
        json!({ "campaign": VALOMBRE, "dictation": "pas du base64 !" }),
    )
    .await;
    assert_eq!(analysis, Err(Refusal::Undecodable));
    assert!(speech.journal().list().is_empty());
    assert!(llm.journal().list().is_empty());
    // Heard, but blank: no notes.
    let (analysis, _, llm) = dictated(
        "   ",
        json!({ "campaign": VALOMBRE, "dictation": STANDARD.encode([1_u8; 4]) }),
    )
    .await;
    assert_eq!(analysis, Err(Refusal::NoNotes));
    assert!(llm.journal().list().is_empty());
}

/// A port whose engine cannot answer.
struct Deaf(RequestJournal);

impl SpeechToText for Deaf {
    async fn transcribe(
        &self,
        _: &Dictation,
        _: &Vocabulary,
    ) -> Result<Transcript, SpeechToTextError> {
        Err(SpeechToTextError::Failed(
            "le moteur ne répond pas".to_owned(),
        ))
    }

    fn journal(&self) -> &RequestJournal {
        &self.0
    }
}

#[tokio::test]
async fn a_dictation_the_port_cannot_transcribe_is_refused_and_nothing_is_sent() {
    let llm = FakeLlm::new();
    let payload = wire(json!({ "campaign": VALOMBRE, "dictation": STANDARD.encode([1_u8; 4]) }));
    let analysis = analyse(&llm, &cloud(), &Deaf(RequestJournal::new()), &payload, &[]).await;
    let refusal = analysis.expect_err("refused");
    assert_eq!(
        refusal,
        Refusal::Unheard(SpeechToTextError::Failed(
            "le moteur ne répond pas".to_owned()
        ))
    );
    assert!(refusal.to_string().contains("le moteur ne répond pas"));
    assert!(llm.journal().list().is_empty());
}

#[tokio::test]
async fn when_the_llm_port_fails_the_note_is_kept_unanalysed_and_the_dm_is_told() {
    let unreachable = ProviderConfig::Api {
        url: "https://inconnu.example.org".to_owned(),
        api_type: None,
        key: None,
        models: Models {
            generation: "g".to_owned(),
            summary: "s".to_owned(),
        },
    };
    let llm = FakeLlm::new();
    let payload = wire(json!({ "campaign": VALOMBRE, "text": draft() }));
    let speech = FakeSpeechToText::new("");
    let analysis = analyse(&llm, &unreachable, &speech, &payload, &[])
        .await
        .expect("the note is kept");
    assert!(analysis.proposals.is_empty());
    let failure = analysis.failure.clone().expect("the DM is told");
    assert!(failure.contains("la note est gardée"), "{failure}");
    let commands = analysis.commands(vec![VALOMBRE.replace('c', "a")], NOW);
    assert!(commands.proposals.is_empty());
    assert_eq!(commands.note.payload["rawText"], json!(draft()));
    assert_eq!(commands.note.payload["analyzedAt"], Value::Null);
}

#[tokio::test]
async fn the_note_takes_the_greatest_id_and_each_command_writes_its_row() {
    let (analysis, _) = pasted(json!({ "campaign": VALOMBRE, "text": draft() })).await;
    let analysis = analysis.expect("analysed");
    let count = analysis.proposals.len();
    // Fresh ids, in no order.
    let ids: Vec<String> = (0..=count)
        .rev()
        .map(|n| format!("{:08x}-0000-4000-8000-{:012x}", (n * 7919) % 101, n))
        .collect();
    let mut sorted = ids.clone();
    sorted.sort();
    let commands = analysis.commands(ids, NOW);
    let note = sorted.last().expect("an id").clone();
    assert_eq!(commands.note.target_id, note);
    assert_eq!(
        commands.note.payload,
        wire(json!({
            "importedNoteId": note,
            "campaignId": VALOMBRE,
            "rawText": draft(),
            "createdAt": NOW,
            "analyzedAt": NOW,
        }))
    );
    assert_eq!(commands.proposals.len(), count);
    for (command, proposal) in commands.proposals.iter().zip(&analysis.proposals) {
        assert!(
            command.target_id < note,
            "the note sorts after its proposals"
        );
        assert_eq!(
            command.payload,
            wire(json!({
                "noteProposalId": command.target_id,
                "noteId": note,
                "type": proposal.kind,
                "name": proposal.name,
                "excerpt": proposal.excerpt,
                "existingEntityId": proposal.existing,
                "checked": proposal.existing.is_none(),
            }))
        );
    }
}
