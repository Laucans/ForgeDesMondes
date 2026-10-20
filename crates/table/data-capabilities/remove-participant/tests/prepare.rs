//! `request` and `prepare`: what the DM sends read, the entry checked
//! against its combat and the instance, and the removal made the deletion of
//! the entry's partition — no field written.

use remove_participant::{
    Prepared, Refusal, Request, Row, combat_of, names, partition, prepare, request,
};
use serde_json::{Value, json};

const INSTANCE: &str = "b1000000-0000-4000-8000-000000000001";
const AUTRE_INSTANCE: &str = "b1000000-0000-4000-8000-000000000002";
const COMBAT: &str = "c0000000-0000-4000-8000-000000000001";
const AUTRE_COMBAT: &str = "c0000000-0000-4000-8000-000000000002";
const ENTREE: &str = "1e000000-0000-4000-8000-000000000002";
const AUTRE_ENTREE: &str = "1e000000-0000-4000-8000-000000000001";
const JETON: &str = "70000000-0000-4000-8000-000000000002";

fn row(value: Value) -> Row {
    let Value::Object(fields) = value else {
        panic!("a row is an object");
    };
    fields.into_iter().collect()
}

fn entry() -> Row {
    row(json!({
        "initiativeEntryId": ENTREE.to_uppercase(),
        "initiativeEntryCombatId": COMBAT.to_uppercase(),
        "initiativeEntryTokenId": JETON,
        "initiative": 15,
        "tiebreak": 0.5,
        "rank": 2,
    }))
}

fn combat() -> Row {
    row(json!({
        "combatId": COMBAT,
        "combatInstanceId": INSTANCE.to_uppercase(),
        "round": 2,
        "currentEntryId": AUTRE_ENTREE,
        "startedAt": "2026-10-20T20:05:00Z",
    }))
}

fn asked(instance: Option<&str>) -> Request {
    Request {
        entry: ENTREE.to_string(),
        instance: instance.map(str::to_string),
    }
}

#[test]
fn the_id_is_named_with_or_without_its_version() {
    assert!(names("table.removeParticipant"));
    assert!(names("table.removeParticipant@1"));
    assert!(!names("table.removeToken"));
}

#[test]
fn an_entry_is_named_in_the_instance_targeted_or_as_the_target_itself() {
    let on = request(
        &row(json!({ "entry": ENTREE.to_uppercase() })),
        Some(INSTANCE),
    );
    assert_eq!(on, Ok(asked(Some(INSTANCE))));
    assert_eq!(request(&Row::new(), Some(ENTREE)), Ok(asked(None)));
    assert_eq!(
        request(&row(json!({ "entry": ENTREE })), None),
        Ok(asked(None))
    );
    // The entry named, and targeted too: no instance to check.
    assert_eq!(
        request(&row(json!({ "entry": ENTREE })), Some(ENTREE)),
        Ok(asked(None))
    );
}

#[test]
fn an_entry_is_its_own_partition_of_the_aggregate() {
    assert_eq!(partition(ENTREE), format!("BattlemapInstance/{ENTREE}"));
}

#[test]
fn what_is_no_uuid_is_refused() {
    let refused = |payload: Value, target: Option<&str>| request(&row(payload), target);
    assert_eq!(refused(json!({}), None), Err(Refusal::NotAnId("entry")));
    assert_eq!(
        refused(json!({ "entry": "kael" }), Some(INSTANCE)),
        Err(Refusal::NotAnId("entry"))
    );
    assert_eq!(
        refused(json!({ "entry": null }), Some(INSTANCE)),
        Err(Refusal::NotAnId("entry"))
    );
    assert_eq!(
        refused(json!({ "entry": ENTREE }), Some("taverne")),
        Err(Refusal::NotAnId("instance"))
    );
    assert_eq!(
        refused(json!({}), Some("kael")),
        Err(Refusal::NotAnId("entry"))
    );
    // A uuid's shape with a digit too many is none.
    assert_eq!(
        refused(json!({ "entry": format!("{ENTREE}a") }), None),
        Err(Refusal::NotAnId("entry"))
    );
}

#[test]
fn an_entry_of_a_combat_of_the_instance_is_removed_and_no_field_written() {
    assert_eq!(combat_of(&entry()), Some(COMBAT.to_string()));
    let removal = Prepared {
        target_id: ENTREE.to_string(),
        payload: Row::new(),
    };
    assert_eq!(
        prepare(&asked(Some(INSTANCE)), &entry(), &combat()),
        Ok(removal.clone())
    );
    // The entry as its own target: whatever instance its combat is of.
    assert_eq!(prepare(&asked(None), &entry(), &combat()), Ok(removal));
}

#[test]
fn an_entry_the_engine_does_not_hold_or_of_another_instance_is_none_to_remove() {
    let unknown = Err(Refusal::UnknownEntry);
    // No entry, a token's row, another entry's row.
    assert_eq!(combat_of(&Row::new()), None);
    assert_eq!(prepare(&asked(None), &Row::new(), &combat()), unknown);
    let token = row(json!({ "tokenId": ENTREE, "tokenInstanceId": INSTANCE }));
    assert_eq!(prepare(&asked(None), &token, &combat()), unknown);
    let mut other = entry();
    other.insert(
        "initiativeEntryId".to_string(),
        json!("1e000000-0000-4000-8000-000000000009"),
    );
    assert_eq!(prepare(&asked(None), &other, &combat()), unknown);
    // A combat the engine does not hold, or not the entry's.
    assert_eq!(prepare(&asked(None), &entry(), &Row::new()), unknown);
    let mut elsewhere = combat();
    elsewhere.insert("combatId".to_string(), json!(AUTRE_COMBAT));
    assert_eq!(prepare(&asked(None), &entry(), &elsewhere), unknown);
    // The combat of another instance than the one targeted.
    assert_eq!(
        prepare(&asked(Some(AUTRE_INSTANCE)), &entry(), &combat()),
        unknown
    );
}

#[test]
fn an_entry_of_a_combat_that_ended_is_out_of_no_running_order() {
    let mut ended = combat();
    ended.insert("endedAt".to_string(), json!("2026-10-20T21:00:00Z"));
    assert_eq!(
        prepare(&asked(Some(INSTANCE)), &entry(), &ended),
        Err(Refusal::CombatEnded)
    );
    // An end written back to null is none.
    ended.insert("endedAt".to_string(), Value::Null);
    assert!(prepare(&asked(Some(INSTANCE)), &entry(), &ended).is_ok());
}

#[test]
fn the_entry_that_holds_the_turn_is_not_removed_until_the_turn_is_passed() {
    let mut held = combat();
    held.insert("currentEntryId".to_string(), json!(ENTREE.to_uppercase()));
    assert_eq!(
        prepare(&asked(Some(INSTANCE)), &entry(), &held),
        Err(Refusal::HoldsTurn)
    );
    // The turn is another's, or no one's once nextTurn has passed it.
    assert!(prepare(&asked(Some(INSTANCE)), &entry(), &combat()).is_ok());
    let mut passed = combat();
    passed.insert("currentEntryId".to_string(), Value::Null);
    assert!(prepare(&asked(Some(INSTANCE)), &entry(), &passed).is_ok());
}

#[test]
fn every_refusal_says_why_in_words() {
    for refusal in [
        Refusal::NotAnId("entry"),
        Refusal::UnknownEntry,
        Refusal::CombatEnded,
        Refusal::HoldsTurn,
    ] {
        assert!(!refusal.to_string().trim().is_empty(), "{refusal:?}");
    }
    assert!(Refusal::NotAnId("entry").to_string().contains("entry"));
}
