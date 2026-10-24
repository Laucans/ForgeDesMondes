//! What the fake promises beyond the contract: it is deterministic, it sends
//! only text, and its journal is the one a Capability lists.

use ports_llm::fake::{FAKE_LATENCY_MS, FakeLlm};
use ports_llm::{
    ApiType, Binary, GenerateRequest, Llm, LlmError, Models, Provider, ProviderConfig, RequestKind,
    TextKind,
};
use serde_json::{Value, json};

const OPENCODE_URL: &str = "http://192.168.1.40:4096";

fn models() -> Models {
    Models {
        generation: "qwen2.5-32b-instruct".into(),
        summary: "qwen2.5-14b-instruct".into(),
    }
}

fn api(url: &str, api_type: Option<ApiType>) -> ProviderConfig {
    ProviderConfig::Api {
        url: url.into(),
        api_type,
        key: None,
        models: models(),
    }
}

fn home_server() -> FakeLlm {
    FakeLlm::new().with_endpoint(OPENCODE_URL, json!({"object": "list", "data": []}))
}

#[tokio::test]
async fn the_cloud_url_connects_with_a_latency_and_both_models() {
    let report = FakeLlm::new()
        .test_connection(&api("https://api.anthropic.com/v1", None))
        .await;
    assert!(report.ok);
    assert_eq!(
        report.provider.and_then(Provider::api_type),
        Some(ApiType::Cloud)
    );
    assert_eq!(report.latency_ms, FAKE_LATENCY_MS);
    assert!(
        report.message.contains("qwen2.5-32b-instruct"),
        "{}",
        report.message
    );
    assert!(
        report.message.contains("qwen2.5-14b-instruct"),
        "{}",
        report.message
    );
}

#[tokio::test]
async fn the_opencode_url_connects_once_its_endpoint_answers_the_list() {
    let report = home_server()
        .test_connection(&api(OPENCODE_URL, None))
        .await;
    assert!(report.ok);
    assert_eq!(report.provider, Some(Provider::OpencodeServer));
    assert!(
        report
            .message
            .contains("endpoint opencode du serveur maison"),
        "{}",
        report.message
    );
}

#[tokio::test]
async fn an_unrecognised_url_fails_with_the_concepts_message() {
    for url in [OPENCODE_URL, "https://inconnu.example.org/pas-une-api", ""] {
        let report = FakeLlm::new().test_connection(&api(url, None)).await;
        assert!(!report.ok, "{url:?}");
        assert!(
            report.message.contains("type d'API non reconnu"),
            "{}",
            report.message
        );
        assert_eq!(report.provider, None);
    }
}

#[tokio::test]
async fn each_binary_connects() {
    for (binary, provider, label) in [
        (
            Binary::Opencode,
            Provider::OpencodeBinary,
            "binaire opencode",
        ),
        (
            Binary::ClaudeCode,
            Provider::ClaudeCodeBinary,
            "binaire Claude Code",
        ),
    ] {
        let config = ProviderConfig::Binary {
            binary,
            models: models(),
        };
        let report = FakeLlm::new().test_connection(&config).await;
        assert!(report.ok);
        assert_eq!(report.provider, Some(provider));
        assert!(report.message.contains(label), "{}", report.message);
    }
}

#[tokio::test]
async fn the_same_request_gets_the_same_answer() {
    let fake = FakeLlm::new();
    let config = api("https://api.anthropic.com", None);
    let request = GenerateRequest::new(TextKind::Prompt, "Les notes");
    let first = fake.generate(&config, &request).await;
    let second = fake.generate(&config, &request).await;
    assert_eq!(first, second);
    let answer = first.unwrap();
    assert_eq!(answer.model, "qwen2.5-32b-instruct");
    assert!(answer.text.contains("9 caractères"), "{}", answer.text);
}

#[tokio::test]
async fn the_summary_model_answers_the_journal() {
    let answer = FakeLlm::new()
        .generate(
            &api("https://api.anthropic.com", None),
            &GenerateRequest::new(TextKind::Journal, "00:01 …"),
        )
        .await
        .unwrap();
    assert_eq!(answer.model, "qwen2.5-14b-instruct");
}

#[tokio::test]
async fn a_saved_type_spares_the_endpoint_for_generation_but_not_for_the_test() {
    let fake = FakeLlm::new();
    let config = api(OPENCODE_URL, Some(ApiType::Opencode));
    let request = GenerateRequest::new(TextKind::Prompt, "bonjour");
    assert!(fake.generate(&config, &request).await.is_ok());
    assert!(!fake.test_connection(&config).await.ok);
}

#[tokio::test]
async fn generation_through_an_unrecognised_url_fails_and_journals_nothing() {
    let fake = FakeLlm::new();
    let request = GenerateRequest::new(TextKind::Prompt, "bonjour");
    assert_eq!(
        fake.generate(&api(OPENCODE_URL, None), &request).await,
        Err(LlmError::UnrecognisedApiType)
    );
    assert!(fake.journal().list().is_empty());
}

#[tokio::test]
async fn the_journal_lists_the_requests_in_order_with_kind_and_text() {
    let fake = FakeLlm::new();
    let config = api("https://api.anthropic.com", None);
    fake.test_connection(&config).await;
    for (kind, text) in [
        (TextKind::Brief, "un brief"),
        (TextKind::Notes, "des notes"),
    ] {
        fake.generate(&config, &GenerateRequest::new(kind, text))
            .await
            .unwrap();
    }
    let listed: Vec<_> = fake
        .journal()
        .list()
        .into_iter()
        .map(|record| (record.sequence, record.kind, record.text))
        .collect();
    assert_eq!(
        listed,
        [
            (
                1,
                RequestKind::ConnectionTest,
                ports_llm::CONNECTION_TEST_PROMPT.to_owned()
            ),
            (2, RequestKind::Brief, "un brief".to_owned()),
            (3, RequestKind::Notes, "des notes".to_owned()),
        ]
    );
}

#[tokio::test]
async fn the_journal_a_capability_holds_is_the_one_the_port_writes() {
    let fake = FakeLlm::new();
    let listing = fake.journal().clone();
    let config = ProviderConfig::Binary {
        binary: Binary::ClaudeCode,
        models: models(),
    };
    fake.generate(&config, &GenerateRequest::new(TextKind::Prompt, "bonjour"))
        .await
        .unwrap();
    assert_eq!(listing.list().len(), 1);
    let json = serde_json::to_value(listing.list()).unwrap();
    assert_eq!(
        json,
        json!([{"sequence": 1, "kind": "prompt", "provider": "claude-code-binary", "text": "bonjour"}])
    );
}

const PROPOSAL_TYPES: [&str; 4] = ["pj", "pnj", "lieu", "evenement"];

/// The draft of the mock-up's notes import.
fn draft() -> String {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../mock/fixtures/NoteImport.json"
    );
    let fixture: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    fixture["draft"].as_str().unwrap().to_owned()
}

/// The proposals the fake answers to `text`, as `(type, name)` pairs.
async fn proposals_of(text: &str) -> Vec<(String, String)> {
    let answer = notes_answer(text).await;
    answer["proposals"]
        .as_array()
        .unwrap()
        .iter()
        .map(|proposal| {
            (
                proposal["type"].as_str().unwrap().to_owned(),
                proposal["name"].as_str().unwrap().to_owned(),
            )
        })
        .collect()
}

async fn notes_answer(text: &str) -> Value {
    let answer = FakeLlm::new()
        .generate(
            &api("https://api.anthropic.com", None),
            &GenerateRequest::new(TextKind::Notes, text),
        )
        .await
        .unwrap();
    assert_eq!(answer.model, "qwen2.5-32b-instruct");
    serde_json::from_str(&answer.text).unwrap_or_else(|error| panic!("{error}: {}", answer.text))
}

#[tokio::test]
async fn the_draft_of_the_mock_up_yields_typed_proposals_taken_from_its_text() {
    let text = draft();
    let answer = notes_answer(&text).await;
    let proposals = answer["proposals"].as_array().unwrap();
    assert!(proposals.len() >= 2, "{answer}");
    for proposal in proposals {
        assert_eq!(
            proposal.as_object().unwrap().len(),
            3,
            "type, name and excerpt, nothing else: {proposal}"
        );
        let kind = proposal["type"].as_str().unwrap();
        assert!(PROPOSAL_TYPES.contains(&kind), "{proposal}");
        let name = proposal["name"].as_str().unwrap();
        assert!(!name.is_empty(), "{proposal}");
        assert!(text.contains(name), "the name is in the notes: {proposal}");
        let excerpt = proposal["excerpt"].as_str().unwrap();
        assert!(!excerpt.is_empty(), "{proposal}");
        assert!(
            text.contains(excerpt),
            "the excerpt is in the notes: {proposal}"
        );
        assert!(excerpt.contains(name) || kind == "evenement", "{proposal}");
    }
}

#[tokio::test]
async fn the_draft_names_its_characters_places_and_event() {
    let proposals = proposals_of(&draft()).await;
    let has = |kind: &str, name: &str| proposals.contains(&(kind.to_owned(), name.to_owned()));
    for (kind, name) in [
        ("pnj", "Gaspard"),
        ("pnj", "Mère Ombeline"),
        ("pnj", "Aerin"),
        ("pnj", "Elowen"),
        ("lieu", "La Tour d’Ébène"),
        ("lieu", "bois de Corbière"),
        ("lieu", "mont Pourpre"),
        ("evenement", "Le moulin a brûlé dans la nuit"),
    ] {
        assert!(has(kind, name), "{kind} {name}: {proposals:?}");
    }
    assert!(
        proposals.iter().all(|(kind, _)| kind != "pj"),
        "a player character cannot be guessed: {proposals:?}"
    );
}

#[tokio::test]
async fn the_same_notes_get_the_same_answer_from_any_fake() {
    let request = GenerateRequest::new(TextKind::Notes, draft());
    let config = api("https://api.anthropic.com", None);
    let fake = FakeLlm::new();
    let first = fake.generate(&config, &request).await.unwrap();
    let second = fake.generate(&config, &request).await.unwrap();
    let other = FakeLlm::new().generate(&config, &request).await.unwrap();
    assert_eq!(first, second);
    assert_eq!(first, other);
}

#[tokio::test]
async fn a_name_is_proposed_once_whatever_the_times_it_is_met() {
    let proposals = proposals_of("Gaspard rit. Puis Gaspard partit. gaspard revint.").await;
    assert_eq!(proposals, [("pnj".to_owned(), "Gaspard".to_owned())]);
}

#[tokio::test]
async fn notes_that_name_nothing_yield_no_proposal() {
    for text in ["", "   \n ", "rien à signaler ici, tout est calme."] {
        let answer = notes_answer(text).await;
        assert_eq!(answer, json!({"proposals": []}), "{text:?}");
    }
}

#[tokio::test]
async fn a_place_is_told_by_the_noun_that_leads_or_holds_its_name() {
    let proposals = proposals_of(
        "Ils marchent vers le mont Pourpre. Puis la forêt de Brume. Ensuite Port Lointain. Hier, Mara et d’Anselme dormaient.",
    )
    .await;
    assert_eq!(
        proposals,
        [
            ("lieu", "mont Pourpre"),
            ("lieu", "forêt de Brume"),
            ("lieu", "Port Lointain"),
            ("pnj", "Mara"),
            ("pnj", "Anselme"),
        ]
        .map(|(kind, name)| (kind.to_owned(), name.to_owned()))
    );
}

#[tokio::test]
async fn a_name_stops_at_a_comma_and_a_function_word_opening_a_sentence_is_no_name() {
    let proposals =
        proposals_of("Il vit Aerin, Elowen. Le moulin a brûlé. Les Cavaliers Gris passent.").await;
    let names: Vec<_> = proposals.iter().map(|(_, name)| name.as_str()).collect();
    assert_eq!(
        names,
        ["Aerin", "Elowen", "Le moulin a brûlé", "Les Cavaliers Gris"]
    );
}

#[tokio::test]
async fn an_event_is_named_by_the_clause_that_holds_it_and_excerpted_by_its_sentence() {
    let answer = notes_answer("La nuit fut calme, une embuscade a eu lieu au gué !").await;
    assert_eq!(
        answer,
        json!({"proposals": [
            {"type": "evenement", "name": "Une embuscade a eu lieu au gué", "excerpt": "La nuit fut calme, une embuscade a eu lieu au gué"},
        ]})
    );
}

#[tokio::test]
async fn an_event_is_named_by_the_words_of_its_clause_spacing_and_all_and_no_more_than_eight() {
    let proposals =
        proposals_of("rien. le  moulin\ta brûlé et puis un deux trois quatre cinq six.").await;
    assert_eq!(
        proposals,
        [(
            "evenement".to_owned(),
            "Le  moulin\ta brûlé et puis un deux".to_owned()
        )]
    );
}

#[tokio::test]
async fn the_other_kinds_keep_their_canned_sentence() {
    let fake = FakeLlm::new();
    let config = api("https://api.anthropic.com", None);
    for (kind, text, model, chars) in [
        (
            TextKind::Prompt,
            "Gaspard le tavernier",
            "qwen2.5-32b-instruct",
            20,
        ),
        (
            TextKind::Journal,
            "00:01 Elowen brûle le moulin.",
            "qwen2.5-14b-instruct",
            29,
        ),
    ] {
        let answer = fake
            .generate(&config, &GenerateRequest::new(kind, text))
            .await
            .unwrap();
        assert_eq!(
            answer.text,
            format!("Réponse simulée de {model} (API cloud, {chars} caractères)"),
            "{kind:?}"
        );
        assert_eq!(answer.model, model);
    }
}

#[tokio::test]
async fn a_notes_request_is_journalled_with_its_text_and_the_answer_is_not() {
    let fake = FakeLlm::new();
    let text = draft();
    fake.generate(
        &api("https://api.anthropic.com", None),
        &GenerateRequest::new(TextKind::Notes, text.clone()),
    )
    .await
    .unwrap();
    let records = fake.journal().list();
    assert_eq!(records.len(), 1);
    assert_eq!(
        (records[0].kind, records[0].provider, &records[0].text),
        (RequestKind::Notes, Provider::Cloud, &text)
    );
}

#[tokio::test]
async fn a_name_runs_over_the_joiners_between_capitalised_words_and_only_those() {
    for (text, names) in [
        (
            "Ils croisent Jean de Brume Noir.",
            vec!["Jean de Brume Noir"],
        ),
        ("Ils croisent Jean de Brume demain.", vec!["Jean de Brume"]),
        ("Ils croisent Jean de la forêt.", vec!["Jean"]),
        ("Ils croisent Jean du, Brume.", vec!["Jean", "Brume"]),
        ("Ils suivent Le Corbeau.", vec!["Le Corbeau"]),
        ("Pour l’annexe B.", vec![]),
        ("du Sanglier", vec!["Sanglier"]),
        ("Dans la forêt, Gaspard dort.", vec!["Gaspard"]),
        ("Ils longent le pont de, Mara.", vec!["Mara"]),
        (
            "Son frère attend. Chaque soir, le village chante. Tous dorment.",
            vec![],
        ),
        (
            "Gaspard, tavernier du Sanglier, parle.",
            vec!["Gaspard", "Sanglier"],
        ),
        ("Ils croisent le roi du Sanglier.", vec!["Sanglier"]),
    ] {
        let proposals = proposals_of(text).await;
        let found: Vec<_> = proposals.iter().map(|(_, name)| name.as_str()).collect();
        assert_eq!(found, names, "{text:?}");
    }
}

/// The brief of the mock-up, as mock/fixtures/OralBrief.json transcribes it,
/// its lines joined.
fn brief() -> String {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../mock/fixtures/OralBrief.json"
    );
    let fixture: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    fixture["transcript"]
        .as_array()
        .unwrap()
        .iter()
        .map(|line| line.as_str().unwrap())
        .collect::<Vec<_>>()
        .join(" ")
}

async fn brief_answer(text: &str) -> Value {
    let answer = FakeLlm::new()
        .generate(
            &api("https://api.anthropic.com", None),
            &GenerateRequest::new(TextKind::Brief, text),
        )
        .await
        .unwrap();
    assert_eq!(answer.model, "qwen2.5-32b-instruct");
    serde_json::from_str(&answer.text).unwrap_or_else(|error| panic!("{error}: {}", answer.text))
}

/// The `(type, name)` of each element of each scene.
fn elements_of(answer: &Value) -> Vec<Vec<(String, String)>> {
    answer["scenes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|scene| {
            scene["elements"]
                .as_array()
                .unwrap()
                .iter()
                .map(|element| {
                    (
                        element["type"].as_str().unwrap().to_owned(),
                        element["name"].as_str().unwrap().to_owned(),
                    )
                })
                .collect()
        })
        .collect()
}

#[tokio::test]
async fn the_brief_of_the_mock_up_is_cut_into_its_three_scenes() {
    let text = brief();
    let answer = brief_answer(&text).await;
    let scenes = answer["scenes"].as_array().unwrap();
    let titles: Vec<&str> = scenes
        .iter()
        .map(|scene| scene["title"].as_str().unwrap())
        .collect();
    assert_eq!(
        titles,
        ["Séance quatre", "Sur la route", "Le gué des Corbeaux"]
    );
    for scene in scenes {
        assert_eq!(
            scene.as_object().unwrap().len(),
            3,
            "title, notes and elements, nothing else: {scene}"
        );
        let notes = scene["notes"].as_str().unwrap();
        assert!(text.contains(notes), "the notes are the brief's: {notes}");
        for element in scene["elements"].as_array().unwrap() {
            let excerpt = element["excerpt"].as_str().unwrap();
            assert!(!excerpt.is_empty(), "{element}");
        }
    }
    // The scenes hold the whole brief, in order.
    let joined: Vec<&str> = scenes
        .iter()
        .map(|scene| scene["notes"].as_str().unwrap())
        .collect();
    assert_eq!(joined.join(" "), text);
}

#[tokio::test]
async fn the_brief_of_the_mock_up_names_its_characters_places_and_events_plainly() {
    let answer = brief_answer(&brief()).await;
    let has = |scene: usize, kind: &str, name: &str| {
        elements_of(&answer)[scene].contains(&(kind.to_owned(), name.to_owned()))
    };
    for (scene, kind, name) in [
        (0, "evenement", "L’embuscade du gué"),
        (0, "lieu", "La Taverne du Sanglier"),
        (0, "pnj", "Gaspard"),
        (0, "lieu", "Le gué des Corbeaux"),
        (0, "pnj", "Morlox"),
        (1, "pnj", "Frère Odric"),
        (2, "lieu", "Le gué des Corbeaux"),
        (2, "pnj", "Vesna la Balafrée"),
        (2, "pnj", "Morlox"),
    ] {
        assert!(has(scene, kind, name), "{scene} {kind} {name}: {answer}");
    }
    let names: Vec<String> = elements_of(&answer)
        .into_iter()
        .flatten()
        .map(|(_, name)| name)
        .collect();
    for not_a_name in [
        "Bon",
        "Deuxième",
        "Troisième",
        "Trois",
        "PV",
        "Balafrée",
        "Odric",
    ] {
        assert!(
            !names.iter().any(|name| name == not_a_name),
            "{not_a_name}: {names:?}"
        );
    }
}

#[tokio::test]
async fn an_element_named_twice_in_a_scene_is_one_with_both_sentences_for_excerpt() {
    let answer = brief_answer("Morlox arrive. Il pleut. Morlox repart ! Morlox, Morlox.").await;
    assert_eq!(
        answer,
        json!({"scenes": [{
            "title": "Morlox arrive",
            "notes": "Morlox arrive. Il pleut. Morlox repart ! Morlox, Morlox.",
            "elements": [{"type": "pnj", "name": "Morlox", "excerpt": "Morlox arrive Morlox repart Morlox, Morlox"}],
        }]})
    );
}

#[tokio::test]
async fn a_brief_with_no_marker_is_one_scene_and_a_scene_with_no_title_is_numbered() {
    let answer = brief_answer("On commence à la Taverne du Sanglier. Puis le groupe tombe sur le Pont des Pendus, où a lieu l'embuscade.").await;
    assert_eq!(answer["scenes"].as_array().unwrap().len(), 1);
    assert_eq!(
        answer["scenes"][0]["title"],
        "On commence à la Taverne du Sanglier"
    );
    assert_eq!(
        elements_of(&answer)[0],
        [
            ("lieu", "La Taverne du Sanglier"),
            ("evenement", "Où a lieu l'embuscade"),
            ("lieu", "Le Pont des Pendus"),
        ]
        .map(|(kind, name)| (kind.to_owned(), name.to_owned()))
    );
    let answer = brief_answer("Bon. Scène deux, Gaspard.").await;
    let titles: Vec<&str> = answer["scenes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|scene| scene["title"].as_str().unwrap())
        .collect();
    assert_eq!(titles, ["Scène 1", "Scène 2"]);
    assert_eq!(answer["scenes"][1]["notes"], "Scène deux, Gaspard.");
}

#[tokio::test]
async fn a_nickname_joins_the_name_it_follows_with_its_article_or_without() {
    for (text, names) in [
        (
            "Puis Vesna, qu’on appelle la Balafrée, cachée.",
            vec!["Vesna la Balafrée"],
        ),
        (
            "Puis Vesna, qu'on surnomme Balafrée Rouge.",
            vec!["Vesna Balafrée Rouge"],
        ),
        ("Puis Gaspard, dit le Rouge.", vec!["Gaspard le Rouge"]),
        ("Puis Gaspard, dit le rouge.", vec!["Gaspard"]),
        ("Puis Gaspard dit le Rouge.", vec!["Gaspard", "Rouge"]),
        ("Puis Gaspard, dit, Rouge.", vec!["Gaspard", "Rouge"]),
        (
            "Puis Gaspard, qu’on, appelle Rouge.",
            vec!["Gaspard", "Rouge"],
        ),
        ("Puis Gaspard,", vec!["Gaspard"]),
        // What follows the marker is an article and a name, or nothing.
        ("Puis Gaspard, dit vieux Rouge.", vec!["Gaspard", "Rouge"]),
        ("Puis Gaspard, dit le, Rouge.", vec!["Gaspard", "Rouge"]),
    ] {
        let answer = brief_answer(text).await;
        let found: Vec<String> = elements_of(&answer)
            .into_iter()
            .flatten()
            .map(|(_, name)| name)
            .collect();
        assert_eq!(found, names, "{text:?}");
    }
}

#[tokio::test]
async fn a_title_leads_a_character_and_an_article_a_place_in_a_brief_only() {
    let text =
        "Ils voient frère Odric au mont Pourpre, puis la Tour Noire et le Sanglier. Le PNJ dort.";
    let answer = brief_answer(text).await;
    assert_eq!(
        elements_of(&answer)[0],
        [
            ("pnj", "Frère Odric"),
            ("lieu", "Mont Pourpre"),
            ("lieu", "La Tour Noire"),
            ("pnj", "Sanglier"),
        ]
        .map(|(kind, name)| (kind.to_owned(), name.to_owned()))
    );
    let notes = proposals_of(text).await;
    assert_eq!(
        notes,
        [
            ("pnj", "Odric"),
            ("lieu", "mont Pourpre"),
            ("lieu", "Tour Noire"),
            ("pnj", "Sanglier"),
            ("pnj", "Le PNJ"),
        ]
        .map(|(kind, name)| (kind.to_owned(), name.to_owned()))
    );
}

#[tokio::test]
async fn a_brief_that_says_nothing_has_no_scene() {
    for text in ["", "  \n "] {
        assert_eq!(brief_answer(text).await, json!({"scenes": []}), "{text:?}");
    }
}
