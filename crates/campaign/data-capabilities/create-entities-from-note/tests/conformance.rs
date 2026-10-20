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
