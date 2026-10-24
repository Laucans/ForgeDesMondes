//! `table.nextTurn`'s judgement before a command is queued: what the DM
//! sends, read ([`request`]); the combat and its participants, read off
//! their rows as the queue leaves them ([`combat`], [`participant`]); and
//! the turn written on the combat's partition ([`prepare`]).

use next_turn::{
    Combat, DATA_CAPABILITY, Participant, Prepared, Refusal, Request, Row, combat, next,
    participant, partition, prepare, request,
};
use serde_json::{Value, json};

const INSTANCE: &str = "b1000000-0000-4000-8000-000000000001";
const AUTRE_INSTANCE: &str = "b1000000-0000-4000-8000-000000000002";
const COMBAT: &str = "c0000000-0000-4000-8000-000000000001";
const FIRST: &str = "1e000000-0000-4000-8000-000000000001";
const SECOND: &str = "1e000000-0000-4000-8000-000000000002";
const THIRD: &str = "1e000000-0000-4000-8000-000000000003";
const BANDIT: &str = "70000000-0000-4000-8000-000000000001";

fn row(value: Value) -> Row {
    serde_json::from_value(value).expect("an object")
}

fn manifest() -> Value {
    serde_json::from_str(include_str!("../data-capability.json")).expect("contract F")
}

fn asked() -> Request {
    Request {
        instance: INSTANCE.to_owned(),
    }
}

fn at(current: Option<&str>) -> Combat {
    Combat {
        id: COMBAT.to_owned(),
        current: current.map(str::to_owned),
    }
}

/// The three participants, ranked 1 to 3, alive as `alive` says.
fn three(alive: [bool; 3]) -> Vec<Participant> {
    [FIRST, SECOND, THIRD]
        .iter()
        .zip(alive)
        .zip(1..)
        .map(|((entry, alive), rank)| Participant {
            entry: (*entry).to_owned(),
            rank,
            alive,
        })
        .collect()
}

/// The combat's row, round 1, `current`'s turn.
fn combat_row(current: Value) -> Row {
    row(json!({
        "combatId": COMBAT,
        "combatInstanceId": INSTANCE,
        "round": 1,
        "currentEntryId": current,
        "startedAt": "2026-10-21T10:00:00Z",
    }))
}

/// The first entry's row, rank 2, Bandit's.
fn entry_row() -> Row {
    row(json!({
        "initiativeEntryId": FIRST,
        "initiativeEntryCombatId": COMBAT,
        "initiativeEntryTokenId": BANDIT,
        "roll": 10,
        "initiative": 11,
        "dexScore": 12,
        "tiebreak": 0.5,
        "rank": 2,
    }))
}

/// Bandit's token at `hp`; `null` for a PC's, which tracks none.
fn token_row(hp: Value, visible: bool) -> Row {
    row(json!({
        "tokenId": BANDIT,
        "tokenInstanceId": INSTANCE,
        "name": "Bandit 1",
        "visible": visible,
        "hpCurrent": hp,
        "hpMax": 11,
    }))
}

#[test]
fn the_manifest_names_this_data_capability_a_relative_update_of_the_turn_and_the_round() {
    let manifest = manifest();
    assert_eq!(manifest["dataCapability"], DATA_CAPABILITY);
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
}

#[test]
fn next_turn_three_participants_at_round_1_go_second_third_first_and_round_2() {
    let participants = three([true, true, true]);
    let (mut combat, mut round) = (at(Some(FIRST)), 1);
    let mut seen = Vec::new();
    for _ in 0..3 {
        let prepared = prepare(Some(&combat), &participants).expect("a turn");
        assert_eq!(prepared.target_id, COMBAT);
        assert_eq!(prepared.payload.len(), 2, "{:?}", prepared.payload);
        // Applied: the entry written, the round stepped.
        round += prepared.payload["round"].as_i64().expect("a step");
        let current = prepared.payload["currentEntryId"]
            .as_str()
            .expect("an entry");
        combat = at(Some(current));
        seen.push(current.to_owned());
    }
    assert_eq!(seen, [SECOND, THIRD, FIRST]);
    assert_eq!(round, 2);
    assert_eq!(partition(COMBAT), format!("BattlemapInstance/{COMBAT}"));
}

#[test]
fn next_turn_writes_the_entry_and_a_step_of_the_round_on_the_combat() {
    assert_eq!(
        prepare(Some(&at(Some(SECOND))), &three([true, true, true])),
        Ok(Prepared {
            target_id: COMBAT.to_owned(),
            payload: row(json!({ "currentEntryId": THIRD, "round": 0 })),
        })
    );
    let wrapped = prepare(Some(&at(Some(THIRD))), &three([true, true, true]));
    assert_eq!(
        wrapped.expect("a turn").payload,
        row(json!({ "currentEntryId": FIRST, "round": 1 }))
    );
}

#[test]
fn dead_derived_the_dead_second_is_skipped_from_the_first() {
    let prepared = prepare(Some(&at(Some(FIRST))), &three([true, false, true]));
    assert_eq!(
        prepared.expect("a turn").payload,
        row(json!({ "currentEntryId": THIRD, "round": 0 }))
    );
}

#[test]
fn dead_derived_a_dead_first_is_skipped_on_the_wrap_and_the_round_still_goes_up() {
    let prepared = prepare(Some(&at(Some(THIRD))), &three([false, true, true]));
    assert_eq!(
        prepared.expect("a turn").payload,
        row(json!({ "currentEntryId": SECOND, "round": 1 }))
    );
}

#[test]
fn the_rank_orders_the_turn_whatever_order_the_participants_come_in() {
    let mut participants = three([true, true, true]);
    participants.reverse();
    let prepared = prepare(Some(&at(Some(FIRST))), &participants);
    assert_eq!(prepared.expect("a turn").payload["currentEntryId"], SECOND);
}

#[test]
fn a_tie_on_the_rank_is_ordered_by_the_entry_id() {
    let mut participants = three([true, true, true]);
    for participant in &mut participants {
        participant.rank = 1;
    }
    participants.reverse();
    let prepared = prepare(Some(&at(Some(FIRST))), &participants);
    assert_eq!(prepared.expect("a turn").payload["currentEntryId"], SECOND);
}

#[test]
fn with_no_current_turn_the_first_living_one_takes_it_and_no_round_is_added() {
    for current in [None, Some("1e000000-0000-4000-8000-0000000000ff")] {
        let prepared = prepare(Some(&at(current)), &three([false, true, true]));
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
        prepare(None, &three([true, true, true])),
        Err(Refusal::NoOpenCombat)
    );
}

#[test]
fn a_combat_with_no_one_alive_is_refused() {
    assert_eq!(
        prepare(Some(&at(Some(FIRST))), &three([false, false, false])),
        Err(Refusal::NoOneAlive)
    );
    assert_eq!(prepare(Some(&at(None)), &[]), Err(Refusal::NoOneAlive));
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
    let upper = format!(" {} ", INSTANCE.to_uppercase());
    assert_eq!(request(&Row::new(), Some(&upper)), Ok(asked()));
    let wire = row(json!({ "instance": INSTANCE }));
    assert_eq!(request(&wire, None), Ok(asked()));
    // The target wins over the payload.
    let other = row(json!({ "instance": AUTRE_INSTANCE }));
    assert_eq!(request(&other, Some(INSTANCE)), Ok(asked()));
}

#[test]
fn an_instance_that_is_no_uuid_is_refused() {
    assert_eq!(request(&Row::new(), None), Err(Refusal::NotAnId));
    assert_eq!(request(&Row::new(), Some("taverne")), Err(Refusal::NotAnId));
    let wire = row(json!({ "instance": 7 }));
    assert_eq!(request(&wire, None), Err(Refusal::NotAnId));
}

#[test]
fn the_combat_is_its_row_s_when_it_is_an_open_combat_of_the_instance() {
    let upper = combat_row(json!(SECOND.to_uppercase()));
    assert_eq!(combat(&upper, INSTANCE), Some(at(Some(SECOND))));
    assert_eq!(combat(&combat_row(Value::Null), INSTANCE), Some(at(None)));
    // Another instance's, ended — an end queued ahead included — or no
    // combat at all.
    assert_eq!(combat(&upper, AUTRE_INSTANCE), None);
    let mut ended = combat_row(json!(FIRST));
    ended.insert("endedAt".to_owned(), json!("2026-10-21T11:00:00Z"));
    assert_eq!(combat(&ended, INSTANCE), None);
    assert_eq!(combat(&Row::new(), INSTANCE), None);
    assert_eq!(combat(&token_row(json!(11), true), INSTANCE), None);
}

#[test]
fn a_participant_is_alive_until_its_hp_reach_0_hidden_or_not() {
    let alive = |token: &Row| participant(&entry_row(), token, COMBAT, INSTANCE).map(|p| p.alive);
    assert_eq!(
        participant(&entry_row(), &token_row(json!(11), true), COMBAT, INSTANCE),
        Some(Participant {
            entry: FIRST.to_owned(),
            rank: 2,
            alive: true,
        })
    );
    // hidden-keeps-rank: a hidden token takes its turn.
    assert_eq!(alive(&token_row(json!(1), false)), Some(true));
    // dead-derived: at 0 HP.
    assert_eq!(alive(&token_row(json!(0), true)), Some(false));
    // A PC tracks no HP: always alive.
    assert_eq!(alive(&token_row(Value::Null, true)), Some(true));
    // A token off the map takes no turn, and keeps its rank; so does one on
    // another instance (instance-isolated).
    assert_eq!(alive(&Row::new()), Some(false));
    let mut elsewhere = token_row(json!(11), true);
    elsewhere.insert("tokenInstanceId".to_owned(), json!(AUTRE_INSTANCE));
    assert_eq!(alive(&elsewhere), Some(false));
    let mut upper = token_row(json!(11), true);
    upper.insert("tokenInstanceId".to_owned(), json!(INSTANCE.to_uppercase()));
    assert_eq!(alive(&upper), Some(true));
}

#[test]
fn an_entry_that_is_no_longer_the_combat_s_is_no_participant() {
    let token = token_row(json!(11), true);
    assert_eq!(participant(&Row::new(), &token, COMBAT, INSTANCE), None);
    assert_eq!(
        participant(
            &entry_row(),
            &token,
            "c0000000-0000-4000-8000-000000000002",
            INSTANCE
        ),
        None
    );
    let mut unranked = entry_row();
    unranked.remove("rank");
    assert_eq!(participant(&unranked, &token, COMBAT, INSTANCE), None);
}

#[test]
fn each_refusal_says_what_it_is() {
    assert_eq!(
        Refusal::NotAnId.to_string(),
        "instance n'est pas un identifiant"
    );
    assert_eq!(
        Refusal::NoOpenCombat.to_string(),
        "next-turn: aucun combat n'est en cours sur cette instance"
    );
    assert_eq!(
        Refusal::NoOneAlive.to_string(),
        "dead-derived: aucun participant vivant à qui passer le tour"
    );
}
