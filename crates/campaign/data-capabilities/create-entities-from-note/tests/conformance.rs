//! The Concept the first version of an element is held to, against its
//! fixtures: EntityVersion@1.

use create_entities_from_note::version;
use serde_json::{Value, json};

/// EntityVersion@1's output for a case's input, through `version`.
fn entity_version(input: &Value) -> Value {
    let previous = &input["previous"];
    let snapshot = input["snapshot"].as_object().expect("a snapshot").clone();
    let before = previous["number"]
        .as_u64()
        .zip(previous["snapshot"].as_object());
    let version = version(before, snapshot);
    json!({
        "number": version.number,
        "source": input["source"],
        "diff": version.diff,
    })
}

#[test]
fn conformance_entity_version() {
    platform_conformance::check("EntityVersion@1", |input| entity_version(&input));
}

/// Numbers are compared by value inside arrays and objects too, and an
/// object holding one more key is another value.
#[test]
fn a_diff_compares_numbers_by_value_inside_arrays_and_objects() {
    let snapshot = |value: Value| value.as_object().expect("a snapshot").clone();
    let previous = snapshot(json!({ "spells": [15], "tokenCrop": { "x": 15, "y": 2 } }));
    let same = version(
        Some((1, &previous)),
        snapshot(json!({ "spells": [15.0], "tokenCrop": { "x": 15.0, "y": 2 } })),
    );
    assert_eq!(same.number, 2);
    assert_eq!(same.diff, Vec::<Value>::new());
    let wider = version(
        Some((1, &previous)),
        snapshot(json!({ "spells": [15, 16], "tokenCrop": { "x": 15, "y": 2, "z": 0 } })),
    );
    let fields: Vec<&Value> = wider.diff.iter().map(|change| &change["field"]).collect();
    assert_eq!(fields, ["spells", "tokenCrop"]);
}
