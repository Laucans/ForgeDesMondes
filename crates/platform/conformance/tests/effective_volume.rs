//! The runner over `EffectiveVolume@1`: it finds and loads the fixtures,
//! they agree with `concept.json`, and an implementation of the Concept —
//! the small one below, local to this test — passes them.

use platform_conformance::{fixtures_of, repository_root};
use serde_json::{Value, json};

const ID: &str = "EffectiveVolume@1";
const OWN_OUT_OF_RANGE: &str = "volume propre hors de 0 à 100 %";
const MASTER_OUT_OF_RANGE: &str = "volume global hors de 0 à 100 %";

/// A percent from 0 to 100, or nothing: never clamped.
fn percent(value: &Value) -> Option<f64> {
    value.as_f64().filter(|p| (0.0..=100.0).contains(p))
}

/// `EffectiveVolume@1`, as the Concept's definition words it.
fn effective_volume(input: &Value) -> Value {
    let rejected = |message: &str| {
        json!({
            "gain": null,
            "volumeFraction": null,
            "masterFraction": null,
            "valid": false,
            "message": message,
        })
    };
    let Some(volume) = percent(&input["volume"]) else {
        return rejected(OWN_OUT_OF_RANGE);
    };
    let Some(master) = percent(&input["masterVolume"]) else {
        return rejected(MASTER_OUT_OF_RANGE);
    };
    json!({
        "gain": volume * master / 10_000.0,
        "volumeFraction": volume / 100.0,
        "masterFraction": master / 100.0,
        "valid": true,
        "message": null,
    })
}

fn concept() -> Value {
    let path = repository_root().join("concepts/EffectiveVolume/v1/concept.json");
    let text = std::fs::read_to_string(&path).expect("concept.json");
    serde_json::from_str(&text).expect("concept.json is JSON")
}

fn gain(volume: u32, master: u32) -> f64 {
    effective_volume(&json!({"volume": volume, "masterVolume": master}))["gain"]
        .as_f64()
        .expect("a valid input has a gain")
}

#[test]
fn conformance_the_fixtures_load_and_an_implementation_passes_them() {
    platform_conformance::check(ID, |input| effective_volume(&input));
}

#[test]
fn the_numbers_of_r111_are_in_the_fixtures() {
    let fixtures = fixtures_of(ID);
    let gain_of = |volume: u32, master: u32| {
        fixtures
            .cases
            .iter()
            .find(|case| case.input == json!({"volume": volume, "masterVolume": master}))
            .unwrap_or_else(|| panic!("no case for {volume} % under {master} %"))
            .output["gain"]
            .clone()
    };
    assert_eq!(gain_of(55, 80), json!(0.44));
    assert_eq!(gain_of(55, 40), json!(0.22));
    assert_eq!(gain_of(80, 40), json!(0.32));
}

#[test]
fn lowering_the_master_keeps_the_ratio_between_two_sounds() {
    let (crowd_high, door_high) = (gain(55, 80), gain(80, 80));
    let (crowd_low, door_low) = (gain(55, 40), gain(80, 40));
    assert!(crowd_low < crowd_high && door_low < door_high);
    // The SPEC allows 0.02; a product of the two fractions keeps it to rounding.
    assert!((crowd_high / door_high - crowd_low / door_low).abs() < 1e-9);
}

#[test]
fn the_fixtures_reject_both_inputs_out_of_range_and_never_clamp() {
    let cases = fixtures_of(ID).cases;
    for message in [OWN_OUT_OF_RANGE, MASTER_OUT_OF_RANGE] {
        assert!(
            cases.iter().any(|case| case.output["message"] == message),
            "no rejection answers {message:?}"
        );
    }
    for case in &cases {
        let in_range = percent(&case.input["volume"]).is_some()
            && percent(&case.input["masterVolume"]).is_some();
        assert_eq!(case.output["valid"], in_range, "{}", case.name);
        if !in_range {
            for field in ["gain", "volumeFraction", "masterFraction"] {
                assert_eq!(case.output[field], Value::Null, "{}: {field}", case.name);
            }
        }
    }
}

#[test]
fn the_outputs_of_the_fixtures_are_the_ones_concept_json_declares() {
    let concept = concept();
    let declared: Vec<&str> = concept["output"]
        .as_object()
        .expect("output")
        .keys()
        .map(String::as_str)
        .collect();
    for case in fixtures_of(ID).cases {
        let mut answered: Vec<&str> = case
            .output
            .as_object()
            .expect("an output is an object")
            .keys()
            .map(String::as_str)
            .collect();
        answered.sort_unstable();
        let mut wanted = declared.clone();
        wanted.sort_unstable();
        assert_eq!(answered, wanted, "{}", case.name);
    }
}
