//! The runner over `SessionLifecycle@1`: it finds and loads the fixtures,
//! they agree with `concept.json`, and an implementation of the Concept —
//! the small one below, local to this test — passes them.

use platform_conformance::{fixtures_of, repository_root};
use serde_json::{Value, json};

const ID: &str = "SessionLifecycle@1";
const STATUSES: [&str; 3] = ["prep", "live", "closed"];

fn label_of(status: &str) -> &'static str {
    match status {
        "prep" => "En préparation",
        "live" => "En direct",
        "closed" => "Jouée",
        other => panic!("{other:?} is not a session status"),
    }
}

fn allowed(from: &str, to: &str) -> bool {
    matches!((from, to), ("prep", "live") | ("live", "closed"))
}

/// `SessionLifecycle@1`, as the Concept's definition words it.
fn lifecycle(input: Value) -> Value {
    let status = input["status"].as_str().expect("a status");
    let transition = input["nextStatus"]
        .as_str()
        .map(|next| allowed(status, next));
    json!({
        "label": label_of(status),
        "acceptsJournalLine": status != "closed",
        "transitionAllowed": transition,
    })
}

fn concept() -> Value {
    let path = repository_root().join("concepts/SessionLifecycle/v1/concept.json");
    let text = std::fs::read_to_string(&path).expect("concept.json");
    serde_json::from_str(&text).expect("concept.json is JSON")
}

fn case_output(from: &str, to: &str) -> Value {
    fixtures_of(ID)
        .cases
        .into_iter()
        .find(|case| case.input["status"] == from && case.input["nextStatus"] == to)
        .unwrap_or_else(|| panic!("no case for {from} to {to}"))
        .output
}

#[test]
fn conformance_the_fixtures_load_and_an_implementation_passes_them() {
    platform_conformance::check(ID, lifecycle);
}

#[test]
fn only_prep_to_live_and_live_to_closed_are_allowed() {
    for (from, to, expected) in [
        ("prep", "live", true),
        ("live", "closed", true),
        ("closed", "live", false),
        ("live", "prep", false),
        ("closed", "prep", false),
        ("prep", "closed", false),
    ] {
        assert_eq!(
            case_output(from, to)["transitionAllowed"],
            expected,
            "{from} to {to}"
        );
    }
}

#[test]
fn every_pair_of_statuses_has_a_case() {
    for from in STATUSES {
        for to in STATUSES {
            assert_eq!(
                case_output(from, to)["transitionAllowed"],
                allowed(from, to),
                "{from} to {to}"
            );
        }
    }
}

#[test]
fn a_closed_session_takes_no_journal_line_and_the_others_do() {
    let cases = fixtures_of(ID).cases;
    assert!(!cases.is_empty());
    for case in cases {
        let status = case.input["status"].as_str().expect("a status");
        assert_eq!(
            case.output["acceptsJournalLine"],
            status != "closed",
            "{}",
            case.name
        );
    }
}

#[test]
fn each_status_answers_one_stable_label() {
    let fixtures = fixtures_of(ID);
    for status in STATUSES {
        let labels: Vec<&Value> = fixtures
            .cases
            .iter()
            .filter(|case| case.input["status"] == status)
            .map(|case| &case.output["label"])
            .collect();
        assert!(!labels.is_empty(), "no case for {status}");
        assert!(
            labels.iter().all(|label| **label == label_of(status)),
            "{status}: labels differ"
        );
    }
}

#[test]
fn concept_json_points_at_the_fixtures_and_every_output_obeys_it() {
    let concept = concept();
    assert_eq!(concept["concept"], "SessionLifecycle");
    assert_eq!(concept["version"], 1);
    assert_eq!(concept["kind"], "authoritative");
    assert_eq!(concept["conformance"], "./fixtures.json");

    let fields: Vec<&String> = concept["output"]
        .as_object()
        .expect("output")
        .keys()
        .collect();
    let labels = concept["output"]["label"]["labels"]
        .as_object()
        .expect("labels");
    for status in STATUSES {
        assert_eq!(labels[status], label_of(status), "{status}");
    }
    for case in fixtures_of(ID).cases {
        let output = case.output.as_object().expect("an output is an object");
        assert_eq!(output.len(), fields.len(), "{}: fields", case.name);
        assert!(
            fields.iter().all(|field| output.contains_key(*field)),
            "{}",
            case.name
        );
    }
}
