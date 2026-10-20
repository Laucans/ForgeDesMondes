//! `request` and `prepare`: what the DM sends made the command written on
//! `BattlemapInstance/<the token's id>` — or refused before it is queued.

use rename_token::{Prepared, Refusal, Request, Row, names, partition, prepare, request};
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
fn the_token_is_the_payload_s_on_the_targeted_instance_and_its_name_is_trimmed() {
    assert_eq!(
        renaming(json!("  Bandit blessé ")),
        Ok(Request {
            token: BANDIT_2.to_owned(),
            instance: Some(INSTANCE.to_owned()),
            name: "Bandit blessé".to_owned(),
        })
    );
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
        json!("   "),
        json!("\t\n"),
        Value::Null,
        json!(2),
        json!(["Bandit"]),
    ] {
        assert_eq!(renaming(name.clone()), Err(Refusal::NoName), "{name}");
    }
    let wire = row(json!({ "token": BANDIT_2 }));
    assert_eq!(request(&wire, Some(INSTANCE)), Err(Refusal::NoName));
}

#[test]
fn a_name_text_cannot_hold_is_refused() {
    assert_eq!(renaming(json!("Bandit\u{0}")), Err(Refusal::Nul));
}

#[test]
fn a_token_or_an_instance_that_is_not_a_uuid_is_refused() {
    for (wire, target, field) in [
        (
            json!({ "token": "bandit-2", "name": "X" }),
            Some(INSTANCE),
            "token",
        ),
        (json!({ "token": 7, "name": "X" }), Some(INSTANCE), "token"),
        (
            json!({ "token": BANDIT_2, "name": "X" }),
            Some("bmi-gue-s4"),
            "instance",
        ),
        (json!({ "name": "X" }), None, "token"),
        (json!({ "name": "X" }), Some("  "), "token"),
        // Hex digits alone are not a uuid: a uuid has 36 characters.
        (json!({ "name": "X" }), Some("70000000"), "token"),
    ] {
        assert_eq!(
            request(&row(wire.clone()), target),
            Err(Refusal::NotAnId(field)),
            "{wire} on {target:?}"
        );
    }
}

#[test]
fn a_rename_writes_the_name_alone_on_the_token_s_partition() {
    let asked = renaming(json!("Bandit blessé")).expect("asked");
    let Prepared { target_id, payload } = prepare(&asked, &bandit_2()).expect("prepared");
    assert_eq!(target_id, BANDIT_2);
    assert_eq!(payload, row(json!({ "name": "Bandit blessé" })));
    assert_eq!(
        partition(&target_id),
        format!("BattlemapInstance/{BANDIT_2}")
    );
}

#[test]
fn every_field_a_rename_writes_is_one_its_manifest_touches() {
    let manifest: Value =
        serde_json::from_str(include_str!("../data-capability.json")).expect("contract F");
    assert_eq!(manifest["effect"], "update");
    assert_eq!(manifest["touches"], json!(["BattlemapInstance.name"]));
    let asked = renaming(json!("Bandit blessé")).expect("asked");
    let Prepared { payload, .. } = prepare(&asked, &bandit_2()).expect("prepared");
    for field in payload.keys() {
        assert!(
            manifest["touches"]
                .as_array()
                .expect("touches")
                .contains(&json!(format!("BattlemapInstance.{field}"))),
            "{field} is written but not touched"
        );
    }
}

#[test]
fn a_token_the_engine_does_not_hold_on_the_instance_is_none_to_rename() {
    let asked = renaming(json!("Bandit blessé")).expect("asked");
    // No row at all, the instance's own row, another token's row.
    let mut other = bandit_2();
    other.insert(
        "tokenId".into(),
        json!("70000000-0000-4000-8000-000000000003"),
    );
    let instance = row(json!({ "battlemapInstanceId": INSTANCE, "sceneId": INSTANCE }));
    for base in [Row::new(), instance, other] {
        assert_eq!(prepare(&asked, &base), Err(Refusal::UnknownToken));
    }
    // A token of another instance than the one targeted.
    let mut elsewhere = bandit_2();
    elsewhere.insert("tokenInstanceId".into(), json!(AUTRE_INSTANCE));
    assert_eq!(prepare(&asked, &elsewhere), Err(Refusal::UnknownToken));
    // Targeted itself, it is renamed wherever it stands.
    let itself = request(&row(json!({ "name": "Bandit blessé" })), Some(BANDIT_2)).expect("asked");
    assert!(prepare(&itself, &elsewhere).is_ok());
}

#[test]
fn a_token_held_in_uppercase_is_the_same_token() {
    let asked = renaming(json!("Bandit blessé")).expect("asked");
    let mut upper = bandit_2();
    upper.insert("tokenId".into(), json!(BANDIT_2.to_uppercase()));
    upper.insert("tokenInstanceId".into(), json!(INSTANCE.to_uppercase()));
    assert!(prepare(&asked, &upper).is_ok());
}

#[test]
fn names_is_this_data_capability_with_or_without_its_version() {
    assert!(names("table.renameToken"));
    assert!(names("table.renameToken@1"));
    assert!(!names("table.renameTokens"));
    assert!(!names("table.placeTokens@1"));
}

#[test]
fn a_refusal_says_why_in_words() {
    let told: Vec<String> = [
        Refusal::NotAnId("token"),
        Refusal::NoName,
        Refusal::Nul,
        Refusal::UnknownToken,
    ]
    .iter()
    .map(ToString::to_string)
    .collect();
    assert_eq!(
        told,
        [
            "token n'est pas un identifiant",
            "un token a un nom",
            "name contient un caractère nul",
            "ce token n'est pas sur cette instance",
        ]
    );
}
