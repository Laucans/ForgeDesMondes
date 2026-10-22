//! `request` and `prepare`: the DM's badge set made the row written on the
//! token's own partition, `BattlemapInstance/<token>` — its conditions with
//! the badge put on or taken off — or refused before it is queued.

use serde_json::{Value, json};
use set_token_condition::{Refusal, Request, Row, prepare, request};

const INSTANCE: &str = "b1000000-0000-4000-8000-000000000001";
const AUTRE_INSTANCE: &str = "b1000000-0000-4000-8000-000000000002";
const BANDIT_2: &str = "70000000-0000-4000-8000-000000000003";
const BANDIT: &str = "e1000000-0000-4000-8000-000000000002";

fn row(value: Value) -> Row {
    let Value::Object(map) = value else {
        panic!("an object");
    };
    map.into_iter().collect()
}

fn asked(state: &str, on: bool) -> Request {
    Request {
        instance: INSTANCE.into(),
        token: BANDIT_2.into(),
        state: state.into(),
        on,
    }
}

/// Bandit 2 on the tavern's instance, holding `conditions`.
fn bandit_2(conditions: Value) -> Row {
    row(json!({
        "tokenId": BANDIT_2,
        "tokenInstanceId": INSTANCE,
        "characterId": BANDIT,
        "number": 2,
        "name": "Bandit 2",
        "q": 6,
        "r": 5,
        "visible": true,
        "hpCurrent": 11,
        "hpMax": 11,
        "conditions": conditions,
    }))
}

/// The conditions `prepared` writes.
fn written(request: &Request, token: &Row) -> Value {
    let prepared = prepare(request, token).expect("prepared");
    assert_eq!(prepared.target_id, BANDIT_2);
    assert_eq!(prepared.payload.len(), 1, "{prepared:?}");
    prepared.payload["conditions"].clone()
}

#[test]
fn a_badge_set_names_its_instance_by_the_target_and_its_state_without_blanks() {
    let wire = row(json!({
        "token": format!(" {} ", BANDIT_2.to_uppercase()),
        "state": "  Empoisonné \t",
        "on": true,
    }));
    assert_eq!(
        request(&wire, Some(&INSTANCE.to_uppercase())),
        Ok(asked("Empoisonné", true))
    );
}

#[test]
fn without_a_target_the_payload_names_the_instance() {
    let wire =
        row(json!({ "instance": INSTANCE, "token": BANDIT_2, "state": "À terre", "on": false }));
    assert_eq!(request(&wire, None), Ok(asked("À terre", false)));
}

#[test]
fn the_target_is_the_instance_whatever_the_payload_names() {
    let wire = row(json!({
        "instance": AUTRE_INSTANCE,
        "token": BANDIT_2,
        "state": "À terre",
        "on": true,
    }));
    assert_eq!(request(&wire, Some(INSTANCE)), Ok(asked("À terre", true)));
}

#[test]
fn an_instance_or_a_token_that_is_no_id_is_refused_by_its_name() {
    let full = json!({ "instance": INSTANCE, "token": BANDIT_2, "state": "À terre", "on": true });
    for field in ["instance", "token"] {
        for bad in [Value::Null, json!("bandit-2"), json!(""), json!(7)] {
            let mut wire = full.clone();
            wire[field] = bad.clone();
            assert_eq!(
                request(&row(wire), None),
                Err(Refusal::NotAnId(field)),
                "{field}: {bad}"
            );
        }
        let mut wire = row(full.clone());
        wire.remove(field);
        assert_eq!(
            request(&wire, None),
            Err(Refusal::NotAnId(field)),
            "{field}"
        );
    }
    assert_eq!(
        request(&row(full), Some("bmi-taverne")),
        Err(Refusal::NotAnId("instance"))
    );
}

#[test]
fn state_persisted_reset_a_condition_is_a_name_the_tables_can_hold() {
    let full = json!({ "token": BANDIT_2, "state": "À terre", "on": true });
    for bad in [
        Value::Null,
        json!(""),
        json!(" \t\n "),
        json!(3),
        json!(["À terre"]),
    ] {
        let mut wire = full.clone();
        wire["state"] = bad.clone();
        assert_eq!(
            request(&row(wire), Some(INSTANCE)),
            Err(Refusal::NoState),
            "{bad}"
        );
    }
    let mut wire = row(full.clone());
    wire.remove("state");
    assert_eq!(request(&wire, Some(INSTANCE)), Err(Refusal::NoState));
    let mut nul = full;
    nul["state"] = json!("À\0terre");
    assert_eq!(request(&row(nul), Some(INSTANCE)), Err(Refusal::Nul));
}

#[test]
fn a_switch_is_true_or_false_and_nothing_else() {
    let full = json!({ "token": BANDIT_2, "state": "À terre", "on": true });
    for bad in [Value::Null, json!("true"), json!(0), json!(1), json!([])] {
        let mut wire = full.clone();
        wire["on"] = bad.clone();
        assert_eq!(
            request(&row(wire), Some(INSTANCE)),
            Err(Refusal::NoSwitch),
            "{bad}"
        );
    }
    let mut wire = row(full);
    wire.remove("on");
    assert_eq!(request(&wire, Some(INSTANCE)), Err(Refusal::NoSwitch));
}

#[test]
fn a_badge_put_on_is_added_after_the_others_once() {
    assert_eq!(
        written(&asked("Empoisonné", true), &bandit_2(json!([]))),
        json!(["Empoisonné"])
    );
    assert_eq!(
        written(&asked("À terre", true), &bandit_2(json!(["Empoisonné"]))),
        json!(["Empoisonné", "À terre"])
    );
    // Already on: the conditions as they were, not a second badge.
    assert_eq!(
        written(
            &asked("Empoisonné", true),
            &bandit_2(json!(["Empoisonné", "À terre"]))
        ),
        json!(["Empoisonné", "À terre"])
    );
}

#[test]
fn a_badge_taken_off_is_gone_and_the_others_keep_their_order() {
    assert_eq!(
        written(
            &asked("À terre", false),
            &bandit_2(json!(["Empoisonné", "À terre", "Effrayé"]))
        ),
        json!(["Empoisonné", "Effrayé"])
    );
    // Not on: nothing to take off.
    assert_eq!(
        written(&asked("À terre", false), &bandit_2(json!(["Empoisonné"]))),
        json!(["Empoisonné"])
    );
    assert_eq!(
        written(
            &asked("Empoisonné", false),
            &bandit_2(json!(["Empoisonné"]))
        ),
        json!([])
    );
}

#[test]
fn a_token_holding_no_conditions_yet_gets_the_badge_alone() {
    let mut bare = bandit_2(json!([]));
    bare.remove("conditions");
    assert_eq!(
        written(&asked("Empoisonné", true), &bare),
        json!(["Empoisonné"])
    );
    assert_eq!(written(&asked("Empoisonné", false), &bare), json!([]));
}

#[test]
fn hit_points_and_death_are_never_written() {
    for on in [true, false] {
        let prepared = prepare(&asked("À terre", on), &bandit_2(json!(["À terre"]))).expect("set");
        for field in [
            "hpCurrent",
            "hpMax",
            "dead",
            "healthBand",
            "visible",
            "name",
        ] {
            assert!(!prepared.payload.contains_key(field), "{field}");
        }
    }
}

#[test]
fn the_fields_a_badge_set_writes_are_exactly_the_ones_its_manifest_touches() {
    let manifest: Value =
        serde_json::from_str(include_str!("../data-capability.json")).expect("contract F");
    let touched: Vec<&str> = manifest["touches"]
        .as_array()
        .expect("touches")
        .iter()
        .map(|touch| touch.as_str().expect("a field"))
        .collect();
    for on in [false, true] {
        let prepared = prepare(&asked("À terre", on), &bandit_2(json!([]))).expect("prepared");
        let written: Vec<String> = prepared
            .payload
            .keys()
            .map(|field| format!("BattlemapInstance.{field}"))
            .collect();
        assert_eq!(written, touched);
    }
    assert_eq!(manifest["effect"], "update");
}

#[test]
fn instance_isolated_a_token_of_another_instance_is_refused() {
    let mut elsewhere = bandit_2(json!([]));
    elsewhere.insert("tokenInstanceId".into(), json!(AUTRE_INSTANCE));
    assert_eq!(
        prepare(&asked("À terre", true), &elsewhere),
        Err(Refusal::TokenOfAnotherInstance)
    );
    // A token that names no instance stands on none named.
    elsewhere.remove("tokenInstanceId");
    assert_eq!(
        prepare(&asked("À terre", true), &elsewhere),
        Err(Refusal::TokenOfAnotherInstance)
    );
}

#[test]
fn the_instance_is_compared_in_any_case() {
    let mut shouted = bandit_2(json!([]));
    shouted.insert("tokenInstanceId".into(), json!(INSTANCE.to_uppercase()));
    assert!(prepare(&asked("À terre", true), &shouted).is_ok());
}

#[test]
fn a_row_that_is_no_token_s_is_an_unknown_token() {
    // Nothing on the partition, the instance's own row, or an entry's.
    for row_of in [
        Row::new(),
        row(json!({ "battlemapInstanceId": INSTANCE, "sceneId": BANDIT })),
        row(json!({ "tokenId": null, "tokenInstanceId": INSTANCE })),
        row(json!({ "initiativeEntryTokenId": BANDIT_2, "rank": 2 })),
    ] {
        assert_eq!(
            prepare(&asked("À terre", true), &row_of),
            Err(Refusal::UnknownToken),
            "{row_of:?}"
        );
    }
}

#[test]
fn only_an_unknown_token_is_refused_as_not_found() {
    assert!(Refusal::UnknownToken.not_found());
    for refusal in [
        Refusal::NotAnId("token"),
        Refusal::NoState,
        Refusal::Nul,
        Refusal::NoSwitch,
        Refusal::TokenOfAnotherInstance,
    ] {
        assert!(!refusal.not_found(), "{refusal:?}");
    }
}

#[test]
fn every_refusal_says_why_in_words() {
    for (refusal, words) in [
        (Refusal::NotAnId("token"), "token n'est pas un identifiant"),
        (Refusal::NoState, "state est le nom d'un état"),
        (Refusal::Nul, "state contient un caractère nul"),
        (Refusal::NoSwitch, "on vaut true ou false"),
        (Refusal::UnknownToken, "ce token n'existe pas"),
        (
            Refusal::TokenOfAnotherInstance,
            "instance-isolated: ce token est sur une autre instance de battlemap",
        ),
    ] {
        assert_eq!(refusal.to_string(), words);
    }
}
