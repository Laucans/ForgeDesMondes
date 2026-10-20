//! `normalized` held to the fixtures of the Concept it implements
//! (concepts/AliasMatch/v1/fixtures.json).

use extract_from_notes::normalized;
use serde_json::json;

#[test]
fn conformance_alias_match() {
    platform_conformance::check("AliasMatch@1", |input| {
        let term = normalized(input["term"].as_str().expect("a term"));
        let matched = input["other"]
            .as_str()
            .map(|other| !term.is_empty() && term == normalized(other));
        json!({ "normalized_term": term, "match": matched })
    });
}
