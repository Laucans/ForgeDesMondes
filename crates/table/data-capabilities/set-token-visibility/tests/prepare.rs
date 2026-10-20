//! `table.setTokenVisibility`'s judgement before a command is queued: what
//! the DM sends, read ([`request`]), and made the one field written on the
//! token's partition ([`prepare`]).

use serde_json::{Value, json};
use set_token_visibility::{Prepared, Refusal, Request, Row, names, partition, prepare, request};

const INSTANCE: &str = "b1000000-0000-4000-8000-000000000001";
const AUTRE_INSTANCE: &str = "b1000000-0000-4000-8000-000000000002";
const CHEF_BANDIT_1: &str = "70000000-0000-4000-8000-000000000003";

fn row(value: Value) -> Row {
    let Value::Object(fields) = value else {
        panic!("a row is an object");
    };
    fields.into_iter().collect()
}

/// Chef bandit 1, as the engine holds it on its partition.
fn chief() -> Row {
    row(json!({
        "tokenId": CHEF_BANDIT_1,
        "tokenInstanceId": INSTANCE,
        "characterId": "e1000000-0000-4000-8000-000000000004",
        "number": 1,
        "name": "Chef bandit 1",
        "q": 6,
        "r": 5,
        "visible": true,
        "hpCurrent": 22,
        "hpMax": 22,
        "conditions": [],
    }))
}

fn manifest() -> Value {
    serde_json::from_str(include_str!("../data-capability.json")).expect("contract F")
}

#[test]
fn the_manifest_names_this_data_capability_an_update_touching_the_visibility_alone() {
    let manifest = manifest();
    assert_eq!(manifest["dataCapability"], "table.setTokenVisibility");
    assert_eq!(manifest["owner"], "dataguard");
    assert_eq!(manifest["effect"], "update");
    assert_eq!(manifest["touches"], json!(["BattlemapInstance.visible"]));
    assert_eq!(manifest["target"]["aggregate"], "BattlemapInstance");
    assert!(
        manifest["invariants"]
            .as_array()
            .expect("invariants")
            .contains(&json!("hidden-keeps-rank"))
    );
    assert!(names("table.setTokenVisibility"));
    assert!(names("table.setTokenVisibility@1"));
    assert!(!names("table.setTokenCondition@1"));
}

#[test]
fn a_visible_token_sent_visible_false_is_written_hidden_and_nothing_else() {
    let wire = row(json!({ "token": CHEF_BANDIT_1, "visible": false }));
    let asked = request(&wire, Some(INSTANCE)).expect("a request");
    assert_eq!(
        asked,
        Request {
            token: CHEF_BANDIT_1.to_string(),
            instance: Some(INSTANCE.to_string()),
            visible: false,
        }
    );
    assert_eq!(
        prepare(&asked, &chief()),
        Ok(Prepared {
            target_id: CHEF_BANDIT_1.to_string(),
            payload: row(json!({ "visible": false })),
        })
    );
}

#[test]
fn hidden_keeps_rank_a_reveal_writes_no_rank_whatever_the_dm_sends_beside_it() {
    let wire = row(json!({
        "token": CHEF_BANDIT_1,
        "visible": true,
        "rank": 1,
        "orderIndex": 0,
        "name": "Vesna",
    }));
    let asked = request(&wire, Some(INSTANCE)).expect("a request");
    let prepared = prepare(&asked, &chief()).expect("prepared");
    assert_eq!(prepared.payload, row(json!({ "visible": true })));
    assert_eq!(prepared.target_id, CHEF_BANDIT_1);
    assert_eq!(
        partition(&prepared.target_id),
        format!("BattlemapInstance/{CHEF_BANDIT_1}")
    );
}

#[test]
fn the_token_may_be_the_target_named_in_any_case() {
    let upper = CHEF_BANDIT_1.to_ascii_uppercase();
    let wire = row(json!({ "visible": false }));
    let asked = request(&wire, Some(&upper)).expect("a request");
    assert_eq!(asked.token, CHEF_BANDIT_1);
    assert_eq!(asked.instance, None);
    // Named twice, as the token and as the target: no instance named.
    let wire = row(json!({ "token": upper, "visible": false }));
    let asked = request(&wire, Some(CHEF_BANDIT_1)).expect("a request");
    assert_eq!(asked.instance, None);
    assert!(prepare(&asked, &chief()).is_ok());
}

#[test]
fn what_the_tables_could_never_hold_is_refused() {
    for (wire, target, refusal) in [
        (
            json!({ "token": CHEF_BANDIT_1 }),
            Some(INSTANCE),
            Refusal::NotAVisibility,
        ),
        (
            json!({ "token": CHEF_BANDIT_1, "visible": null }),
            Some(INSTANCE),
            Refusal::NotAVisibility,
        ),
        (
            json!({ "token": CHEF_BANDIT_1, "visible": "false" }),
            Some(INSTANCE),
            Refusal::NotAVisibility,
        ),
        (
            json!({ "token": CHEF_BANDIT_1, "visible": 0 }),
            Some(INSTANCE),
            Refusal::NotAVisibility,
        ),
        (
            json!({ "token": "chef-bandit-1", "visible": false }),
            Some(INSTANCE),
            Refusal::NotAnId("token"),
        ),
        (
            json!({ "token": 3, "visible": false }),
            Some(INSTANCE),
            Refusal::NotAnId("token"),
        ),
        (
            json!({ "token": CHEF_BANDIT_1, "visible": false }),
            Some("taverne"),
            Refusal::NotAnId("instance"),
        ),
        (json!({ "visible": false }), None, Refusal::NotAnId("token")),
        (
            json!({ "visible": false }),
            Some("  "),
            Refusal::NotAnId("token"),
        ),
    ] {
        assert_eq!(request(&row(wire.clone()), target), Err(refusal), "{wire}");
    }
}

#[test]
fn instance_isolated_a_token_the_engine_does_not_hold_on_the_instance_is_none_to_hide() {
    let asked = |target: &str| {
        request(
            &row(json!({ "token": CHEF_BANDIT_1, "visible": false })),
            Some(target),
        )
        .expect("a request")
    };
    // On another instance.
    assert_eq!(
        prepare(&asked(AUTRE_INSTANCE), &chief()),
        Err(Refusal::UnknownToken)
    );
    // Not held at all.
    assert_eq!(
        prepare(&asked(INSTANCE), &Row::new()),
        Err(Refusal::UnknownToken)
    );
    // A row of another token, or one that is no token's.
    let mut other = chief();
    other.insert("tokenId".to_string(), json!(AUTRE_INSTANCE));
    assert_eq!(
        prepare(&asked(INSTANCE), &other),
        Err(Refusal::UnknownToken)
    );
    let combat = row(json!({ "combatId": CHEF_BANDIT_1, "combatInstanceId": INSTANCE }));
    assert_eq!(
        prepare(&asked(INSTANCE), &combat),
        Err(Refusal::UnknownToken)
    );
    // The instance the engine holds in another case is the same one.
    let mut upper = chief();
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
        Refusal::NotAVisibility.to_string(),
        "visible vaut true ou false"
    );
    assert_eq!(
        Refusal::UnknownToken.to_string(),
        "ce token n'est pas sur cette instance"
    );
}
