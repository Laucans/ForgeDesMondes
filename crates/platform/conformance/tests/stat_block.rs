//! The runner over `StatBlock@1`: it finds and loads the fixtures, they
//! agree with `concept.json`, and an implementation of the Concept — the
//! small one below, local to this test — passes them.

use platform_conformance::{fixtures_of, repository_root};
use serde_json::{Value, json};

const ID: &str = "StatBlock@1";

const SCORES: [&str; 6] = [
    "strength",
    "dexterity",
    "constitution",
    "intelligence",
    "wisdom",
    "charisma",
];

/// The ten fields, in the order `incomplete` lists them.
const ORDER: [&str; 10] = [
    "strength",
    "dexterity",
    "constitution",
    "intelligence",
    "wisdom",
    "charisma",
    "armorClass",
    "hpMax",
    "actions",
    "spells",
];

fn sound_text(entry: &Value, key: &str) -> bool {
    entry[key].as_str().is_some_and(|s| !s.trim().is_empty())
}

fn sound_list(value: &Value, may_be_empty: bool) -> bool {
    value.as_array().is_some_and(|entries| {
        (may_be_empty || !entries.is_empty())
            && entries
                .iter()
                .all(|e| sound_text(e, "name") && sound_text(e, "description"))
    })
}

/// `StatBlock@1`, as the Concept's definition words it.
fn stat_block(input: Value) -> Value {
    let sound = |field: &str| match field {
        "armorClass" | "hpMax" => input[field].as_i64().is_some_and(|n| n > 0),
        "actions" => sound_list(&input[field], false),
        "spells" => sound_list(&input[field], true),
        score if SCORES.contains(&score) => {
            input[score].as_i64().is_some_and(|n| (1..=30).contains(&n))
        }
        other => panic!("{other} is not a field of a statblock"),
    };
    let incomplete: Vec<&str> = ORDER.into_iter().filter(|f| !sound(f)).collect();
    json!({"complete": incomplete.is_empty(), "incomplete": incomplete})
}

/// A shortcut an implementation could take, and what it is.
type Lax = (&'static str, fn(Value) -> Value);

fn concept() -> Value {
    let path = repository_root().join("concepts/StatBlock/v1/concept.json");
    let text = std::fs::read_to_string(&path).expect("concept.json");
    serde_json::from_str(&text).expect("concept.json is JSON")
}

fn case(name: &str) -> platform_conformance::Case {
    fixtures_of(ID)
        .cases
        .into_iter()
        .find(|case| case.name == name)
        .unwrap_or_else(|| panic!("no case named {name:?}"))
}

#[test]
fn conformance_the_fixtures_load_and_an_implementation_passes_them() {
    platform_conformance::check(ID, stat_block);
}

#[test]
fn concept_json_points_at_the_fixtures_and_every_output_obeys_it() {
    let concept = concept();
    assert_eq!(concept["concept"], "StatBlock");
    assert_eq!(concept["version"], 1);
    assert_eq!(concept["kind"], "derived");
    assert_eq!(concept["conformance"], "./fixtures.json");
    assert_eq!(concept["output"]["complete"]["type"], "boolean");
    assert_eq!(concept["output"]["incomplete"]["type"], "array");

    for case in fixtures_of(ID).cases {
        let output = case.output.as_object().expect("an output is an object");
        assert_eq!(output.len(), 2, "{}: fields", case.name);
        let incomplete = output["incomplete"].as_array().expect("a list");
        assert_eq!(
            output["complete"],
            json!(incomplete.is_empty()),
            "{}: complete says whether incomplete is empty",
            case.name
        );
        let positions: Vec<usize> = incomplete
            .iter()
            .map(|f| {
                ORDER
                    .iter()
                    .position(|o| Some(*o) == f.as_str())
                    .unwrap_or_else(|| panic!("{}: {f} is not a field", case.name))
            })
            .collect();
        assert!(
            positions.windows(2).all(|w| w[0] < w[1]),
            "{}: fields once each, in the fixed order",
            case.name
        );
    }
}

#[test]
fn the_fixtures_hold_a_complete_statblock_of_the_business_goal() {
    let complete: Vec<_> = fixtures_of(ID)
        .cases
        .into_iter()
        .filter(|case| case.output["complete"] == true)
        .collect();
    assert!(
        complete.iter().any(|case| {
            let input = &case.input;
            SCORES
                .iter()
                .all(|s| input[s].as_i64().is_some_and(|n| (1..=30).contains(&n)))
                && input["armorClass"].as_i64().is_some_and(|n| n > 0)
                && input["hpMax"].as_i64().is_some_and(|n| n > 0)
                && input["actions"][0]["name"].is_string()
                && input["actions"][0]["description"].is_string()
        }),
        "one complete statblock: 6 scores from 1 to 30, AC > 0, HP max > 0, an ability with a name and a description"
    );
}

#[test]
fn the_fixtures_hold_the_incomplete_cases_of_the_business_goal() {
    for (part, field) in [
        ("a score missing: no charisma", "charisma"),
        ("armour class 0 is not positive", "armorClass"),
        ("no action: an empty list cannot be played", "actions"),
    ] {
        let case = case(part);
        assert_eq!(case.output["complete"], false, "{}", case.name);
        assert_eq!(case.output["incomplete"], json!([field]), "{}", case.name);
    }
}

#[test]
fn the_fixtures_hold_both_ends_of_the_score_range() {
    let cases = fixtures_of(ID).cases;
    let all_at = |n: i64| {
        cases.iter().any(|case| {
            case.output["complete"] == true && SCORES.iter().all(|s| case.input[s] == n)
        })
    };
    assert!(all_at(1) && all_at(30), "1 and 30 are scores");
    for edge in [0, 31] {
        assert!(
            cases.iter().any(|case| {
                SCORES
                    .iter()
                    .any(|s| case.input[s] == edge && case.output["incomplete"] == json!([s]))
            }),
            "{edge} is not a score"
        );
    }
}

#[test]
fn a_lax_implementation_fails_the_fixtures() {
    // Each shortcut an implementation could take is caught by a case.
    let lax: [Lax; 7] = [
        ("an empty action list is accepted", |mut input| {
            if input["actions"] == json!([]) {
                input["actions"] = json!([{"name": "x", "description": "y"}]);
            }
            stat_block(input)
        }),
        ("a blank text is accepted", |mut input| {
            for key in ["actions", "spells"] {
                if let Some(entries) = input[key].as_array_mut() {
                    for e in entries {
                        for k in ["name", "description"] {
                            if e[k].as_str().is_some_and(|s| s.trim().is_empty()) {
                                e[k] = json!("x");
                            }
                        }
                    }
                }
            }
            stat_block(input)
        }),
        ("a score is not range-checked", |mut input| {
            for s in SCORES {
                if input[s].as_i64().is_some() {
                    input[s] = json!(10);
                }
            }
            stat_block(input)
        }),
        ("only the first entry of a list is checked", |mut input| {
            for key in ["actions", "spells"] {
                if let Some(entries) = input[key].as_array_mut() {
                    entries.truncate(1);
                }
            }
            stat_block(input)
        }),
        ("a spell needs no description", |mut input| {
            if let Some(spells) = input["spells"].as_array_mut() {
                for spell in spells.iter_mut().filter(|s| s.is_object()) {
                    spell["description"] = json!("x");
                }
            }
            stat_block(input)
        }),
        ("an HP max of 0 is accepted", |mut input| {
            if input["hpMax"] == 0 {
                input["hpMax"] = json!(1);
            }
            stat_block(input)
        }),
        ("AC 0 is accepted", |mut input| {
            if input["armorClass"] == 0 {
                input["armorClass"] = json!(1);
            }
            stat_block(input)
        }),
    ];
    let fixtures = fixtures_of(ID);
    for (label, implementation) in lax {
        assert!(
            !platform_conformance::failures(&fixtures, implementation).is_empty(),
            "{label}: the fixtures must refuse it"
        );
    }
}
