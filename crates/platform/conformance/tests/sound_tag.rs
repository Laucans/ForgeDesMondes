//! The runner over `SoundTag@1`: it finds and loads the fixtures, they agree
//! with `concept.json`, and an implementation of the Concept — the small one
//! below, local to this test — passes them.

use platform_conformance::{fixtures_of, repository_root};
use serde_json::{Value, json};

const ID: &str = "SoundTag@1";

/// A list of tags, normalised: trimmed, lowercased, empties dropped, first occurrence kept.
fn normalise(tags: &Value) -> Vec<String> {
    let mut seen: Vec<String> = Vec::new();
    // An absent filter is an empty one; anything else must be a list of strings.
    let list = if tags.is_null() {
        &Vec::new()
    } else {
        tags.as_array().expect("a list of tags")
    };
    for tag in list {
        let tag = tag
            .as_str()
            .expect("a tag is a string")
            .trim()
            .to_lowercase();
        if !tag.is_empty() && !seen.contains(&tag) {
            seen.push(tag);
        }
    }
    seen
}

/// `SoundTag@1`, as the Concept's definition words it.
fn sound_tag(input: Value) -> Value {
    let tags = normalise(&input["tags"]);
    let filter = normalise(&input["filter"]);
    let matches = filter.is_empty() || filter.iter().any(|tag| tags.contains(tag));
    json!({"tags": tags, "matches": matches})
}

fn concept() -> Value {
    let path = repository_root().join("concepts/SoundTag/v1/concept.json");
    let text = std::fs::read_to_string(&path).expect("concept.json");
    serde_json::from_str(&text).expect("concept.json is JSON")
}

#[test]
fn conformance_the_fixtures_load_and_an_implementation_passes_them() {
    platform_conformance::check(ID, sound_tag);
}

#[test]
fn the_example_of_the_task_normalises_to_two_tags() {
    let case = fixtures_of(ID)
        .cases
        .into_iter()
        .find(|case| case.input["tags"] == json!(["Taverne", "taverne ", "Intérieur"]))
        .expect("the case of the task");
    assert_eq!(case.output["tags"], json!(["taverne", "intérieur"]));
}

#[test]
fn the_fixtures_cover_drops_duplicates_accents_and_both_filter_answers() {
    let cases = fixtures_of(ID).cases;
    let len = |value: &Value| value.as_array().expect("a list").len();
    let blank = |tag: &Value| tag.as_str().expect("a tag").trim().is_empty();
    let distinct = |tags: &Value| normalise(tags).len();

    // An empty or whitespace-only tag is dropped.
    assert!(cases.iter().any(|c| {
        c.input["tags"]
            .as_array()
            .is_some_and(|t| t.iter().any(blank))
    }));
    // A tag repeated apart from case and spacing is dropped.
    assert!(cases.iter().any(|c| {
        let kept = len(&c.output["tags"]);
        let non_blank = c.input["tags"]
            .as_array()
            .expect("tags")
            .iter()
            .filter(|t| !blank(t))
            .count();
        kept < non_blank
    }));
    // An accent survives.
    assert!(cases.iter().any(|c| {
        c.output["tags"]
            .as_array()
            .expect("tags")
            .iter()
            .any(|t| !t.as_str().expect("a tag").is_ascii())
    }));
    // The filter answers both ways, and a non-empty filter keeps a sound.
    assert!(
        cases
            .iter()
            .any(|c| c.output["matches"] == true && distinct(&c.input["filter"]) > 0)
    );
    assert!(cases.iter().any(|c| c.output["matches"] == false));
}

#[test]
fn concept_json_points_at_the_fixtures_and_every_output_obeys_it() {
    let concept = concept();
    assert_eq!(concept["concept"], "SoundTag");
    assert_eq!(concept["version"], 1);
    assert_eq!(concept["kind"], "derived");
    assert_eq!(concept["conformance"], "./fixtures.json");

    let fields: Vec<&String> = concept["output"]
        .as_object()
        .expect("output")
        .keys()
        .collect();
    for case in fixtures_of(ID).cases {
        let output = case.output.as_object().expect("an output is an object");
        assert_eq!(output.len(), fields.len(), "{}: fields", case.name);
        assert!(
            fields.iter().all(|field| output.contains_key(*field)),
            "{}",
            case.name
        );
        assert!(output["matches"].is_boolean(), "{}", case.name);
        let tags = output["tags"].as_array().expect("tags");
        let strings: Vec<&str> = tags.iter().map(|t| t.as_str().expect("a tag")).collect();
        for (i, tag) in strings.iter().enumerate() {
            assert!(!tag.is_empty(), "{}: empty tag", case.name);
            assert_eq!(*tag, tag.trim().to_lowercase(), "{}: {tag}", case.name);
            assert!(!strings[..i].contains(tag), "{}: {tag} twice", case.name);
        }
    }
}
