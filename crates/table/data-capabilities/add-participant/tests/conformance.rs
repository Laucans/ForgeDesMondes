//! `table.addParticipant` held to Initiative@1's fixtures: the cases of its
//! `initiative` and `order` operations, which a joining computes. An order
//! is built the way a combat grows by joinings: each participant, in the
//! order given, placed among the ones before it ([`placed`]), the ones it
//! lands ahead of moved one rank down. Its `next` cases are « Suivant »'s
//! (`table.nextTurn`): a joining never moves the turn.

use add_participant::{Seated, Standing, initiative, modifier, placed};
use platform_conformance::{Fixtures, failures, fixtures_of};
use serde_json::{Value, json};

/// Initiative@1's cases of a joining's operations.
fn of_a_joining() -> Fixtures {
    let mut fixtures = fixtures_of("Initiative@1");
    fixtures.cases.retain(|case| {
        ["initiative", "order"].contains(&case.input["operation"].as_str().unwrap_or(""))
    });
    fixtures
}

/// `participants` joined one after the other, by id, rank 1 first.
fn joined_one_by_one(participants: &[Value]) -> Vec<String> {
    let mut seated: Vec<Seated> = Vec::new();
    for p in participants {
        let standing = Standing {
            initiative: p["initiative"].as_i64().expect("an initiative"),
            dex: p["dex"].as_i64(),
            draw: p["draw"].as_f64().expect("a draw"),
        };
        let (rank, moves) = placed(&seated, &standing);
        for other in &mut seated {
            if moves.contains(&other.id) {
                other.rank += 1;
            }
        }
        seated.push(Seated {
            id: p["id"].as_str().expect("an id").to_owned(),
            token: String::new(),
            initiative: standing.initiative,
            dex: standing.dex,
            tiebreak: standing.draw,
            rank,
        });
    }
    seated.sort_by_key(|seated| seated.rank);
    let ranks: Vec<i64> = seated.iter().map(|seated| seated.rank).collect();
    assert_eq!(ranks, (1..=seated.len() as i64).collect::<Vec<_>>());
    seated.into_iter().map(|seated| seated.id).collect()
}

/// A case's answer, as the joining computes it.
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
            json!({ "order": joined_one_by_one(participants) })
        }
    }
}

#[test]
fn conformance_initiative_and_order_of_initiative_1() {
    let fixtures = of_a_joining();
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
