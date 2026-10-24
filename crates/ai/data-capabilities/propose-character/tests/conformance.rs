//! `statblock` held to the fixtures of the Concept it implements
//! (concepts/StatBlock/v1/fixtures.json).

use propose_character::statblock;
use serde_json::json;

#[test]
fn conformance_stat_block() {
    platform_conformance::check("StatBlock@1", |input| {
        let incomplete = statblock(input.as_object().expect("a sheet"));
        json!({ "complete": incomplete.is_empty(), "incomplete": incomplete })
    });
}
