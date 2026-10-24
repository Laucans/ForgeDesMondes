//! The runner over `TokenAppearance@1`: it finds and loads the fixtures,
//! they agree with `concept.json`, and an implementation of the Concept —
//! the small one below, local to this test — passes them.

use platform_conformance::{fixtures_of, repository_root};
use serde_json::{Value, json};

const ID: &str = "TokenAppearance@1";

/// A field that is given: present and not null.
fn given<'a>(input: &'a Value, field: &str) -> Option<&'a Value> {
    input.get(field).filter(|value| !value.is_null())
}

/// The first character of the trimmed name, in capitals.
fn initial(name: &str) -> String {
    let letter = name.trim_start().chars().next().expect("a name");
    letter.to_uppercase().to_string()
}

/// `TokenAppearance@1`, as the Concept's definition words it.
fn appearance(input: &Value) -> Value {
    if let Some(media) = given(input, "token_media_id") {
        json!({"source": "token_image", "media_id": media, "crop": null, "color": null, "initial": null})
    } else if let Some(media) = given(input, "portrait_media_id") {
        json!({"source": "portrait", "media_id": media, "crop": input.get("token_crop").unwrap_or(&Value::Null), "color": null, "initial": null})
    } else {
        let name = input["name"].as_str().expect("`name` is a string");
        json!({"source": "disc", "media_id": null, "crop": null, "color": input["token_color"], "initial": initial(name)})
    }
}

fn concept() -> Value {
    let path = repository_root().join("concepts/TokenAppearance/v1/concept.json");
    let text = std::fs::read_to_string(&path).expect("concept.json");
    serde_json::from_str(&text).expect("concept.json is JSON")
}

fn sources() -> Vec<String> {
    fixtures_of(ID)
        .cases
        .iter()
        .map(|case| {
            case.output["source"]
                .as_str()
                .expect("a source")
                .to_string()
        })
        .collect()
}

#[test]
fn conformance_the_fixtures_load_and_an_implementation_passes_them() {
    platform_conformance::check(ID, |input| appearance(&input));
}

#[test]
fn each_source_has_an_example() {
    let sources = sources();
    for source in ["token_image", "portrait", "disc"] {
        assert!(sources.iter().any(|s| s == source), "no case for {source}");
    }
}

#[test]
fn the_fixtures_cover_the_precedence_and_the_initials() {
    let cases = fixtures_of(ID).cases;
    let wins_over_portrait = |case: &&platform_conformance::Case| {
        given(&case.input, "token_media_id").is_some()
            && given(&case.input, "portrait_media_id").is_some()
    };
    assert!(
        cases.iter().filter(wins_over_portrait).count() >= 1,
        "image over portrait"
    );
    assert!(
        cases
            .iter()
            .any(|c| c.output["source"] == "portrait" && c.output["crop"].is_null()),
        "an uncropped portrait"
    );
    assert!(
        cases
            .iter()
            .any(|c| c.output["source"] == "portrait" && c.output["crop"].is_object()),
        "a cropped portrait"
    );
    let initial_of = |name: &str| {
        cases
            .iter()
            .find(|c| c.input["name"] == name)
            .unwrap_or_else(|| panic!("no case for {name}"))
            .output["initial"]
            .clone()
    };
    assert_eq!(initial_of("ondine"), "O", "lowercase");
    assert_eq!(initial_of("élowen"), "É", "accented");
}

#[test]
fn an_implementation_that_prefers_the_portrait_fails_the_fixtures() {
    let portrait_first = |input: Value| {
        if given(&input, "portrait_media_id").is_some() {
            let mut output = appearance(
                &json!({"portrait_media_id": input["portrait_media_id"], "token_crop": input["token_crop"], "name": "x", "token_color": "x"}),
            );
            output["media_id"] = input["portrait_media_id"].clone();
            output
        } else {
            appearance(&input)
        }
    };
    let failed = platform_conformance::failures(&fixtures_of(ID), portrait_first);
    assert!(
        failed.iter().any(|f| f.contains("wins over")),
        "a token image beats the portrait: {failed:?}"
    );
}

#[test]
fn an_implementation_that_drops_the_accent_fails_the_fixtures() {
    // The first plain letter of the name: « élowen » gives « L ».
    let ascii = |input: Value| {
        let mut output = appearance(&input);
        if output["source"] == "disc" {
            let name = input["name"].as_str().expect("name");
            let letter = name
                .chars()
                .find(char::is_ascii_alphabetic)
                .expect("a letter");
            output["initial"] = json!(letter.to_uppercase().to_string());
        }
        output
    };
    let failed = platform_conformance::failures(&fixtures_of(ID), ascii);
    assert!(
        failed.iter().any(|f| f.contains("accent")),
        "the accent is part of the initial: {failed:?}"
    );
}

#[test]
fn concept_json_points_at_the_fixtures_and_every_output_obeys_it() {
    let concept = concept();
    assert_eq!(concept["concept"], "TokenAppearance");
    assert_eq!(concept["version"], 1);
    assert_eq!(concept["kind"], "derived");
    assert_eq!(concept["conformance"], "./fixtures.json");
    let declared: Vec<&String> = concept["output"]
        .as_object()
        .expect("output")
        .keys()
        .collect();

    for case in fixtures_of(ID).cases {
        let output = case.output.as_object().expect("an output is an object");
        assert_eq!(output.len(), declared.len(), "{}: fields", case.name);
        for field in &declared {
            assert!(output.contains_key(*field), "{}: {field}", case.name);
        }
        match output["source"].as_str().expect("source") {
            "token_image" => assert!(
                output["media_id"].is_string()
                    && output["crop"].is_null()
                    && output["initial"].is_null(),
                "{}",
                case.name
            ),
            "portrait" => assert!(
                output["media_id"].is_string()
                    && output["initial"].is_null()
                    && output["color"].is_null(),
                "{}",
                case.name
            ),
            "disc" => assert!(
                output["media_id"].is_null()
                    && output["crop"].is_null()
                    && output["color"].is_string()
                    && output["initial"].is_string(),
                "{}",
                case.name
            ),
            other => panic!("{}: unknown source {other}", case.name),
        }
    }
}
