//! The runner over `HexGrid@1`: it finds and loads the fixtures, they agree
//! with `concept.json`, and an implementation of the Concept — the small one
//! below, local to this test — passes them.

use platform_conformance::{fixtures_of, repository_root};
use serde_json::{Value, json};

const ID: &str = "HexGrid@1";
const FEET_PER_HEX: u64 = 5;
const FIELDS: [&str; 6] = ["size", "ox", "oy", "orient", "color", "opacity"];

/// Whether `field` of the grid holds, as the Concept's definition words it.
fn holds(input: &Value, field: &str) -> bool {
    let value = &input[field];
    match field {
        "size" => value.as_f64().is_some_and(|size| size > 0.0),
        "ox" | "oy" => value.is_number(),
        "orient" => value == "pointy" || value == "flat",
        "color" => value.as_str().is_some_and(|color| !color.is_empty()),
        "opacity" => value
            .as_f64()
            .is_some_and(|opacity| (0.0..=1.0).contains(&opacity)),
        other => panic!("no such field {other}"),
    }
}

/// `HexGrid@1`, as the Concept's definition words it.
fn measure(input: Value) -> Value {
    let problems: Vec<&str> = FIELDS
        .into_iter()
        .filter(|field| !holds(&input, field))
        .collect();
    let feet = input["hexes"]
        .as_f64()
        .filter(|hexes| *hexes >= 0.0 && hexes.fract() == 0.0)
        .and_then(|hexes| (hexes as u64).checked_mul(FEET_PER_HEX));
    json!({
        "valid": problems.is_empty(),
        "problems": problems,
        "feetPerHex": FEET_PER_HEX,
        "feet": feet,
    })
}

fn concept() -> Value {
    let path = repository_root().join("concepts/HexGrid/v1/concept.json");
    let text = std::fs::read_to_string(&path).expect("concept.json");
    serde_json::from_str(&text).expect("concept.json is JSON")
}

#[test]
fn conformance_the_fixtures_load_and_an_implementation_passes_them() {
    platform_conformance::check(ID, measure);
}

#[test]
fn a_hex_is_five_feet_in_every_case_and_six_hexes_read_thirty_feet() {
    let cases = fixtures_of(ID).cases;
    assert!(cases.iter().all(|case| case.output["feetPerHex"] == 5));
    let six = cases
        .iter()
        .find(|case| case.input["hexes"] == 6 && case.output["valid"] == true)
        .expect("a valid case of 6 hexes");
    assert_eq!(six.output["feet"], 30);
}

#[test]
fn the_fixtures_cover_both_orientations_and_each_way_to_be_not_valid() {
    let cases = fixtures_of(ID).cases;
    for orient in ["pointy", "flat"] {
        assert!(
            cases
                .iter()
                .any(|case| case.input["orient"] == orient && case.output["valid"] == true),
            "no valid {orient} case"
        );
    }
    for field in FIELDS {
        assert!(
            cases
                .iter()
                .any(|case| case.output["problems"] == json!([field])),
            "no case is not valid on {field} alone"
        );
    }
    assert!(
        cases
            .iter()
            .any(|case| case.input["size"].as_f64() == Some(0.0)),
        "no case of a hex size of 0"
    );
    assert!(
        cases
            .iter()
            .any(|case| case.input["size"].as_f64().is_some_and(|size| size < 0.0)),
        "no case of a negative hex size"
    );
}

#[test]
fn concept_json_points_at_the_fixtures_and_every_output_obeys_it() {
    let concept = concept();
    assert_eq!(concept["concept"], "HexGrid");
    assert_eq!(concept["version"], 1);
    assert_eq!(concept["kind"], "derived");
    assert_eq!(concept["conformance"], "./fixtures.json");

    let fields: Vec<&String> = concept["output"]
        .as_object()
        .expect("output")
        .keys()
        .collect();
    let allowed = concept["output"]["problems"]["items"]["enum"]
        .as_array()
        .expect("problems enum");
    for case in fixtures_of(ID).cases {
        let output = case.output.as_object().expect("an output is an object");
        assert_eq!(output.len(), fields.len(), "{}: fields", case.name);
        assert!(
            fields.iter().all(|field| output.contains_key(*field)),
            "{}",
            case.name
        );

        let problems = output["problems"].as_array().expect("problems");
        assert_eq!(output["valid"], problems.is_empty(), "{}", case.name);
        assert!(
            problems.iter().all(|problem| allowed.contains(problem)),
            "{}",
            case.name
        );
        let order = |problem: &Value| FIELDS.iter().position(|field| problem == field);
        assert!(
            problems
                .windows(2)
                .all(|pair| order(&pair[0]) < order(&pair[1])),
            "{}: problems out of order",
            case.name
        );
    }
}
