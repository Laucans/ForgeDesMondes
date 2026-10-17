//! The runner over `ProviderApiType@1`: it finds and loads the fixtures,
//! they agree with `concept.json`, and an implementation of the Concept —
//! the small one below, local to this test — passes them.

use platform_conformance::{fixtures_of, repository_root};
use serde_json::{Value, json};

const ID: &str = "ProviderApiType@1";
const UNRECOGNISED: &str = "type d'API non reconnu";

/// The host of an absolute http(s) URL, lowercased, without userinfo or port.
fn host_of(url: &str) -> Option<String> {
    let (scheme, rest) = url.split_once("://")?;
    if !scheme.eq_ignore_ascii_case("http") && !scheme.eq_ignore_ascii_case("https") {
        return None;
    }
    let authority = rest.split(['/', '?', '#']).next()?;
    let host_port = authority.rsplit('@').next()?;
    let host = if host_port.starts_with('[') {
        &host_port[..=host_port.find(']')?]
    } else {
        host_port
            .rsplit_once(':')
            .map_or(host_port, |(host, _)| host)
    };
    (!host.is_empty()).then(|| host.to_ascii_lowercase())
}

fn is_openai_model_list(answer: &Value) -> bool {
    answer["object"] == "list" && answer["data"].is_array()
}

/// `ProviderApiType@1`, as the Concept's definition words it.
fn classify(input: Value) -> Value {
    let kind = host_of(input["url"].as_str().unwrap_or_default()).and_then(|host| {
        if host == "api.anthropic.com" {
            Some("cloud")
        } else {
            is_openai_model_list(&input["answer"]).then_some("opencode")
        }
    });
    match kind {
        Some(kind) => json!({"type": kind, "recognized": true, "message": null}),
        None => json!({"type": null, "recognized": false, "message": UNRECOGNISED}),
    }
}

fn concept() -> Value {
    let path = repository_root().join("concepts/ProviderApiType/v1/concept.json");
    let text = std::fs::read_to_string(&path).expect("concept.json");
    serde_json::from_str(&text).expect("concept.json is JSON")
}

#[test]
fn conformance_the_fixtures_load_and_an_implementation_passes_them() {
    platform_conformance::check(ID, classify);
}

#[test]
fn the_three_urls_of_r92_are_in_the_fixtures_with_their_types() {
    let fixtures = fixtures_of(ID);
    let outcome = |url: &str| {
        let case = fixtures
            .cases
            .iter()
            .find(|case| case.input["url"] == url)
            .unwrap_or_else(|| panic!("no case for {url}"));
        case.output.clone()
    };
    assert_eq!(outcome("https://api.anthropic.com/v1")["type"], "cloud");
    assert_eq!(outcome("http://192.168.1.40:4096")["type"], "opencode");
    let unknown = outcome("https://inconnu.example.org/pas-une-api");
    assert_eq!(unknown["recognized"], false);
    assert_eq!(unknown["message"], UNRECOGNISED);
}

#[test]
fn the_fixtures_cover_both_types_and_the_unrecognised() {
    let outputs: Vec<Value> = fixtures_of(ID)
        .cases
        .into_iter()
        .map(|case| case.output)
        .collect();
    for wanted in [json!("cloud"), json!("opencode"), Value::Null] {
        assert!(
            outputs.iter().any(|output| output["type"] == wanted),
            "no case answers type {wanted}"
        );
    }
}

#[test]
fn concept_json_points_at_the_fixtures_and_every_output_obeys_it() {
    let concept = concept();
    assert_eq!(concept["concept"], "ProviderApiType");
    assert_eq!(concept["version"], 1);
    assert_eq!(concept["kind"], "derived");
    assert_eq!(concept["conformance"], "./fixtures.json");

    let fields: Vec<&String> = concept["output"]
        .as_object()
        .expect("output")
        .keys()
        .collect();
    let types = concept["output"]["type"]["enum"]
        .as_array()
        .expect("type enum");
    for case in fixtures_of(ID).cases {
        let output = case.output.as_object().expect("an output is an object");
        assert_eq!(output.len(), fields.len(), "{}: fields", case.name);
        assert!(
            fields.iter().all(|field| output.contains_key(*field)),
            "{}",
            case.name
        );

        let recognized = output["recognized"].as_bool().expect("recognized");
        assert_eq!(output["type"].is_null(), !recognized, "{}", case.name);
        assert_eq!(output["message"].is_null(), recognized, "{}", case.name);
        if recognized {
            assert!(types.contains(&output["type"]), "{}", case.name);
        } else {
            assert_eq!(output["message"], UNRECOGNISED, "{}", case.name);
        }
    }
}
