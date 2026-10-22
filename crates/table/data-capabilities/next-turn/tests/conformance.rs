//! `table.nextTurn` held to Initiative@1's fixtures: the cases of its `next`
//! operation, « Suivant ». Its `initiative` and `order` cases are a start's
//! (`table.startCombat`): « Suivant » never ranks anyone.

use next_turn::{Combat, Participant, prepare};
use platform_conformance::{Fixtures, failures, fixtures_of};
use serde_json::{Value, json};

const COMBAT: &str = "c0000000-0000-4000-8000-000000000001";

/// Initiative@1's cases of « Suivant ».
fn of_next() -> Fixtures {
    let mut fixtures = fixtures_of("Initiative@1");
    fixtures
        .cases
        .retain(|case| case.input["operation"] == "next");
    fixtures
}

/// A case's answer, as « Suivant » computes it: the participants, given in
/// rank order, ranked 1 to n; the round, the case's plus the step written.
fn computed(input: &Value) -> Value {
    let participants: Vec<Participant> = input["participants"]
        .as_array()
        .expect("participants")
        .iter()
        .zip(1..)
        .map(|(p, rank)| Participant {
            entry: p["id"].as_str().expect("an id").to_owned(),
            rank,
            alive: p["alive"].as_bool().unwrap_or(true),
        })
        .collect();
    let round = input["round"].as_i64().expect("a round");
    let combat = Combat {
        id: COMBAT.to_owned(),
        current: input["current"].as_str().map(str::to_owned),
    };
    let prepared = prepare(Some(&combat), &participants).expect("a turn");
    let step = prepared.payload["round"].as_i64().expect("a step");
    json!({ "current": prepared.payload["currentEntryId"], "round": round + step })
}

#[test]
fn conformance_next_of_initiative_1() {
    let fixtures = of_next();
    assert!(fixtures.cases.len() >= 2, "Initiative@1 has next cases");
    let failed = failures(&fixtures, |input| computed(&input));
    assert!(
        failed.is_empty(),
        "Initiative@1: {} case(s) fail:\n  - {}",
        failed.len(),
        failed.join("\n  - ")
    );
}
