//! `table.nextTurn`'s judgement before a command is queued: what the DM
//! sends, read ([`request`]), and made the turn written on the combat's
//! partition ([`prepare`]), against the combat and its participants.

use next_turn::{
    Combat, Participant, Prepared, Refusal, Request, Row, names, next, partition, prepare, request,
};
use serde_json::{Value, json};

const INSTANCE: &str = "b1000000-0000-4000-8000-000000000001";
const COMBAT: &str = "c0000000-0000-4000-8000-000000000001";
const FIRST: &str = "1e000000-0000-4000-8000-000000000001";
const SECOND: &str = "1e000000-0000-4000-8000-000000000002";
const THIRD: &str = "1e000000-0000-4000-8000-000000000003";

fn row(value: Value) -> Row {
    let Value::Object(fields) = value else {
        panic!("a row is an object");
    };
    fields.into_iter().collect()
}

fn manifest() -> Value {
    serde_json::from_str(include_str!("../data-capability.json")).expect("contract F")
}

fn asked() -> Request {
    Request {
        instance: INSTANCE.to_string(),
    }
}

fn at(current: Option<&str>) -> Combat {
    Combat {
        id: COMBAT.to_string(),
        current: current.map(str::to_string),
    }
}

/// The three participants, ranked 1 to 3, alive as `alive` says.
fn three(alive: [bool; 3]) -> Vec<Participant> {
    [FIRST, SECOND, THIRD]
        .iter()
        .zip(alive)
        .zip(1..)
        .map(|((entry, alive), rank)| Participant {
            entry: (*entry).to_string(),
            rank,
            alive,
        })
        .collect()
}

#[test]
fn the_manifest_names_this_data_capability_a_relative_update_of_the_turn_and_the_round() {
    let manifest = manifest();
    assert_eq!(manifest["dataCapability"], "table.nextTurn");
    assert_eq!(manifest["owner"], "dataguard");
    assert_eq!(manifest["effect"], "update");
    assert_eq!(manifest["mode"], "relative");
    assert_eq!(
        manifest["touches"],
        json!([
            "BattlemapInstance.currentEntryId",
            "BattlemapInstance.round"
        ])
    );
    assert_eq!(manifest["target"]["aggregate"], "BattlemapInstance");
    for invariant in ["next-turn", "dead-derived", "hidden-keeps-rank"] {
        assert!(
            manifest["invariants"]
                .as_array()
                .expect("invariants")
                .contains(&json!(invariant)),
            "{invariant}"
        );
    }
    assert!(names("table.nextTurn"));
    assert!(names("table.nextTurn@1"));
    assert!(!names("table.startCombat@1"));
}

#[test]
fn next_turn_three_participants_at_round_1_go_second_third_first_and_round_2() {
    let participants = three([true, true, true]);
    let (mut combat, mut round) = (at(Some(FIRST)), 1);
    let mut seen = Vec::new();
    for _ in 0..3 {
        let prepared = prepare(&asked(), Some(&combat), &participants).expect("a turn");
        assert_eq!(prepared.target_id, COMBAT);
        assert_eq!(prepared.payload.len(), 2, "{:?}", prepared.payload);
        // Applied: the entry written, the round stepped.
        round += prepared.payload["round"].as_i64().expect("a step");
        let current = prepared.payload["currentEntryId"]
            .as_str()
            .expect("an entry");
        combat = at(Some(current));
        seen.push(current.to_string());
    }
    assert_eq!(seen, [SECOND, THIRD, FIRST]);
    assert_eq!(round, 2);
    assert_eq!(partition(COMBAT), format!("BattlemapInstance/{COMBAT}"));
}

#[test]
fn next_turn_writes_the_entry_and_a_step_of_the_round_on_the_combat() {
    let prepared = prepare(
        &asked(),
        Some(&at(Some(SECOND))),
        &three([true, true, true]),
    );
    assert_eq!(
        prepared,
        Ok(Prepared {
            target_id: COMBAT.to_string(),
            payload: row(json!({ "currentEntryId": THIRD, "round": 0 })),
        })
    );
    let wrapped = prepare(&asked(), Some(&at(Some(THIRD))), &three([true, true, true]));
    assert_eq!(
        wrapped.expect("a turn").payload,
        row(json!({ "currentEntryId": FIRST, "round": 1 }))
    );
}

#[test]
fn dead_derived_the_dead_second_is_skipped_from_the_first() {
    let prepared = prepare(
        &asked(),
        Some(&at(Some(FIRST))),
        &three([true, false, true]),
    );
    assert_eq!(
        prepared.expect("a turn").payload,
        row(json!({ "currentEntryId": THIRD, "round": 0 }))
    );
}

#[test]
fn dead_derived_a_dead_first_is_skipped_on_the_wrap_and_the_round_still_goes_up() {
    let prepared = prepare(
        &asked(),
        Some(&at(Some(THIRD))),
        &three([false, true, true]),
    );
    assert_eq!(
        prepared.expect("a turn").payload,
        row(json!({ "currentEntryId": SECOND, "round": 1 }))
    );
}

#[test]
fn the_rank_orders_the_turn_whatever_order_the_participants_come_in() {
    let mut participants = three([true, true, true]);
    participants.reverse();
    let prepared = prepare(&asked(), Some(&at(Some(FIRST))), &participants);
    assert_eq!(prepared.expect("a turn").payload["currentEntryId"], SECOND);
}

#[test]
fn with_no_current_turn_the_first_living_one_takes_it_and_no_round_is_added() {
    for current in [None, Some("1e000000-0000-4000-8000-0000000000ff")] {
        let prepared = prepare(&asked(), Some(&at(current)), &three([false, true, true]));
        assert_eq!(
            prepared.expect("a turn").payload,
            row(json!({ "currentEntryId": SECOND, "round": 0 })),
            "{current:?}"
        );
    }
}

#[test]
fn no_open_combat_is_refused() {
    assert_eq!(
        prepare(&asked(), None, &three([true, true, true])),
        Err(Refusal::NoOpenCombat)
    );
}

#[test]
fn a_combat_with_no_one_alive_is_refused() {
    assert_eq!(
        prepare(
            &asked(),
            Some(&at(Some(FIRST))),
            &three([false, false, false])
        ),
        Err(Refusal::NoOneAlive)
    );
    assert_eq!(
        prepare(&asked(), Some(&at(None)), &[]),
        Err(Refusal::NoOneAlive)
    );
}

#[test]
fn next_wraps_to_the_current_one_when_it_is_the_only_one_alive() {
    assert_eq!(next(&[true, false], Some(0)), Some((0, true)));
    assert_eq!(next(&[false, true], Some(1)), Some((1, true)));
    assert_eq!(next(&[true, true, true], Some(1)), Some((2, false)));
    assert_eq!(next(&[false, false], Some(0)), None);
    assert_eq!(next(&[], None), None);
}

#[test]
fn the_instance_is_the_target_or_the_payload_s_lowercased() {
    let upper = INSTANCE.to_uppercase();
    assert_eq!(request(&Row::new(), Some(&upper)), Ok(asked()));
    let wire = row(json!({ "instance": INSTANCE }));
    assert_eq!(request(&wire, None), Ok(asked()));
    assert_eq!(request(&wire, Some("  ")), Ok(asked()));
}

#[test]
fn an_instance_that_is_no_uuid_is_refused() {
    assert_eq!(request(&Row::new(), None), Err(Refusal::NotAnId));
    assert_eq!(request(&Row::new(), Some("taverne")), Err(Refusal::NotAnId));
    let wire = row(json!({ "instance": 7 }));
    assert_eq!(request(&wire, None), Err(Refusal::NotAnId));
}
