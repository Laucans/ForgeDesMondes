//! Initiative@1 (`concepts/Initiative/v1/fixtures.json`), held through
//! `prepare` itself: an `initiative` case is one participant joining an
//! empty combat with the case's die, an `order` case its participants
//! joining one by one in the order given, each landing where `prepare` puts
//! it and moving the ones it lands ahead of. The `next` cases are
//! « Suivant »'s (`table.nextTurn`), which no joining does.

use std::collections::VecDeque;

use add_participant::{Character, Dice, Request, Seated, Seating, Token, modifier, prepare};
use serde_json::{Value, json};

const COMBAT: &str = "c0000000-0000-4000-8000-000000000001";
const INSTANCE: &str = "b1000000-0000-4000-8000-000000000001";

/// A die that rolls what it was told, in order.
struct Loaded(VecDeque<i64>);

impl Dice for Loaded {
    fn roll(&mut self, _sides: u32) -> Option<i64> {
        self.0.pop_front()
    }
}

fn token(id: &str) -> Token {
    Token {
        id: id.to_string(),
        instance: INSTANCE.to_string(),
        character: "e1000000-0000-4000-8000-000000000001".to_string(),
    }
}

/// One participant's initiative: an NPC rolls the case's roll (and a draw),
/// a PC takes the figure entered.
fn initiative(input: &Value) -> Value {
    let pc = input["kind"] == "pc";
    let dex = input["dex"].as_i64();
    let seating = Seating {
        combat: Some(COMBAT.to_string()),
        entries: Vec::new(),
        character: Some(Character {
            kind: if pc { "pj" } else { "pnj" }.to_string(),
            dex,
        }),
    };
    let request = Request {
        token: "t".to_string(),
        instance: None,
        initiative: input["enteredInitiative"].as_i64(),
    };
    let mut dice = Loaded(input["roll"].as_i64().into_iter().chain([1]).collect());
    let prepared =
        prepare(&request, &token("t"), &seating, &mut dice, "e").expect("a participant joins");
    let row = prepared.entry.1;
    json!({
        "roll": row["roll"],
        "dexModifier": if pc { Value::Null } else { json!(modifier(dex.expect("an NPC's DEX"))) },
        "initiative": row["initiative"],
    })
}

/// The participants joining one by one, in the order given: their order by
/// rank once the last joined.
fn order(input: &Value) -> Value {
    let mut seated: Vec<Seated> = Vec::new();
    for participant in input["participants"].as_array().expect("participants") {
        let id = participant["id"].as_str().expect("an id");
        let seating = Seating {
            combat: Some(COMBAT.to_string()),
            entries: seated.clone(),
            character: Some(Character {
                kind: "pj".to_string(),
                dex: participant["dex"].as_i64(),
            }),
        };
        let request = Request {
            token: id.to_string(),
            instance: None,
            initiative: participant["initiative"].as_i64(),
        };
        let mut dice = Loaded(participant["draw"].as_i64().into_iter().collect());
        let prepared =
            prepare(&request, &token(id), &seating, &mut dice, id).expect("a participant joins");
        for (moved, step) in prepared.moves {
            let entry = seated
                .iter_mut()
                .find(|entry| entry.id == moved)
                .expect("a participant seated");
            entry.rank += step["rank"].as_i64().expect("a step");
        }
        let row = prepared.entry.1;
        seated.push(Seated {
            id: id.to_string(),
            token: id.to_string(),
            roll: None,
            initiative: row["initiative"].as_i64().expect("an initiative"),
            dex: row["dexScore"].as_i64(),
            tiebreak: row["tiebreak"].as_f64().expect("a draw"),
            rank: row["rank"].as_i64().expect("a rank"),
            held: true,
        });
    }
    seated.sort_by(|a, b| a.rank.cmp(&b.rank).then_with(|| a.id.cmp(&b.id)));
    json!({ "order": seated.iter().map(|entry| entry.id.as_str()).collect::<Vec<_>>() })
}

#[test]
fn conformance_initiative_1_a_participant_joining_rolls_and_ranks_as_the_concept_says() {
    let mut fixtures = platform_conformance::fixtures_of("Initiative@1");
    fixtures
        .cases
        .retain(|case| case.input["operation"] != "next");
    assert!(!fixtures.cases.is_empty());
    let failed =
        platform_conformance::failures(&fixtures, |input| match input["operation"].as_str() {
            Some("initiative") => initiative(&input),
            _ => order(&input),
        });
    assert!(failed.is_empty(), "{}", failed.join("\n"));
}
