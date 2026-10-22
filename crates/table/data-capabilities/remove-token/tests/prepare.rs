//! `request` and `prepare`: what the DM sends made the deletion of
//! `BattlemapInstance/<the token's id>` — or refused before it is queued.

use remove_token::{DATA_CAPABILITY, Refusal, Request, Row, partition, prepare, request};
use serde_json::{Value, json};

const INSTANCE: &str = "b1000000-0000-4000-8000-000000000001";
const AUTRE_INSTANCE: &str = "b1000000-0000-4000-8000-000000000002";
const BANDIT: &str = "e1000000-0000-4000-8000-0000000000b1";
const BANDIT_2: &str = "70000000-0000-4000-8000-000000000002";

fn row(value: Value) -> Row {
    let Value::Object(map) = value else {
        panic!("a row is an object");
    };
    map.into_iter().collect()
}

/// Bandit 2 as the engine holds it, on the tavern's instance.
fn bandit_2() -> Row {
    row(json!({
        "tokenId": BANDIT_2,
        "tokenInstanceId": INSTANCE,
        "characterId": BANDIT,
        "number": 2,
        "name": "Bandit 2",
        "q": 5,
        "r": 5,
        "visible": true,
        "hpCurrent": 0,
        "hpMax": 11,
        "conditions": [],
    }))
}

/// Bandit 2 removed from the tavern's instance.
fn removing() -> Request {
    request(&row(json!({ "token": BANDIT_2 })), Some(INSTANCE)).expect("asked")
}

#[test]
fn the_data_capability_is_the_one_its_manifest_names_a_deletion() {
    let manifest: Value =
        serde_json::from_str(include_str!("../data-capability.json")).expect("JSON");
    assert_eq!(manifest["dataCapability"], DATA_CAPABILITY);
    assert_eq!(manifest["effect"], "delete");
    assert_eq!(partition(BANDIT_2), format!("BattlemapInstance/{BANDIT_2}"));
}

#[test]
fn the_token_is_the_payload_s_on_the_targeted_instance() {
    assert_eq!(
        removing(),
        Request {
            token: BANDIT_2.to_owned(),
            instance: Some(INSTANCE.to_owned()),
        }
    );
    // With no target at all, the token stands on no instance named.
    let wire = row(json!({ "token": BANDIT_2 }));
    assert_eq!(request(&wire, None).expect("asked").instance, None);
}

#[test]
fn a_token_targeted_itself_stands_on_no_instance_named() {
    for wire in [
        json!({}),
        json!({ "token": null }),
        json!({ "token": BANDIT_2 }),
    ] {
        let asked = request(&row(wire.clone()), Some(BANDIT_2)).expect("asked");
        assert_eq!(asked.token, BANDIT_2, "{wire}");
        assert_eq!(asked.instance, None, "{wire}");
    }
}

#[test]
fn ids_are_read_in_lowercase_as_the_tables_hold_them() {
    let wire = row(json!({ "token": format!(" {} ", BANDIT_2.to_uppercase()) }));
    let asked = request(&wire, Some(&INSTANCE.to_uppercase())).expect("asked");
    assert_eq!(asked.token, BANDIT_2);
    assert_eq!(asked.instance.as_deref(), Some(INSTANCE));
}

#[test]
fn a_token_or_an_instance_that_is_no_uuid_is_refused_by_its_name() {
    for token in [json!("bandit-2"), json!(2), json!([BANDIT_2])] {
        let wire = row(json!({ "token": token }));
        assert_eq!(
            request(&wire, Some(INSTANCE)),
            Err(Refusal::NotAnId("token")),
            "{token}"
        );
    }
    let wire = row(json!({ "token": BANDIT_2 }));
    assert_eq!(
        request(&wire, Some("la-taverne")),
        Err(Refusal::NotAnId("instance"))
    );
    // No token in the payload and no target: nothing names the token.
    assert_eq!(request(&Row::new(), None), Err(Refusal::NotAnId("token")));
    assert_eq!(
        request(&Row::new(), Some("la-taverne")),
        Err(Refusal::NotAnId("token"))
    );
}

#[test]
fn a_token_of_the_instance_named_may_be_removed() {
    assert_eq!(prepare(&removing(), &bandit_2()), Ok(()));
    // The token as its own target, ids held in another case: the same.
    let mut held = bandit_2();
    held.insert("tokenId".to_owned(), json!(BANDIT_2.to_uppercase()));
    held.insert("tokenInstanceId".to_owned(), json!(INSTANCE.to_uppercase()));
    let asked = request(&Row::new(), Some(BANDIT_2)).expect("asked");
    assert_eq!(prepare(&asked, &held), Ok(()));
}

#[test]
fn instance_isolated_a_token_of_another_instance_is_none_to_remove() {
    let wire = row(json!({ "token": BANDIT_2 }));
    let asked = request(&wire, Some(AUTRE_INSTANCE)).expect("asked");
    assert_eq!(prepare(&asked, &bandit_2()), Err(Refusal::UnknownToken));
}

#[test]
fn a_token_the_engine_does_not_hold_is_none_to_remove() {
    let asked = removing();
    // Never written, or removed: the partition reads empty.
    assert_eq!(prepare(&asked, &Row::new()), Err(Refusal::UnknownToken));
    // A row on the partition that is no token: the instance's own.
    let instance = row(json!({ "battlemapInstanceId": BANDIT_2, "sessionId": INSTANCE }));
    assert_eq!(prepare(&asked, &instance), Err(Refusal::UnknownToken));
    // A row that names another token than its partition's.
    let mut other = bandit_2();
    other.insert(
        "tokenId".to_owned(),
        json!("70000000-0000-4000-8000-000000000003"),
    );
    assert_eq!(prepare(&asked, &other), Err(Refusal::UnknownToken));
    // A token with no id of its own.
    let mut nameless = bandit_2();
    nameless.remove("tokenId");
    assert_eq!(prepare(&asked, &nameless), Err(Refusal::UnknownToken));
}

#[test]
fn only_an_unknown_token_is_not_found_and_every_refusal_says_why() {
    assert!(Refusal::UnknownToken.not_found());
    assert!(!Refusal::NotAnId("token").not_found());
    assert_eq!(
        Refusal::NotAnId("instance").to_string(),
        "instance n'est pas un identifiant"
    );
    assert_eq!(
        Refusal::UnknownToken.to_string(),
        "ce token n'est pas sur cette instance"
    );
}
