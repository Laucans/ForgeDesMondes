//! A brief read and settled before anything is queued: the answer of the
//! `llm` port read into scenes ([`read_answer`]), its elements settled
//! against what the campaign holds ([`analysed`]), an NPC's notes completed
//! ([`noted`]), the commands it is submitted as ([`Analysis::commands`]),
//! and what [`analyse`] refuses — the `llm` port's fake standing for the
//! provider.

use std::collections::BTreeMap;

use analyze_brief::{
    Analysis, Brief, Holdings, Known, Media, Named, Planned, Read, Refusal, Scene, ToCreate,
    Update, analyse, analysed, names, normalized, noted, read_answer, timestamp, vocabulary,
};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use ports_llm::fake::FakeLlm;
use ports_llm::{ApiType, Llm, Models, ProviderConfig, RequestKind, TextKind};
use ports_speech_to_text::fake::FakeSpeechToText;
use serde_json::{Value, json};

const TAVERNE: &str = "e1000000-0000-4000-8000-000000000002";
const MORLOX: &str = "e1000000-0000-4000-8000-000000000003";
const VESNA: &str = "e1000000-0000-4000-8000-000000000004";
const CARTE: &str = "a1000000-0000-4000-8000-000000000001";

type Row = BTreeMap<String, Value>;

fn wire(value: Value) -> Row {
    serde_json::from_value(value).expect("an object")
}

fn known(id: &str, kind: &str, name: &str, aliases: &[&str]) -> Known {
    Known {
        id: id.to_owned(),
        kind: kind.to_owned(),
        name: name.to_owned(),
        aliases: aliases.iter().map(|alias| (*alias).to_owned()).collect(),
    }
}

fn named(kind: &str, name: &str, excerpt: &str) -> Named {
    Named {
        kind: kind.to_owned(),
        name: name.to_owned(),
        excerpt: excerpt.to_owned(),
    }
}

fn scene(title: &str, notes: &str, elements: Vec<Named>) -> Scene {
    Scene {
        title: title.to_owned(),
        notes: notes.to_owned(),
        elements,
    }
}

/// Valombre: the tavern, known by an alias too, Morlox the NPC, Vesna a PC,
/// a battlemap, and the bridge waiting on the to-create list.
fn valombre() -> Holdings {
    Holdings {
        known: vec![
            known(TAVERNE, "lieu", "La Taverne du Sanglier", &["la taverne"]),
            known(MORLOX, "pnj", "Morlox", &["le gobelin"]),
            known(VESNA, "pj", "Vesna", &[]),
        ],
        media: vec![Media {
            id: CARTE.to_owned(),
            title: "La Carte du Gué".to_owned(),
        }],
        waiting: vec![("lieu".to_owned(), "Le Pont des Pendus".to_owned())],
    }
}

fn cloud() -> ProviderConfig {
    ProviderConfig::Api {
        url: "https://api.anthropic.com".to_owned(),
        api_type: Some(ApiType::Cloud),
        key: None,
        models: Models {
            generation: "claude-sonnet-5-5".to_owned(),
            summary: "claude-haiku-4-5-20251001".to_owned(),
        },
    }
}

#[test]
fn the_answer_is_read_through_prose_or_a_code_fence_around_it() {
    let answer = "Voici :\n```json\n{\"scenes\":[{\"title\":\" La taverne \",\"notes\":\"On boit.\",\"elements\":[{\"type\":\"pnj\",\"name\":\" Morlox \",\"excerpt\":\" Morlox boit \"}]}]}\n```";
    assert_eq!(
        read_answer(answer),
        Some(vec![scene(
            "La taverne",
            "On boit.",
            vec![named("pnj", "Morlox", "Morlox boit")]
        )])
    );
}

#[test]
fn an_answer_that_is_no_list_of_scenes_is_none() {
    for answer in [
        "Réponse simulée",
        "} {",
        "{\"scenes\": 3}",
        "{\"proposals\": []}",
        "{pas du json}",
    ] {
        assert_eq!(read_answer(answer), None, "{answer}");
    }
}

#[test]
fn a_scene_that_is_no_object_and_an_element_with_no_name_are_dropped() {
    let answer = r#"{"scenes":[
        "une scène",
        {"elements":[{"type":"pnj","name":"  "},{"type":"lieu"},{"name":"Gaspard"},7]},
        {"title":"Sans éléments","elements":"aucun"}
    ]}"#;
    assert_eq!(
        read_answer(answer),
        Some(vec![
            scene("", "", vec![named("", "Gaspard", "")]),
            scene("Sans éléments", "", Vec::new()),
        ])
    );
}

#[test]
fn an_element_the_campaign_holds_is_linked_once_and_an_npc_it_names_is_updated() {
    let scenes = [
        scene(
            "Retour à la taverne",
            "On reprend à la Taverne du Sanglier. Morlox débarque.",
            vec![
                // By an alias, in another case, with an accent it has not.
                named("lieu", "LA TAVÉRNE", "On reprend à la taverne."),
                named("pnj", "Morlox", "Morlox débarque trempé."),
                named("lieu", "La Taverne du Sanglier", "La taverne encore."),
                // By its alias, whatever type the answer gives it.
                named("evenement", "le Gobelin", "Le gobelin a vendu le plan."),
                // A PC is linked, and never updated.
                named("pj", "Vesna", "Vesna guette."),
            ],
        ),
        scene(
            "Au gué",
            "",
            vec![
                named("pnj", "Morlox", "Morlox débarque trempé."),
                named("pnj", "Morlox", "Morlox arrive en courant."),
                // An NPC named with no excerpt is linked, not updated.
                named("pnj", "morlox", ""),
            ],
        ),
    ];
    let analysis = analysed(&scenes, &valombre());
    assert_eq!(
        analysis.scenes,
        [
            Planned {
                title: "Retour à la taverne".to_owned(),
                notes: Some("On reprend à la Taverne du Sanglier. Morlox débarque.".to_owned()),
                links: vec![TAVERNE.to_owned(), MORLOX.to_owned(), VESNA.to_owned()],
                media: Vec::new(),
            },
            Planned {
                title: "Au gué".to_owned(),
                notes: None,
                links: vec![MORLOX.to_owned()],
                media: Vec::new(),
            },
        ]
    );
    assert!(analysis.entries.is_empty(), "{:?}", analysis.entries);
    assert_eq!(
        analysis.updates,
        [Update {
            entity: MORLOX.to_owned(),
            name: "Morlox".to_owned(),
            excerpts: vec![
                "Morlox débarque trempé.".to_owned(),
                "Le gobelin a vendu le plan.".to_owned(),
                "Morlox arrive en courant.".to_owned(),
            ],
        }]
    );
}

#[test]
fn an_unknown_npc_place_or_event_goes_on_the_list_once_with_its_type_and_excerpt() {
    let scenes = [
        scene(
            "La route",
            "Sur la route, frère Odric. Le moulin a brûlé.",
            vec![
                named("pnj", "Frère Odric", "un pèlerin, frère Odric, les arrête"),
                named("Événement", "L’incendie du moulin", ""),
                // A PC cannot be created from a brief, nor an object.
                named("pj", "Elowen", "Elowen arrive."),
                named("objet", "La clé", "une clé"),
            ],
        ),
        scene(
            "Le gué",
            "",
            vec![
                // The same type and name again: on the list once.
                named("pnj", "FRÈRE ODRIC", "Odric revient."),
                // The same name as another type: an entry of its own.
                named("lieu", "Frère Odric", "chez frère Odric"),
                // What the list awaits already is not added again.
                named("lieu", "le pont des pendus", "le Pont des Pendus"),
                // No excerpt and no notes: the name is the excerpt.
                named("pnj", "Vesna la Balafrée", ""),
            ],
        ),
    ];
    let analysis = analysed(&scenes, &valombre());
    let entry = |scene: usize, kind: &'static str, name: &str, excerpt: &str| ToCreate {
        scene,
        kind,
        name: name.to_owned(),
        excerpt: excerpt.to_owned(),
    };
    assert_eq!(
        analysis.entries,
        [
            entry(
                0,
                "pnj",
                "Frère Odric",
                "un pèlerin, frère Odric, les arrête"
            ),
            entry(
                0,
                "evenement",
                "L’incendie du moulin",
                "Sur la route, frère Odric. Le moulin a brûlé."
            ),
            entry(1, "lieu", "Frère Odric", "chez frère Odric"),
            entry(1, "pnj", "Vesna la Balafrée", "Vesna la Balafrée"),
        ]
    );
    assert!(analysis.updates.is_empty());
    assert!(analysis.scenes.iter().all(|scene| scene.links.is_empty()));
}

#[test]
fn a_media_of_the_campaign_is_linked_by_its_title_unless_an_element_answers_first() {
    let mut holdings = valombre();
    holdings.media.push(Media {
        id: "a1000000-0000-4000-8000-000000000002".to_owned(),
        title: "Morlox".to_owned(),
    });
    let scenes = [scene(
        "",
        "",
        vec![
            named("lieu", "la carte du gué", "la carte du gué"),
            named("lieu", "La Carte du Gue", "encore la carte"),
            named("pnj", "Morlox", ""),
        ],
    )];
    let analysis = analysed(&scenes, &holdings);
    assert_eq!(analysis.scenes[0].media, [CARTE]);
    assert_eq!(analysis.scenes[0].links, [MORLOX]);
    assert!(analysis.entries.is_empty());
}

#[test]
fn a_scene_with_nothing_is_dropped_and_one_with_no_title_numbered_by_its_rank() {
    let scenes = [
        scene("", "", Vec::new()),
        scene("", "Il pleut.", Vec::new()),
        scene("", "", vec![named("pnj", "Morlox", "")]),
        scene("Le gué", "", Vec::new()),
    ];
    let titles: Vec<String> = analysed(&scenes, &valombre())
        .scenes
        .into_iter()
        .map(|scene| scene.title)
        .collect();
    assert_eq!(titles, ["Scène 1", "Scène 2", "Le gué"]);
}

#[test]
fn a_blank_name_or_alias_recognises_nothing() {
    let holdings = Holdings {
        known: vec![known(MORLOX, "pnj", "Morlox", &[" "])],
        media: vec![Media {
            id: CARTE.to_owned(),
            title: " ".to_owned(),
        }],
        waiting: Vec::new(),
    };
    let analysis = analysed(&[scene("", "", vec![named("pnj", " ", "x")])], &holdings);
    assert!(analysis.scenes[0].links.is_empty());
    assert!(analysis.scenes[0].media.is_empty());
    assert_eq!(analysis.entries[0].name, " ");
}

#[test]
fn an_npc_s_notes_take_what_they_do_not_hold_each_on_a_line() {
    let said = |excerpts: &[&str]| -> Vec<String> {
        excerpts
            .iter()
            .map(|excerpt| (*excerpt).to_owned())
            .collect()
    };
    assert_eq!(
        noted(
            Some("Doit 40 po à Vesna.\n"),
            &said(&[
                "Morlox débarque trempé.",
                " Doit 40 po à Vesna. ",
                "Il fuit."
            ])
        ),
        Some("Doit 40 po à Vesna.\nMorlox débarque trempé.\nIl fuit.".to_owned())
    );
    assert_eq!(
        noted(None, &said(&["Morlox débarque trempé."])),
        Some("Morlox débarque trempé.".to_owned())
    );
    assert_eq!(
        noted(Some("  "), &said(&["Il fuit.", " "])),
        Some("Il fuit.".to_owned())
    );
    assert_eq!(
        noted(Some("Il fuit. Il revient."), &said(&["Il fuit."])),
        None
    );
    assert_eq!(noted(Some("Notes."), &said(&[" "])), None);
    assert_eq!(noted(Some("Notes."), &[]), None);
}

fn brief() -> Brief {
    Brief {
        session: "51000000-0000-4000-8000-000000000004".to_owned(),
        campaign: "c1000000-0000-4000-8000-000000000001".to_owned(),
        transcript: "Le brief.".to_owned(),
        duration_seconds: 252,
    }
}

#[test]
fn the_brief_takes_the_least_id_then_the_scenes_in_order_then_media_then_entries() {
    let analysis = Analysis {
        scenes: vec![
            Planned {
                title: "Un".to_owned(),
                notes: Some("Il pleut.".to_owned()),
                links: vec![TAVERNE.to_owned()],
                media: vec![CARTE.to_owned()],
            },
            Planned {
                title: "Deux".to_owned(),
                notes: None,
                links: Vec::new(),
                media: Vec::new(),
            },
        ],
        entries: vec![ToCreate {
            scene: 1,
            kind: "lieu",
            name: "Le Pont".to_owned(),
            excerpt: "le Pont".to_owned(),
        }],
        updates: Vec::new(),
    };
    assert_eq!(analysis.rows(), 5);
    let ids: Vec<String> = ["5", "3", "1", "4", "2"]
        .iter()
        .map(|id| format!("id-{id}"))
        .collect();
    let now = "2026-10-24T09:30:00.000Z";
    let commands = analysis.commands(&brief(), ids, now);
    let brief = brief();
    assert_eq!(commands.brief.target_id, "id-1");
    assert_eq!(
        Value::Object(commands.brief.payload.into_iter().collect()),
        json!({
            "oralBriefId": "id-1",
            "oralBriefSessionId": brief.session,
            "transcript": "Le brief.",
            "durationSeconds": 252,
            "recordedAt": now,
            "analyzedAt": now,
        })
    );
    let then: Vec<(String, Value)> = commands
        .then
        .into_iter()
        .map(|c| (c.target_id, Value::Object(c.payload.into_iter().collect())))
        .collect();
    assert_eq!(
        then,
        [
            (
                "id-2".to_owned(),
                json!({ "sceneId": "id-2", "sceneSessionId": brief.session, "sceneTitle": "Un", "notes": "Il pleut.", "entityId": [TAVERNE] })
            ),
            (
                "id-3".to_owned(),
                json!({ "sceneId": "id-3", "sceneSessionId": brief.session, "sceneTitle": "Deux", "notes": null, "entityId": [] })
            ),
            (
                "id-4".to_owned(),
                json!({ "sceneMediaSceneId": "id-2", "sceneMediaMediaId": CARTE })
            ),
            (
                "id-5".to_owned(),
                json!({
                    "entityToCreateId": "id-5",
                    "entityToCreateCampaignId": brief.campaign,
                    "entityToCreateSessionId": brief.session,
                    "briefId": "id-1",
                    "entityToCreateSceneId": "id-3",
                    "type": "lieu",
                    "name": "Le Pont",
                    "excerpt": "le Pont",
                    "entityToCreateStatus": "a_creer",
                    "createdAt": now,
                })
            ),
        ]
    );
}

#[test]
#[should_panic(expected = "one id per row")]
fn commands_need_one_id_per_row() {
    let analysis = Analysis {
        scenes: Vec::new(),
        entries: Vec::new(),
        updates: Vec::new(),
    };
    let _ = analysis.commands(&brief(), Vec::new(), "now");
}

#[tokio::test]
async fn a_typed_brief_is_sent_as_text_and_cut_into_its_scenes() {
    let llm = FakeLlm::new();
    let speech = FakeSpeechToText::new("jamais entendu");
    let text = "On commence à la Taverne du Sanglier. Deuxième scène, le Pont des Pendus.";
    let read = analyse(
        &llm,
        &cloud(),
        &speech,
        &wire(json!({ "session": "s", "text": text, "durationSeconds": 42 })),
        &[],
    )
    .await
    .expect("analysed");
    assert_eq!(read.transcript, text);
    assert_eq!(read.duration_seconds, 42);
    assert_eq!(read.scenes.len(), 2, "{:?}", read.scenes);
    assert_eq!(read.scenes[0].elements[0].name, "La Taverne du Sanglier");
    let records = llm.journal().list();
    assert_eq!(records.len(), 1);
    assert_eq!(
        (records[0].kind, records[0].text.as_str()),
        (RequestKind::Brief, text)
    );
}

#[tokio::test]
async fn a_dictated_brief_is_transcribed_and_only_its_transcript_goes_further() {
    let llm = FakeLlm::new();
    let speech = FakeSpeechToText::new("Morlox débarque à la Taverne du Sanglier.");
    let audio = STANDARD.encode([1u8, 2, 3]);
    let campaign = [known(MORLOX, "pnj", "Morlox", &["le gobelin"])];
    let read = analyse(
        &llm,
        &cloud(),
        &speech,
        &wire(json!({ "session": "s", "text": "  ", "dictation": audio })),
        &campaign,
    )
    .await
    .expect("analysed");
    assert_eq!(read.transcript, "Morlox débarque à la Taverne du Sanglier.");
    assert_eq!(read.duration_seconds, 0);
    assert_eq!(
        llm.journal().list()[0].text,
        "Morlox débarque à la Taverne du Sanglier."
    );
}

#[tokio::test]
async fn a_brief_with_no_word_an_undecodable_dictation_or_a_bad_duration_is_refused() {
    let llm = FakeLlm::new();
    let speech = FakeSpeechToText::new("");
    for (payload, refusal) in [
        (json!({ "text": "  " }), Refusal::NoBrief),
        (json!({ "dictation": " " }), Refusal::NoBrief),
        (
            json!({ "dictation": STANDARD.encode([1u8]) }),
            Refusal::NoBrief,
        ),
        (
            json!({ "dictation": "pas du base64 !" }),
            Refusal::Undecodable,
        ),
        (
            json!({ "text": "Morlox.", "durationSeconds": -1 }),
            Refusal::NotADuration,
        ),
        (
            json!({ "text": "Morlox.", "durationSeconds": 2.5 }),
            Refusal::NotADuration,
        ),
        (
            json!({ "text": "Morlox.", "durationSeconds": 2_147_483_648_u64 }),
            Refusal::NotADuration,
        ),
    ] {
        let found = analyse(&llm, &cloud(), &speech, &wire(payload.clone()), &[]).await;
        assert_eq!(found, Err(refusal), "{payload}");
    }
    assert!(llm.journal().list().is_empty(), "nothing is sent");
    let longest = analyse(
        &llm,
        &cloud(),
        &speech,
        &wire(json!({ "text": "Morlox.", "durationSeconds": 2_147_483_647_u64 })),
        &[],
    )
    .await
    .expect("analysed");
    assert_eq!(longest.duration_seconds, 2_147_483_647);
}

#[tokio::test]
async fn a_failing_or_an_unreadable_llm_refuses_the_brief() {
    let speech = FakeSpeechToText::new("");
    let unknown = ProviderConfig::Api {
        url: "https://inconnu.example".to_owned(),
        api_type: None,
        key: None,
        models: Models {
            generation: "g".to_owned(),
            summary: "s".to_owned(),
        },
    };
    let refused = analyse(
        &FakeLlm::new(),
        &unknown,
        &speech,
        &wire(json!({ "text": "Morlox." })),
        &[],
    )
    .await;
    let Err(Refusal::Unanalysed(why)) = refused else {
        panic!("refused, not {refused:?}");
    };
    assert!(!why.is_empty());
    assert!(
        Refusal::Unanalysed(why)
            .to_string()
            .ends_with("rien n’est écrit")
    );
    /// An llm that answers prose.
    struct Prose;
    impl Llm for Prose {
        async fn test_connection(&self, _: &ProviderConfig) -> ports_llm::ConnectionReport {
            unreachable!("never tested")
        }
        async fn generate(
            &self,
            _: &ProviderConfig,
            request: &ports_llm::GenerateRequest,
        ) -> Result<ports_llm::GenerateAnswer, ports_llm::LlmError> {
            assert_eq!(request.kind, TextKind::Brief);
            Ok(ports_llm::GenerateAnswer {
                text: "Trois scènes, je crois.".to_owned(),
                model: "m".to_owned(),
            })
        }
        fn journal(&self) -> &ports_llm::RequestJournal {
            unreachable!("never listed")
        }
    }
    assert_eq!(
        analyse(
            &Prose,
            &cloud(),
            &speech,
            &wire(json!({ "text": "Morlox." })),
            &[]
        )
        .await,
        Err(Refusal::Unanalysed(
            "la réponse de l’IA n’est pas une liste de scènes".to_owned()
        ))
    );
}

#[test]
fn every_refusal_says_why() {
    for (refusal, says) in [
        (
            Refusal::NoSession,
            "aucune séance : `session` la nomme par son id",
        ),
        (Refusal::NoBrief, "aucun brief à analyser"),
        (
            Refusal::Undecodable,
            "la dictée n’est pas un son encodé en base64",
        ),
        (
            Refusal::NotADuration,
            "`durationSeconds` est un nombre entier de secondes, positif",
        ),
        (
            Refusal::Unanalysed("panne".to_owned()),
            "l’analyse du brief a échoué (panne) : rien n’est écrit",
        ),
    ] {
        assert_eq!(refusal.to_string(), says);
    }
    let unheard = Refusal::Unheard(ports_speech_to_text::SpeechToTextError::Failed(
        "bruit".to_owned(),
    ));
    assert!(
        unheard
            .to_string()
            .starts_with("la dictée n’a pas été transcrite : ")
    );
}

#[test]
fn a_dictation_listens_for_the_names_and_aliases_of_the_campaign() {
    let vocabulary = vocabulary(&valombre().known);
    assert_eq!(
        vocabulary.campaign_names,
        ["La Taverne du Sanglier", "Morlox", "Vesna"]
    );
    assert_eq!(vocabulary.aliases, ["la taverne", "le gobelin"]);
    assert!(vocabulary.sound_names.is_empty());
}

#[test]
fn names_is_this_data_capability_with_or_without_its_version() {
    assert!(names("ai.analyzeBrief"));
    assert!(names("ai.analyzeBrief@1"));
    assert!(!names("ai.extractFromNotes"));
    assert!(!names("ai.analyzeBriefs"));
}

#[test]
fn a_term_is_normalised_without_case_accent_or_white_space_around_it() {
    assert_eq!(
        normalized("  L’Incendie du Moulin "),
        "l’incendie du moulin"
    );
    assert_eq!(normalized("FRÈRE Odric"), "frere odric");
}

#[test]
fn a_time_is_written_rfc_3339_to_the_millisecond() {
    assert_eq!(timestamp(0), "1970-01-01T00:00:00.000Z");
    assert_eq!(timestamp(1_792_833_000_123), "2026-10-24T09:10:00.123Z");
    assert_eq!(timestamp(951_782_400_000), "2000-02-29T00:00:00.000Z");
    assert_eq!(timestamp(4_107_542_400_000), "2100-03-01T00:00:00.000Z");
    assert_eq!(timestamp(4_107_542_399_999), "2100-02-28T23:59:59.999Z");
    assert_eq!(timestamp(1_704_067_199_000), "2023-12-31T23:59:59.000Z");
}

#[test]
fn a_read_brief_is_its_transcript_its_duration_and_its_scenes() {
    let read = Read {
        transcript: "t".to_owned(),
        duration_seconds: 1,
        scenes: Vec::new(),
    };
    assert_eq!(read.clone(), read);
}
