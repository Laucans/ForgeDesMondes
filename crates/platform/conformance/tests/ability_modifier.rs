//! The runner over `AbilityModifier@1`: it finds and loads the fixtures,
//! they agree with `concept.json`, and an implementation of the Concept —
//! the small one below, local to this test — passes them.

use platform_conformance::{fixtures_of, repository_root};
use serde_json::{Value, json};

const ID: &str = "AbilityModifier@1";

/// `AbilityModifier@1`, as the Concept's definition words it.
fn modifier(input: Value) -> Value {
    let score = input["score"].as_i64().expect("an integer score");
    json!({"modifier": (score - 10).div_euclid(2)})
}

fn concept() -> Value {
    let path = repository_root().join("concepts/AbilityModifier/v1/concept.json");
    let text = std::fs::read_to_string(&path).expect("concept.json");
    serde_json::from_str(&text).expect("concept.json is JSON")
}

fn modifier_for(score: i64) -> i64 {
    fixtures_of(ID)
        .cases
        .into_iter()
        .find(|case| case.input["score"] == score)
        .unwrap_or_else(|| panic!("no case for score {score}"))
        .output["modifier"]
        .as_i64()
        .expect("an integer modifier")
}

#[test]
fn conformance_the_fixtures_load_and_an_implementation_passes_them() {
    platform_conformance::check(ID, modifier);
}

#[test]
fn the_scores_of_the_business_goal_are_in_the_fixtures() {
    assert_eq!(modifier_for(18), 4);
    assert_eq!(modifier_for(9), -1);
    assert_eq!(modifier_for(1), -5);
}

#[test]
fn the_fixtures_cover_odd_even_low_and_high_scores() {
    let scores: Vec<i64> = fixtures_of(ID)
        .cases
        .iter()
        .map(|case| case.input["score"].as_i64().expect("score"))
        .collect();
    assert!(scores.contains(&1) && scores.contains(&30), "both ends");
    assert!(scores.iter().any(|s| s % 2 == 0 && *s < 10), "even, low");
    assert!(scores.iter().any(|s| s % 2 == 1 && *s < 10), "odd, low");
    assert!(scores.iter().any(|s| s % 2 == 0 && *s > 10), "even, high");
    assert!(scores.iter().any(|s| s % 2 == 1 && *s > 10), "odd, high");
}

#[test]
fn a_truncating_implementation_fails_the_fixtures() {
    let truncating = |input: Value| {
        let score = input["score"].as_i64().expect("score");
        json!({"modifier": (score - 10) / 2})
    };
    let fixtures = fixtures_of(ID);
    let failed = platform_conformance::failures(&fixtures, truncating);
    assert!(
        failed.iter().any(|f| f.contains("score 9")),
        "floor and truncation differ at 9: {failed:?}"
    );
}

#[test]
fn concept_json_points_at_the_fixtures_and_every_output_obeys_it() {
    let concept = concept();
    assert_eq!(concept["concept"], "AbilityModifier");
    assert_eq!(concept["version"], 1);
    assert_eq!(concept["kind"], "derived");
    assert_eq!(concept["conformance"], "./fixtures.json");
    assert_eq!(concept["output"]["modifier"]["type"], "integer");

    for case in fixtures_of(ID).cases {
        let output = case.output.as_object().expect("an output is an object");
        assert_eq!(output.len(), 1, "{}: fields", case.name);
        assert!(output["modifier"].is_i64(), "{}", case.name);
    }
}
