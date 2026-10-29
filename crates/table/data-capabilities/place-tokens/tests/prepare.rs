//! `request` and `prepare`: the DM's drop made the token rows written on
//! `BattlemapInstance/<token id>` — or refused before anything is queued.

use place_tokens::{
    Character, MAX_COUNT, Refusal, Request, Roster, Row, follow_ups, fresh_token_id, names,
    prepare, request, token_id,
};
use serde_json::{Value, json};

const VALOMBRE: &str = "c1000000-0000-4000-8000-000000000001";
const BRUME: &str = "c1000000-0000-4000-8000-000000000002";
const INSTANCE: &str = "b1000000-0000-4000-8000-000000000001";
const BANDIT: &str = "e1000000-0000-4000-8000-000000000007";
const ELOWEN: &str = "e1000000-0000-4000-8000-000000000001";

fn row(value: Value) -> Row {
    let Value::Object(map) = value else {
        panic!("an object");
    };
    map.into_iter().collect()
}

fn dropped(character: &str, count: u16) -> Request {
    Request {
        instance: INSTANCE.into(),
        character: character.into(),
        q: 5,
        r: -2,
        count,
    }
}

fn bandit() -> Character {
    Character {
        campaign: VALOMBRE.into(),
        kind: "pnj".into(),
        name: "Bandit".into(),
        hp_max: Some(11),
    }
}

fn elowen() -> Character {
    Character {
        campaign: VALOMBRE.into(),
        kind: "pj".into(),
        name: "Elowen Ventargent".into(),
        hp_max: Some(44),
    }
}

fn roster(character: Character) -> Roster {
    Roster {
        campaign: Some(VALOMBRE.into()),
        character: Some(character),
        present: false,
        placed: false,
        highest: None,
        removed: Vec::new(),
    }
}

fn numbers(tokens: &[Row]) -> Vec<Value> {
    tokens.iter().map(|token| token["number"].clone()).collect()
}

#[test]
fn the_data_capability_is_named_with_or_without_its_version() {
    assert!(names("table.placeTokens"));
    assert!(names("table.placeTokens@1"));
    assert!(!names("table.placeToken"));
    assert!(!names("table.createBattlemapInstance@1"));
}

#[test]
fn a_request_names_its_instance_by_the_target_or_the_payload() {
    let wire = row(json!({
        "character": format!(" {} ", BANDIT.to_uppercase()),
        "q": 5,
        "r": -2,
        "count": 3,
    }));
    let asked = dropped(BANDIT, 3);
    assert_eq!(
        request(&wire, Some(&INSTANCE.to_uppercase())),
        Ok(asked.clone())
    );
    let mut named = wire.clone();
    named.insert("instance".into(), json!(INSTANCE));
    assert_eq!(request(&named, None), Ok(asked));
    // The target wins over the payload.
    named.insert("instance".into(), json!("nowhere"));
    assert!(request(&named, Some(INSTANCE)).is_ok());
    assert_eq!(request(&wire, None), Err(Refusal::NotAnId("instance")));
}

#[test]
fn a_request_drops_one_token_unless_it_says_how_many() {
    for wire in [
        json!({ "character": BANDIT, "q": 0, "r": 0 }),
        json!({ "character": BANDIT, "q": 0, "r": 0, "count": null }),
    ] {
        assert_eq!(request(&row(wire), Some(INSTANCE)).map(|r| r.count), Ok(1));
    }
    let most = row(json!({ "character": BANDIT, "q": 0, "r": 0, "count": MAX_COUNT }));
    assert_eq!(
        request(&most, Some(INSTANCE)).map(|r| r.count),
        Ok(MAX_COUNT)
    );
}

#[test]
fn a_count_is_a_whole_number_from_1_to_12() {
    for bad in [
        json!(0),
        json!(13),
        json!(-1),
        json!(2.5),
        json!("3"),
        json!(70_000),
    ] {
        let wire = row(json!({ "character": BANDIT, "q": 0, "r": 0, "count": bad }));
        assert_eq!(request(&wire, Some(INSTANCE)), Err(Refusal::Count), "{bad}");
    }
}

#[test]
fn a_field_that_is_no_id_or_no_hex_is_refused_by_its_name() {
    let full = json!({ "character": BANDIT, "q": 1, "r": 2 });
    for bad in [Value::Null, json!("bandit"), json!(7)] {
        let mut wire = full.clone();
        wire["character"] = bad.clone();
        assert_eq!(
            request(&row(wire), Some(INSTANCE)),
            Err(Refusal::NotAnId("character")),
            "{bad}"
        );
    }
    assert_eq!(
        request(&row(full.clone()), Some("bmi-gue")),
        Err(Refusal::NotAnId("instance"))
    );
    for field in ["q", "r"] {
        for bad in [
            Value::Null,
            json!("1"),
            json!(1.5),
            json!(3_000_000_000_i64),
        ] {
            let mut wire = full.clone();
            wire[field] = bad.clone();
            assert_eq!(
                request(&row(wire), Some(INSTANCE)),
                Err(Refusal::NotAHex(field)),
                "{field}: {bad}"
            );
        }
    }
    // The whole range of an `integer`, negative included: axial hexes.
    let edge = row(json!({ "character": BANDIT, "q": i32::MIN, "r": i32::MAX }));
    assert!(request(&edge, Some(INSTANCE)).is_ok());
}

#[test]
fn multi_drop_numbering_a_first_drop_of_three_is_numbered_1_to_3() {
    let tokens = prepare(&dropped(BANDIT, 3), &roster(bandit())).expect("dropped");
    assert_eq!(numbers(&tokens), [json!(1), json!(2), json!(3)]);
    let named: Vec<&Value> = tokens.iter().map(|token| &token["name"]).collect();
    assert_eq!(
        named,
        [&json!("Bandit 1"), &json!("Bandit 2"), &json!("Bandit 3")]
    );
}

#[test]
fn multi_drop_numbering_a_drop_numbers_after_the_highest_taken() {
    let taken = Roster {
        highest: Some(3),
        ..roster(bandit())
    };
    let tokens = prepare(&dropped(BANDIT, 2), &taken).expect("dropped");
    assert_eq!(numbers(&tokens), [json!(4), json!(5)]);
    assert_eq!(tokens[1]["name"], "Bandit 5");
}

#[test]
fn multi_drop_numbering_each_number_has_its_own_token() {
    let tokens = prepare(&dropped(BANDIT, 3), &roster(bandit())).expect("dropped");
    let ids: Vec<Value> = (1..=3)
        .map(|n| json!(token_id(INSTANCE, BANDIT, Some(n))))
        .collect();
    let written: Vec<Value> = tokens
        .iter()
        .map(|token| token["tokenId"].clone())
        .collect();
    assert_eq!(written, ids);
    assert_ne!(ids[0], ids[1]);
    assert_ne!(
        token_id(INSTANCE, BANDIT, Some(1)),
        token_id("b1000000-0000-4000-8000-000000000002", BANDIT, Some(1))
    );
    assert_ne!(
        token_id(INSTANCE, BANDIT, Some(1)),
        token_id(INSTANCE, ELOWEN, Some(1))
    );
    assert_ne!(
        token_id(INSTANCE, ELOWEN, None),
        token_id(INSTANCE, ELOWEN, Some(1))
    );
}

#[test]
fn multi_drop_numbering_no_number_past_what_a_smallint_holds() {
    let full = Roster {
        highest: Some(i64::from(i16::MAX) - 1),
        ..roster(bandit())
    };
    assert!(prepare(&dropped(BANDIT, 1), &full).is_ok());
    assert_eq!(
        prepare(&dropped(BANDIT, 2), &full),
        Err(Refusal::NoNumberLeft)
    );
}

#[test]
fn a_dropped_npc_token_is_the_whole_row_on_the_hex() {
    let tokens = prepare(&dropped(BANDIT, 1), &roster(bandit())).expect("dropped");
    assert_eq!(
        tokens,
        [row(json!({
            "tokenId": token_id(INSTANCE, BANDIT, Some(1)),
            "tokenInstanceId": INSTANCE,
            "characterId": BANDIT,
            "characterType": "pnj",
            "number": 1,
            "name": "Bandit 1",
            "q": 5,
            "r": -2,
            "visible": true,
            "hpCurrent": 11,
            "hpMax": 11,
            "conditions": [],
        }))]
    );
}

#[test]
fn pj_hp_not_tracked_an_npc_copies_its_sheet_s_hp_max_a_pc_none() {
    for token in prepare(&dropped(BANDIT, 3), &roster(bandit())).expect("dropped") {
        assert_eq!(
            (&token["hpMax"], &token["hpCurrent"]),
            (&json!(11), &json!(11))
        );
    }
    let present = Roster {
        present: true,
        ..roster(elowen())
    };
    let tokens = prepare(&dropped(ELOWEN, 1), &present).expect("dropped");
    assert_eq!(
        tokens,
        [row(json!({
            "tokenId": token_id(INSTANCE, ELOWEN, None),
            "tokenInstanceId": INSTANCE,
            "characterId": ELOWEN,
            "characterType": "pj",
            "number": null,
            "name": "Elowen Ventargent",
            "q": 5,
            "r": -2,
            "visible": true,
            "hpCurrent": null,
            "hpMax": null,
            "conditions": [],
        }))]
    );
}

#[test]
fn pj_hp_not_tracked_an_hp_max_the_table_cannot_hold_is_none_to_copy() {
    for hp_max in [None, Some(-1), Some(40_000)] {
        let sheet = Character { hp_max, ..bandit() };
        let tokens = prepare(&dropped(BANDIT, 1), &roster(sheet)).expect("dropped");
        assert_eq!(tokens[0]["hpMax"], Value::Null, "{hp_max:?}");
        assert_eq!(tokens[0]["hpCurrent"], Value::Null, "{hp_max:?}");
    }
    let unhurt = Character {
        hp_max: Some(0),
        ..bandit()
    };
    let tokens = prepare(&dropped(BANDIT, 1), &roster(unhurt)).expect("dropped");
    assert_eq!(tokens[0]["hpMax"], 0);
    let most = Character {
        hp_max: Some(i64::from(i16::MAX)),
        ..bandit()
    };
    let tokens = prepare(&dropped(BANDIT, 1), &roster(most)).expect("dropped");
    assert_eq!(tokens[0]["hpMax"], i16::MAX);
}

#[test]
fn token_roster_a_pc_is_placed_once_and_only_when_present() {
    let absent = roster(elowen());
    assert_eq!(prepare(&dropped(ELOWEN, 1), &absent), Err(Refusal::Absent));
    let placed = Roster {
        present: true,
        placed: true,
        ..roster(elowen())
    };
    assert_eq!(
        prepare(&dropped(ELOWEN, 1), &placed),
        Err(Refusal::AlreadyPlaced)
    );
    let present = Roster {
        present: true,
        ..roster(elowen())
    };
    assert_eq!(
        prepare(&dropped(ELOWEN, 2), &present),
        Err(Refusal::OnePcToken)
    );
    // An NPC needs no presence, and has as many tokens as it is dropped.
    let npc = Roster {
        placed: true,
        highest: Some(1),
        ..roster(bandit())
    };
    assert_eq!(prepare(&dropped(BANDIT, 2), &npc).map(|t| t.len()), Ok(2));
}

#[test]
fn what_the_tables_do_not_hold_is_refused_as_unknown() {
    let nowhere = Roster {
        campaign: None,
        ..roster(bandit())
    };
    assert_eq!(
        prepare(&dropped(BANDIT, 1), &nowhere),
        Err(Refusal::UnknownInstance)
    );
    assert_eq!(
        prepare(&dropped(BANDIT, 1), &Roster::default()),
        Err(Refusal::UnknownInstance)
    );
    let nobody = Roster {
        character: None,
        ..roster(bandit())
    };
    assert_eq!(
        prepare(&dropped(BANDIT, 1), &nobody),
        Err(Refusal::UnknownCharacter)
    );
    // A place is no character.
    let place = Character {
        kind: "lieu".into(),
        ..bandit()
    };
    assert_eq!(
        prepare(&dropped(BANDIT, 1), &roster(place)),
        Err(Refusal::UnknownCharacter)
    );
}

#[test]
fn a_character_of_another_campaign_is_refused_and_ids_compare_in_any_case() {
    let other = Character {
        campaign: BRUME.into(),
        ..bandit()
    };
    assert_eq!(
        prepare(&dropped(BANDIT, 1), &roster(other)),
        Err(Refusal::CharacterOfAnotherCampaign)
    );
    let upper = Character {
        campaign: VALOMBRE.to_uppercase(),
        ..bandit()
    };
    assert!(prepare(&dropped(BANDIT, 1), &roster(upper)).is_ok());
}

#[test]
fn a_drop_of_n_is_followed_by_n_minus_1_drops_of_one() {
    assert!(follow_ups(&dropped(BANDIT, 1)).is_empty());
    let then = follow_ups(&dropped(BANDIT, 3));
    assert_eq!(then.len(), 2);
    for follow in then {
        assert_eq!(follow.data_capability, "table.placeTokens");
        assert_eq!(follow.target_id.as_deref(), Some(INSTANCE));
        assert_eq!(
            follow.payload,
            row(json!({ "character": BANDIT, "q": 5, "r": -2 }))
        );
        assert!(follow.idempotency_key.is_none());
        // A follow-up asks for one token: its own Prepare makes no follow-up.
        assert_eq!(
            request(&follow.payload, follow.target_id.as_deref()).map(|r| r.count),
            Ok(1)
        );
    }
}

#[test]
fn the_fields_a_drop_writes_are_exactly_the_ones_its_manifest_touches() {
    let manifest: Value =
        serde_json::from_str(include_str!("../data-capability.json")).expect("contract F");
    let mut touches: Vec<String> = manifest["touches"]
        .as_array()
        .expect("touches")
        .iter()
        .map(|touch| touch.as_str().expect("a touch").to_owned())
        .collect();
    touches.sort();
    let written: Vec<String> = prepare(&dropped(BANDIT, 1), &roster(bandit())).expect("dropped")[0]
        .keys()
        .map(|field| format!("BattlemapInstance.{field}"))
        .collect();
    assert_eq!(written, touches);
    assert_eq!(manifest["effect"], "insert");
    assert_eq!(manifest["target"]["aggregate"], "BattlemapInstance");
    assert_eq!(manifest["target"]["id"], "$instance");
}

#[test]
fn only_what_is_not_there_is_refused_as_not_found_and_every_refusal_says_why() {
    let all = [
        Refusal::NotAnId("character"),
        Refusal::NotAHex("q"),
        Refusal::Count,
        Refusal::UnknownInstance,
        Refusal::UnknownCharacter,
        Refusal::CharacterOfAnotherCampaign,
        Refusal::Absent,
        Refusal::AlreadyPlaced,
        Refusal::OnePcToken,
        Refusal::NoNumberLeft,
    ];
    for refusal in &all {
        assert_eq!(
            refusal.not_found(),
            matches!(
                refusal,
                Refusal::UnknownInstance | Refusal::UnknownCharacter
            ),
            "{refusal:?}"
        );
        assert!(!refusal.to_string().is_empty(), "{refusal:?}");
    }
    assert!(
        Refusal::NotAnId("character")
            .to_string()
            .contains("character")
    );
    assert!(Refusal::NotAHex("r").to_string().contains('r'));
    assert!(Refusal::Count.to_string().contains("12"));
    for refusal in [Refusal::Absent, Refusal::AlreadyPlaced, Refusal::OnePcToken] {
        assert!(refusal.to_string().contains("token-roster"), "{refusal:?}");
    }
}

#[test]
fn a_token_removed_keeps_its_tombstone_when_its_number_is_dropped_again() {
    let first = token_id(INSTANCE, BANDIT, Some(2));
    assert_eq!(fresh_token_id(INSTANCE, BANDIT, Some(2), &[]), first);
    let second = fresh_token_id(INSTANCE, BANDIT, Some(2), &[first.to_uppercase()]);
    assert_ne!(second, first);
    let third = fresh_token_id(INSTANCE, BANDIT, Some(2), &[first.clone(), second.clone()]);
    assert!(third != first && third != second, "{third}");
    // Another number's removal changes nothing.
    assert_eq!(
        fresh_token_id(INSTANCE, BANDIT, Some(3), std::slice::from_ref(&first)),
        token_id(INSTANCE, BANDIT, Some(3))
    );
    let removed = Roster {
        highest: Some(1),
        removed: vec![first.clone()],
        ..roster(bandit())
    };
    let tokens = prepare(&dropped(BANDIT, 2), &removed).expect("dropped");
    assert_eq!(numbers(&tokens), [json!(2), json!(3)]);
    assert_eq!(tokens[0]["tokenId"], second);
    assert_eq!(tokens[1]["tokenId"], token_id(INSTANCE, BANDIT, Some(3)));
    // A PC taken off the map and placed again: a token of its own.
    let pc = token_id(INSTANCE, ELOWEN, None);
    let back = Roster {
        present: true,
        removed: vec![pc.clone()],
        ..roster(elowen())
    };
    let tokens = prepare(&dropped(ELOWEN, 1), &back).expect("dropped");
    assert_eq!(
        tokens[0]["tokenId"],
        fresh_token_id(INSTANCE, ELOWEN, None, std::slice::from_ref(&pc))
    );
    assert_ne!(tokens[0]["tokenId"], json!(pc));
}

#[test]
fn a_token_s_id_never_changes_from_one_version_to_the_next() {
    // UUIDv5 of `token:<instance>:<character>[:<number>][#<generation>]`.
    for (character, number, id) in [
        (BANDIT, Some(1), "46f1c042-cf7b-50c3-a3f2-cfc6f0d54818"),
        (BANDIT, Some(2), "bec3fa66-b997-5224-95da-9d7212684491"),
        (ELOWEN, None, "3edfc08d-9a1a-5337-96f0-87cc0fd87372"),
    ] {
        assert_eq!(token_id(INSTANCE, character, number), id, "{number:?}");
    }
    let removed = ["bec3fa66-b997-5224-95da-9d7212684491".to_owned()];
    assert_eq!(
        fresh_token_id(INSTANCE, BANDIT, Some(2), &removed),
        "7a85fa24-336f-5f48-9759-4f15cef94abd"
    );
}
