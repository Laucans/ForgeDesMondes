//! What a change of HP asks for ([`request`]) and the step it writes
//! ([`prepare`]), against the token's row as the queue leaves it: an NPC's
//! HP land between 0 and its HP max (`pj-hp-not-tracked`), on the token's
//! own partition alone (`hp-change-one-token`), and only a token of the
//! instance named is changed (`instance-isolated`).

use change_hit_points::{DATA_CAPABILITY, Prepared, Refusal, Request, Row, prepare, request};
use serde_json::{Value, json};

const INSTANCE: &str = "b1000000-0000-4000-8000-000000000001";
const AUTRE_INSTANCE: &str = "b1000000-0000-4000-8000-000000000002";
const BANDIT_1: &str = "70000000-0000-4000-8000-000000000002";

fn row(value: Value) -> Row {
    serde_json::from_value(value).expect("an object")
}

/// Bandit 1 on the tavern's instance, at `hp` of `max`; `None` for a PC's
/// token, which tracks none.
fn bandit(hp: Option<(i64, i64)>) -> Row {
    row(json!({
        "tokenId": BANDIT_1,
        "tokenInstanceId": INSTANCE,
        "characterId": "e1000000-0000-4000-8000-000000000002",
        "name": "Bandit 1",
        "hpCurrent": hp.map(|(current, _)| current),
        "hpMax": hp.map(|(_, max)| max),
    }))
}

fn asked(delta: i64) -> Request {
    Request {
        instance: INSTANCE.to_owned(),
        token: BANDIT_1.to_owned(),
        delta,
    }
}

/// The step `delta` writes on Bandit 1 at `hp`.
fn step(hp: Option<(i64, i64)>, delta: i64) -> Value {
    let prepared = prepare(&asked(delta), &bandit(hp)).expect("prepared");
    assert_eq!(prepared.target_id, BANDIT_1);
    prepared.payload["hpCurrent"].clone()
}

#[test]
fn the_id_is_the_manifest_s() {
    let manifest: Value =
        serde_json::from_str(include_str!("../data-capability.json")).expect("JSON");
    assert_eq!(manifest["dataCapability"], DATA_CAPABILITY);
}

#[test]
fn the_request_reads_the_target_the_token_and_the_delta_in_any_case() {
    let wire = row(json!({ "token": format!(" {} ", BANDIT_1.to_uppercase()), "delta": -7 }));
    assert_eq!(
        request(&wire, Some(&INSTANCE.to_uppercase())),
        Ok(asked(-7))
    );
}

#[test]
fn without_a_target_the_instance_is_the_payload_s() {
    let wire = row(json!({ "instance": INSTANCE, "token": BANDIT_1, "delta": 5 }));
    assert_eq!(request(&wire, None), Ok(asked(5)));
    // The target wins over the payload's.
    let wire = row(json!({ "instance": AUTRE_INSTANCE, "token": BANDIT_1, "delta": 5 }));
    assert_eq!(request(&wire, Some(INSTANCE)), Ok(asked(5)));
}

#[test]
fn a_request_naming_no_instance_no_token_or_no_whole_delta_is_refused() {
    for (wire, target, refusal) in [
        (
            json!({ "token": BANDIT_1, "delta": -1 }),
            None,
            Refusal::NotAnId("instance"),
        ),
        (
            json!({ "token": BANDIT_1, "delta": -1 }),
            Some("pas-un-id"),
            Refusal::NotAnId("instance"),
        ),
        (
            json!({ "delta": -1 }),
            Some(INSTANCE),
            Refusal::NotAnId("token"),
        ),
        (
            json!({ "token": 7, "delta": -1 }),
            Some(INSTANCE),
            Refusal::NotAnId("token"),
        ),
        (
            json!({ "token": BANDIT_1 }),
            Some(INSTANCE),
            Refusal::NotADelta,
        ),
        (
            json!({ "token": BANDIT_1, "delta": "-7" }),
            Some(INSTANCE),
            Refusal::NotADelta,
        ),
        (
            json!({ "token": BANDIT_1, "delta": -1.5 }),
            Some(INSTANCE),
            Refusal::NotADelta,
        ),
    ] {
        assert_eq!(request(&row(wire.clone()), target), Err(refusal), "{wire}");
    }
}

#[test]
fn hp_change_one_token_the_step_lands_on_the_token_s_partition_and_writes_hp_current_alone() {
    let prepared = prepare(&asked(-7), &bandit(Some((20, 20)))).expect("prepared");
    assert_eq!(
        prepared,
        Prepared {
            target_id: BANDIT_1.to_owned(),
            payload: row(json!({ "hpCurrent": -7 })),
        }
    );
    // What it writes is what the manifest says it touches, and no more.
    let manifest: Value =
        serde_json::from_str(include_str!("../data-capability.json")).expect("JSON");
    let touched: Vec<String> = prepared
        .payload
        .keys()
        .map(|field| format!("BattlemapInstance.{field}"))
        .collect();
    assert_eq!(json!(touched), manifest["touches"]);
}

#[test]
fn pj_hp_not_tracked_an_npc_s_hp_land_between_0_and_its_max() {
    // 20/20 − 7 → 13.
    assert_eq!(step(Some((20, 20)), -7), json!(-7));
    // 3 − 10 → 0, never below.
    assert_eq!(step(Some((3, 20)), -10), json!(-3));
    // 13 + 50 → 20, never beyond.
    assert_eq!(step(Some((13, 20)), 50), json!(7));
    // Exactly to the bounds, and nothing at all.
    assert_eq!(step(Some((5, 20)), -5), json!(-5));
    assert_eq!(step(Some((5, 20)), 15), json!(15));
    assert_eq!(step(Some((5, 20)), 0), json!(0));
    // Already dead, or already whole: nothing to step.
    assert_eq!(step(Some((0, 20)), -4), json!(0));
    assert_eq!(step(Some((20, 20)), 4), json!(0));
    // Healing the dead brings it back.
    assert_eq!(step(Some((0, 20)), 6), json!(6));
}

#[test]
fn a_delta_beyond_any_hp_saturates_instead_of_overflowing() {
    assert_eq!(step(Some((3, 20)), i64::MIN), json!(-3));
    assert_eq!(step(Some((3, 20)), i64::MAX), json!(17));
}

#[test]
fn pj_hp_not_tracked_a_pc_s_step_is_the_delta_as_sent_for_the_dataguard_to_reject() {
    assert_eq!(step(None, -5), json!(-5));
    assert_eq!(step(None, 0), json!(0));
    // HP half-tracked are no NPC's: sent as is, for the judge.
    let mut half = bandit(Some((5, 20)));
    half.insert("hpMax".to_owned(), Value::Null);
    let prepared = prepare(&asked(-9), &half).expect("prepared");
    assert_eq!(prepared.payload["hpCurrent"], json!(-9));
}

#[test]
fn a_row_that_is_no_token_s_is_unknown_and_not_found() {
    for base in [
        Row::new(),
        row(json!({ "battlemapInstanceId": INSTANCE, "sceneId": INSTANCE })),
    ] {
        let refusal = prepare(&asked(-1), &base).expect_err("refused");
        assert_eq!(refusal, Refusal::UnknownToken);
        assert!(refusal.not_found());
        assert_eq!(refusal.to_string(), "ce token n'existe pas");
    }
}

#[test]
fn instance_isolated_a_token_of_another_instance_or_of_none_is_refused_unprocessable() {
    let mut elsewhere = bandit(Some((20, 20)));
    elsewhere.insert("tokenInstanceId".to_owned(), json!(AUTRE_INSTANCE));
    let mut nowhere = bandit(Some((20, 20)));
    nowhere.remove("tokenInstanceId");
    for base in [elsewhere, nowhere] {
        let refusal = prepare(&asked(-1), &base).expect_err("refused");
        assert_eq!(refusal, Refusal::TokenOfAnotherInstance);
        assert!(!refusal.not_found());
        assert!(refusal.to_string().starts_with("instance-isolated"));
    }
    // The tables' case is not the request's: the same instance.
    let mut upper = bandit(Some((20, 20)));
    upper.insert("tokenInstanceId".to_owned(), json!(INSTANCE.to_uppercase()));
    assert!(prepare(&asked(-1), &upper).is_ok());
}

#[test]
fn every_refusal_says_what_is_wrong() {
    assert_eq!(
        Refusal::NotAnId("token").to_string(),
        "token n'est pas un identifiant"
    );
    assert_eq!(
        Refusal::NotADelta.to_string(),
        "delta est un nombre entier de PV"
    );
    for refusal in [Refusal::NotAnId("instance"), Refusal::NotADelta] {
        assert!(!refusal.not_found(), "{refusal}");
    }
}
