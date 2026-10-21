//! `request` and `prepare`: the DM's hide or reveal made the row written on
//! the token's own partition, `BattlemapInstance/<token>` — or refused
//! before it is queued.

use serde_json::{Value, json};
use set_token_visibility::{Refusal, Request, Row, prepare, request};

const INSTANCE: &str = "b1000000-0000-4000-8000-000000000001";
const AUTRE_INSTANCE: &str = "b1000000-0000-4000-8000-000000000002";
const VESNA_1: &str = "70000000-0000-4000-8000-000000000002";
const VESNA: &str = "e1000000-0000-4000-8000-000000000002";

fn row(value: Value) -> Row {
    let Value::Object(map) = value else {
        panic!("an object");
    };
    map.into_iter().collect()
}

fn asked(visible: bool) -> Request {
    Request {
        instance: INSTANCE.into(),
        token: VESNA_1.into(),
        visible,
    }
}

/// Vesna 1, the ambush's chief, standing visible on the tavern's instance.
fn vesna_1() -> Row {
    row(json!({
        "tokenId": VESNA_1,
        "tokenInstanceId": INSTANCE,
        "characterId": VESNA,
        "number": 1,
        "name": "Vesna 1",
        "q": 6,
        "r": 5,
        "visible": true,
        "hpCurrent": 11,
        "hpMax": 11,
        "conditions": [],
    }))
}

#[test]
fn a_change_names_its_instance_by_the_target_and_its_token_written_as_the_tables_write_them() {
    let wire = row(json!({ "token": format!(" {} ", VESNA_1.to_uppercase()), "visible": false }));
    assert_eq!(
        request(&wire, Some(&INSTANCE.to_uppercase())),
        Ok(asked(false))
    );
}

#[test]
fn without_a_target_the_payload_names_the_instance() {
    let wire = row(json!({ "instance": INSTANCE, "token": VESNA_1, "visible": true }));
    assert_eq!(request(&wire, None), Ok(asked(true)));
}

#[test]
fn the_target_is_the_instance_whatever_the_payload_names() {
    let wire = row(json!({ "instance": AUTRE_INSTANCE, "token": VESNA_1, "visible": false }));
    assert_eq!(request(&wire, Some(INSTANCE)), Ok(asked(false)));
}

#[test]
fn an_instance_or_a_token_that_is_no_id_is_refused_by_its_name() {
    let full = json!({ "instance": INSTANCE, "token": VESNA_1, "visible": false });
    for field in ["instance", "token"] {
        for bad in [Value::Null, json!("vesna-1"), json!(""), json!(7)] {
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
fn players_see_band_only_a_visibility_is_true_or_false_and_nothing_else() {
    let full = json!({ "token": VESNA_1, "visible": false });
    for bad in [
        Value::Null,
        json!("false"),
        json!(0),
        json!(1),
        json!([]),
        json!({}),
    ] {
        let mut wire = full.clone();
        wire["visible"] = bad.clone();
        assert_eq!(
            request(&row(wire), Some(INSTANCE)),
            Err(Refusal::NotAVisibility),
            "{bad}"
        );
    }
    let mut wire = row(full);
    wire.remove("visible");
    assert_eq!(request(&wire, Some(INSTANCE)), Err(Refusal::NotAVisibility));
}

#[test]
fn a_hide_writes_the_token_s_visible_alone_on_its_own_partition() {
    let prepared = prepare(&asked(false), &vesna_1()).expect("hidden");
    assert_eq!(prepared.target_id, VESNA_1);
    assert_eq!(prepared.payload, row(json!({ "visible": false })));
}

#[test]
fn hidden_keeps_rank_a_reveal_writes_visible_and_no_rank() {
    let mut hidden = vesna_1();
    hidden.insert("visible".into(), json!(false));
    let prepared = prepare(&asked(true), &hidden).expect("revealed");
    assert_eq!(prepared.target_id, VESNA_1);
    assert_eq!(prepared.payload, row(json!({ "visible": true })));
    assert!(!prepared.payload.contains_key("rank"));
}

#[test]
fn the_fields_a_change_writes_are_exactly_the_ones_its_manifest_touches() {
    let manifest: Value =
        serde_json::from_str(include_str!("../data-capability.json")).expect("contract F");
    let touched: Vec<&str> = manifest["touches"]
        .as_array()
        .expect("touches")
        .iter()
        .map(|touch| touch.as_str().expect("a field"))
        .collect();
    for visible in [false, true] {
        let prepared = prepare(&asked(visible), &vesna_1()).expect("prepared");
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
    let mut elsewhere = vesna_1();
    elsewhere.insert("tokenInstanceId".into(), json!(AUTRE_INSTANCE));
    assert_eq!(
        prepare(&asked(false), &elsewhere),
        Err(Refusal::TokenOfAnotherInstance)
    );
    // A token that names no instance stands on none named.
    elsewhere.remove("tokenInstanceId");
    assert_eq!(
        prepare(&asked(false), &elsewhere),
        Err(Refusal::TokenOfAnotherInstance)
    );
}

#[test]
fn the_instance_is_compared_in_any_case() {
    let mut shouted = vesna_1();
    shouted.insert("tokenInstanceId".into(), json!(INSTANCE.to_uppercase()));
    assert!(prepare(&asked(false), &shouted).is_ok());
}

#[test]
fn a_row_that_is_no_token_s_is_an_unknown_token() {
    // Nothing on the partition, the instance's own row, or an entry's.
    for row_of in [
        Row::new(),
        row(json!({ "battlemapInstanceId": INSTANCE, "sceneId": VESNA })),
        row(json!({ "tokenId": null, "tokenInstanceId": INSTANCE })),
        row(json!({ "initiativeEntryTokenId": VESNA_1, "rank": 2 })),
    ] {
        assert_eq!(
            prepare(&asked(false), &row_of),
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
        Refusal::NotAVisibility,
        Refusal::TokenOfAnotherInstance,
    ] {
        assert!(!refusal.not_found(), "{refusal:?}");
    }
}

#[test]
fn every_refusal_says_why_in_words() {
    for (refusal, words) in [
        (Refusal::NotAnId("token"), "token n'est pas un identifiant"),
        (Refusal::NotAVisibility, "visible vaut true ou false"),
        (Refusal::UnknownToken, "ce token n'existe pas"),
        (
            Refusal::TokenOfAnotherInstance,
            "instance-isolated: ce token est sur une autre instance de battlemap",
        ),
    ] {
        assert_eq!(refusal.to_string(), words);
    }
}
