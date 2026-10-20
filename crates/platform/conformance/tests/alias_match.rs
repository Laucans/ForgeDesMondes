//! The runner over `AliasMatch@1`: it finds and loads the fixtures, they
//! agree with `concept.json`, and an implementation of the Concept — the
//! small one below, local to this test — passes them.

use platform_conformance::{fixtures_of, repository_root};
use serde_json::{Value, json};
use unicode_normalization::UnicodeNormalization;

const ID: &str = "AliasMatch@1";

/// The term without accents, in lowercase, trimmed.
fn normalize(term: &str) -> String {
    term.nfd()
        .filter(|c| !('\u{300}'..='\u{36f}').contains(c))
        .nfc()
        .collect::<String>()
        .to_lowercase()
        .trim()
        .to_string()
}

/// `AliasMatch@1`, as the Concept's definition words it.
fn alias_match(input: &Value) -> Value {
    let term = normalize(input["term"].as_str().expect("`term` is a string"));
    let matches = input.get("other").map(|other| {
        let other = normalize(other.as_str().expect("`other` is absent or a string"));
        !term.is_empty() && term == other
    });
    json!({"normalized_term": term, "match": matches})
}

fn concept() -> Value {
    let path = repository_root().join("concepts/AliasMatch/v1/concept.json");
    let text = std::fs::read_to_string(&path).expect("concept.json");
    serde_json::from_str(&text).expect("concept.json is JSON")
}

#[test]
fn conformance_the_fixtures_load_and_an_implementation_passes_them() {
    platform_conformance::check(ID, |input| alias_match(&input));
}

#[test]
fn la_rodeuse_normalises_to_la_rodeuse() {
    let got = alias_match(&json!({"term": "la Rôdeuse"}));
    assert_eq!(got["normalized_term"], "la rodeuse");
    assert_eq!(got["match"], Value::Null);
}

#[test]
fn terms_differing_by_case_accents_or_surrounding_spaces_match() {
    for other in [
        "la rodeuse",
        "LA RÔDEUSE",
        "  La Rodeuse ",
        "la ro\u{302}deuse",
    ] {
        let got = alias_match(&json!({"term": "La Rôdeuse", "other": other}));
        assert_eq!(got["match"], true, "{other:?}");
    }
}

#[test]
fn the_cases_of_the_issue_are_in_the_fixtures() {
    let cases = fixtures_of(ID).cases;
    let has = |term: &str, normalized: &str| {
        cases
            .iter()
            .any(|c| c.input["term"] == term && c.output["normalized_term"] == normalized)
    };
    assert!(has("la Rôdeuse", "la rodeuse"));
    assert!(has("LA RÔDEUSE", "la rodeuse"));
    assert!(cases.iter().any(|c| c.input["term"] == "La Rôdeuse"
        && c.input["other"] == "la rodeuse"
        && c.output["match"] == true));
}

#[test]
fn the_fixtures_cover_a_match_a_mismatch_and_a_normalisation_alone() {
    let matches: Vec<Value> = fixtures_of(ID)
        .cases
        .into_iter()
        .map(|case| case.output["match"].clone())
        .collect();
    for wanted in [json!(true), json!(false), Value::Null] {
        assert!(matches.contains(&wanted), "no case answers match {wanted}");
    }
}

#[test]
fn concept_json_points_at_the_fixtures_and_every_output_obeys_it() {
    let concept = concept();
    assert_eq!(concept["concept"], "AliasMatch");
    assert_eq!(concept["version"], 1);
    assert_eq!(concept["kind"], "derived");
    assert_eq!(concept["conformance"], "./fixtures.json");

    let fields: Vec<&String> = concept["output"]
        .as_object()
        .expect("output")
        .keys()
        .collect();
    for case in fixtures_of(ID).cases {
        let output = case.output.as_object().expect("an output is an object");
        assert_eq!(output.len(), fields.len(), "{}: fields", case.name);
        assert!(
            fields.iter().all(|field| output.contains_key(*field)),
            "{}",
            case.name
        );
        assert!(output["normalized_term"].is_string(), "{}", case.name);
        assert_eq!(
            output["match"].is_null(),
            case.input.get("other").is_none(),
            "{}: match is null exactly when other is absent",
            case.name
        );
        let term = output["normalized_term"].as_str().expect("string");
        assert_eq!(term, term.trim(), "{}: not trimmed", case.name);
    }
}
