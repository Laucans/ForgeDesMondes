//! `request` and `prepare`: what the DM sends made the command written on
//! `BattlemapInstance/<the token's id>` — or refused before it is queued.

use rename_token::{DATA_CAPABILITY, Refusal, Request, Row, partition, prepare, request};
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
        "hpCurrent": 11,
        "hpMax": 11,
        "conditions": [],
    }))
}

/// Bandit 2 renamed `name`, on the tavern's instance.
fn renaming(name: Value) -> Result<Request, Refusal> {
    request(
        &row(json!({ "token": BANDIT_2, "name": name })),
        Some(INSTANCE),
    )
}

#[test]
fn the_data_capability_is_the_one_its_manifest_names() {
    let manifest: Value =
        serde_json::from_str(include_str!("../data-capability.json")).expect("JSON");
    assert_eq!(manifest["dataCapability"], DATA_CAPABILITY);
    assert_eq!(partition(BANDIT_2), format!("BattlemapInstance/{BANDIT_2}"));
}

#[test]
fn the_token_is_the_payload_s_on_the_targeted_instance_and_its_name_is_trimmed() {
    assert_eq!(
        renaming(json!("  Bandit blessé ")),
        Ok(Request {
            token: BANDIT_2.to_owned(),
            instance: Some(INSTANCE.to_owned()),
            name: "Bandit blessé".to_owned(),
        })
    );
    // With no target at all, the token stands on no instance named.
    let wire = row(json!({ "token": BANDIT_2, "name": "Bandit blessé" }));
    assert_eq!(request(&wire, None).expect("asked").instance, None);
}

#[test]
fn a_token_targeted_itself_stands_on_no_instance_named() {
    let wire = row(json!({ "name": "Bandit blessé" }));
    let asked = request(&wire, Some(BANDIT_2)).expect("asked");
    assert_eq!(asked.token, BANDIT_2);
    assert_eq!(asked.instance, None);
    // Named twice, as the payload's token and as the target, it is one.
    let wire = row(json!({ "token": BANDIT_2, "name": "Bandit blessé" }));
    assert_eq!(
        request(&wire, Some(BANDIT_2)).expect("asked").instance,
        None
    );
}

#[test]
fn ids_are_read_in_lowercase_as_the_tables_hold_them() {
    let wire = row(json!({ "token": BANDIT_2.to_uppercase(), "name": "Bandit blessé" }));
    let asked = request(&wire, Some(&INSTANCE.to_uppercase())).expect("asked");
    assert_eq!(asked.token, BANDIT_2);
    assert_eq!(asked.instance.as_deref(), Some(INSTANCE));
}

#[test]
fn an_empty_name_is_refused_and_so_is_one_of_blanks_or_none() {
    for name in [
        json!(""),
        json!("   \t"),
        json!(null),
        json!(7),
        json!(["Bandit"]),
    ] {
        assert_eq!(renaming(name.clone()), Err(Refusal::NoName), "{name}");
    }
    let wire = row(json!({ "token": BANDIT_2 }));
    assert_eq!(request(&wire, Some(INSTANCE)), Err(Refusal::NoName));
}

#[test]
fn a_null_token_is_no_token_the_target_is_the_token() {
    let wire = row(json!({ "token": null, "name": "Bandit blessé" }));
    let asked = request(&wire, Some(BANDIT_2)).expect("asked");
    assert_eq!(asked.token, BANDIT_2);
    assert_eq!(asked.instance, None);
}

#[test]
fn a_name_the_text_column_cannot_hold_is_refused() {
    assert_eq!(renaming(json!("Bandit\u{0}2")), Err(Refusal::Nul));
}

#[test]
fn a_token_or_an_instance_that_is_no_uuid_is_refused_by_its_name() {
    let wire = row(json!({ "token": "bandit-2", "name": "Bandit blessé" }));
    assert_eq!(
        request(&wire, Some(INSTANCE)),
        Err(Refusal::NotAnId("token"))
    );
    let wire = row(json!({ "token": 2, "name": "Bandit blessé" }));
    assert_eq!(
        request(&wire, Some(INSTANCE)),
        Err(Refusal::NotAnId("token"))
    );
    let wire = row(json!({ "token": BANDIT_2, "name": "Bandit blessé" }));
    assert_eq!(
        request(&wire, Some("la-taverne")),
        Err(Refusal::NotAnId("instance"))
    );
    // No token in the payload and no target: nothing names the token.
    let wire = row(json!({ "name": "Bandit blessé" }));
    assert_eq!(request(&wire, None), Err(Refusal::NotAnId("token")));
    assert_eq!(
        request(&wire, Some("la-taverne")),
        Err(Refusal::NotAnId("token"))
    );
}

#[test]
fn multi_drop_numbering_a_rename_writes_the_name_alone() {
    let asked = renaming(json!(" Bandit blessé ")).expect("asked");
    assert_eq!(
        prepare(&asked, &bandit_2()),
        Ok(row(json!({ "name": "Bandit blessé" })))
    );
    // The token as its own target, ids in another case: the same command.
    let mut held = bandit_2();
    held.insert("tokenId".to_owned(), json!(BANDIT_2.to_uppercase()));
    held.insert("tokenInstanceId".to_owned(), json!(INSTANCE.to_uppercase()));
    let asked = request(&row(json!({ "name": "Bandit blessé" })), Some(BANDIT_2)).expect("asked");
    assert_eq!(
        prepare(&asked, &held),
        Ok(row(json!({ "name": "Bandit blessé" })))
    );
}

#[test]
fn instance_isolated_a_token_of_another_instance_is_none_to_rename() {
    let wire = row(json!({ "token": BANDIT_2, "name": "Bandit blessé" }));
    let asked = request(&wire, Some(AUTRE_INSTANCE)).expect("asked");
    assert_eq!(prepare(&asked, &bandit_2()), Err(Refusal::UnknownToken));
}

#[test]
fn a_token_the_engine_does_not_hold_is_none_to_rename() {
    let asked = renaming(json!("Bandit blessé")).expect("asked");
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
}

#[test]
fn only_an_unknown_token_is_not_found_and_every_refusal_says_why() {
    assert!(Refusal::UnknownToken.not_found());
    for refusal in [Refusal::NotAnId("token"), Refusal::NoName, Refusal::Nul] {
        assert!(!refusal.not_found(), "{refusal:?}");
    }
    assert_eq!(
        Refusal::NotAnId("token").to_string(),
        "token n'est pas un identifiant"
    );
    assert_eq!(
        Refusal::NoName.to_string(),
        "multi-drop-numbering: un token a un nom"
    );
    assert_eq!(Refusal::Nul.to_string(), "name contient un caractère nul");
    assert_eq!(
        Refusal::UnknownToken.to_string(),
        "ce token n'est pas sur cette instance"
    );
}
