//! The tags an addition writes are `SoundTag@1`'s `tags`
//! (`concepts/SoundTag/v1/fixtures.json`): every case's normalised list is
//! what `add_sound::normalized` answers. The fixtures' `matches` is the tag
//! filter's, which a reader computes, not this unit.

use add_sound::normalized;
use serde_json::Value;

#[test]
fn conformance_the_tags_are_sound_tag_v1() {
    let fixtures = platform_conformance::fixtures_of("SoundTag@1");
    assert!(!fixtures.cases.is_empty(), "SoundTag@1 has no case");
    let failed: Vec<String> = fixtures
        .cases
        .iter()
        .filter_map(|case| {
            let tags = case.input["tags"]
                .as_array()
                .expect("a case's tags")
                .iter()
                .map(|tag| tag.as_str().expect("a tag"));
            let got = Value::from(normalized(tags));
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
