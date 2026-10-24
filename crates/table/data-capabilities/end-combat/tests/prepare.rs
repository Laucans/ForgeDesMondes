//! `table.endCombat`'s judgement before a command is queued: what the DM
//! sends, read ([`request`]), the instance's combat as the queue leaves it
//! ([`open`]), and the end written on its partition ([`prepare`]).

use end_combat::{
    DATA_CAPABILITY, Prepared, Refusal, Request, Row, open, partition, prepare, request,
};
use serde_json::{Value, json};

const INSTANCE: &str = "b1a00000-0000-4000-8000-000000000001";
const OTHER: &str = "b1a00000-0000-4000-8000-000000000002";
const COMBAT: &str = "c0000000-0000-4000-8000-000000000001";
const NOW: &str = "2026-10-22T21:00:00+00:00";

fn row(value: Value) -> Row {
    let Value::Object(fields) = value else {
        panic!("a row is an object");
    };
    fields.into_iter().collect()
}

/// The tavern's combat as `table.startCombat` writes it, with `fields` over.
fn combat(fields: Value) -> Row {
    let mut combat = row(json!({
        "combatId": COMBAT,
        "combatInstanceId": INSTANCE,
        "round": 2,
        "currentEntryId": "ee000000-0000-4000-8000-000000000001",
        "startedAt": "2026-10-22T20:00:00+00:00",
    }));
    combat.extend(row(fields));
    combat
}

#[test]
fn the_manifest_names_this_data_capability_an_update_touching_the_end_and_the_turn() {
    let manifest: Value =
        serde_json::from_str(include_str!("../data-capability.json")).expect("contract F");
    assert_eq!(manifest["dataCapability"], DATA_CAPABILITY);
    assert_eq!(manifest["owner"], "dataguard");
    assert_eq!(manifest["effect"], "update");
    assert_eq!(manifest["target"]["aggregate"], "BattlemapInstance");
    assert_eq!(
        manifest["touches"],
        json!([
            "BattlemapInstance.endedAt",
            "BattlemapInstance.currentEntryId"
        ])
    );
    assert!(
        manifest["invariants"]
            .as_array()
            .expect("invariants")
            .contains(&json!("one-active-combat"))
    );
    assert_eq!(partition(COMBAT), format!("BattlemapInstance/{COMBAT}"));
}

#[test]
fn the_instance_is_the_target_else_the_payload_s_lowercase() {
    let expected = Ok(Request {
        instance: INSTANCE.to_owned(),
    });
    assert_eq!(
        request(
            &row(json!({ "instance": OTHER })),
            Some(&INSTANCE.to_uppercase())
        ),
        expected
    );
    assert_eq!(
        request(
            &row(json!({ "instance": format!(" {} ", INSTANCE.to_uppercase()) })),
            None
        ),
        expected
    );
}

#[test]
fn an_instance_that_is_no_uuid_is_refused() {
    for (wire, target) in [
        (json!({}), None),
        (json!({ "instance": "la-taverne" }), None),
        (json!({ "instance": 7 }), None),
        (json!({ "instance": INSTANCE }), Some("la-taverne")),
    ] {
        assert_eq!(
            request(&row(wire.clone()), target),
            Err(Refusal::NotAnId),
            "{wire} on {target:?}"
        );
    }
}

#[test]
fn a_combat_of_the_instance_with_no_end_is_open_by_its_id_lowercase() {
    assert_eq!(open(&combat(json!({})), INSTANCE), Some(COMBAT.to_owned()));
    assert_eq!(
        open(&combat(json!({ "endedAt": null })), INSTANCE),
        Some(COMBAT.to_owned())
    );
    assert_eq!(
        open(
            &combat(json!({ "combatId": COMBAT.to_uppercase() })),
            INSTANCE
        ),
        Some(COMBAT.to_owned())
    );
    assert_eq!(
        open(
            &combat(json!({ "combatInstanceId": INSTANCE.to_uppercase() })),
            INSTANCE
        ),
        Some(COMBAT.to_owned())
    );
}

#[test]
fn an_ended_combat_another_instance_s_or_no_combat_is_not_open() {
    assert_eq!(open(&combat(json!({ "endedAt": NOW })), INSTANCE), None);
    assert_eq!(open(&combat(json!({})), OTHER), None);
    assert_eq!(
        open(&combat(json!({ "combatInstanceId": null })), INSTANCE),
        None
    );
    assert_eq!(open(&combat(json!({ "combatId": "c1" })), INSTANCE), None);
    // A token's row, or a row the queue emptied: no combat.
    assert_eq!(
        open(
            &row(json!({ "tokenId": COMBAT, "tokenInstanceId": INSTANCE })),
            INSTANCE
        ),
        None
    );
    assert_eq!(open(&Row::new(), INSTANCE), None);
}

#[test]
fn an_open_combat_is_written_ended_now_with_no_turn_and_nothing_else() {
    assert_eq!(
        prepare(Some(COMBAT), NOW),
        Ok(Prepared {
            target_id: COMBAT.to_owned(),
            payload: row(json!({ "endedAt": NOW, "currentEntryId": null })),
        })
    );
}

#[test]
fn one_active_combat_an_instance_with_no_combat_open_ends_none() {
    assert_eq!(prepare(None, NOW), Err(Refusal::NoOpenCombat));
}

#[test]
fn each_refusal_says_why_in_words() {
    assert_eq!(
        Refusal::NotAnId.to_string(),
        "instance n'est pas un identifiant"
    );
    assert_eq!(
        Refusal::NoOpenCombat.to_string(),
        "one-active-combat: aucun combat n'est en cours sur cette instance"
    );
}
