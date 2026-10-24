//! `request` and `prepare`: the DM's move made the row written on the token's
//! own partition, `BattlemapInstance/<token>` — or refused before it is
//! queued.

use move_token::{Refusal, Request, Row, prepare, request};
use serde_json::{Value, json};

const INSTANCE: &str = "b1000000-0000-4000-8000-000000000001";
const AUTRE_INSTANCE: &str = "b1000000-0000-4000-8000-000000000002";
const MORLOX_1: &str = "70000000-0000-4000-8000-000000000002";
const MORLOX: &str = "e1000000-0000-4000-8000-000000000002";

fn row(value: Value) -> Row {
    let Value::Object(map) = value else {
        panic!("an object");
    };
    map.into_iter().collect()
}

fn asked() -> Request {
    Request {
        instance: INSTANCE.into(),
        token: MORLOX_1.into(),
        q: 2,
        r: 3,
    }
}

/// Morlox 1, standing on (5, 5) of the tavern's instance.
fn morlox_1() -> Row {
    row(json!({
        "tokenId": MORLOX_1,
        "tokenInstanceId": INSTANCE,
        "characterId": MORLOX,
        "number": 1,
        "name": "Morlox 1",
        "q": 5,
        "r": 5,
        "visible": true,
        "hpCurrent": 7,
        "hpMax": 7,
        "conditions": [],
    }))
}

#[test]
fn a_move_names_its_instance_by_the_target_and_its_token_written_as_the_tables_write_them() {
    let wire = row(json!({ "token": format!(" {} ", MORLOX_1.to_uppercase()), "q": 2, "r": 3 }));
    assert_eq!(request(&wire, Some(&INSTANCE.to_uppercase())), Ok(asked()));
}

#[test]
fn without_a_target_the_payload_names_the_instance() {
    let wire = row(json!({ "instance": INSTANCE, "token": MORLOX_1, "q": 2, "r": 3 }));
    assert_eq!(request(&wire, None), Ok(asked()));
}

#[test]
fn the_target_is_the_instance_whatever_the_payload_names() {
    let wire = row(json!({ "instance": AUTRE_INSTANCE, "token": MORLOX_1, "q": 2, "r": 3 }));
    assert_eq!(request(&wire, Some(INSTANCE)), Ok(asked()));
}

#[test]
fn an_instance_or_a_token_that_is_no_id_is_refused_by_its_name() {
    let full = json!({ "instance": INSTANCE, "token": MORLOX_1, "q": 2, "r": 3 });
    for field in ["instance", "token"] {
        for bad in [Value::Null, json!("morlox-1"), json!(""), json!(7)] {
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
        request(&row(full), Some("bmi-gue")),
        Err(Refusal::NotAnId("instance"))
    );
}

#[test]
fn single_position_source_a_hex_is_a_pair_of_whole_numbers_the_tables_hold() {
    let full = json!({ "token": MORLOX_1, "q": 2, "r": 3 });
    for field in ["q", "r"] {
        for bad in [
            Value::Null,
            json!(2.5),
            json!("2"),
            json!(i64::from(i32::MAX) + 1),
            json!(i64::from(i32::MIN) - 1),
        ] {
            let mut wire = full.clone();
            wire[field] = bad.clone();
            assert_eq!(
                request(&row(wire), Some(INSTANCE)),
                Err(Refusal::NotAHex(field)),
                "{field}: {bad}"
            );
        }
        let mut wire = row(full.clone());
        wire.remove(field);
        assert_eq!(
            request(&wire, Some(INSTANCE)),
            Err(Refusal::NotAHex(field)),
            "{field}"
        );
    }
    // Negative hexes and the table's bounds are hexes.
    for (q, r) in [(-4, 0), (i64::from(i32::MIN), i64::from(i32::MAX))] {
        let wire = row(json!({ "token": MORLOX_1, "q": q, "r": r }));
        let asked = request(&wire, Some(INSTANCE)).expect("a hex");
        assert_eq!((asked.q, asked.r), (q, r));
    }
}

#[test]
fn single_position_source_a_move_writes_the_token_s_q_and_r_on_its_own_partition() {
    let prepared = prepare(&asked(), &morlox_1()).expect("moved");
    assert_eq!(prepared.target_id, MORLOX_1);
    assert_eq!(prepared.payload, row(json!({ "q": 2, "r": 3 })));
}

#[test]
fn the_fields_a_move_writes_are_exactly_the_ones_its_manifest_touches() {
    let manifest: Value =
        serde_json::from_str(include_str!("../data-capability.json")).expect("contract F");
    let touched: Vec<&str> = manifest["touches"]
        .as_array()
        .expect("touches")
        .iter()
        .map(|touch| touch.as_str().expect("a field"))
        .collect();
    let prepared = prepare(&asked(), &morlox_1()).expect("moved");
    let written: Vec<String> = prepared
        .payload
        .keys()
        .map(|field| format!("BattlemapInstance.{field}"))
        .collect();
    assert_eq!(written, touched);
    assert_eq!(manifest["effect"], "update");
}

#[test]
fn instance_isolated_a_token_of_another_instance_is_refused() {
    let mut elsewhere = morlox_1();
    elsewhere.insert("tokenInstanceId".into(), json!(AUTRE_INSTANCE));
    assert_eq!(
        prepare(&asked(), &elsewhere),
        Err(Refusal::TokenOfAnotherInstance)
    );
    // A token that names no instance stands on none named.
    elsewhere.remove("tokenInstanceId");
    assert_eq!(
        prepare(&asked(), &elsewhere),
        Err(Refusal::TokenOfAnotherInstance)
    );
}

#[test]
fn the_instance_is_compared_in_any_case() {
    let mut shouted = morlox_1();
    shouted.insert("tokenInstanceId".into(), json!(INSTANCE.to_uppercase()));
    assert!(prepare(&asked(), &shouted).is_ok());
}

#[test]
fn a_row_that_is_no_token_s_is_an_unknown_token() {
    // Nothing on the partition, or the instance's own row.
    for row_of in [
        Row::new(),
        row(json!({ "battlemapInstanceId": INSTANCE, "sceneId": MORLOX })),
        row(json!({ "tokenId": null, "tokenInstanceId": INSTANCE })),
    ] {
        assert_eq!(
            prepare(&asked(), &row_of),
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
        Refusal::NotAHex("q"),
        Refusal::TokenOfAnotherInstance,
    ] {
        assert!(!refusal.not_found(), "{refusal:?}");
    }
}

#[test]
fn every_refusal_says_why_in_words() {
    for (refusal, words) in [
        (Refusal::NotAnId("token"), "token n'est pas un identifiant"),
        (Refusal::NotAHex("r"), "r n'est pas une coordonnée de case"),
        (Refusal::UnknownToken, "ce token n'existe pas"),
        (
            Refusal::TokenOfAnotherInstance,
            "instance-isolated: ce token est sur une autre instance de battlemap",
        ),
    ] {
        assert_eq!(refusal.to_string(), words);
    }
}
