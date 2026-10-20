//! `table.resetBattlemap`'s judgement before a command is queued: what the
//! DM sends, read ([`request`]), and made the deletions of what the instance
//! holds ([`prepare`]).

use reset_battlemap::{Map, Refusal, Request, Row, names, prepare, request};
use serde_json::{Value, json};

const INSTANCE: &str = "b1a00000-0000-4000-8000-000000000001";
const TOKEN_1: &str = "70000000-0000-4000-8000-000000000001";
const TOKEN_2: &str = "70000000-0000-4000-8000-000000000002";
const COMBAT: &str = "c0000000-0000-4000-8000-000000000001";
const ENTRY_1: &str = "1e000000-0000-4000-8000-000000000001";
const ENTRY_2: &str = "1e000000-0000-4000-8000-000000000002";
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

fn asked() -> Request {
    Request {
        instance: INSTANCE.to_string(),
    }
}

fn ids(texts: &[&str]) -> Vec<String> {
    texts.iter().map(ToString::to_string).collect()
}

/// The instance with two tokens in a combat.
fn set() -> Map {
    Map {
        instance: true,
        tokens: ids(&[TOKEN_2, TOKEN_1]),
        combats: ids(&[COMBAT]),
        entries: ids(&[ENTRY_2, ENTRY_1]),
    }
}

#[test]
fn the_manifest_names_this_data_capability_a_delete_touching_the_stamp_and_the_rows_ids() {
    let manifest = manifest();
    assert_eq!(manifest["dataCapability"], "table.resetBattlemap");
    assert_eq!(manifest["owner"], "dataguard");
    assert_eq!(manifest["effect"], "delete");
    assert_eq!(manifest["mode"], "overwrite");
    assert_eq!(
        manifest["touches"],
        json!([
            "BattlemapInstance.resetAt",
            "BattlemapInstance.tokenId",
            "BattlemapInstance.combatId",
            "BattlemapInstance.initiativeEntryId"
        ])
    );
    assert_eq!(manifest["target"]["aggregate"], "BattlemapInstance");
    assert_eq!(
        manifest["invariants"],
        json!(["instance-isolated", "state-persisted-reset"])
    );
    assert!(names("table.resetBattlemap"));
    assert!(names("table.resetBattlemap@1"));
    assert!(!names("table.endCombat@1"));
    assert!(!names("table.resetBattlemapNow"));
}

#[test]
fn the_instance_is_the_target_else_the_payload_s_lowercase() {
    let upper = INSTANCE.to_uppercase();
    let other = "b1a00000-0000-4000-8000-000000000002";
    let named = row(json!({ "instance": other }));
    assert_eq!(request(&named, Some(&upper)), Ok(asked()));
    assert_eq!(
        request(&row(json!({ "instance": format!(" {upper} ") })), None),
        Ok(asked())
    );
    // A blank target names none: the payload's is the instance.
    assert_eq!(
        request(&row(json!({ "instance": INSTANCE })), Some("  ")),
        Ok(asked())
    );
}

#[test]
fn an_instance_that_is_no_uuid_is_refused() {
    for (wire, target) in [
        (json!({}), None),
        (json!({ "instance": "taverne" }), None),
        (json!({ "instance": 7 }), None),
        (json!({ "instance": INSTANCE }), Some("taverne")),
        // Hex and hyphens, but not where a uuid has them.
        (json!({}), Some("b1a00000-0000-4000-8000-00000000-001")),
        (json!({}), Some("b1a000000000-4000-8000-0000000000012")),
        (json!({}), Some("g1a00000-0000-4000-8000-000000000001")),
    ] {
        assert_eq!(
            request(&row(wire.clone()), target),
            Err(Refusal::NotAnId),
            "{wire} {target:?}"
        );
    }
}

#[test]
fn each_entry_then_each_combat_then_each_token_is_deleted_by_id_carrying_the_stamp() {
    let deletions = prepare(&asked(), &set(), NOW).expect("prepared");
    let order: Vec<&str> = deletions.iter().map(|(id, _)| id.as_str()).collect();
    assert_eq!(order, [ENTRY_1, ENTRY_2, COMBAT, TOKEN_1, TOKEN_2]);
    // Each carries the reset's size, which the stamp waits for.
    let stamp = row(json!({
        "battlemapInstanceId": INSTANCE,
        "resetAt": NOW,
        "resetSize": 5,
    }));
    for (id, payload) in &deletions {
        assert_eq!(*payload, stamp, "{id}");
    }
}

#[test]
fn an_id_named_twice_in_any_case_is_deleted_once_and_the_instance_never() {
    let map = Map {
        instance: true,
        tokens: ids(&[TOKEN_1, &TOKEN_1.to_uppercase(), INSTANCE, "jeton"]),
        combats: ids(&[COMBAT, &format!(" {COMBAT} "), TOKEN_1]),
        entries: ids(&[]),
    };
    let deletions = prepare(&asked(), &map, NOW).expect("prepared");
    let order: Vec<&str> = deletions.iter().map(|(id, _)| id.as_str()).collect();
    // A token named a combat too is deleted once, as a combat.
    assert_eq!(order, [TOKEN_1, COMBAT]);
}

#[test]
fn an_ended_combat_alone_is_wiped_too() {
    let map = Map {
        instance: true,
        combats: ids(&[COMBAT]),
        ..Map::default()
    };
    let deletions = prepare(&asked(), &map, NOW).expect("prepared");
    assert_eq!(deletions.len(), 1);
    assert_eq!(deletions[0].0, COMBAT);
}

#[test]
fn an_instance_the_tables_do_not_hold_or_with_nothing_to_wipe_is_refused() {
    let unknown = Map {
        instance: false,
        ..set()
    };
    assert_eq!(
        prepare(&asked(), &unknown, NOW),
        Err(Refusal::UnknownInstance)
    );
    let blank = Map {
        instance: true,
        ..Map::default()
    };
    assert_eq!(prepare(&asked(), &blank, NOW), Err(Refusal::NothingToReset));
    // Only the instance itself, or what is no id, is nothing to wipe.
    let nothing = Map {
        instance: true,
        tokens: ids(&[INSTANCE, "jeton"]),
        ..Map::default()
    };
    assert_eq!(
        prepare(&asked(), &nothing, NOW),
        Err(Refusal::NothingToReset)
    );
}

#[test]
fn each_refusal_says_why_in_words() {
    assert_eq!(
        Refusal::NotAnId.to_string(),
        "instance n'est pas un identifiant"
    );
    assert_eq!(
        Refusal::UnknownInstance.to_string(),
        "aucune instance de carte de ce nom"
    );
    assert_eq!(
        Refusal::NothingToReset.to_string(),
        "la carte n'a rien à réinitialiser"
    );
}
