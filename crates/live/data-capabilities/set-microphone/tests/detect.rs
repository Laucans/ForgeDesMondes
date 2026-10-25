//! What the microphone listens for and how a heard phrase names a term:
//! DetectionVocabulary@1 and AliasMatch@1, held to the Concept's fixtures. The
//! campaign filter is the SQL's (`tests/postgres.rs` runs it); the rest — what
//! names nothing, the longest term, the overlap, a target named once — is the
//! detection's, here.

use ports_speech_to_text::Vocabulary as Heard;
use serde_json::{Value, json};

use set_microphone::detect::{Kind, Vocabulary};

/// The rows `VOCABULARY` answers for `input`'s campaign: the entities' names then
/// their aliases, then the sounds' names, in the order given, of the campaign alone.
fn rows(input: &Value) -> Value {
    let campaign = &input["campaign"];
    let of_campaign = |list: &str| -> Vec<Value> {
        input[list]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|item| item["campaign"] == *campaign)
            .cloned()
            .collect()
    };
    let mut rows = Vec::new();
    for entity in of_campaign("entities") {
        rows.push(
            json!({ "term": entity["name"], "kind": "entity", "id": entity["id"], "alias": false }),
        );
        for alias in entity["aliases"].as_array().into_iter().flatten() {
            rows.push(
                json!({ "term": alias, "kind": "entity", "id": entity["id"], "alias": true }),
            );
        }
    }
    for sound in of_campaign("sounds") {
        rows.push(
            json!({ "term": sound["name"], "kind": "sound", "id": sound["id"], "alias": false }),
        );
    }
    Value::Array(rows)
}

/// The Concept's own answer to `input`: the vocabulary, and the detections of its
/// phrase (`null` when it has none).
fn compute(input: &Value) -> Value {
    let vocabulary = Vocabulary::from_rows(&rows(input));
    let terms: Vec<Value> = vocabulary
        .terms()
        .iter()
        .map(|t| {
            let kind = if t.kind == Kind::Entity {
                "entity"
            } else {
                "sound"
            };
            json!({ "term": t.term, "kind": kind, "id": t.id })
        })
        .collect();
    let detections = input
        .get("phrase")
        .and_then(Value::as_str)
        .map_or(Value::Null, |phrase| {
            Value::Array(
                vocabulary
                    .detect(phrase)
                    .iter()
                    .map(|d| {
                        let kind = if d.kind == Kind::Entity {
                            "entity"
                        } else {
                            "sound"
                        };
                        json!({ "kind": kind, "id": d.id, "via": d.via })
                    })
                    .collect(),
            )
        });
    json!({ "vocabulary": terms, "detections": detections })
}

#[test]
fn conformance() {
    platform_conformance::check("DetectionVocabulary@1", |input| compute(&input));
}

#[test]
fn alias_match_conformance_of_the_terms_the_vocabulary_keeps() {
    // AliasMatch@1: « La Taverne » and « la taverne » are one term of one target.
    let vocabulary = Vocabulary::from_rows(&json!([
        { "term": "La Taverne", "kind": "entity", "id": "a", "alias": false },
        { "term": "  la TAVERNE ", "kind": "entity", "id": "a", "alias": true },
        { "term": "la taverne", "kind": "entity", "id": "b", "alias": false },
        { "term": "  ", "kind": "entity", "id": "c", "alias": false },
    ]));
    let kept: Vec<(&str, &str)> = vocabulary
        .terms()
        .iter()
        .map(|t| (t.term.as_str(), t.id.as_str()))
        .collect();
    assert_eq!(kept, [("La Taverne", "a"), ("la taverne", "b")]);
}

#[test]
fn a_row_that_names_no_kind_or_no_target_is_left_out() {
    let vocabulary = Vocabulary::from_rows(&json!([
        { "term": "Morlox", "kind": "monster", "id": "a" },
        { "term": "Morlox", "kind": "entity" },
        { "kind": "entity", "id": "a" },
        "bare",
        { "term": "Morlox", "kind": "entity", "id": "a" },
    ]));
    assert_eq!(vocabulary.terms().len(), 1);
    assert_eq!(Vocabulary::from_rows(&Value::Null).terms().len(), 0);
}

#[test]
fn the_port_listens_for_names_aliases_and_sound_names_each_once() {
    let vocabulary = Vocabulary::from_rows(&json!([
        { "term": "La Taverne du Sanglier", "kind": "entity", "id": "t", "alias": false },
        { "term": "la taverne", "kind": "entity", "id": "t", "alias": true },
        { "term": "Morlox", "kind": "entity", "id": "m", "alias": false },
        { "term": "Morlox", "kind": "entity", "id": "m2", "alias": false },
        { "term": "Porte qui grince", "kind": "sound", "id": "p", "alias": false },
    ]));
    assert_eq!(
        vocabulary.to_hear(),
        Heard {
            campaign_names: vec!["La Taverne du Sanglier".into(), "Morlox".into()],
            aliases: vec!["la taverne".into()],
            sound_names: vec!["Porte qui grince".into()],
        }
    );
}

#[test]
fn a_term_with_no_word_is_in_the_vocabulary_but_the_port_has_nothing_to_hear_of_it() {
    let vocabulary = Vocabulary::from_rows(&json!([
        { "term": "???", "kind": "entity", "id": "a", "alias": true },
        { "term": "Morlox", "kind": "entity", "id": "a", "alias": false },
    ]));
    assert_eq!(
        vocabulary.terms().len(),
        2,
        "DetectionVocabulary@1 keeps it"
    );
    assert!(
        vocabulary
            .detect("??? Morlox")
            .iter()
            .all(|d| d.via == "Morlox")
    );
    assert_eq!(vocabulary.to_hear().aliases, Vec::<String>::new());
    assert_eq!(vocabulary.to_hear().campaign_names, ["Morlox"]);
}

#[test]
fn a_phrase_naming_an_alias_names_its_element() {
    let vocabulary = Vocabulary::from_rows(&json!([
        { "term": "Vesna la Balafrée", "kind": "entity", "id": "vesna", "alias": false },
        { "term": "la Balafrée", "kind": "entity", "id": "vesna", "alias": true },
    ]));
    let found = vocabulary.detect("La Balafrée ? Jamais entendu parler.");
    assert_eq!(found.len(), 1);
    assert_eq!(
        (found[0].kind, found[0].id.as_str()),
        (Kind::Entity, "vesna")
    );
    assert_eq!(found[0].via, "la Balafrée");
}

#[test]
fn a_phrase_naming_nothing_names_nothing() {
    let vocabulary = Vocabulary::from_rows(&json!([
        { "term": "Morlox", "kind": "entity", "id": "m", "alias": false },
    ]));
    assert!(vocabulary.detect("Il fait beau ce soir.").is_empty());
    assert!(vocabulary.detect("").is_empty());
    assert!(Vocabulary::default().detect("Morlox").is_empty());
}
