//! The runner over `HealthBand@1`: it finds and loads the fixtures, they
//! agree with `concept.json`, and an implementation of the Concept — the
//! small one below, local to this test — passes them.

use platform_conformance::{fixtures_of, repository_root};
use serde_json::{Value, json};

const ID: &str = "HealthBand@1";

/// `HealthBand@1`, as the Concept's definition words it.
fn band(input: Value) -> Value {
    let band = match (input["hp_current"].as_i64(), input["hp_max"].as_i64()) {
        (Some(current), Some(max)) => {
            let current = current.clamp(0, max.max(0));
            Some(if current == 0 {
                "mort"
            } else if current == max {
                "indemne"
            } else if 2 * current > max {
                "blessé"
            } else {
                "en sang"
            })
        }
        _ => None,
    };
    json!({ "band": band })
}

fn concept() -> Value {
    let path = repository_root().join("concepts/HealthBand/v1/concept.json");
    let text = std::fs::read_to_string(&path).expect("concept.json");
    serde_json::from_str(&text).expect("concept.json is JSON")
}

#[test]
fn conformance_the_fixtures_load_and_an_implementation_passes_them() {
    platform_conformance::check(ID, band);
}

#[test]
fn the_npc_of_20_hit_points_of_r108_walks_the_four_bands_in_order() {
    let bands: Vec<Value> = fixtures_of(ID)
        .cases
        .into_iter()
        .filter(|case| case.input["hp_max"] == 20 && case.name.starts_with("R108"))
        .map(|case| case.output["band"].clone())
        .collect();
    assert_eq!(bands, ["indemne", "blessé", "en sang", "mort"]);
}

#[test]
fn a_pc_token_has_no_band_whether_its_hit_points_are_absent_or_null() {
    let pc: Vec<Value> = fixtures_of(ID)
        .cases
        .into_iter()
        .filter(|case| case.name.starts_with("pj-hp-not-tracked"))
        .map(|case| case.input)
        .collect();
    assert!(pc.contains(&json!({})), "no PC case with the keys absent");
    assert!(
        pc.contains(&json!({"hp_current": null, "hp_max": null})),
        "no PC case with the keys null"
    );
    for input in pc {
        assert_eq!(band(input)["band"], Value::Null);
    }
}

#[test]
fn the_fixtures_cover_every_band_and_the_absence_of_one() {
    let outputs: Vec<Value> = fixtures_of(ID)
        .cases
        .into_iter()
        .map(|case| case.output)
        .collect();
    for wanted in [
        json!("indemne"),
        json!("blessé"),
        json!("en sang"),
        json!("mort"),
        Value::Null,
    ] {
        assert!(
            outputs.iter().any(|output| output["band"] == wanted),
            "no case answers band {wanted}"
        );
    }
}

#[test]
fn concept_json_points_at_the_fixtures_and_every_output_obeys_it() {
    let concept = concept();
    assert_eq!(concept["concept"], "HealthBand");
    assert_eq!(concept["version"], 1);
    assert_eq!(concept["kind"], "derived");
    assert_eq!(concept["conformance"], "./fixtures.json");

    let fields: Vec<&String> = concept["output"]
        .as_object()
        .expect("output")
        .keys()
        .collect();
    assert_eq!(fields, ["band"], "the output is the band alone");
    let bands = concept["output"]["band"]["enum"]
        .as_array()
        .expect("band enum");
    for case in fixtures_of(ID).cases {
        // players-see-band-only: nothing but the band, never a hit point figure.
        let output = case.output.as_object().expect("an output is an object");
        assert_eq!(output.len(), 1, "{}: fields", case.name);
        let band = &output["band"];
        assert!(band.is_null() || bands.contains(band), "{}", case.name);
        // dead-derived: dead exactly when the current hit points are 0 or less.
        if let (Some(current), Some(_)) = (
            case.input["hp_current"].as_i64(),
            case.input["hp_max"].as_i64(),
        ) {
            assert_eq!(*band == "mort", current <= 0, "{}", case.name);
        }
    }
}
