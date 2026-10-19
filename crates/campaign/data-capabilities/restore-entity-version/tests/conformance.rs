//! The Concept a restoration is held to, against its fixtures: the version it
//! adds (EntityVersion@1).

use restore_entity_version::diff;
use serde_json::{Map, Value, json};

/// EntityVersion@1's output for a case's input, through `diff`.
fn version(input: &Value) -> Value {
    let empty = Map::new();
    let previous = &input["previous"];
    let snapshot = input["snapshot"].as_object().expect("a snapshot");
    let before = previous["snapshot"].as_object().unwrap_or(&empty);
    let number = previous["number"].as_u64().map_or(1, |n| n + 1);
    json!({
        "number": number,
        "source": input["source"],
        "diff": diff(before, snapshot),
    })
}

#[test]
fn conformance_entity_version() {
    platform_conformance::check("EntityVersion@1", |input| version(&input));
}
