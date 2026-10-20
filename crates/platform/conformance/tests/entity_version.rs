//! The runner over `EntityVersion@1`: it finds and loads the fixtures, they
//! agree with `concept.json`, and an implementation of the Concept — the
//! small one below, local to this test — passes them.

use platform_conformance::{failures, fixtures_of, repository_root};
use serde_json::{Map, Value, json};

const ID: &str = "EntityVersion@1";
const SOURCES: [&str; 5] = ["dm", "ia", "dndbeyond", "brief", "restauration"];

/// Two JSON values are the same: numbers by value, the rest as a whole.
fn same(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(a), Value::Number(b)) => a.as_f64() == b.as_f64(),
        (Value::Array(a), Value::Array(b)) => {
            a.len() == b.len() && a.iter().zip(b).all(|(a, b)| same(a, b))
        }
        (Value::Object(a), Value::Object(b)) => {
            a.len() == b.len()
                && a.iter()
                    .all(|(key, a)| b.get(key).is_some_and(|b| same(a, b)))
        }
        (a, b) => a == b,
    }
}

/// `EntityVersion@1`, as the Concept's definition words it.
fn version(input: Value) -> Value {
    let (number, before) = match &input["previous"] {
        Value::Null => (1, Map::new()),
        previous => (
            previous["number"].as_u64().expect("previous.number") + 1,
            previous["snapshot"]
                .as_object()
                .expect("previous.snapshot")
                .clone(),
        ),
    };
    let after = input["snapshot"].as_object().expect("snapshot").clone();

    let mut fields: Vec<&String> = before.keys().chain(after.keys()).collect();
    fields.sort();
    fields.dedup();
    let diff: Vec<Value> = fields
        .into_iter()
        .filter_map(|field| {
            let old = before.get(field).unwrap_or(&Value::Null);
            let new = after.get(field).unwrap_or(&Value::Null);
            (!same(old, new)).then(|| json!({"field": field, "old": old, "new": new}))
        })
        .collect();

    json!({"number": number, "source": input["source"], "diff": diff})
}

fn concept() -> Value {
    let path = repository_root().join("concepts/EntityVersion/v1/concept.json");
    let text = std::fs::read_to_string(&path).expect("concept.json");
    serde_json::from_str(&text).expect("concept.json is JSON")
}

fn case(name_starts_with: &str) -> platform_conformance::Case {
    fixtures_of(ID)
        .cases
        .into_iter()
        .find(|case| case.name.starts_with(name_starts_with))
        .unwrap_or_else(|| panic!("no case starting with {name_starts_with:?}"))
}

#[test]
fn conformance_the_fixtures_load_and_an_implementation_passes_them() {
    platform_conformance::check(ID, version);
}

#[test]
fn a_wrong_implementation_is_caught_by_the_fixtures() {
    let numbering_by_the_restored_version = |input: Value| {
        let mut answer = version(input.clone());
        if input["source"] == "restauration" {
            answer["number"] = json!(1);
        }
        answer
    };
    let never_empty = |input: Value| {
        let mut answer = version(input);
        if answer["diff"] == json!([]) {
            answer["diff"] = json!([{"field": "name", "old": null, "new": null}]);
        }
        answer
    };
    let unsorted = |input: Value| {
        let mut answer = version(input);
        if let Some(diff) = answer["diff"].as_array_mut() {
            diff.reverse();
        }
        answer
    };
    let fixtures = fixtures_of(ID);
    for wrong in [
        &numbering_by_the_restored_version as &dyn Fn(Value) -> Value,
        &never_empty,
        &unsorted,
    ] {
        assert!(!failures(&fixtures, wrong).is_empty());
    }
}

#[test]
fn notes_and_behaviour_changed_give_exactly_those_two_fields_with_old_and_new() {
    let case = case("two snapshots that differ in notes and behaviour");
    let diff = case.output["diff"].as_array().expect("diff");
    let fields: Vec<&Value> = diff.iter().map(|entry| &entry["field"]).collect();
    assert_eq!(fields, [&json!("behavior"), &json!("notes")]);
    for entry in diff {
        assert_eq!(entry.as_object().expect("entry").len(), 3);
        assert_ne!(entry["old"], entry["new"]);
        let (field, old) = (entry["field"].as_str().expect("field"), &entry["old"]);
        assert_eq!(&case.input["previous"]["snapshot"][field], old);
        assert_eq!(&case.input["snapshot"][field], &entry["new"]);
    }
}

#[test]
fn an_identical_snapshot_has_an_empty_diff() {
    let case = case("a snapshot identical to the previous one");
    assert!(same(
        &case.input["previous"]["snapshot"],
        &case.input["snapshot"]
    ));
    assert_eq!(case.output["diff"], json!([]));
}

#[test]
fn the_fixtures_cover_the_first_version_every_source_and_a_restoration_that_does_not_reuse_a_number()
 {
    let cases = fixtures_of(ID).cases;
    assert!(cases.iter().any(|case| case.input["previous"].is_null()));
    for source in SOURCES {
        assert!(
            cases.iter().any(|case| case.input["source"] == source),
            "no case for the source {source}"
        );
    }
    let restoration = case("a restoration takes the next number");
    let previous = restoration.input["previous"]["number"]
        .as_u64()
        .expect("number");
    assert_eq!(restoration.output["number"], previous + 1);
}

#[test]
fn concept_json_points_at_the_fixtures_and_every_output_obeys_it() {
    let concept = concept();
    assert_eq!(concept["concept"], "EntityVersion");
    assert_eq!(concept["version"], 1);
    assert_eq!(concept["kind"], "derived");
    assert_eq!(concept["conformance"], "./fixtures.json");

    let fields: Vec<&String> = concept["output"]
        .as_object()
        .expect("output")
        .keys()
        .collect();
    let sources = concept["output"]["source"]["enum"]
        .as_array()
        .expect("source enum");
    assert_eq!(sources, &SOURCES.map(Value::from));
    for case in fixtures_of(ID).cases {
        let output = case.output.as_object().expect("an output is an object");
        assert_eq!(output.len(), fields.len(), "{}: fields", case.name);
        assert!(
            fields.iter().all(|field| output.contains_key(*field)),
            "{}",
            case.name
        );
        assert!(sources.contains(&output["source"]), "{}", case.name);
        assert_eq!(output["source"], case.input["source"], "{}", case.name);
        let before = case.input["previous"]["number"].as_u64().unwrap_or(0);
        assert_eq!(
            output["number"],
            before + 1,
            "{}: it only goes up",
            case.name
        );

        let diff = output["diff"].as_array().expect("diff");
        let names: Vec<&str> = diff
            .iter()
            .map(|entry| entry["field"].as_str().expect("field"))
            .collect();
        assert!(
            names.windows(2).all(|pair| pair[0] < pair[1]),
            "{}: the diff is sorted by field, once each",
            case.name
        );
        assert!(
            diff.iter()
                .all(|entry| entry.as_object().map(Map::len) == Some(3)),
            "{}: an entry is {{field, old, new}}",
            case.name
        );
    }
}
