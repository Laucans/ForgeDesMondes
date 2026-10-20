//! `table.endCombat`'s judgement before a command is queued: what the DM
//! sends, read ([`request`]), and made the end written on the open combat's
//! partition ([`prepare`]).

use end_combat::{Prepared, Refusal, Request, Row, names, prepare, request};
use serde_json::{Value, json};

const INSTANCE: &str = "b1a00000-0000-4000-8000-000000000001";
const COMBAT: &str = "c0000000-0000-4000-8000-000000000001";
const NOW: &str = "2026-10-20T21:00:00.000Z";

fn row(value: Value) -> Row {
    let Value::Object(fields) = value else {
        panic!("a row is an object");
    };
    fields.into_iter().collect()
}

fn manifest() -> Value {
    serde_json::from_str(include_str!("../data-capability.json")).expect("contract F")
}

#[test]
fn the_manifest_names_this_data_capability_an_update_touching_the_end_and_the_turn() {
    let manifest = manifest();
    assert_eq!(manifest["dataCapability"], "table.endCombat");
    assert_eq!(manifest["owner"], "dataguard");
    assert_eq!(manifest["effect"], "update");
    assert_eq!(
        manifest["touches"],
        json!([
            "BattlemapInstance.endedAt",
            "BattlemapInstance.currentEntryId"
        ])
    );
    assert_eq!(manifest["target"]["aggregate"], "BattlemapInstance");
    assert!(
        manifest["invariants"]
            .as_array()
            .expect("invariants")
            .contains(&json!("one-active-combat"))
    );
    assert!(names("table.endCombat"));
    assert!(names("table.endCombat@1"));
    assert!(!names("table.startCombat@1"));
    assert!(!names("table.endCombatNow"));
}

#[test]
fn the_instance_is_the_target_else_the_payload_s_lowercase() {
    let upper = INSTANCE.to_uppercase();
    let other = "b1a00000-0000-4000-8000-000000000002";
    let expected = Request {
        instance: INSTANCE.to_string(),
    };
    let named = row(json!({ "instance": other }));
    assert_eq!(request(&named, Some(&upper)), Ok(expected.clone()));
    assert_eq!(
        request(&row(json!({ "instance": format!(" {upper} ") })), None),
        Ok(expected.clone())
    );
    // A blank target names nothing: the payload's instance stands.
    assert_eq!(
        request(&row(json!({ "instance": INSTANCE })), Some("  ")),
        Ok(expected)
    );
}

#[test]
fn an_instance_that_is_no_uuid_is_refused() {
    for (wire, target) in [
        (json!({}), None),
        (json!({ "instance": "la-taverne" }), None),
        (json!({ "instance": 7 }), None),
        (json!({ "instance": INSTANCE }), Some("la-taverne")),
        // A uuid with a letter where a hyphen stands, or a hyphen where a
        // digit does, is none.
        (
            json!({ "instance": "b1a00000x0000-4000-8000-000000000001" }),
            None,
        ),
        (
            json!({ "instance": "b1a00000-0000-4000-8000-00000000000-" }),
            None,
        ),
        (
            json!({ "instance": "b1a00000-0000-4000-8000-0000000000001" }),
            None,
        ),
        (
            json!({ "instance": "g1a00000-0000-4000-8000-000000000001" }),
            None,
        ),
    ] {
        assert_eq!(
            request(&row(wire.clone()), target),
            Err(Refusal::NotAnId),
            "{wire} on {target:?}"
        );
    }
}

#[test]
fn an_open_combat_is_written_ended_now_with_no_turn_and_nothing_else() {
    let prepared = prepare(Some(&COMBAT.to_uppercase()), NOW).expect("an end");
    assert_eq!(
        prepared,
        Prepared {
            target_id: COMBAT.to_string(),
            payload: row(json!({ "endedAt": NOW, "currentEntryId": null })),
        }
    );
}

#[test]
fn one_active_combat_an_instance_with_no_combat_open_ends_none() {
    assert_eq!(prepare(None, NOW), Err(Refusal::NoOpenCombat));
    assert_eq!(
        prepare(Some("not-a-combat"), NOW),
        Err(Refusal::NoOpenCombat)
    );
}

#[test]
fn each_refusal_says_why_in_words() {
    assert_eq!(
        Refusal::NotAnId.to_string(),
        "instance n'est pas un identifiant"
    );
    assert_eq!(
        Refusal::NoOpenCombat.to_string(),
        "aucun combat en cours sur cette carte"
    );
}
