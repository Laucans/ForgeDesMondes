//! The runner over `DetectionVocabulary@1`: it finds and loads the fixtures,
//! they agree with `concept.json`, and an implementation of the Concept — the
//! small one below, local to this test — passes them.

use std::collections::HashSet;

use platform_conformance::{fixtures_of, repository_root};
use serde_json::{Value, json};
use unicode_normalization::UnicodeNormalization;

const ID: &str = "DetectionVocabulary@1";

/// `AliasMatch@1`'s normalised term: no accents, lowercase, trimmed.
fn normalize(term: &str) -> String {
    term.nfd()
        .filter(|c| !('\u{300}'..='\u{36f}').contains(c))
        .nfc()
        .collect::<String>()
        .to_lowercase()
        .trim()
        .to_string()
}

/// The words of a text: maximal runs of letters, digits and combining marks,
/// each normalised with `AliasMatch@1`.
fn words(text: &str) -> Vec<String> {
    text.split(|c: char| !(c.is_alphanumeric() || ('\u{300}'..='\u{36f}').contains(&c)))
        .filter(|word| !word.is_empty())
        .map(normalize)
        .collect()
}

struct Entry {
    term: String,
    kind: &'static str,
    id: String,
}

/// The vocabulary of `campaign`: the entities' names and aliases, then the
/// sounds' names; nothing blank, nothing twice for one target.
fn vocabulary(input: &Value) -> Vec<Entry> {
    let campaign = &input["campaign"];
    let mut entries: Vec<Entry> = Vec::new();
    let mut push = |term: &str, kind: &'static str, id: &str| {
        let normalized = normalize(term);
        let twice = entries
            .iter()
            .any(|e| e.kind == kind && e.id == id && normalize(&e.term) == normalized);
        if !normalized.is_empty() && !twice {
            entries.push(Entry {
                term: term.to_string(),
                kind,
                id: id.to_string(),
            });
        }
    };
    for entity in input["entities"].as_array().expect("`entities` is a list") {
        if entity["campaign"] != *campaign {
            continue;
        }
        let id = entity["id"].as_str().expect("an entity has an id");
        push(entity["name"].as_str().expect("a name"), "entity", id);
        for alias in entity["aliases"].as_array().expect("`aliases` is a list") {
            push(alias.as_str().expect("an alias is a string"), "entity", id);
        }
    }
    for sound in input["sounds"].as_array().expect("`sounds` is a list") {
        if sound["campaign"] != *campaign {
            continue;
        }
        let id = sound["id"].as_str().expect("a sound has an id");
        push(sound["name"].as_str().expect("a name"), "sound", id);
    }
    entries
}

/// `DetectionVocabulary@1`, as the Concept's definition words it.
fn detection_vocabulary(input: &Value) -> Value {
    let entries = vocabulary(input);
    let listed: Vec<Value> = entries
        .iter()
        .map(|e| json!({"term": e.term, "kind": e.kind, "id": e.id}))
        .collect();
    let Some(phrase) = input.get("phrase") else {
        return json!({"vocabulary": listed, "detections": null});
    };
    let phrase = words(phrase.as_str().expect("`phrase` is absent or a string"));

    // (length, start, entry, end): every occurrence of every entry.
    let mut candidates: Vec<(usize, usize, usize, usize)> = Vec::new();
    for (index, entry) in entries.iter().enumerate() {
        let term = words(&entry.term);
        if term.is_empty() {
            continue;
        }
        let length = term.join(" ").chars().count();
        for start in 0..phrase.len() {
            if phrase[start..].starts_with(&term) {
                candidates.push((length, start, index, start + term.len()));
            }
        }
    }
    candidates.sort_by_key(|&(length, start, index, _)| (std::cmp::Reverse(length), start, index));

    let mut taken: HashSet<usize> = HashSet::new();
    let mut named: HashSet<(&str, &str)> = HashSet::new();
    let mut kept: Vec<(usize, usize)> = Vec::new();
    for (_, start, index, end) in candidates {
        let entry = &entries[index];
        let target = (entry.kind, entry.id.as_str());
        if (start..end).any(|word| taken.contains(&word)) || named.contains(&target) {
            continue;
        }
        taken.extend(start..end);
        named.insert(target);
        kept.push((start, index));
    }
    kept.sort_unstable();
    let detections: Vec<Value> = kept
        .iter()
        .map(|&(_, index)| {
            let e = &entries[index];
            json!({"kind": e.kind, "id": e.id, "via": e.term})
        })
        .collect();
    json!({"vocabulary": listed, "detections": detections})
}

fn concept() -> Value {
    let path = repository_root().join("concepts/DetectionVocabulary/v1/concept.json");
    let text = std::fs::read_to_string(&path).expect("concept.json");
    serde_json::from_str(&text).expect("concept.json is JSON")
}

fn world() -> Value {
    json!({
        "campaign": "c1",
        "entities": [
            {"id": "tav", "campaign": "c1", "name": "La Taverne du Sanglier", "aliases": ["la taverne"]},
            {"id": "ves", "campaign": "c1", "name": "Vesna la Balafrée", "aliases": ["la Balafrée"]},
            {"id": "far", "campaign": "c2", "name": "Au Loin", "aliases": ["la lune"]}
        ],
        "sounds": [
            {"id": "snd", "campaign": "c1", "name": "Porte qui grince"},
            {"id": "far-snd", "campaign": "c2", "name": "Cloche"}
        ]
    })
}

fn with_phrase(phrase: &str) -> Value {
    let mut input = world();
    input["phrase"] = json!(phrase);
    input
}

fn detections(phrase: &str) -> Vec<(String, String)> {
    detection_vocabulary(&with_phrase(phrase))["detections"]
        .as_array()
        .expect("a phrase gives a list")
        .iter()
        .map(|d| {
            (
                d["id"].as_str().unwrap().to_string(),
                d["via"].as_str().unwrap().to_string(),
            )
        })
        .collect()
}

fn pair(id: &str, via: &str) -> (String, String) {
    (id.to_string(), via.to_string())
}

#[test]
fn conformance_the_fixtures_load_and_an_implementation_passes_them() {
    platform_conformance::check(ID, |input| detection_vocabulary(&input));
}

#[test]
fn concept_json_names_the_concept_and_points_to_its_fixtures() {
    let concept = concept();
    assert_eq!(concept["concept"], "DetectionVocabulary");
    assert_eq!(concept["version"], 1);
    assert_eq!(concept["kind"], "derived");
    assert_eq!(concept["conformance"], "./fixtures.json");
    assert_eq!(concept["deprecates"], Value::Null);
    assert!(
        concept["definition"]
            .as_str()
            .is_some_and(|d| d.contains("AliasMatch@1")),
        "the definition refers to AliasMatch@1 instead of copying it"
    );
}

#[test]
fn every_case_answers_the_fields_concept_json_declares() {
    let concept = concept();
    let fields: HashSet<&str> = concept["output"]
        .as_object()
        .expect("output")
        .keys()
        .map(String::as_str)
        .collect();
    let fixtures = fixtures_of(ID);
    assert!(!fixtures.cases.is_empty());
    let mut names = HashSet::new();
    for case in &fixtures.cases {
        assert!(names.insert(case.name.clone()), "{} twice", case.name);
        let answered: HashSet<&str> = case
            .output
            .as_object()
            .expect("an output is an object")
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(answered, fields, "{}", case.name);
        assert!(
            case.input.get("phrase") != Some(&Value::Null),
            "{}",
            case.name
        );
        assert_eq!(
            case.input.get("phrase").is_none(),
            case.output["detections"].is_null(),
            "{}: detections are null exactly when the phrase is absent",
            case.name
        );
    }
}

#[test]
fn the_fixtures_cover_each_rule_of_the_definition() {
    let fixtures = fixtures_of(ID);
    let has = |keyword: &str| fixtures.cases.iter().any(|c| c.name.contains(keyword));
    for keyword in [
        "another campaign",
        "blank term",
        "inside a longer word",
        "overlapping",
        "at most once",
        "without accents",
        "order of the phrase",
        "tie",
    ] {
        assert!(has(keyword), "no case about {keyword:?}");
    }
}

#[test]
fn the_vocabulary_holds_names_and_aliases_of_the_campaign_only() {
    let got = detection_vocabulary(&world());
    let terms: Vec<&str> = got["vocabulary"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["term"].as_str().unwrap())
        .collect();
    assert_eq!(
        terms,
        [
            "La Taverne du Sanglier",
            "la taverne",
            "Vesna la Balafrée",
            "la Balafrée",
            "Porte qui grince"
        ]
    );
    assert_eq!(got["detections"], Value::Null);
}

#[test]
fn la_balafree_is_vesna_with_or_without_accents_and_case() {
    for phrase in [
        "La Balafrée ? Jamais entendu parler.",
        "LA BALAFREE",
        "la balafrée",
    ] {
        assert_eq!(detections(phrase), [pair("ves", "la Balafrée")], "{phrase}");
    }
}

#[test]
fn a_term_is_never_found_inside_a_longer_word() {
    assert!(detections("la taverne").len() == 1);
    assert!(detections("la tavernes, la tavernette").is_empty());
    assert!(detections("lla taverne").is_empty());
}

#[test]
fn the_longer_of_two_overlapping_terms_wins() {
    assert_eq!(
        detections("à la taverne du sanglier"),
        [pair("tav", "La Taverne du Sanglier")]
    );
}

#[test]
fn a_term_of_another_campaign_names_nothing() {
    assert!(detections("la lune et la cloche").is_empty());
}

#[test]
fn a_target_is_named_once_and_the_answer_follows_the_phrase() {
    assert_eq!(
        detections("la porte qui grince, la taverne, la taverne"),
        [pair("snd", "Porte qui grince"), pair("tav", "la taverne")]
    );
}

#[test]
fn a_blank_phrase_names_nothing_but_is_not_an_absent_phrase() {
    for phrase in ["", "   ", "…"] {
        assert!(detections(phrase).is_empty(), "{phrase:?}");
    }
}
