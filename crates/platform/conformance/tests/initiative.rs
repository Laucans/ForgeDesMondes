//! The runner over `Initiative@1`: the fixtures load and agree with
//! `concept.json`, an implementation of the Concept — the small one below,
//! local to this test — passes them, and the usual wrong readings of the
//! Concept do not.

use std::cmp::Ordering;

use platform_conformance::{check, failures, fixtures_of, repository_root};
use serde_json::{Value, json};

const ID: &str = "Initiative@1";

/// `floor((score - 10) / 2)`: the Concept restates the DEX modifier, it imports none.
fn dex_modifier(score: i64) -> i64 {
    (score - 10).div_euclid(2)
}

fn int(value: &Value) -> i64 {
    value.as_i64().expect("an integer")
}

fn initiative_of(input: &Value) -> Value {
    if input["kind"] == "pc" {
        json!({"roll": null, "dexModifier": null, "initiative": input["enteredInitiative"]})
    } else {
        let modifier = dex_modifier(int(&input["dex"]));
        json!({
            "roll": input["roll"],
            "dexModifier": modifier,
            "initiative": int(&input["roll"]) + modifier,
        })
    }
}

/// Initiative descending, then DEX score descending (none last), then draw
/// descending; the sort is stable, so a full tie keeps the given order.
fn rank(a: &Value, b: &Value) -> Ordering {
    int(&b["initiative"])
        .cmp(&int(&a["initiative"]))
        .then_with(|| b["dex"].as_i64().cmp(&a["dex"].as_i64()))
        .then_with(|| int(&b["draw"]).cmp(&int(&a["draw"])))
}

fn order_of(input: &Value) -> Value {
    let mut participants = input["participants"]
        .as_array()
        .expect("participants")
        .clone();
    participants.sort_by(rank);
    json!({"order": participants.iter().map(|p| p["id"].clone()).collect::<Vec<_>>()})
}

/// The next living participant after `current`, wrapping; a wrap is a round.
fn next_of(input: &Value) -> Value {
    let participants = input["participants"].as_array().expect("participants");
    let at = participants
        .iter()
        .position(|p| p["id"] == input["current"])
        .expect("the current participant is in the order");
    let round = int(&input["round"]);
    (1..=participants.len())
        .map(|step| (at + step, &participants[(at + step) % participants.len()]))
        .find(|(_, p)| p["alive"] != false)
        .map(|(position, p)| {
            let wrapped = i64::from(position >= participants.len());
            json!({"current": p["id"], "round": round + wrapped})
        })
        .expect("a combat has a living participant")
}

/// `Initiative@1`, as the Concept's definition words it.
fn compute(input: Value) -> Value {
    match input["operation"].as_str().expect("operation") {
        "initiative" => initiative_of(&input),
        "order" => order_of(&input),
        "next" => next_of(&input),
        other => panic!("unknown operation {other}"),
    }
}

fn concept() -> Value {
    let path = repository_root().join("concepts/Initiative/v1/concept.json");
    let text = std::fs::read_to_string(&path).expect("concept.json");
    serde_json::from_str(&text).expect("concept.json is JSON")
}

fn cases_of(operation: &str) -> Vec<platform_conformance::Case> {
    fixtures_of(ID)
        .cases
        .into_iter()
        .filter(|case| case.input["operation"] == operation)
        .collect()
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
    check(ID, compute);
}

#[test]
fn an_npc_with_dex_16_rolling_11_has_initiative_14() {
    let case = case("an NPC with DEX 16 rolling 11 has initiative 14 (11 + 3)");
    assert_eq!(case.input["dex"], 16);
    assert_eq!(case.input["roll"], 11);
    assert_eq!(case.output["initiative"], 14);
}

#[test]
fn every_npc_initiative_is_its_roll_plus_its_modifier_and_a_pcs_has_no_roll() {
    let cases = cases_of("initiative");
    for case in &cases {
        let output = &case.output;
        if case.input["kind"] == "pc" {
            assert!(output["roll"].is_null(), "{}", case.name);
            assert!(output["dexModifier"].is_null(), "{}", case.name);
            assert_eq!(output["initiative"], case.input["enteredInitiative"]);
        } else {
            let roll = int(&output["roll"]);
            assert!((1..=20).contains(&roll), "{}: roll {roll}", case.name);
            // Floored division written another way than the implementation's.
            #[allow(clippy::cast_possible_truncation)]
            let modifier = ((int(&case.input["dex"]) - 10) as f64 / 2.0).floor() as i64;
            assert_eq!(int(&output["dexModifier"]), modifier, "{}", case.name);
            assert_eq!(int(&output["initiative"]), roll + modifier, "{}", case.name);
        }
    }
    assert!(cases.iter().any(|case| case.input["kind"] == "pc"));
    assert!(
        cases
            .iter()
            .any(|case| case.output["dexModifier"].as_i64().is_some_and(|m| m < 0))
    );
}

#[test]
fn the_two_npcs_and_the_pc_at_14_are_ranked_dex_first_then_by_the_draw() {
    let order = |name: &str| {
        let case = case(name);
        let tied = case.input["participants"].as_array().expect("participants");
        assert!(tied.iter().all(|p| p["initiative"] == 14));
        case.output["order"].clone()
    };
    let first =
        order("R105: at 14 all round, DEX 16 comes first and an equal DEX goes to the higher draw");
    let swapped = order("R105: the same tie with the draws swapped goes the other way");
    assert_eq!(first[0], "npc-dex-16");
    assert_eq!(swapped[0], "npc-dex-16");
    // The DEX 12 pair: the draw settles them, and swapping the draws swaps them.
    assert_eq!(first[1], "npc-dex-12");
    assert_eq!(swapped[1], "pc-entered");
}

#[test]
fn suivant_skips_the_dead_second_and_wraps_to_the_first_with_a_new_round() {
    let from_first =
        case("« Suivant » from the first goes to the next living one, the dead second skipped");
    assert_eq!(from_first.output, json!({"current": "third", "round": 1}));
    let from_last = case("« Suivant » from the last goes back to the first and adds a round");
    assert_eq!(from_last.output, json!({"current": "first", "round": 2}));
    for case in [from_first, from_last] {
        let alive: Vec<&Value> = case.input["participants"]
            .as_array()
            .expect("participants")
            .iter()
            .map(|p| &p["alive"])
            .collect();
        assert_eq!(alive, [true, false, true], "{}", case.name);
    }
}

#[test]
fn the_fixtures_cover_each_operation_and_a_turn_with_and_without_a_new_round() {
    for operation in ["initiative", "order", "next"] {
        assert!(!cases_of(operation).is_empty(), "no {operation} case");
    }
    let rounds: Vec<(i64, i64)> = cases_of("next")
        .iter()
        .map(|case| (int(&case.input["round"]), int(&case.output["round"])))
        .collect();
    assert!(rounds.iter().any(|(before, after)| after == before));
    assert!(rounds.iter().any(|(before, after)| *after == before + 1));
}

#[test]
fn concept_json_points_at_the_fixtures_and_documents_every_output_field() {
    let concept = concept();
    assert_eq!(concept["concept"], "Initiative");
    assert_eq!(concept["version"], 1);
    assert_eq!(concept["kind"], "derived");
    assert_eq!(concept["conformance"], "./fixtures.json");

    let documented = concept["output"].as_object().expect("output");
    for case in fixtures_of(ID).cases {
        for field in case
            .output
            .as_object()
            .expect("an output is an object")
            .keys()
        {
            assert!(documented.contains_key(field), "{}: {field}", case.name);
        }
    }
}

/// A wrong reading of the Concept must fail some case: the fixtures
/// constrain it, they do not merely accept the right answer.
fn fails_with(wrong: impl Fn(Value) -> Value) -> usize {
    failures(&fixtures_of(ID), wrong).len()
}

#[test]
fn a_modifier_truncated_toward_zero_fails_the_fixtures() {
    let truncating = |input: Value| {
        if input["operation"] == "initiative" && input["kind"] == "npc" {
            let modifier = (int(&input["dex"]) - 10) / 2;
            json!({
                "roll": input["roll"],
                "dexModifier": modifier,
                "initiative": int(&input["roll"]) + modifier,
            })
        } else {
            compute(input)
        }
    };
    assert!(fails_with(truncating) > 0);
}

#[test]
fn a_tie_break_that_skips_the_dex_or_the_draw_fails_the_fixtures() {
    let ranked_by = |keys: fn(&Value, &Value) -> Ordering| {
        move |input: Value| {
            if input["operation"] != "order" {
                return compute(input);
            }
            let mut participants = input["participants"]
                .as_array()
                .expect("participants")
                .clone();
            participants.sort_by(keys);
            json!({"order": participants.iter().map(|p| p["id"].clone()).collect::<Vec<_>>()})
        }
    };
    let without_dex = |a: &Value, b: &Value| {
        int(&b["initiative"])
            .cmp(&int(&a["initiative"]))
            .then_with(|| int(&b["draw"]).cmp(&int(&a["draw"])))
    };
    let without_draw = |a: &Value, b: &Value| {
        int(&b["initiative"])
            .cmp(&int(&a["initiative"]))
            .then_with(|| b["dex"].as_i64().cmp(&a["dex"].as_i64()))
    };
    assert!(fails_with(ranked_by(without_dex)) > 0);
    assert!(fails_with(ranked_by(without_draw)) > 0);
}

#[test]
fn a_suivant_that_misses_the_round_or_does_not_skip_the_dead_fails_the_fixtures() {
    let never_a_new_round = |input: Value| {
        let mut answer = compute(input.clone());
        if input["operation"] == "next" {
            answer["round"] = input["round"].clone();
        }
        answer
    };
    let dead_included = |input: Value| {
        if input["operation"] != "next" {
            return compute(input);
        }
        let mut everyone_alive = input;
        for participant in everyone_alive["participants"]
            .as_array_mut()
            .expect("participants")
        {
            participant["alive"] = json!(true);
        }
        compute(everyone_alive)
    };
    assert!(fails_with(never_a_new_round) > 0);
    assert!(fails_with(dead_included) > 0);
}

#[test]
fn an_order_that_drops_the_dead_or_sends_the_hidden_last_fails_the_fixtures() {
    let without_the_dead = |input: Value| {
        let mut answer = compute(input.clone());
        if input["operation"] == "order" {
            let dead: Vec<Value> = input["participants"]
                .as_array()
                .expect("participants")
                .iter()
                .filter(|p| p["alive"] == false)
                .map(|p| p["id"].clone())
                .collect();
            answer["order"]
                .as_array_mut()
                .expect("order")
                .retain(|id| !dead.contains(id));
        }
        answer
    };
    let hidden_last = |input: Value| {
        let mut answer = compute(input.clone());
        if input["operation"] == "order" {
            let hidden: Vec<Value> = input["participants"]
                .as_array()
                .expect("participants")
                .iter()
                .filter(|p| p["hidden"] == true)
                .map(|p| p["id"].clone())
                .collect();
            let order = answer["order"].as_array_mut().expect("order");
            order.sort_by_key(|id| hidden.contains(id));
        }
        answer
    };
    assert!(fails_with(without_the_dead) > 0);
    assert!(fails_with(hidden_last) > 0);
}

#[test]
fn a_round_counted_on_landing_on_the_first_rank_instead_of_on_the_wrap_fails_the_fixtures() {
    let round_on_landing_first = |input: Value| {
        let mut answer = compute(input.clone());
        if input["operation"] == "next" {
            let first = &input["participants"][0]["id"];
            let landed_first = answer["current"] == *first;
            answer["round"] = json!(int(&input["round"]) + i64::from(landed_first));
        }
        answer
    };
    assert!(fails_with(round_on_landing_first) > 0);
}

#[test]
fn an_unstable_tie_break_by_id_fails_the_fixtures() {
    let tie_by_id = |input: Value| {
        if input["operation"] != "order" {
            return compute(input);
        }
        let mut participants = input["participants"]
            .as_array()
            .expect("participants")
            .clone();
        participants
            .sort_by(|a, b| rank(a, b).then_with(|| a["id"].as_str().cmp(&b["id"].as_str())));
        json!({"order": participants.iter().map(|p| p["id"].clone()).collect::<Vec<_>>()})
    };
    assert!(fails_with(tie_by_id) > 0);
}
