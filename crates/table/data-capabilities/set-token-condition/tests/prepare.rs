//! `request` and `prepare`: what the DM sends made the command written on
//! `BattlemapInstance/<the token's id>` — or refused before it is queued.

use serde_json::{Value, json};
use set_token_condition::{Prepared, Refusal, Request, Row, names, partition, prepare, request};

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

/// Bandit 2 as the engine holds it, on the tavern's instance, with
/// `conditions`.
fn bandit_2(conditions: Value) -> Row {
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
        "conditions": conditions,
    }))
}

/// Bandit 2's badge `state` put on, or taken off, on the tavern's instance.
fn marking(state: Value, on: Value) -> Result<Request, Refusal> {
    request(
        &row(json!({ "token": BANDIT_2, "state": state, "on": on })),
        Some(INSTANCE),
    )
}

/// The conditions Bandit 2, holding `held`, is left with once `state` is
/// set `on`.
fn left(held: Value, state: &str, on: bool) -> Value {
    let asked = marking(json!(state), json!(on)).expect("asked");
    let Prepared { payload, .. } = prepare(&asked, &bandit_2(held)).expect("prepared");
    payload["conditions"].clone()
}

#[test]
fn the_token_is_the_payload_s_on_the_targeted_instance_and_its_state_is_trimmed() {
    assert_eq!(
        marking(json!("  Empoisonné "), json!(true)),
        Ok(Request {
            token: BANDIT_2.to_owned(),
            instance: Some(INSTANCE.to_owned()),
            state: "Empoisonné".to_owned(),
            on: true,
        })
    );
}

#[test]
fn a_token_targeted_itself_stands_on_no_instance_named() {
    let wire = row(json!({ "state": "À terre", "on": true }));
    let asked = request(&wire, Some(BANDIT_2)).expect("asked");
    assert_eq!(asked.token, BANDIT_2);
    assert_eq!(asked.instance, None);
    let wire = row(json!({ "token": BANDIT_2, "state": "À terre", "on": true }));
    assert_eq!(
        request(&wire, Some(BANDIT_2)).expect("asked").instance,
        None
    );
    // A token named null is none named: the target is the token.
    let wire = row(json!({ "token": null, "state": "À terre", "on": true }));
    let asked = request(&wire, Some(BANDIT_2)).expect("asked");
    assert_eq!((asked.token.as_str(), asked.instance), (BANDIT_2, None));
}

#[test]
fn ids_are_read_in_lowercase_as_the_tables_hold_them() {
    let wire = row(json!({ "token": BANDIT_2.to_uppercase(), "state": "À terre", "on": true }));
    let asked = request(&wire, Some(&INSTANCE.to_uppercase())).expect("asked");
    assert_eq!(asked.token, BANDIT_2);
    assert_eq!(asked.instance.as_deref(), Some(INSTANCE));
}

#[test]
fn an_empty_state_is_refused_and_so_is_one_of_blanks_or_none() {
    for state in [
        json!(""),
        json!("   "),
        json!("\t\n"),
        Value::Null,
        json!(2),
        json!(["Empoisonné"]),
    ] {
        assert_eq!(
            marking(state.clone(), json!(true)),
            Err(Refusal::NoState),
            "{state}"
        );
    }
    let wire = row(json!({ "token": BANDIT_2, "on": true }));
    assert_eq!(request(&wire, Some(INSTANCE)), Err(Refusal::NoState));
}

#[test]
fn a_state_text_cannot_hold_is_refused() {
    assert_eq!(
        marking(json!("Empoisonné\u{0}"), json!(true)),
        Err(Refusal::Nul)
    );
}

#[test]
fn on_is_true_or_false_and_nothing_else() {
    for on in [Value::Null, json!("true"), json!(1), json!([true])] {
        assert_eq!(
            marking(json!("Empoisonné"), on.clone()),
            Err(Refusal::NoSwitch),
            "{on}"
        );
    }
    let wire = row(json!({ "token": BANDIT_2, "state": "Empoisonné" }));
    assert_eq!(request(&wire, Some(INSTANCE)), Err(Refusal::NoSwitch));
    assert!(
        !marking(json!("Empoisonné"), json!(false))
            .expect("asked")
            .on
    );
}

#[test]
fn a_token_or_an_instance_that_is_not_a_uuid_is_refused() {
    for (wire, target, field) in [
        (
            json!({ "token": "bandit-2", "state": "X", "on": true }),
            Some(INSTANCE),
            "token",
        ),
        (
            json!({ "token": 7, "state": "X", "on": true }),
            Some(INSTANCE),
            "token",
        ),
        (
            json!({ "token": BANDIT_2, "state": "X", "on": true }),
            Some("bmi-gue-s4"),
            "instance",
        ),
        (json!({ "state": "X", "on": true }), None, "token"),
        (json!({ "state": "X", "on": true }), Some("  "), "token"),
        (
            json!({ "state": "X", "on": true }),
            Some("70000000"),
            "token",
        ),
    ] {
        assert_eq!(
            request(&row(wire.clone()), target),
            Err(Refusal::NotAnId(field)),
            "{wire} on {target:?}"
        );
    }
}

#[test]
fn a_badge_put_on_is_added_after_the_others_once() {
    assert_eq!(left(json!([]), "Empoisonné", true), json!(["Empoisonné"]));
    assert_eq!(
        left(json!(["À terre"]), "Empoisonné", true),
        json!(["À terre", "Empoisonné"])
    );
    // Put on again, it stays where it was.
    assert_eq!(
        left(json!(["Empoisonné", "À terre"]), "Empoisonné", true),
        json!(["Empoisonné", "À terre"])
    );
}

#[test]
fn a_badge_taken_off_is_gone_and_the_others_keep_their_order() {
    assert_eq!(
        left(
            json!(["À terre", "Empoisonné", "Effrayé"]),
            "Empoisonné",
            false
        ),
        json!(["À terre", "Effrayé"])
    );
    // Taken off a token that has none, nothing changes.
    assert_eq!(
        left(json!(["À terre"]), "Empoisonné", false),
        json!(["À terre"])
    );
    assert_eq!(left(json!([]), "Empoisonné", false), json!([]));
}

#[test]
fn a_token_with_no_conditions_held_starts_from_none() {
    let asked = marking(json!("Empoisonné"), json!(true)).expect("asked");
    let mut base = bandit_2(json!([]));
    base.remove("conditions");
    let Prepared { payload, .. } = prepare(&asked, &base).expect("prepared");
    assert_eq!(payload["conditions"], json!(["Empoisonné"]));
}

#[test]
fn a_badge_set_writes_the_conditions_alone_on_the_token_s_partition() {
    let asked = marking(json!("Empoisonné"), json!(true)).expect("asked");
    let Prepared { target_id, payload } =
        prepare(&asked, &bandit_2(json!(["À terre"]))).expect("prepared");
    assert_eq!(target_id, BANDIT_2);
    assert_eq!(
        payload,
        row(json!({ "conditions": ["À terre", "Empoisonné"] }))
    );
    assert_eq!(
        partition(&target_id),
        format!("BattlemapInstance/{BANDIT_2}")
    );
}

#[test]
fn every_field_a_badge_set_writes_is_one_its_manifest_touches() {
    let manifest: Value =
        serde_json::from_str(include_str!("../data-capability.json")).expect("contract F");
    assert_eq!(manifest["effect"], "update");
    assert_eq!(manifest["touches"], json!(["BattlemapInstance.conditions"]));
    let asked = marking(json!("Empoisonné"), json!(true)).expect("asked");
    let Prepared { payload, .. } = prepare(&asked, &bandit_2(json!([]))).expect("prepared");
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
fn a_token_the_engine_does_not_hold_on_the_instance_is_none_to_mark() {
    let asked = marking(json!("Empoisonné"), json!(true)).expect("asked");
    let mut other = bandit_2(json!([]));
    other.insert(
        "tokenId".into(),
        json!("70000000-0000-4000-8000-000000000003"),
    );
    let instance = row(json!({ "battlemapInstanceId": INSTANCE, "sceneId": INSTANCE }));
    for base in [Row::new(), instance, other] {
        assert_eq!(prepare(&asked, &base), Err(Refusal::UnknownToken));
    }
    let mut elsewhere = bandit_2(json!([]));
    elsewhere.insert("tokenInstanceId".into(), json!(AUTRE_INSTANCE));
    assert_eq!(prepare(&asked, &elsewhere), Err(Refusal::UnknownToken));
    // Targeted itself, it is marked wherever it stands.
    let itself = request(
        &row(json!({ "state": "Empoisonné", "on": true })),
        Some(BANDIT_2),
    )
    .expect("asked");
    assert!(prepare(&itself, &elsewhere).is_ok());
}

#[test]
fn a_token_held_in_uppercase_is_the_same_token() {
    let asked = marking(json!("Empoisonné"), json!(true)).expect("asked");
    let mut upper = bandit_2(json!([]));
    upper.insert("tokenId".into(), json!(BANDIT_2.to_uppercase()));
    upper.insert("tokenInstanceId".into(), json!(INSTANCE.to_uppercase()));
    assert!(prepare(&asked, &upper).is_ok());
}

#[test]
fn names_is_this_data_capability_with_or_without_its_version() {
    assert!(names("table.setTokenCondition"));
    assert!(names("table.setTokenCondition@1"));
    assert!(!names("table.setTokenConditions"));
    assert!(!names("table.renameToken@1"));
}

#[test]
fn a_refusal_says_why_in_words() {
    let told: Vec<String> = [
        Refusal::NotAnId("token"),
        Refusal::NoState,
        Refusal::Nul,
        Refusal::NoSwitch,
        Refusal::UnknownToken,
    ]
    .iter()
    .map(ToString::to_string)
    .collect();
    assert_eq!(
        told,
        [
            "token n'est pas un identifiant",
            "un état a un nom",
            "state contient un caractère nul",
            "on vaut true ou false",
            "ce token n'est pas sur cette instance",
        ]
    );
}
