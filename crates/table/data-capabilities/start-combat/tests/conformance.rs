//! `table.startCombat` held to Initiative@1's fixtures: the cases of its
//! `initiative` and `order` operations, which a start computes. Its `next`
//! cases are « Suivant »'s (`table.nextTurn`): a start never moves the turn.

use platform_conformance::{Fixtures, failures, fixtures_of};
use serde_json::{Value, json};
use start_combat::{Standing, initiative, modifier, ranked};

/// Initiative@1's cases of a start's operations.
fn of_a_start() -> Fixtures {
    let mut fixtures = fixtures_of("Initiative@1");
    fixtures.cases.retain(|case| {
        ["initiative", "order"].contains(&case.input["operation"].as_str().unwrap_or(""))
    });
    fixtures
}

/// A case's answer, as the start computes it.
fn computed(input: &Value) -> Value {
    match input["operation"].as_str() {
        Some("initiative") if input["kind"] == "npc" => {
            let (roll, dex) = (input["roll"].as_i64(), input["dex"].as_i64());
            let (roll, dex) = (roll.expect("a roll"), dex.expect("a DEX"));
            json!({ "roll": roll, "dexModifier": modifier(dex), "initiative": initiative(roll, dex) })
        }
        // A PC rolls nothing: its initiative is the DM's figure.
        Some("initiative") => {
            json!({ "roll": null, "dexModifier": null, "initiative": input["enteredInitiative"] })
        }
        _ => {
            let participants = input["participants"].as_array().expect("participants");
            let standings: Vec<Standing> = participants
                .iter()
                .map(|p| Standing {
                    initiative: p["initiative"].as_i64().expect("an initiative"),
                    dex: p["dex"].as_i64(),
                    draw: p["draw"].as_i64().expect("a draw"),
                })
                .collect();
            let order: Vec<&Value> = ranked(&standings)
                .into_iter()
                .map(|at| &participants[at]["id"])
                .collect();
            json!({ "order": order })
        }
    }
}

#[test]
fn conformance_initiative_and_order_of_initiative_1() {
    let fixtures = of_a_start();
    assert!(
        fixtures.cases.len() >= 2,
        "Initiative@1 has initiative and order cases"
    );
    let failed = failures(&fixtures, |input| computed(&input));
    assert!(
        failed.is_empty(),
        "Initiative@1: {} case(s) fail:\n  - {}",
        failed.len(),
        failed.join("\n  - ")
    );
}
