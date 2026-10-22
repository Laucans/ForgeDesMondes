//! What table.reorderInitiative makes of a request, against the lineup the
//! engine's rows give, without a database: the order read off the wire, the
//! entries ranked 1 to n in the order given, the follow-ups that carry it,
//! and every refusal — an order that omits, repeats or adds a participant
//! (`initiative-tiebreak`: the ranks are the DM's to edit, never to drop
//! one), an instance unknown or with no open combat.

use reorder_initiative::{
    DATA_CAPABILITY, Lineup, Participant, Refusal, Request, Row, Seals, follow_ups, follows,
    ordered, partition, rank, request,
};
use serde_json::{Value, json};

const INSTANCE: &str = "b1000000-0000-4000-8000-000000000001";
const COMBAT: &str = "c0000000-0000-4000-8000-000000000001";
const T1: &str = "70000000-0000-4000-8000-000000000001";
const T2: &str = "70000000-0000-4000-8000-000000000002";
const T3: &str = "70000000-0000-4000-8000-000000000003";
const T4: &str = "70000000-0000-4000-8000-000000000004";
const E1: &str = "ee000000-0000-4000-8000-000000000001";
const E2: &str = "ee000000-0000-4000-8000-000000000002";
const E3: &str = "ee000000-0000-4000-8000-000000000003";
const E4: &str = "ee000000-0000-4000-8000-000000000004";
const NOWHERE: &str = "70000000-0000-4000-8000-0000000000ff";

fn wire(value: Value) -> Row {
    serde_json::from_value(value).expect("an object")
}

/// The open combat of four: T1 to T4, ranked so.
fn lineup() -> Lineup {
    Lineup {
        instance: true,
        combat: Some(COMBAT.to_owned()),
        participants: [(E1, T1), (E2, T2), (E3, T3), (E4, T4)]
            .iter()
            .map(|(entry, token)| Participant {
                entry: (*entry).to_owned(),
                token: (*token).to_owned(),
            })
            .collect(),
    }
}

fn asking(order: &[&str]) -> Request {
    Request {
        instance: INSTANCE.to_owned(),
        order: order.iter().map(|id| (*id).to_owned()).collect(),
        entry: None,
        seal: None,
    }
}

#[test]
fn request_reads_the_target_else_the_instance_and_the_order_lowercase() {
    let upper = T1.to_uppercase();
    let read = request(
        &wire(json!({ "order": [upper, format!(" {E2} ")] })),
        Some(INSTANCE),
    )
    .expect("read");
    assert_eq!(read, asking(&[T1, E2]));
    let read = request(&wire(json!({ "instance": INSTANCE, "order": [T1] })), None).expect("read");
    assert_eq!(read.instance, INSTANCE);
    // The target wins over the payload's instance.
    let read = request(
        &wire(json!({ "instance": NOWHERE, "order": [T1] })),
        Some(INSTANCE),
    )
    .expect("read");
    assert_eq!(read.instance, INSTANCE);
}

#[test]
fn request_reads_a_follow_up_s_seal_trimmed_and_a_null_one_as_none() {
    let read = request(
        &wire(json!({ "order": [E1, E2], "entry": E2, "seal": " s-1 " })),
        Some(INSTANCE),
    )
    .expect("read");
    assert_eq!(read.seal.as_deref(), Some("s-1"));
    let read = request(
        &wire(json!({ "order": [E1], "seal": null })),
        Some(INSTANCE),
    )
    .expect("read");
    assert_eq!(read.seal, None);
}

#[test]
fn request_reads_a_follow_up_s_entry_and_a_null_one_as_none() {
    let read = request(
        &wire(json!({ "order": [E1, E2], "entry": E2.to_uppercase() })),
        Some(INSTANCE),
    )
    .expect("read");
    assert_eq!(read.entry.as_deref(), Some(E2));
    let read = request(
        &wire(json!({ "order": [E1], "entry": null })),
        Some(INSTANCE),
    )
    .expect("read");
    assert_eq!(read.entry, None);
}

#[test]
fn request_refuses_what_names_no_uuid_and_an_order_that_is_no_list_or_empty() {
    let cases = [
        (json!({ "order": [T1] }), None, Refusal::NotAnId("instance")),
        (
            json!({ "order": [T1] }),
            Some("porte"),
            Refusal::NotAnId("instance"),
        ),
        (json!({}), Some(INSTANCE), Refusal::NoOrder),
        (json!({ "order": [] }), Some(INSTANCE), Refusal::NoOrder),
        (json!({ "order": T1 }), Some(INSTANCE), Refusal::NoOrder),
        (
            json!({ "order": [T1, "cafe"] }),
            Some(INSTANCE),
            Refusal::NotAnId("order"),
        ),
        (
            json!({ "order": [T1, 4] }),
            Some(INSTANCE),
            Refusal::NotAnId("order"),
        ),
        (
            json!({ "order": [T1], "entry": "x" }),
            Some(INSTANCE),
            Refusal::NotAnId("entry"),
        ),
        (
            json!({ "order": [T1], "entry": 1 }),
            Some(INSTANCE),
            Refusal::NotAnId("entry"),
        ),
        (
            json!({ "order": [T1], "seal": 1 }),
            Some(INSTANCE),
            Refusal::NotAnId("seal"),
        ),
        (
            json!({ "order": [T1], "seal": "  " }),
            Some(INSTANCE),
            Refusal::NotAnId("seal"),
        ),
    ];
    for (payload, target, refusal) in cases {
        assert_eq!(
            request(&wire(payload.clone()), target),
            Err(refusal),
            "{payload}"
        );
    }
}

#[test]
fn initiative_tiebreak_the_fourth_moved_first_ranks_every_entry_1_to_4_in_the_new_order() {
    let entries = ordered(&asking(&[T4, T1, T2, T3]), &lineup()).expect("ordered");
    assert_eq!(entries, [E4, E1, E2, E3]);
    let ranks: Vec<i64> = [E1, E2, E3, E4]
        .iter()
        .map(|entry| rank(&entries, entry).expect("ranked"))
        .collect();
    // E1 2nd, E2 3rd, E3 4th, E4 1st: no gap, no duplicate.
    assert_eq!(ranks, [2, 3, 4, 1]);
}

#[test]
fn a_participant_may_be_named_by_its_entry_as_by_its_token() {
    let entries = ordered(&asking(&[E2, T1, E4, T3]), &lineup()).expect("ordered");
    assert_eq!(entries, [E2, E1, E4, E3]);
}

#[test]
fn an_order_that_omits_repeats_or_adds_a_participant_is_refused() {
    let cases: [&[&str]; 5] = [
        // T4 omitted.
        &[T1, T2, T3],
        // T1 twice, T4 omitted.
        &[T1, T2, T3, T1],
        // T1 by its token, then by its entry.
        &[T1, T2, T3, E1],
        // A stranger in T4's place.
        &[T1, T2, T3, NOWHERE],
        // Every one, and a stranger.
        &[T1, T2, T3, T4, NOWHERE],
    ];
    for order in cases {
        assert_eq!(
            ordered(&asking(order), &lineup()),
            Err(Refusal::NotTheParticipants),
            "{order:?}"
        );
    }
}

#[test]
fn an_unknown_instance_or_one_with_no_open_combat_is_refused() {
    let unknown = Lineup {
        instance: false,
        ..lineup()
    };
    assert_eq!(
        ordered(&asking(&[T1, T2, T3, T4]), &unknown),
        Err(Refusal::UnknownInstance)
    );
    let calm = Lineup {
        combat: None,
        participants: Vec::new(),
        ..lineup()
    };
    assert_eq!(ordered(&asking(&[T1]), &calm), Err(Refusal::NoOpenCombat));
    assert!(Refusal::UnknownInstance.not_found());
    for refusal in [
        Refusal::NotAnId("order"),
        Refusal::NoOrder,
        Refusal::NoOpenCombat,
        Refusal::NotTheParticipants,
        Refusal::OutOfSequence,
        Refusal::Unsealed,
    ] {
        assert!(!refusal.not_found(), "{refusal:?}");
    }
}

#[test]
fn rank_is_the_place_from_1_and_refuses_an_entry_out_of_the_order() {
    let entries: Vec<String> = [E3, E1].iter().map(|e| (*e).to_owned()).collect();
    assert_eq!(rank(&entries, E3), Ok(1));
    assert_eq!(rank(&entries, E1), Ok(2));
    assert_eq!(rank(&entries, E2), Err(Refusal::NotTheParticipants));
}

#[test]
fn the_follow_ups_rank_every_entry_after_the_first_carrying_the_whole_order() {
    let entries: Vec<String> = [E4, E1, E2].iter().map(|e| (*e).to_owned()).collect();
    let then = follow_ups(INSTANCE, &entries, "s-1");
    assert_eq!(then.len(), 2);
    for (follow_up, entry) in then.iter().zip([E1, E2]) {
        assert_eq!(follow_up.data_capability, DATA_CAPABILITY);
        assert_eq!(follow_up.target_id.as_deref(), Some(INSTANCE));
        assert_eq!(follow_up.payload["order"], json!([E4, E1, E2]));
        assert_eq!(follow_up.payload["entry"], entry);
        assert_eq!(follow_up.payload["seal"], "s-1");
        // Read back, a follow-up asks the same order, for its entry.
        let read = request(&follow_up.payload, follow_up.target_id.as_deref()).expect("read");
        assert_eq!(read.entry.as_deref(), Some(entry));
        assert_eq!(read.order, entries);
        assert_eq!(read.seal.as_deref(), Some("s-1"));
    }
    assert!(follow_ups(INSTANCE, &entries[..1], "s-1").is_empty());
}

#[test]
fn the_refusals_say_why_in_french() {
    assert_eq!(
        Refusal::NotAnId("order").to_string(),
        "order n'est pas un identifiant"
    );
    for refusal in [
        Refusal::NoOrder,
        Refusal::UnknownInstance,
        Refusal::NoOpenCombat,
        Refusal::NotTheParticipants,
        Refusal::OutOfSequence,
        Refusal::Unsealed,
    ] {
        assert!(!refusal.to_string().is_empty(), "{refusal:?}");
    }
    assert_eq!(partition(E1), format!("BattlemapInstance/{E1}"));
}

#[test]
fn a_follow_up_follows_only_the_entries_ahead_holding_their_places_and_never_ranks_the_first() {
    assert!(follows(&[json!(1)]));
    assert!(follows(&[json!(1), json!(2), json!(3)]));
    // The first entry is the command's.
    assert!(!follows(&[]));
    // The command ahead not queued: the first entry kept its old rank.
    assert!(!follows(&[json!(4)]));
    assert!(!follows(&[json!(1), json!(3)]));
    assert!(!follows(&[json!(2), json!(2)]));
    // An entry ahead with no rank.
    assert!(!follows(&[json!(1), Value::Null]));
}

fn entries(ids: &[&str]) -> Vec<String> {
    ids.iter().map(|id| (*id).to_owned()).collect()
}

#[test]
fn a_seal_is_the_one_the_latest_reorder_left_for_an_entry_and_is_taken_once() {
    let seals = Seals::default();
    // Nothing issued: nothing is held, nothing taken.
    assert!(!seals.holds(INSTANCE, "s-1", E2));
    assert!(!seals.take(INSTANCE, "s-1", E2));

    seals.issue(INSTANCE, "s-1", &entries(&[E2, E3]));
    assert!(seals.holds(INSTANCE, "s-1", E2));
    assert!(seals.holds(INSTANCE, "s-1", E3));
    // Another seal, another instance, the first entry (the command's own).
    assert!(!seals.holds(INSTANCE, "s-2", E2));
    assert!(!seals.holds(NOWHERE, "s-1", E2));
    assert!(!seals.holds(INSTANCE, "s-1", E1));
    assert!(!seals.take(INSTANCE, "s-2", E2));
    assert!(!seals.take(NOWHERE, "s-1", E2));
    assert!(!seals.take(INSTANCE, "s-1", E1));
    // Holding takes nothing; taking is once.
    assert!(seals.holds(INSTANCE, "s-1", E2));
    assert!(seals.take(INSTANCE, "s-1", E2));
    assert!(!seals.holds(INSTANCE, "s-1", E2));
    assert!(!seals.take(INSTANCE, "s-1", E2));
    assert!(seals.take(INSTANCE, "s-1", E3));
}

#[test]
fn a_newer_reorder_of_the_instance_voids_the_older_one_s_seal_and_only_its_own() {
    let seals = Seals::default();
    seals.issue(INSTANCE, "s-1", &entries(&[E2]));
    seals.issue(NOWHERE, "s-9", &entries(&[E2]));
    seals.issue(INSTANCE, "s-2", &entries(&[E3]));
    assert!(!seals.holds(INSTANCE, "s-1", E2));
    assert!(!seals.take(INSTANCE, "s-1", E2));
    assert!(seals.holds(INSTANCE, "s-2", E3));
    assert!(seals.holds(NOWHERE, "s-9", E2));
}
