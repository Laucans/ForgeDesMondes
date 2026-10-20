//! table.placeTokens' request and preparation: what a drop asks for, and the
//! token rows it makes against the roster the tables and the queue hold.

use place_tokens::{Character, MAX_COUNT, Refusal, Request, Roster, Row, names, prepare, request};
use serde_json::{Value, json};

const INSTANCE: &str = "b1a00000-0000-4000-8000-000000000001";
const BANDIT: &str = "e1000000-0000-4000-8000-0000000000b1";
const ELOWEN: &str = "e1000000-0000-4000-8000-0000000000e1";
const VALOMBRE: &str = "c1000000-0000-4000-8000-000000000001";

fn wire(value: Value) -> Row {
    serde_json::from_value(value).expect("a row")
}

fn ids(n: usize) -> Vec<String> {
    (1..=n)
        .map(|i| format!("70000000-0000-4000-8000-{i:012}"))
        .collect()
}

fn drop(character: &str, count: u16) -> Request {
    Request {
        instance: INSTANCE.to_string(),
        character: character.to_string(),
        q: 5,
        r: -2,
        count,
    }
}

fn npc(hp_max: Option<i64>) -> Roster {
    Roster {
        campaign: Some(VALOMBRE.to_string()),
        character: Some(Character {
            campaign: VALOMBRE.to_string(),
            kind: "pnj".to_string(),
            name: "Bandit".to_string(),
            hp_max,
        }),
        ..Roster::default()
    }
}

fn pc(present: bool, placed: usize) -> Roster {
    Roster {
        campaign: Some(VALOMBRE.to_string()),
        character: Some(Character {
            campaign: VALOMBRE.to_string(),
            kind: "pj".to_string(),
            name: "Elowen".to_string(),
            hp_max: Some(38),
        }),
        present,
        placed,
        taken: Vec::new(),
    }
}

fn field<'a>(rows: &'a [Row], field: &str) -> Vec<&'a Value> {
    rows.iter().map(|row| &row[field]).collect()
}

#[test]
fn it_names_its_data_capability_at_any_version() {
    assert!(names("table.placeTokens"));
    assert!(names("table.placeTokens@1"));
    assert!(!names("table.placeToken"));
    assert!(!names("table.moveToken@1"));
}

#[test]
fn a_request_names_the_target_instance_the_character_the_hex_and_the_count() {
    let asked = request(
        &wire(json!({ "character": BANDIT.to_uppercase(), "q": 5, "r": -2, "count": 3 })),
        Some(&INSTANCE.to_uppercase()),
    )
    .expect("a drop");
    assert_eq!(asked, drop(BANDIT, 3));
    // With no target, the payload's instance; with no count, one token.
    let asked = request(
        &wire(json!({ "instance": INSTANCE, "character": BANDIT, "q": 5, "r": -2 })),
        None,
    )
    .expect("a drop");
    assert_eq!(asked.count, 1);
    assert_eq!(asked.instance, INSTANCE);
}

#[test]
fn a_request_the_tables_could_never_hold_is_refused() {
    let base = json!({ "character": BANDIT, "q": 5, "r": -2 });
    let refused = |change: Value, target: Option<&str>| {
        let mut payload = base.clone();
        for (k, v) in change.as_object().expect("an object") {
            payload[k] = v.clone();
        }
        request(&wire(payload), target).expect_err("refused")
    };
    assert_eq!(refused(json!({}), None), Refusal::NotAnId("instance"));
    assert_eq!(
        refused(json!({}), Some("bm-gue")),
        Refusal::NotAnId("instance")
    );
    assert_eq!(
        refused(json!({ "character": "bandit" }), Some(INSTANCE)),
        Refusal::NotAnId("character")
    );
    assert_eq!(
        refused(json!({ "q": 1.5 }), Some(INSTANCE)),
        Refusal::NotAHex("q")
    );
    assert_eq!(
        refused(json!({ "r": 3_000_000_000_i64 }), Some(INSTANCE)),
        Refusal::NotAHex("r")
    );
    for count in [
        json!(0),
        json!(-1),
        json!(MAX_COUNT + 1),
        json!("3"),
        json!(2.5),
    ] {
        assert_eq!(
            refused(json!({ "count": count }), Some(INSTANCE)),
            Refusal::Count,
            "{count}"
        );
    }
}

#[test]
fn multi_drop_numbering_a_first_drop_of_three_numbers_them_1_to_3() {
    let tokens = prepare(&drop(BANDIT, 3), &npc(Some(11)), &ids(3)).expect("dropped");
    assert_eq!(tokens.len(), 3);
    assert_eq!(field(&tokens, "number"), [&json!(1), &json!(2), &json!(3)]);
    assert_eq!(
        field(&tokens, "name"),
        [&json!("Bandit 1"), &json!("Bandit 2"), &json!("Bandit 3")]
    );
    // Each its own token, on the hex dropped on, visible, with no condition.
    let expected: Vec<Value> = ids(3).into_iter().map(Value::from).collect();
    assert_eq!(
        field(&tokens, "tokenId"),
        expected.iter().collect::<Vec<_>>()
    );
    for token in &tokens {
        assert_eq!(token["tokenInstanceId"], INSTANCE);
        assert_eq!(token["characterId"], BANDIT);
        assert_eq!((&token["q"], &token["r"]), (&json!(5), &json!(-2)));
        assert_eq!(token["visible"], true);
        assert_eq!(token["conditions"], json!([]));
    }
}

#[test]
fn multi_drop_numbering_two_more_follow_the_numbers_taken() {
    let roster = Roster {
        placed: 3,
        taken: vec![1, 2, 3],
        ..npc(Some(11))
    };
    let tokens = prepare(&drop(BANDIT, 2), &roster, &ids(2)).expect("dropped");
    assert_eq!(field(&tokens, "number"), [&json!(4), &json!(5)]);
    assert_eq!(
        field(&tokens, "name"),
        [&json!("Bandit 4"), &json!("Bandit 5")]
    );
    // After the highest one, whatever was renamed or removed in between.
    let roster = Roster {
        placed: 2,
        taken: vec![7, 2],
        ..npc(Some(11))
    };
    let tokens = prepare(&drop(BANDIT, 1), &roster, &ids(1)).expect("dropped");
    assert_eq!(field(&tokens, "number"), [&json!(8)]);
}

#[test]
fn multi_drop_numbering_refuses_numbers_a_smallint_cannot_hold() {
    let roster = Roster {
        taken: vec![i64::from(i16::MAX) - 1],
        ..npc(Some(11))
    };
    assert_eq!(
        prepare(&drop(BANDIT, 2), &roster, &ids(2)),
        Err(Refusal::NoNumberLeft)
    );
    let one = prepare(&drop(BANDIT, 1), &roster, &ids(1)).expect("the last number");
    assert_eq!(one[0]["number"], i16::MAX);
}

#[test]
fn pj_hp_not_tracked_an_npc_copies_its_sheet_s_hp_max_a_pc_tracks_none() {
    let tokens = prepare(&drop(BANDIT, 2), &npc(Some(11)), &ids(2)).expect("dropped");
    assert_eq!(field(&tokens, "hpMax"), [&json!(11), &json!(11)]);
    assert_eq!(field(&tokens, "hpCurrent"), [&json!(11), &json!(11)]);
    // A sheet without HP max gives none to copy, current nor maximum.
    let tokens = prepare(&drop(BANDIT, 1), &npc(None), &ids(1)).expect("dropped");
    assert_eq!(tokens[0]["hpMax"], Value::Null);
    assert_eq!(tokens[0]["hpCurrent"], Value::Null);
    // A PC's token tracks no HP, whatever its sheet says.
    let tokens = prepare(&drop(ELOWEN, 1), &pc(true, 0), &ids(1)).expect("placed");
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0]["hpMax"], Value::Null);
    assert_eq!(tokens[0]["hpCurrent"], Value::Null);
    assert_eq!(tokens[0]["number"], Value::Null);
    assert_eq!(tokens[0]["name"], "Elowen");
}

#[test]
fn token_roster_a_pc_absent_or_already_placed_or_dropped_twice_is_refused() {
    assert_eq!(
        prepare(&drop(ELOWEN, 1), &pc(false, 0), &ids(1)),
        Err(Refusal::Absent)
    );
    assert_eq!(
        prepare(&drop(ELOWEN, 1), &pc(true, 1), &ids(1)),
        Err(Refusal::AlreadyPlaced)
    );
    assert_eq!(
        prepare(&drop(ELOWEN, 2), &pc(true, 0), &ids(2)),
        Err(Refusal::OnePcToken)
    );
}

#[test]
fn an_unknown_instance_or_character_or_one_of_another_campaign_is_refused() {
    let none = Roster {
        campaign: None,
        ..npc(Some(11))
    };
    assert_eq!(
        prepare(&drop(BANDIT, 1), &none, &ids(1)),
        Err(Refusal::UnknownInstance)
    );
    let nobody = Roster {
        character: None,
        ..npc(Some(11))
    };
    assert_eq!(
        prepare(&drop(BANDIT, 1), &nobody, &ids(1)),
        Err(Refusal::UnknownCharacter)
    );
    let mut place = npc(Some(11));
    if let Some(character) = place.character.as_mut() {
        character.kind = "lieu".to_string();
    }
    assert_eq!(
        prepare(&drop(BANDIT, 1), &place, &ids(1)),
        Err(Refusal::UnknownCharacter)
    );
    let mut elsewhere = npc(Some(11));
    if let Some(character) = elsewhere.character.as_mut() {
        character.campaign = "c1000000-0000-4000-8000-000000000002".to_string();
    }
    assert_eq!(
        prepare(&drop(BANDIT, 1), &elsewhere, &ids(1)),
        Err(Refusal::CharacterOfAnotherCampaign)
    );
}

#[test]
fn a_token_writes_the_fields_the_manifest_touches_and_no_other() {
    let manifest: Value =
        serde_json::from_str(include_str!("../data-capability.json")).expect("contract F");
    let mut touched: Vec<String> = manifest["touches"]
        .as_array()
        .expect("touches")
        .iter()
        .map(|touch| {
            touch
                .as_str()
                .and_then(|touch| touch.strip_prefix("BattlemapInstance."))
                .expect("a field of the aggregate")
                .to_string()
        })
        .collect();
    touched.sort();
    assert_eq!(manifest["effect"], "insert");
    for (request, roster) in [
        (drop(BANDIT, 1), npc(Some(11))),
        (drop(ELOWEN, 1), pc(true, 0)),
    ] {
        let tokens = prepare(&request, &roster, &ids(1)).expect("dropped");
        let written: Vec<String> = tokens[0].keys().cloned().collect();
        assert_eq!(written, touched);
    }
}
