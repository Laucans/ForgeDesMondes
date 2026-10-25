//! The Capability's SQL, run on a scratch database migrated as the server migrates it: what a
//! campaign's vocabulary holds, and what it never holds.

use std::collections::BTreeSet;

use get_transcription_vocabulary::SQL;
use platform_test_kit::Scratch;
use serde_json::json;

/// Two campaigns, each with elements and sounds; the answer of [`SQL`] for `campaign`.
async fn terms_of(db: &Scratch, campaign: &str) -> Vec<String> {
    db.rows(SQL, &[json!(campaign)])
        .await
        .iter()
        .map(|row| row["term"].as_str().expect("a text term").to_string())
        .collect()
}

const A: &str = "a1000000-0000-4000-8000-000000000001";
const B: &str = "b1000000-0000-4000-8000-000000000002";

/// An element of `campaign` as the sheet's save writes it: the row, its character sheet
/// when a `pj` or a `pnj`, its aliases and its first version, in the same transaction.
fn element(id: &str, campaign: &str, kind: &str, name: &str, aliases: &[&str]) -> String {
    let sheet = if kind == "lieu" || kind == "evenement" {
        String::new()
    } else {
        format!("INSERT INTO character (entity_id, token_color) VALUES ('{id}', '#888');")
    };
    let aliases: String = aliases
        .iter()
        .map(|alias| {
            format!("INSERT INTO entity_alias (entity_id, alias) VALUES ('{id}', '{alias}');")
        })
        .collect();
    format!(
        "INSERT INTO entity (id, campaign_id, type, name) VALUES ('{id}', '{campaign}', '{kind}', '{name}');
         {sheet} {aliases}
         INSERT INTO entity_version (entity_id, version_number, source, snapshot, diff)
         VALUES ('{id}', 1, 'dm', '{{}}', '[]');"
    )
}

async fn seeded(label: &str) -> Scratch {
    let db = Scratch::new(label).await;
    let elements = [
        element(
            "a2000000-0000-4000-8000-000000000001",
            A,
            "lieu",
            "La Taverne du Sanglier",
            &["la taverne"],
        ),
        element(
            "a2000000-0000-4000-8000-000000000002",
            A,
            "pnj",
            "Gaspard Grosmollet",
            &["Gaspard"],
        ),
        element(
            "a2000000-0000-4000-8000-000000000003",
            A,
            "evenement",
            "L’incendie du moulin",
            &["l’incendie"],
        ),
        element(
            "a2000000-0000-4000-8000-000000000004",
            A,
            "pj",
            "Elowen Ventargent",
            &["la rôdeuse"],
        ),
        element(
            "b2000000-0000-4000-8000-000000000001",
            B,
            "pnj",
            "Mordred l’Autre",
            &["le traître"],
        ),
    ]
    .concat();
    db.seed(&format!(
        "INSERT INTO campaign (id, name, kind) VALUES ('{A}', 'Valombre', 'campagne'), ('{B}', 'Autre', 'campagne');
         {elements}
         INSERT INTO sound (campaign_id, name, file_path) VALUES
           ('{A}', 'Porte qui grince', 'porte.ogg'),
           ('{A}', 'Cri de corbeau', 'corbeau.ogg'),
           ('{B}', 'Tonnerre lointain', 'tonnerre.ogg');"
    ))
    .await;
    db
}

fn has(terms: &[String], wanted: &str) -> bool {
    terms.iter().any(|t| t == wanted)
}

#[tokio::test]
async fn an_alias_is_a_term_beside_the_name_it_belongs_to() {
    let db = seeded("vocabulary_alias").await;
    let terms = terms_of(&db, A).await;
    assert!(has(&terms, "La Taverne du Sanglier"));
    assert!(has(&terms, "la taverne"));
}

#[tokio::test]
async fn every_name_and_alias_of_the_characters_places_and_events_is_a_term() {
    let db = seeded("vocabulary_entities").await;
    let terms: Vec<String> = terms_of(&db, A)
        .await
        .iter()
        .map(|t| t.to_lowercase())
        .collect();
    for wanted in [
        "gaspard grosmollet",
        "gaspard",
        "elowen ventargent",
        "la rôdeuse",
        "l’incendie du moulin",
        "l’incendie",
        "la taverne du sanglier",
        "la taverne",
    ] {
        assert!(has(&terms, wanted), "{wanted} is a term: {terms:?}");
    }
}

#[tokio::test]
async fn the_names_of_the_campaign_s_sounds_are_terms() {
    let db = seeded("vocabulary_sounds").await;
    let terms = terms_of(&db, A).await;
    assert!(has(&terms, "Porte qui grince"));
    assert!(has(&terms, "Cri de corbeau"));
}

#[tokio::test]
async fn nothing_of_another_campaign_is_a_term() {
    let db = seeded("vocabulary_other_campaign").await;
    let terms = terms_of(&db, A).await;
    assert_eq!(terms.len(), 10, "{terms:?}");
    for other in ["Mordred l’Autre", "le traître", "Tonnerre lointain"] {
        assert!(!has(&terms, other), "{other} belongs to another campaign");
    }
    let mut own = terms_of(&db, B).await;
    own.sort();
    assert_eq!(own, ["Mordred l’Autre", "Tonnerre lointain", "le traître"]);
    assert!(terms_of(&db, "not-a-uuid").await.is_empty());
}

#[tokio::test]
async fn a_term_is_listed_once_as_written_the_entities_before_the_sounds() {
    let db = seeded("vocabulary_once").await;
    db.seed(&format!(
        "INSERT INTO sound (campaign_id, name, file_path) VALUES
           ('{A}', 'Gaspard', 'gaspard.ogg'), ('{A}', 'Gaspard', 'gaspard2.ogg'), ('{A}', '   ', 'blank.ogg');"
    ))
    .await;
    let terms = terms_of(&db, A).await;
    assert_eq!(terms.iter().filter(|t| *t == "Gaspard").count(), 1);
    assert!(!has(&terms, "   "));
    let first_sound = terms.iter().position(|t| t == "Cri de corbeau").unwrap();
    let last_entity = terms.iter().position(|t| t == "la taverne").unwrap();
    assert!(last_entity < first_sound, "{terms:?}");
}

/// `AliasMatch@1`: trimmed, lowercase, the accents of the decomposed letter removed.
fn normalised(term: &str) -> String {
    use unicode_normalization::UnicodeNormalization;
    term.trim()
        .to_lowercase()
        .nfd()
        .filter(|c| !('\u{300}'..='\u{36f}').contains(c))
        .nfc()
        .collect()
}

/// A uuid for the `n`th thing of the `case`th fixture.
fn uuid_of(case: usize, n: usize) -> String {
    format!("{:08x}-0000-4000-8000-{n:012x}", case + 1)
}

fn quoted(text: &str) -> String {
    format!("'{}'", text.replace('\'', "''"))
}

/// The cases of `DetectionVocabulary@1` the database itself refuses to hold: one alias on two
/// elements of a campaign (`alias-unique-in-campaign`), which the Concept's fixtures allow.
const REFUSED_BY_THE_DATABASE: [&str; 2] = [
    "the same term on two targets stays on both",
    "the same term on two targets: the earlier entry of the vocabulary wins the tie",
];

// The fixtures of `DetectionVocabulary@1` planted in a real database, one campaign per case, and
// the shipped SQL read over it. The Capability answers `{ terms }` — no target, no order the
// Concept could be held to (an alias has no position) — so a case holds when the terms answered
// are those of the vocabulary the case expects, once per spelling, as written.
#[tokio::test]
async fn the_shipped_sql_answers_the_terms_of_the_concept_s_vocabulary() {
    let fixtures = platform_conformance::fixtures_of("DetectionVocabulary@1");
    let db = Scratch::new("vocabulary_fixtures").await;
    let mut refused = Vec::new();
    for (i, case) in fixtures.cases.iter().enumerate() {
        let campaigns = ["campaign", "other"].map(|which| {
            let name = if which == "campaign" {
                case.input["campaign"].as_str().unwrap_or_default()
            } else {
                "autre-campagne"
            };
            (name.to_string(), uuid_of(i, usize::from(which == "other")))
        });
        let campaign_id = |name: &str| {
            campaigns
                .iter()
                .find(|(n, _)| n == name)
                .map(|(_, id)| id.clone())
                .unwrap_or_else(|| {
                    panic!("{}: a campaign {name} the case does not name", case.name)
                })
        };
        let mut seed = format!(
            "INSERT INTO campaign (id, name, kind) VALUES ({}, 'asked', 'campagne'), ({}, 'other', 'campagne');",
            quoted(&campaigns[0].1),
            quoted(&campaigns[1].1)
        );
        let mut written: Vec<&str> = Vec::new();
        for (n, entity) in case.input["entities"]
            .as_array()
            .into_iter()
            .flatten()
            .enumerate()
        {
            let aliases: Vec<&str> = entity["aliases"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|a| a.as_str())
                .collect();
            let name = entity["name"].as_str().unwrap_or_default();
            let mine = entity["campaign"].as_str() == case.input["campaign"].as_str();
            if mine {
                written.push(name);
                written.extend(&aliases);
            }
            let campaign = campaign_id(entity["campaign"].as_str().unwrap_or_default());
            seed.push_str(&element(
                &uuid_of(i, 10 + n),
                &campaign,
                "lieu",
                &name.replace('\'', "''"),
                &aliases,
            ));
        }
        for sound in case.input["sounds"].as_array().into_iter().flatten() {
            let name = sound["name"].as_str().unwrap_or_default();
            if sound["campaign"].as_str() == case.input["campaign"].as_str() {
                written.push(name);
            }
            let campaign = campaign_id(sound["campaign"].as_str().unwrap_or_default());
            seed.push_str(&format!(
                "INSERT INTO sound (campaign_id, name, file_path) VALUES ({}, {}, 'x.ogg');",
                quoted(&campaign),
                quoted(name)
            ));
        }
        if let Err(error) = db.client().batch_execute(&seed).await {
            let message = error
                .as_db_error()
                .map_or_else(String::new, |e| e.message().to_string());
            assert!(
                message.starts_with("alias-unique-in-campaign"),
                "{}: the seed is refused for another reason: {error}",
                case.name
            );
            refused.push(case.name.as_str());
            continue;
        }
        let answered = terms_of(
            &db,
            &campaign_id(case.input["campaign"].as_str().unwrap_or_default()),
        )
        .await;
        let expected: BTreeSet<String> = case.output["vocabulary"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|entry| entry["term"].as_str())
            .map(normalised)
            .collect();
        let heard: BTreeSet<String> = answered.iter().map(|t| normalised(t)).collect();
        assert_eq!(
            heard, expected,
            "{}: the terms answered {answered:?}",
            case.name
        );
        assert_eq!(
            answered.iter().collect::<BTreeSet<_>>().len(),
            answered.len(),
            "{}: a term is listed once: {answered:?}",
            case.name
        );
        for term in &answered {
            assert!(
                written.contains(&term.as_str()),
                "{}: {term:?} is a term as written",
                case.name
            );
        }
    }
    assert_eq!(
        refused, REFUSED_BY_THE_DATABASE,
        "the cases the database refuses"
    );
}
