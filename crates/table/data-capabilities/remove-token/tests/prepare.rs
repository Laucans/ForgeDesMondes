//! `table.removeToken`'s judgement before a command is queued: what the DM
//! sends, read ([`request`]), and made the deletion of the token's partition
//! ([`prepare`]), which writes no field.

use remove_token::{Prepared, Refusal, Request, Row, names, partition, prepare, request};
use serde_json::{Value, json};

const INSTANCE: &str = "b1000000-0000-4000-8000-000000000001";
const AUTRE_INSTANCE: &str = "b1000000-0000-4000-8000-000000000002";
const BANDIT_2: &str = "70000000-0000-4000-8000-000000000004";

fn row(value: Value) -> Row {
    let Value::Object(fields) = value else {
        panic!("a row is an object");
    };
    fields.into_iter().collect()
}

/// Bandit 2, dead, as the engine holds it on its partition.
fn bandit() -> Row {
    row(json!({
        "tokenId": BANDIT_2,
        "tokenInstanceId": INSTANCE,
        "characterId": "e1000000-0000-4000-8000-000000000003",
        "number": 2,
        "name": "Bandit 2",
        "q": 4,
        "r": 5,
        "visible": true,
        "hpCurrent": 0,
        "hpMax": 11,
        "conditions": [],
    }))
}

fn manifest() -> Value {
    serde_json::from_str(include_str!("../data-capability.json")).expect("contract F")
}

#[test]
fn the_manifest_names_this_data_capability_a_delete_of_the_token_s_partition() {
    let manifest = manifest();
    assert_eq!(manifest["dataCapability"], "table.removeToken");
    assert_eq!(manifest["owner"], "dataguard");
    assert_eq!(manifest["effect"], "delete");
    assert_eq!(manifest["touches"], json!(["BattlemapInstance.tokenId"]));
    assert_eq!(manifest["target"]["aggregate"], "BattlemapInstance");
    assert_eq!(manifest["target"]["id"], "$token");
    assert_eq!(manifest["mode"], "confirm_on_stale");
    assert_eq!(manifest["invariants"], json!(["instance-isolated"]));
    assert!(names("table.removeToken"));
    assert!(names("table.removeToken@1"));
    assert!(!names("table.removeParticipant@1"));
}

#[test]
fn a_token_of_the_instance_is_its_partition_deleted_and_no_field_written() {
    let wire = row(json!({ "token": BANDIT_2 }));
    let asked = request(&wire, Some(INSTANCE)).expect("a request");
    assert_eq!(
        asked,
        Request {
            token: BANDIT_2.to_string(),
            instance: Some(INSTANCE.to_string()),
        }
    );
    assert_eq!(
        prepare(&asked, &bandit()),
        Ok(Prepared {
            target_id: BANDIT_2.to_string(),
            payload: Row::new(),
        })
    );
    assert_eq!(partition(BANDIT_2), format!("BattlemapInstance/{BANDIT_2}"));
}

#[test]
fn no_rank_nor_turn_is_written_whatever_the_dm_sends_beside_it() {
    let wire = row(json!({
        "token": BANDIT_2,
        "rank": 1,
        "currentEntryId": "1e000000-0000-4000-8000-000000000003",
        "hpCurrent": 5,
    }));
    let asked = request(&wire, Some(INSTANCE)).expect("a request");
    assert_eq!(
        prepare(&asked, &bandit()).expect("prepared").payload,
        Row::new()
    );
}

#[test]
fn the_token_may_be_the_target_named_in_any_case() {
    let upper = BANDIT_2.to_ascii_uppercase();
    let asked = request(&Row::new(), Some(&upper)).expect("a request");
    assert_eq!(asked.token, BANDIT_2);
    assert_eq!(asked.instance, None);
    // Named twice, as the token and as the target: no instance named.
    let wire = row(json!({ "token": upper }));
    let asked = request(&wire, Some(BANDIT_2)).expect("a request");
    assert_eq!(asked.instance, None);
    assert!(prepare(&asked, &bandit()).is_ok());
}

#[test]
fn what_names_no_uuid_is_refused() {
    for (wire, target, refusal) in [
        (
            json!({ "token": "bandit-2" }),
            Some(INSTANCE),
            Refusal::NotAnId("token"),
        ),
        (
            json!({ "token": &BANDIT_2[..35] }),
            Some(INSTANCE),
            Refusal::NotAnId("token"),
        ),
        (
            json!({ "token": format!("{BANDIT_2}0") }),
            Some(INSTANCE),
            Refusal::NotAnId("token"),
        ),
        (
            json!({ "token": null }),
            Some(INSTANCE),
            Refusal::NotAnId("token"),
        ),
        (
            json!({ "token": 2 }),
            Some(INSTANCE),
            Refusal::NotAnId("token"),
        ),
        (
            json!({ "token": BANDIT_2 }),
            Some("taverne"),
            Refusal::NotAnId("instance"),
        ),
        (json!({}), None, Refusal::NotAnId("token")),
        (json!({}), Some("  "), Refusal::NotAnId("token")),
    ] {
        assert_eq!(request(&row(wire.clone()), target), Err(refusal), "{wire}");
    }
}

#[test]
fn instance_isolated_a_token_the_engine_does_not_hold_on_the_instance_is_none_to_remove() {
    let asked = |target: &str| {
        request(&row(json!({ "token": BANDIT_2 })), Some(target)).expect("a request")
    };
    // On another instance.
    assert_eq!(
        prepare(&asked(AUTRE_INSTANCE), &bandit()),
        Err(Refusal::UnknownToken)
    );
    // Not held at all.
    assert_eq!(
        prepare(&asked(INSTANCE), &Row::new()),
        Err(Refusal::UnknownToken)
    );
    // A row of another token, or the instance's own row.
    let mut other = bandit();
    other.insert("tokenId".to_string(), json!(AUTRE_INSTANCE));
    assert_eq!(
        prepare(&asked(INSTANCE), &other),
        Err(Refusal::UnknownToken)
    );
    let instance = row(json!({ "battlemapInstanceId": BANDIT_2, "tokenInstanceId": INSTANCE }));
    assert_eq!(
        prepare(&asked(INSTANCE), &instance),
        Err(Refusal::UnknownToken)
    );
    // The instance the engine holds in another case is the same one.
    let mut upper = bandit();
    upper.insert(
        "tokenInstanceId".to_string(),
        json!(INSTANCE.to_ascii_uppercase()),
    );
    assert!(prepare(&asked(INSTANCE), &upper).is_ok());
}

#[test]
fn every_refusal_says_why_in_words() {
    assert_eq!(
        Refusal::NotAnId("token").to_string(),
        "token n'est pas un identifiant"
    );
    assert_eq!(
        Refusal::UnknownToken.to_string(),
        "ce token n'est pas sur cette instance"
    );
}
