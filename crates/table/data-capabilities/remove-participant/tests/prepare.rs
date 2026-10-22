//! table.removeParticipant's reading of what the DM sends and its judgement
//! of the entry against its row and its combat's: the participant named by
//! its entry or its token, under `participant` beside the instance as the
//! target, or the entry itself as the target; no uuid refused; an entry of
//! another combat, another instance, an ended combat, or one holding the
//! turn, refused.

use remove_participant::{
    Named, Refusal, Request, Row, combat_of, judge, named, partition, request,
};
use serde_json::{Value, json};

const INSTANCE: &str = "b1000000-0000-4000-8000-000000000001";
const AUTRE_INSTANCE: &str = "b1000000-0000-4000-8000-000000000002";
const COMBAT: &str = "c0000000-0000-4000-8000-000000000001";
const AUTRE_COMBAT: &str = "c0000000-0000-4000-8000-000000000002";
const ENTRY: &str = "ee000000-0000-4000-8000-000000000002";
const CURRENT: &str = "ee000000-0000-4000-8000-000000000001";
const TOKEN: &str = "70000000-0000-4000-8000-000000000002";

fn row(value: Value) -> Row {
    let Value::Object(map) = value else {
        unreachable!("a row is an object");
    };
    map.into_iter().collect()
}

fn entry() -> Row {
    row(json!({
        "initiativeEntryId": ENTRY,
        "initiativeEntryCombatId": COMBAT,
        "initiativeEntryTokenId": TOKEN,
        "rank": 2,
    }))
}

fn combat() -> Row {
    row(json!({
        "combatId": COMBAT,
        "combatInstanceId": INSTANCE,
        "round": 1,
        "currentEntryId": CURRENT,
        "startedAt": "2026-10-21T10:00:00Z",
    }))
}

fn with(mut row: Row, field: &str, value: Value) -> Row {
    row.insert(field.to_owned(), value);
    row
}

#[test]
fn the_partition_is_the_aggregate_s_row_of_that_id() {
    assert_eq!(partition(ENTRY), format!("BattlemapInstance/{ENTRY}"));
}

#[test]
fn a_participant_beside_the_instance_targeted_is_read_lowercase_and_trimmed() {
    let wire = row(json!({ "participant": format!(" {} ", ENTRY.to_uppercase()) }));
    assert_eq!(
        request(&wire, Some(&INSTANCE.to_uppercase())),
        Ok(Request {
            participant: ENTRY.to_owned(),
            instance: Some(INSTANCE.to_owned()),
        })
    );
}

#[test]
fn with_no_participant_the_target_is_the_entry_in_no_instance_named() {
    for wire in [json!({}), json!({ "participant": null })] {
        assert_eq!(
            request(&row(wire), Some(ENTRY)),
            Ok(Request {
                participant: ENTRY.to_owned(),
                instance: None,
            })
        );
    }
    // The entry named, as its own target too.
    assert_eq!(
        request(&row(json!({ "participant": ENTRY })), Some(ENTRY)),
        Ok(Request {
            participant: ENTRY.to_owned(),
            instance: None,
        })
    );
    // Named, with no target: in no instance named.
    assert_eq!(
        request(&row(json!({ "participant": ENTRY })), None),
        Ok(Request {
            participant: ENTRY.to_owned(),
            instance: None,
        })
    );
}

#[test]
fn an_id_that_is_no_uuid_is_refused() {
    for (wire, target, field) in [
        (
            json!({ "participant": "bandit-2" }),
            Some(INSTANCE),
            "participant",
        ),
        (json!({ "participant": 2 }), Some(INSTANCE), "participant"),
        (
            json!({ "participant": ENTRY }),
            Some("la-taverne"),
            "instance",
        ),
        (json!({}), Some("la-taverne"), "participant"),
        (json!({}), None, "participant"),
    ] {
        let refusal = request(&row(wire), target).expect_err("no uuid");
        assert_eq!(refusal, Refusal::NotAnId(field));
        assert_eq!(
            refusal.to_string(),
            format!("{field} n'est pas un identifiant")
        );
        assert!(!refusal.not_found());
    }
}

#[test]
fn a_row_names_an_entry_a_token_or_nothing() {
    assert_eq!(named(&entry(), ENTRY), Named::Entry);
    assert_eq!(
        named(&row(json!({ "tokenId": TOKEN })), TOKEN),
        Named::Token
    );
    // The ids are compared as the tables write them.
    assert_eq!(
        named(&row(json!({ "tokenId": TOKEN.to_uppercase() })), TOKEN),
        Named::Token
    );
    assert_eq!(named(&Row::new(), ENTRY), Named::Nothing);
    // A row of another id is not the one named.
    assert_eq!(named(&entry(), CURRENT), Named::Nothing);
    assert_eq!(
        named(&row(json!({ "tokenId": TOKEN })), ENTRY),
        Named::Nothing
    );
}

#[test]
fn an_entry_is_of_the_combat_it_names() {
    assert_eq!(combat_of(&entry()), Some(COMBAT.to_owned()));
    assert_eq!(combat_of(&row(json!({ "tokenId": TOKEN }))), None);
}

#[test]
fn an_entry_of_the_open_combat_not_holding_the_turn_is_taken_out() {
    assert_eq!(judge(ENTRY, Some(INSTANCE), &entry(), &combat()), Ok(()));
    assert_eq!(judge(ENTRY, None, &entry(), &combat()), Ok(()));
    // A combat with no turn current, or an end of null, is running.
    let open = with(
        with(combat(), "currentEntryId", Value::Null),
        "endedAt",
        Value::Null,
    );
    assert_eq!(judge(ENTRY, Some(INSTANCE), &entry(), &open), Ok(()));
}

#[test]
fn an_entry_not_held_of_another_combat_or_instance_is_unknown() {
    for (id, instance, entry, combat) in [
        (ENTRY, Some(INSTANCE), Row::new(), combat()),
        (ENTRY, Some(INSTANCE), entry(), Row::new()),
        // Another entry's row.
        (CURRENT, Some(INSTANCE), entry(), combat()),
        // A combat that is not the entry's.
        (
            ENTRY,
            Some(INSTANCE),
            entry(),
            with(combat(), "combatId", json!(AUTRE_COMBAT)),
        ),
        // A combat of another instance than the one named.
        (ENTRY, Some(AUTRE_INSTANCE), entry(), combat()),
    ] {
        let refusal = judge(id, instance, &entry, &combat).expect_err("unknown");
        assert_eq!(refusal, Refusal::UnknownParticipant);
        assert!(refusal.not_found());
        assert_eq!(
            refusal.to_string(),
            "ce participant n'est pas dans un combat de cette instance"
        );
    }
}

#[test]
fn an_entry_of_an_ended_combat_is_out_of_no_running_order() {
    let ended = with(combat(), "endedAt", json!("2026-10-21T11:00:00Z"));
    let refusal = judge(ENTRY, Some(INSTANCE), &entry(), &ended).expect_err("ended");
    assert_eq!(refusal, Refusal::CombatEnded);
    assert!(!refusal.not_found());
    assert_eq!(refusal.to_string(), "ce combat est terminé");
}

#[test]
fn the_entry_holding_the_turn_is_passed_first() {
    let turn = with(combat(), "currentEntryId", json!(ENTRY.to_uppercase()));
    let refusal = judge(ENTRY, Some(INSTANCE), &entry(), &turn).expect_err("its turn");
    assert_eq!(refusal, Refusal::HoldsTurn);
    assert!(!refusal.not_found());
    assert_eq!(
        refusal.to_string(),
        "next-turn: ce participant a la main, passer le tour d'abord"
    );
}

#[test]
fn a_token_s_entry_unlisted_is_unprocessable() {
    assert!(!Refusal::Unlisted.not_found());
    assert_eq!(
        Refusal::Unlisted.to_string(),
        "les participants du combat n'ont pas pu être lus"
    );
}
