//! A proposal made from a prompt, outside the engine: the prompt — typed,
//! dictated or the sheet's — sent as text alone to the `llm` port's fake, its
//! answer read whatever the provider and held to StatBlock@1, and the command
//! it becomes, versioned with source `ia`. The engine's side is
//! `tests/submit.rs`.

use std::collections::BTreeMap;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use ports_llm::fake::FakeLlm;
use ports_llm::{ApiType, Llm, Models, Provider, ProviderConfig, RequestKind, TextKind};
use ports_speech_to_text::fake::FakeSpeechToText;
use ports_speech_to_text::{
    Dictation, RequestJournal, SpeechToText, SpeechToTextError, Transcript, Vocabulary,
};
use propose_character::{
    Fresh, Proposal, Refusal, Row, prepare, propose, proposed, request, vocabulary,
};
use serde_json::{Value, json};

const MORLOX: &str = "e1000000-0000-4000-8000-000000000003";

/// The default dictation of the mock-up (mock/fixtures/EntityEditor.json): a
/// copy, the crate reading nothing outside its folder.
const PROMPT: &str = "Une ancienne sergente de la garde, balafrée, qui commande une bande de \
coupe-jarrets au gué. Elle a mis le feu au moulin pour le compte de quelqu’un de la Tour d’Ébène.";

/// The fixture's default proposal, in the editor's shape: `stats`, `ca`,
/// `pvMax`, `features` with a `text`.
fn sergente() -> Value {
    json!({
        "background": "Renvoyée de la garde pour avoir refusé de brûler un village.",
        "behavior": "Parle peu, d’une voix basse.",
        "stats": { "FOR": 14, "DEX": 16, "CON": 14, "INT": 14, "SAG": 11, "CHA": 15 },
        "ca": 15,
        "pvMax": 65,
        "features": [
            { "name": "Attaques multiples", "text": "Trois attaques au corps à corps." },
            { "name": "Parade", "text": "Réaction : +2 à la CA." }
        ],
        "changes": [{ "field": "Background", "old": "", "new": "…" }]
    })
}

fn row(fields: Value) -> Row {
    serde_json::from_value(fields).expect("a row")
}

/// An NPC as a save leaves it: version 1, from the DM.
fn npc() -> Row {
    row(json!({
        "entityId": MORLOX,
        "entityCampaignId": "c1000000-0000-4000-8000-000000000001",
        "type": "pnj",
        "name": "Morlox",
        "alias": ["le gobelin"],
        "entityCreatedAt": "2026-10-01T10:00:00Z",
        "characterEntityId": MORLOX,
        "initialPrompt": "Un gobelin marchand.",
        "tokenColor": "accent",
        "spells": [],
        "actions": [],
        "entityVersionId": "a1000000-0000-4000-8000-000000000001",
        "versionNumber": 1,
        "source": "dm",
        "snapshot": {
            "type": "pnj", "name": "Morlox", "aliases": ["le gobelin"],
            "initialPrompt": "Un gobelin marchand.", "tokenColor": "accent",
            "spells": [], "actions": []
        },
        "diff": [],
        "restoredFromVersionId": "a1000000-0000-4000-8000-000000000000",
    }))
}

fn cloud() -> ProviderConfig {
    ProviderConfig::Api {
        url: "https://api.anthropic.com/v1".into(),
        api_type: Some(ApiType::Cloud),
        key: None,
        models: Models {
            generation: "claude-sonnet-5-5".into(),
            summary: "claude-haiku-4-5-20251001".into(),
        },
    }
}

fn home() -> ProviderConfig {
    ProviderConfig::Api {
        url: "http://192.168.1.40:4096".into(),
        api_type: Some(ApiType::Opencode),
        key: None,
        models: Models {
            generation: "qwen2.5-32b-instruct".into(),
            summary: "qwen2.5-14b-instruct".into(),
        },
    }
}

fn answering(proposal: Value) -> FakeLlm {
    FakeLlm::new().with_character("", proposal)
}

fn deaf() -> FakeSpeechToText {
    FakeSpeechToText::new("")
}

#[tokio::test]
async fn a_typed_prompt_is_sent_as_text_and_the_answer_fills_the_sheet() {
    let llm = answering(sergente());
    let (prompt, proposal) = propose(
        &llm,
        &cloud(),
        &deaf(),
        &row(json!({ "prompt": PROMPT })),
        &npc(),
    )
    .await
    .expect("a proposal");
    assert_eq!(prompt, PROMPT);
    assert_eq!(
        proposal,
        Proposal {
            background: "Renvoyée de la garde pour avoir refusé de brûler un village.".into(),
            behavior: "Parle peu, d’une voix basse.".into(),
            statblock: row(json!({
                "strength": 14, "dexterity": 16, "constitution": 14,
                "intelligence": 14, "wisdom": 11, "charisma": 15,
                "armorClass": 15, "hpMax": 65,
                "actions": [
                    { "name": "Attaques multiples", "description": "Trois attaques au corps à corps." },
                    { "name": "Parade", "description": "Réaction : +2 à la CA." }
                ]
            })),
        }
    );
    let sent = llm.journal().list();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].kind, RequestKind::Prompt);
    assert!(sent[0].text.contains(PROMPT), "{}", sent[0].text);
    assert!(sent[0].text.contains("« Morlox »"), "{}", sent[0].text);
}

#[tokio::test]
async fn the_cloud_api_and_the_opencode_endpoint_fill_the_same_fields() {
    let llm = answering(sergente());
    let wire = row(json!({ "prompt": PROMPT }));
    let mut proposals = Vec::new();
    for provider in [cloud(), home()] {
        let (_, proposal) = propose(&llm, &provider, &deaf(), &wire, &npc())
            .await
            .expect("a proposal");
        proposals.push(proposal);
    }
    assert_eq!(proposals[0], proposals[1]);
    let reached: Vec<Provider> = llm.journal().list().iter().map(|r| r.provider).collect();
    assert_eq!(reached, [Provider::Cloud, Provider::OpencodeServer]);
}

#[tokio::test]
async fn a_dictation_is_transcribed_locally_and_only_its_transcript_is_sent() {
    let llm = answering(sergente());
    let speech = FakeSpeechToText::new(PROMPT);
    let audio = STANDARD.encode([0x52_u8, 0x49, 0x46, 0x46, 1, 2, 3]);
    let (prompt, _) = propose(
        &llm,
        &cloud(),
        &speech,
        &row(json!({ "audio": audio, "prompt": "  " })),
        &npc(),
    )
    .await
    .expect("a proposal");
    assert_eq!(prompt, PROMPT);
    let heard = speech.journal().list();
    assert_eq!(heard.len(), 1);
    assert_eq!(heard[0].vocabulary, vocabulary(&npc()));
    let sent = llm.journal().list();
    assert_eq!(sent.len(), 1);
    assert!(sent[0].text.contains(PROMPT));
    assert!(!sent[0].text.contains(&audio), "the audio is never sent");
}

#[test]
fn a_dictation_listens_for_the_elements_name_and_aliases() {
    assert_eq!(
        vocabulary(&npc()),
        Vocabulary {
            campaign_names: vec!["Morlox".into()],
            aliases: vec!["le gobelin".into()],
            sound_names: Vec::new(),
        }
    );
    assert_eq!(vocabulary(&Row::new()), Vocabulary::default());
}

#[tokio::test]
async fn with_no_prompt_given_the_sheets_own_is_used_and_with_none_nothing_is_sent() {
    let llm = answering(sergente());
    for wire in [json!({}), json!({ "prompt": "", "audio": " " })] {
        let (prompt, _) = propose(&llm, &cloud(), &deaf(), &row(wire), &npc())
            .await
            .expect("a proposal");
        assert_eq!(prompt, "Un gobelin marchand.");
    }
    let mut silent = npc();
    silent.insert("initialPrompt".into(), json!("  "));
    let refused = propose(&llm, &cloud(), &deaf(), &Row::new(), &silent).await;
    assert_eq!(refused, Err(Refusal::NoPrompt));
    assert_eq!(llm.journal().list().len(), 2);
}

/// A port that hears nothing in any dictation, or cannot hear it.
struct Silent(Option<SpeechToTextError>);

impl SpeechToText for Silent {
    async fn transcribe(
        &self,
        _dictation: &Dictation,
        _vocabulary: &Vocabulary,
    ) -> Result<Transcript, SpeechToTextError> {
        self.0.clone().map_or_else(|| Ok(Transcript::new(" ")), Err)
    }

    fn journal(&self) -> &RequestJournal {
        unreachable!("the journal is not read")
    }
}

#[tokio::test]
async fn a_dictation_that_is_no_base64_unheard_or_silent_is_refused_and_nothing_is_sent() {
    let llm = answering(sergente());
    let refused = propose(
        &llm,
        &cloud(),
        &deaf(),
        &row(json!({ "audio": "pas du base64 !" })),
        &npc(),
    )
    .await;
    assert_eq!(refused, Err(Refusal::Undecodable));
    let audio = row(json!({ "audio": STANDARD.encode([1_u8, 2, 3]) }));
    let unheard = SpeechToTextError::Failed("pas de micro".into());
    let refused = propose(
        &llm,
        &cloud(),
        &Silent(Some(unheard.clone())),
        &audio,
        &npc(),
    )
    .await;
    assert_eq!(refused, Err(Refusal::Unheard(unheard)));
    // A dictation heard as silence is no prompt: the sheet's is not taken.
    let refused = propose(&llm, &cloud(), &Silent(None), &audio, &npc()).await;
    assert_eq!(refused, Err(Refusal::NoPrompt));
    assert!(llm.journal().list().is_empty());
}

#[tokio::test]
async fn a_place_or_an_unknown_element_is_refused_and_nothing_is_sent() {
    let llm = answering(sergente());
    let wire = row(json!({ "prompt": PROMPT }));
    let mut place = npc();
    place.insert("type".into(), json!("lieu"));
    let refused = propose(&llm, &cloud(), &deaf(), &wire, &place).await;
    assert_eq!(refused, Err(Refusal::NotACharacter));
    let refused = propose(&llm, &cloud(), &deaf(), &wire, &Row::new()).await;
    assert_eq!(refused, Err(Refusal::NoElement));
    assert!(llm.journal().list().is_empty());
}

#[tokio::test]
async fn a_prompt_holding_u0000_is_refused_before_it_is_sent() {
    let llm = answering(sergente());
    let refused = propose(
        &llm,
        &cloud(),
        &deaf(),
        &row(json!({ "prompt": "un\0gobelin" })),
        &npc(),
    )
    .await;
    assert_eq!(refused, Err(Refusal::Unwritable("initialPrompt")));
    assert!(llm.journal().list().is_empty());
}

#[tokio::test]
async fn a_provider_that_cannot_be_reached_is_told() {
    let unknown = ProviderConfig::Api {
        url: "https://inconnu.example.org".into(),
        api_type: None,
        key: None,
        models: Models {
            generation: "g".into(),
            summary: "s".into(),
        },
    };
    let refused = propose(
        &answering(sergente()),
        &unknown,
        &deaf(),
        &row(json!({ "prompt": PROMPT })),
        &npc(),
    )
    .await;
    assert_eq!(
        refused,
        Err(Refusal::Unanswered("type d'API non reconnu".into()))
    );
}

#[tokio::test]
async fn an_answer_that_is_no_proposal_fills_nothing_and_is_refused_naming_every_field() {
    // The fake handed no proposal answers its canned sentence.
    let refused = propose(
        &FakeLlm::new(),
        &cloud(),
        &deaf(),
        &row(json!({ "prompt": PROMPT })),
        &npc(),
    )
    .await;
    let every = vec![
        "background",
        "behavior",
        "strength",
        "dexterity",
        "constitution",
        "intelligence",
        "wisdom",
        "charisma",
        "armorClass",
        "hpMax",
        "actions",
    ];
    assert_eq!(refused, Err(Refusal::Incomplete(every.clone())));
    for answer in ["", "} {", "[1, 2]", "{pas du json}"] {
        assert_eq!(
            proposed(answer),
            Err(Refusal::Incomplete(every.clone())),
            "{answer}"
        );
    }
    assert_eq!(
        Refusal::Incomplete(vec!["background", "armorClass"]).to_string(),
        "proposal-fills-sheet-fields : la proposition ne remplit pas background, armorClass"
    );
}

/// `sergente()` with `field` set to `value` (`null` removes it).
fn with(field: &str, value: Value) -> String {
    let mut proposal = sergente();
    match value {
        Value::Null => {
            proposal.as_object_mut().expect("an object").remove(field);
        }
        value => proposal[field] = value,
    }
    proposal.to_string()
}

#[test]
fn an_incomplete_statblock_is_refused_naming_its_unsound_fields_in_order() {
    let cases = [
        (with("ca", Value::Null), vec!["armorClass"]),
        (with("pvMax", json!(0)), vec!["hpMax"]),
        (with("features", json!([])), vec!["actions"]),
        (
            with("features", json!([{ "name": "Parade", "text": "  " }])),
            vec!["actions"],
        ),
        (with("features", json!(["Parade"])), vec!["actions"]),
        (
            with(
                "stats",
                json!({ "FOR": 14, "DEX": "16", "CON": 31, "INT": 14, "SAG": 11 }),
            ),
            vec!["dexterity", "constitution", "charisma"],
        ),
        (with("background", json!("   ")), vec!["background"]),
        (with("behavior", Value::Null), vec!["behavior"]),
    ];
    for (answer, unsound) in cases {
        assert_eq!(
            proposed(&answer),
            Err(Refusal::Incomplete(unsound)),
            "{answer}"
        );
    }
}

#[test]
fn an_answer_is_read_through_prose_and_whatever_names_its_provider_gives() {
    let fenced = format!("Voici la fiche :\n```json\n{}\n```", sergente());
    let expected = proposed(&sergente().to_string()).expect("a proposal");
    assert_eq!(proposed(&fenced), Ok(expected.clone()));
    // The SRD's abbreviations, the aggregate's names, `actions` with a
    // `description`: the same sheet.
    let srd = json!({
        "background": " Renvoyée de la garde pour avoir refusé de brûler un village. ",
        "behavior": "Parle peu, d’une voix basse.",
        "stats": { "STR": 14, "DEX": 16, "CON": 14, "INT": 14, "WIS": 11 },
        "charisma": 15,
        "armorClass": 15,
        "hpMax": 65,
        "actions": [
            { "name": "Attaques multiples", "description": "Trois attaques au corps à corps." },
            { "name": "Parade", "description": "Réaction : +2 à la CA." }
        ]
    });
    assert_eq!(proposed(&srd.to_string()), Ok(expected));
}

#[test]
fn a_dex_modifier_or_spells_answered_are_never_taken() {
    let mut answer = sergente();
    answer["dexterityModifier"] = json!(3);
    answer["spells"] = json!([{ "name": "Bouclier", "description": "+5 à la CA." }]);
    let proposal = proposed(&answer.to_string()).expect("a proposal");
    assert!(!proposal.statblock.contains_key("dexterityModifier"));
    assert!(!proposal.statblock.contains_key("spells"));
}

#[test]
fn a_value_the_tables_cannot_hold_is_refused() {
    assert_eq!(
        proposed(&with("pvMax", json!(32_768))),
        Err(Refusal::Unwritable("hpMax"))
    );
    assert_eq!(
        proposed(&with("ca", json!(40_000))),
        Err(Refusal::Unwritable("armorClass"))
    );
    assert!(proposed(&with("pvMax", json!(32_767))).is_ok());
    for (field, value) in [
        ("background", json!("feu\0")),
        ("behavior", json!("\0")),
        ("features", json!([{ "name": "Parade", "text": "a\0" }])),
    ] {
        assert_eq!(
            proposed(&with(field, value)),
            Err(Refusal::Unwritable("background, behavior ou actions")),
            "{field}"
        );
    }
}

#[test]
fn the_request_is_a_prompt_naming_the_character_the_shape_and_the_prompt() {
    let npc = request("pnj", "Morlox", PROMPT);
    assert_eq!(npc.kind, TextKind::Prompt);
    assert!(
        npc.text.starts_with("Propose pour le PNJ « Morlox »"),
        "{}",
        npc.text
    );
    assert!(npc.text.ends_with(&format!("Prompt initial : {PROMPT}")));
    assert!(npc.text.contains("\"stats\":{\"FOR\":10"), "{}", npc.text);
    let pc = request("pj", "Elowen", "une rôdeuse");
    assert!(
        pc.text.starts_with("Propose pour le PJ « Elowen »"),
        "{}",
        pc.text
    );
}

#[test]
fn the_command_writes_the_prompt_the_proposal_and_the_next_version_from_the_ia() {
    let proposal = proposed(&sergente().to_string()).expect("a proposal");
    let fresh = Fresh {
        version: "a1000000-0000-4000-8000-000000000002",
        now: "2026-10-24T12:00:00Z",
    };
    let payload = prepare(&npc(), PROMPT, &proposal, fresh);
    let keys: Vec<&str> = payload.keys().map(String::as_str).collect();
    assert_eq!(
        keys,
        [
            "actions",
            "armorClass",
            "background",
            "behavior",
            "charisma",
            "constitution",
            "dexterity",
            "diff",
            "entityVersionCreatedAt",
            "entityVersionEntityId",
            "entityVersionId",
            "hpMax",
            "initialPrompt",
            "intelligence",
            "restoredFromVersionId",
            "snapshot",
            "source",
            "strength",
            "updatedAt",
            "versionNumber",
            "wisdom",
        ]
    );
    assert_eq!(payload["initialPrompt"], json!(PROMPT));
    assert_eq!(payload["source"], json!("ia"));
    assert_eq!(payload["versionNumber"], json!(2));
    assert_eq!(payload["entityVersionId"], json!(fresh.version));
    assert_eq!(payload["entityVersionEntityId"], json!(MORLOX));
    assert_eq!(payload["entityVersionCreatedAt"], json!(fresh.now));
    assert_eq!(payload["updatedAt"], json!(fresh.now));
    assert_eq!(payload["restoredFromVersionId"], Value::Null);
    assert_eq!(payload["snapshot"]["hpMax"], json!(65));
    assert_eq!(payload["snapshot"]["name"], json!("Morlox"));
    assert_eq!(payload["snapshot"]["aliases"], json!(["le gobelin"]));
    let changed: Vec<&str> = payload["diff"]
        .as_array()
        .expect("a diff")
        .iter()
        .map(|change| change["field"].as_str().expect("a field"))
        .collect();
    assert_eq!(
        changed,
        [
            "actions",
            "armorClass",
            "background",
            "behavior",
            "charisma",
            "constitution",
            "dexterity",
            "hpMax",
            "initialPrompt",
            "intelligence",
            "strength",
            "wisdom",
        ]
    );
    let prompt = &payload["diff"][8];
    assert_eq!(prompt["old"], json!("Un gobelin marchand."));
    assert_eq!(prompt["new"], json!(PROMPT));
}

#[test]
fn an_element_with_no_version_yet_takes_the_first() {
    let proposal = proposed(&sergente().to_string()).expect("a proposal");
    let mut base: BTreeMap<String, Value> = npc();
    for field in ["versionNumber", "snapshot"] {
        base.remove(field);
    }
    let fresh = Fresh {
        version: "v",
        now: "2026-10-24T12:00:00Z",
    };
    let payload = prepare(&base, PROMPT, &proposal, fresh);
    assert_eq!(payload["versionNumber"], json!(1));
    // Every field the snapshot holds is a change from nothing.
    assert_eq!(
        payload["diff"].as_array().map(Vec::len),
        payload["snapshot"].as_object().map(|s| s.len())
    );
}

#[test]
fn braces_in_the_prose_around_the_answer_are_skipped() {
    let expected = proposed(&sergente().to_string()).expect("a proposal");
    let after = format!(
        "Voici la fiche : {}\nRemplace {{nom}} si besoin.",
        sergente()
    );
    assert_eq!(proposed(&after), Ok(expected.clone()));
    let before = format!("Pour {{nom}} : {} et voilà.", sergente());
    assert_eq!(proposed(&before), Ok(expected));
}

#[test]
fn an_integer_written_with_a_fraction_of_zero_is_taken_as_that_integer() {
    let mut answer = sergente();
    answer["ca"] = json!(15.0);
    answer["pvMax"] = json!(65.0);
    answer["stats"]["FOR"] = json!(14.0);
    let proposal = proposed(&answer.to_string()).expect("a proposal");
    assert_eq!(proposal.statblock["armorClass"], json!(15));
    assert!(proposal.statblock["armorClass"].is_i64());
    assert!(proposal.statblock["hpMax"].is_i64());
    assert!(proposal.statblock["strength"].is_i64());
    answer["stats"]["DEX"] = json!(16.5);
    assert_eq!(
        proposed(&answer.to_string()),
        Err(Refusal::Incomplete(vec!["dexterity"]))
    );
}

#[tokio::test]
async fn the_prompt_is_kept_without_the_white_space_around_it() {
    let llm = answering(sergente());
    let typed = format!("  {PROMPT} \n");
    let (prompt, _) = propose(
        &llm,
        &cloud(),
        &deaf(),
        &row(json!({ "prompt": typed })),
        &npc(),
    )
    .await
    .expect("a proposal");
    assert_eq!(prompt, PROMPT);
    let mut padded = npc();
    padded.insert("initialPrompt".into(), json!(" Un gobelin marchand. "));
    let (prompt, _) = propose(&llm, &cloud(), &deaf(), &Row::new(), &padded)
        .await
        .expect("a proposal");
    assert_eq!(prompt, "Un gobelin marchand.");
}

#[test]
fn a_row_numbering_its_version_without_its_snapshot_still_takes_the_next_number() {
    let proposal = proposed(&sergente().to_string()).expect("a proposal");
    let mut base = npc();
    base.remove("snapshot");
    let fresh = Fresh {
        version: "v",
        now: "2026-10-24T12:00:00Z",
    };
    let payload = prepare(&base, PROMPT, &proposal, fresh);
    assert_eq!(payload["versionNumber"], json!(2));
}
