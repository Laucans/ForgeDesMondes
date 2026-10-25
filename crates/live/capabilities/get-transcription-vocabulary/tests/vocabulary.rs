//! The Capability's SQL, run on a scratch database migrated as the server migrates it: what a
//! campaign's vocabulary holds, and what it never holds.

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
