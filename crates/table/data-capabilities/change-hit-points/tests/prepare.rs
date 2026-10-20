//! `table.changeHitPoints`' judgement before a command is queued: what the
//! DM sends, read ([`request`]), and made the step of the current HP written
//! on the token's partition ([`prepare`]), cut to 0..hp_max.

use change_hit_points::{Prepared, Refusal, Request, Row, names, partition, prepare, request};
use serde_json::{Value, json};

const INSTANCE: &str = "b1000000-0000-4000-8000-000000000001";
const AUTRE_INSTANCE: &str = "b1000000-0000-4000-8000-000000000002";
const BANDIT_1: &str = "70000000-0000-4000-8000-000000000003";
const ELOWEN: &str = "70000000-0000-4000-8000-000000000001";

fn row(value: Value) -> Row {
    let Value::Object(fields) = value else {
        panic!("a row is an object");
    };
    fields.into_iter().collect()
}

/// Bandit 1, an NPC at `current` of 20 HP, as the engine holds it on its
/// partition.
fn bandit(current: i64) -> Row {
    row(json!({
        "tokenId": BANDIT_1,
        "tokenInstanceId": INSTANCE,
        "characterId": "e1000000-0000-4000-8000-000000000003",
        "number": 1,
        "name": "Bandit 1",
        "q": 5,
        "r": 5,
        "visible": true,
        "hpCurrent": current,
        "hpMax": 20,
        "conditions": [],
    }))
}

/// Elowen's token, a PC's: no HP tracked.
fn elowen() -> Row {
    row(json!({
        "tokenId": ELOWEN,
        "tokenInstanceId": INSTANCE,
        "characterId": "e1000000-0000-4000-8000-000000000001",
        "number": null,
        "name": "Elowen",
        "q": 1,
        "r": 1,
        "visible": true,
        "hpCurrent": null,
        "hpMax": null,
        "conditions": [],
    }))
}

fn manifest() -> Value {
    serde_json::from_str(include_str!("../data-capability.json")).expect("contract F")
}

/// The step `delta` makes on `base`, sent on the instance.
fn step(delta: i64, base: &Row) -> Value {
    let wire = row(json!({ "token": BANDIT_1, "delta": delta }));
    let asked = request(&wire, Some(INSTANCE)).expect("a request");
    let prepared = prepare(&asked, base).expect("prepared");
    assert_eq!(prepared.target_id, BANDIT_1);
    assert_eq!(prepared.payload.len(), 1, "{:?}", prepared.payload);
    prepared.payload["hpCurrent"].clone()
}

#[test]
fn the_manifest_names_this_data_capability_a_relative_update_touching_the_current_hp_alone() {
    let manifest = manifest();
    assert_eq!(manifest["dataCapability"], "table.changeHitPoints");
    assert_eq!(manifest["owner"], "dataguard");
    assert_eq!(manifest["effect"], "update");
    assert_eq!(manifest["mode"], "relative");
    assert_eq!(manifest["touches"], json!(["BattlemapInstance.hpCurrent"]));
    assert_eq!(manifest["target"]["aggregate"], "BattlemapInstance");
    for invariant in ["pj-hp-not-tracked", "hp-change-one-token", "dead-derived"] {
        assert!(
            manifest["invariants"]
                .as_array()
                .expect("invariants")
                .contains(&json!(invariant)),
            "{invariant}"
        );
    }
    assert!(names("table.changeHitPoints"));
    assert!(names("table.changeHitPoints@1"));
    assert!(!names("table.setTokenCondition@1"));
}

#[test]
fn hp_change_one_token_an_npc_at_20_sent_minus_7_steps_its_own_hp_by_7_and_nothing_else() {
    let wire = row(json!({ "token": BANDIT_1, "delta": -7 }));
    let asked = request(&wire, Some(INSTANCE)).expect("a request");
    assert_eq!(
        asked,
        Request {
            token: BANDIT_1.to_string(),
            instance: Some(INSTANCE.to_string()),
            delta: -7,
        }
    );
    let prepared = prepare(&asked, &bandit(20)).expect("prepared");
    assert_eq!(
        prepared,
        Prepared {
            target_id: BANDIT_1.to_string(),
            payload: row(json!({ "hpCurrent": -7 })),
        }
    );
    assert_eq!(
        partition(&prepared.target_id),
        format!("BattlemapInstance/{BANDIT_1}")
    );
}

#[test]
fn pj_hp_not_tracked_an_npc_s_hp_land_between_0_and_its_max() {
    // 3 - 10: at 0, never below.
    assert_eq!(step(-10, &bandit(3)), json!(-3));
    // 13 + 50: at 20, never above.
    assert_eq!(step(50, &bandit(13)), json!(7));
    // Already at 0, or at its max: no step.
    assert_eq!(step(-4, &bandit(0)), json!(0));
    assert_eq!(step(5, &bandit(20)), json!(0));
    // Within the bounds: the delta as sent.
    assert_eq!(step(4, &bandit(10)), json!(4));
    assert_eq!(step(0, &bandit(10)), json!(0));
    // A delta no HP could ever reach is cut all the same.
    assert_eq!(step(i64::MIN, &bandit(13)), json!(-13));
    assert_eq!(step(i64::MAX, &bandit(13)), json!(7));
}

#[test]
fn pj_hp_not_tracked_a_pc_s_token_is_sent_the_delta_as_it_came_for_the_dataguard_to_reject() {
    let wire = row(json!({ "token": ELOWEN, "delta": -5 }));
    let asked = request(&wire, Some(INSTANCE)).expect("a request");
    assert_eq!(
        prepare(&asked, &elowen()),
        Ok(Prepared {
            target_id: ELOWEN.to_string(),
            payload: row(json!({ "hpCurrent": -5 })),
        })
    );
}

#[test]
fn a_payload_naming_other_fields_steps_the_current_hp_alone() {
    let wire = row(json!({
        "token": BANDIT_1,
        "delta": -2,
        "hpMax": 99,
        "dead": true,
        "healthBand": "indemne",
        "name": "Vesna",
    }));
    let asked = request(&wire, Some(INSTANCE)).expect("a request");
    let prepared = prepare(&asked, &bandit(20)).expect("prepared");
    assert_eq!(prepared.payload, row(json!({ "hpCurrent": -2 })));
}

#[test]
fn the_token_may_be_the_target_named_in_any_case() {
    let upper = BANDIT_1.to_ascii_uppercase();
    let wire = row(json!({ "delta": -1 }));
    let asked = request(&wire, Some(&upper)).expect("a request");
    assert_eq!(asked.token, BANDIT_1);
    assert_eq!(asked.instance, None);
    // Named twice, as the token and as the target: no instance named.
    let wire = row(json!({ "token": upper, "delta": -1 }));
    let asked = request(&wire, Some(BANDIT_1)).expect("a request");
    assert_eq!(asked.instance, None);
    assert!(prepare(&asked, &bandit(20)).is_ok());
}

#[test]
fn what_the_tables_could_never_hold_is_refused() {
    for (wire, target, refusal) in [
        (
            json!({ "token": BANDIT_1 }),
            Some(INSTANCE),
            Refusal::NotADelta,
        ),
        (
            json!({ "token": BANDIT_1, "delta": null }),
            Some(INSTANCE),
            Refusal::NotADelta,
        ),
        (
            json!({ "token": BANDIT_1, "delta": "-7" }),
            Some(INSTANCE),
            Refusal::NotADelta,
        ),
        (
            json!({ "token": BANDIT_1, "delta": -2.5 }),
            Some(INSTANCE),
            Refusal::NotADelta,
        ),
        (
            json!({ "token": BANDIT_1, "delta": u64::MAX }),
            Some(INSTANCE),
            Refusal::NotADelta,
        ),
        (
            json!({ "token": "bandit-1", "delta": -7 }),
            Some(INSTANCE),
            Refusal::NotAnId("token"),
        ),
        // Hex digits and dashes where a uuid has them, one digit short or over.
        (
            json!({ "token": &BANDIT_1[..35], "delta": -7 }),
            Some(INSTANCE),
            Refusal::NotAnId("token"),
        ),
        (
            json!({ "token": format!("{BANDIT_1}0"), "delta": -7 }),
            Some(INSTANCE),
            Refusal::NotAnId("token"),
        ),
        (
            json!({ "token": 3, "delta": -7 }),
            Some(INSTANCE),
            Refusal::NotAnId("token"),
        ),
        (
            json!({ "token": BANDIT_1, "delta": -7 }),
            Some("taverne"),
            Refusal::NotAnId("instance"),
        ),
        (json!({ "delta": -7 }), None, Refusal::NotAnId("token")),
        (
            json!({ "delta": -7 }),
            Some("  "),
            Refusal::NotAnId("token"),
        ),
    ] {
        assert_eq!(request(&row(wire.clone()), target), Err(refusal), "{wire}");
    }
}

#[test]
fn instance_isolated_a_token_the_engine_does_not_hold_on_the_instance_is_none_to_wound() {
    let asked = |target: &str| {
        request(
            &row(json!({ "token": BANDIT_1, "delta": -7 })),
            Some(target),
        )
        .expect("a request")
    };
    // On another instance.
    assert_eq!(
        prepare(&asked(AUTRE_INSTANCE), &bandit(20)),
        Err(Refusal::UnknownToken)
    );
    // Not held at all.
    assert_eq!(
        prepare(&asked(INSTANCE), &Row::new()),
        Err(Refusal::UnknownToken)
    );
    // A row of another token, or one that is no token's.
    let mut other = bandit(20);
    other.insert("tokenId".to_string(), json!(ELOWEN));
    assert_eq!(
        prepare(&asked(INSTANCE), &other),
        Err(Refusal::UnknownToken)
    );
    let combat = row(json!({ "combatId": BANDIT_1, "combatInstanceId": INSTANCE }));
    assert_eq!(
        prepare(&asked(INSTANCE), &combat),
        Err(Refusal::UnknownToken)
    );
    // The instance the engine holds in another case is the same one.
    let mut upper = bandit(20);
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
        Refusal::NotADelta.to_string(),
        "delta est un nombre entier de PV"
    );
    assert_eq!(
        Refusal::UnknownToken.to_string(),
        "ce token n'est pas sur cette instance"
    );
}
