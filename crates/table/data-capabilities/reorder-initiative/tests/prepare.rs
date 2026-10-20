//! `table.reorderInitiative`'s judgement before a command is queued: what the
//! DM sends, read ([`request`]), and made one rank per participant of the
//! open combat ([`prepare`]), 1 to n in the order given.

use reorder_initiative::{
    Participant, Prepared, Refusal, Request, Row, names, partition, prepare, request,
};
use serde_json::{Value, json};

const INSTANCE: &str = "b1000000-0000-4000-8000-000000000001";
/// The four participants' tokens, in their launch order.
const TOKENS: [&str; 4] = [
    "70000000-0000-4000-8000-000000000001",
    "70000000-0000-4000-8000-000000000002",
    "70000000-0000-4000-8000-000000000003",
    "70000000-0000-4000-8000-000000000004",
];
/// Their entries.
const ENTRIES: [&str; 4] = [
    "1e000000-0000-4000-8000-000000000001",
    "1e000000-0000-4000-8000-000000000002",
    "1e000000-0000-4000-8000-000000000003",
    "1e000000-0000-4000-8000-000000000004",
];
const STRANGER: &str = "70000000-0000-4000-8000-000000000009";

fn row(value: Value) -> Row {
    let Value::Object(fields) = value else {
        panic!("a row is an object");
    };
    fields.into_iter().collect()
}

fn manifest() -> Value {
    serde_json::from_str(include_str!("../data-capability.json")).expect("contract F")
}

/// The open combat's four participants.
fn four() -> Vec<Participant> {
    ENTRIES
        .iter()
        .zip(TOKENS)
        .map(|(entry, token)| Participant {
            entry: (*entry).to_string(),
            token: token.to_string(),
        })
        .collect()
}

/// The order `order`, of tokens or entries, on the instance.
fn asked(order: &[&str]) -> Request {
    request(&row(json!({ "order": order })), Some(INSTANCE)).expect("a request")
}

/// Each entry and the rank prepared for it, in the order of the commands.
fn ranks(prepared: &[Prepared]) -> Vec<(&str, Value)> {
    prepared
        .iter()
        .map(|one| {
            assert_eq!(one.payload.len(), 1, "{:?}", one.payload);
            (one.target_id.as_str(), one.payload["rank"].clone())
        })
        .collect()
}

#[test]
fn the_manifest_names_this_data_capability_an_update_of_the_ranks() {
    let manifest = manifest();
    assert_eq!(manifest["dataCapability"], "table.reorderInitiative");
    assert_eq!(manifest["owner"], "dataguard");
    assert_eq!(manifest["effect"], "update");
    assert_eq!(manifest["touches"], json!(["BattlemapInstance.rank"]));
    assert_eq!(manifest["target"]["aggregate"], "BattlemapInstance");
    assert_eq!(manifest["target"]["id"], "$instance");
    assert_eq!(manifest["mode"], "confirm_on_stale");
    assert_eq!(
        manifest["invariants"],
        json!([
            "instance-isolated",
            "initiative-tiebreak",
            "hidden-keeps-rank"
        ])
    );
    assert!(names("table.reorderInitiative"));
    assert!(names("table.reorderInitiative@1"));
    assert!(!names("table.nextTurn@1"));
    assert_eq!(
        partition(ENTRIES[0]),
        format!("BattlemapInstance/{}", ENTRIES[0])
    );
}

#[test]
fn initiative_tiebreak_four_participants_the_fourth_moved_first_are_ranked_1_to_4() {
    let order = [TOKENS[3], TOKENS[0], TOKENS[1], TOKENS[2]];
    let prepared = prepare(&asked(&order), Some(&four())).expect("prepared");
    assert_eq!(
        ranks(&prepared),
        [
            (ENTRIES[3], json!(1)),
            (ENTRIES[0], json!(2)),
            (ENTRIES[1], json!(3)),
            (ENTRIES[2], json!(4)),
        ]
    );
}

#[test]
fn every_rank_is_rewritten_even_one_that_does_not_move() {
    // The order as it was: every entry gets its rank again, 1 to 4, so ranks
    // a removal left with a gap close up.
    let prepared = prepare(&asked(&TOKENS), Some(&four())).expect("prepared");
    assert_eq!(
        ranks(&prepared),
        [
            (ENTRIES[0], json!(1)),
            (ENTRIES[1], json!(2)),
            (ENTRIES[2], json!(3)),
            (ENTRIES[3], json!(4)),
        ]
    );
}

#[test]
fn a_participant_may_be_named_by_its_entry_and_in_any_case() {
    let upper = TOKENS[1].to_ascii_uppercase();
    let order = [ENTRIES[2], upper.as_str(), TOKENS[0], ENTRIES[3]];
    let prepared = prepare(&asked(&order), Some(&four())).expect("prepared");
    assert_eq!(
        ranks(&prepared),
        [
            (ENTRIES[2], json!(1)),
            (ENTRIES[1], json!(2)),
            (ENTRIES[0], json!(3)),
            (ENTRIES[3], json!(4)),
        ]
    );
}

#[test]
fn hidden_keeps_rank_only_the_rank_is_written_whatever_the_dm_sends_beside_it() {
    // What the DM sends beside the order — a visibility, a turn — is never
    // written: a hidden participant takes the rank it is given, and no more.
    let wire = row(json!({
        "order": [TOKENS[1], TOKENS[0], TOKENS[2], TOKENS[3]],
        "visible": true,
        "currentEntryId": ENTRIES[3],
        "round": 3,
    }));
    let asked = request(&wire, Some(INSTANCE)).expect("a request");
    for one in prepare(&asked, Some(&four())).expect("prepared") {
        assert_eq!(one.payload.keys().collect::<Vec<_>>(), ["rank"]);
    }
}

#[test]
fn an_order_that_omits_or_repeats_a_participant_is_refused() {
    for order in [
        // The fourth omitted.
        vec![TOKENS[0], TOKENS[1], TOKENS[2]],
        // The first twice, the fourth omitted.
        vec![TOKENS[0], TOKENS[1], TOKENS[2], TOKENS[0]],
        // The same one by its token and by its entry.
        vec![TOKENS[0], ENTRIES[0], TOKENS[2], TOKENS[3]],
        // One that is not in the combat, in place of the fourth.
        vec![TOKENS[0], TOKENS[1], TOKENS[2], STRANGER],
        // Every one, and one more.
        vec![TOKENS[0], TOKENS[1], TOKENS[2], TOKENS[3], STRANGER],
    ] {
        assert_eq!(
            prepare(&asked(&order), Some(&four())),
            Err(Refusal::NotTheParticipants),
            "{order:?}"
        );
    }
}

#[test]
fn an_instance_with_no_open_combat_has_no_order_to_rewrite() {
    assert_eq!(prepare(&asked(&TOKENS), None), Err(Refusal::NoOpenCombat));
}

#[test]
fn the_instance_is_the_target_or_named_in_the_payload() {
    let order = json!([TOKENS[0]]);
    let upper = INSTANCE.to_ascii_uppercase();
    assert_eq!(
        request(&row(json!({ "order": order })), Some(&upper)),
        Ok(Request {
            instance: INSTANCE.to_string(),
            order: vec![TOKENS[0].to_string()],
        })
    );
    let wire = row(json!({ "instance": INSTANCE, "order": order }));
    assert_eq!(
        request(&wire, None).map(|asked| asked.instance),
        Ok(INSTANCE.to_string())
    );
    assert_eq!(
        request(&wire, Some(" ")).map(|asked| asked.instance),
        Ok(INSTANCE.to_string())
    );
}

#[test]
fn what_is_no_order_of_uuids_is_refused() {
    for (wire, target, refusal) in [
        (
            json!({ "order": [TOKENS[0]] }),
            None,
            Refusal::NotAnId("instance"),
        ),
        (
            json!({ "order": [TOKENS[0]] }),
            Some("taverne"),
            Refusal::NotAnId("instance"),
        ),
        (json!({}), Some(INSTANCE), Refusal::NoOrder),
        (json!({ "order": null }), Some(INSTANCE), Refusal::NoOrder),
        (json!({ "order": [] }), Some(INSTANCE), Refusal::NoOrder),
        (
            json!({ "order": TOKENS[0] }),
            Some(INSTANCE),
            Refusal::NoOrder,
        ),
        (
            json!({ "order": [TOKENS[0], "bandit-2"] }),
            Some(INSTANCE),
            Refusal::NotAnId("order"),
        ),
        (
            json!({ "order": [TOKENS[0], 2] }),
            Some(INSTANCE),
            Refusal::NotAnId("order"),
        ),
        (
            json!({ "order": [&TOKENS[0][..35]] }),
            Some(INSTANCE),
            Refusal::NotAnId("order"),
        ),
    ] {
        assert_eq!(request(&row(wire.clone()), target), Err(refusal), "{wire}");
    }
}

#[test]
fn every_refusal_says_why_in_words() {
    assert_eq!(
        Refusal::NotAnId("order").to_string(),
        "order n'est pas un identifiant"
    );
    assert_eq!(
        Refusal::NoOrder.to_string(),
        "un ordre est une liste de participants"
    );
    assert_eq!(
        Refusal::NoOpenCombat.to_string(),
        "cette instance n'a pas de combat en cours"
    );
    assert_eq!(
        Refusal::NotTheParticipants.to_string(),
        "l'ordre nomme chaque participant du combat une fois, et eux seuls"
    );
}
