//! The Concept an addition is held to, against its fixtures: the tags it
//! writes are SoundTag@1's `tags`. Its `matches` is the tag filter's, which
//! the library's reader answers, never an addition.

use add_sound::normalized;
use serde_json::Value;

#[test]
fn conformance_sound_tag() {
    let fixtures = platform_conformance::fixtures_of("SoundTag@1");
    assert!(!fixtures.cases.is_empty(), "SoundTag@1 has no case");
    let failed: Vec<String> = fixtures
        .cases
        .iter()
        .filter_map(|case| {
            let given = case.input["tags"].as_array().expect("tags");
            let tags = normalized(given.iter().map(|tag| tag.as_str().expect("a tag")));
            let got = Value::from(tags);
            (got != case.output["tags"])
                .then(|| format!("{}: expected {}, got {got}", case.name, case.output["tags"]))
        })
        .collect();
    assert!(
        failed.is_empty(),
        "SoundTag@1:\n  - {}",
        failed.join("\n  - ")
    );
}
