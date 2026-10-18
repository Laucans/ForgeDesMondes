//! `prepare`: the editor's payload made the command written on
//! `Entity/<id>` — or refused before it is queued.

use save_entity::{
    DEFAULT_TOKEN_COLOR, Fresh, Prepared, Refusal, Row, diff, element_id, names, prepare,
};
use serde_json::{Map, Value, json};

const VALOMBRE: &str = "c1000000-0000-4000-8000-000000000001";
const BRUME: &str = "c1000000-0000-4000-8000-000000000002";
const CLAIRE: &str = "b1000000-0000-4000-8000-000000000001";
const MORLOX: &str = "e1000000-0000-4000-8000-000000000003";
const NOW: &str = "2026-10-17T10:00:00Z";
const LATER: &str = "2026-10-17T11:00:00Z";

fn row(value: Value) -> Row {
    let Value::Object(map) = value else {
        panic!("a row is an object");
    };
    map.into_iter().collect()
}

fn fresh<'a>(version: &'a str, now: &'a str) -> Fresh<'a> {
    Fresh {
        entity: MORLOX,
        version,
        now,
    }
}

/// Morlox saved new, from `entity`.
fn new(entity: Value) -> Result<Prepared, Refusal> {
    prepare(
        &row(json!({ "campaign": VALOMBRE, "entity": entity })),
        &Row::new(),
        fresh("f1", NOW),
    )
}

/// The row Morlox's save leaves: `base` and the payload over it.
fn after(base: &Row, prepared: &Prepared) -> Row {
    let mut after = base.clone();
    after.extend(prepared.payload.clone());
    after
}

fn morlox() -> Value {
    json!({
        "kind": "pnj",
        "name": "Morlox",
        "aliases": ["le gobelin", " Le Gobelin ", "", "le marchand", 7],
        "stats": { "FOR": 8, "DEX": 15, "CON": 10, "INT": 13, "SAG": 9, "CHA": 12 },
        "ca": 13,
        "pvMax": 21,
        "prompt": "Un gobelin marchand.",
        "features": [{ "name": "Fuite agile" }],
        "subtitle": "Gobelin, marchand",
        "type": "Gobelin, marchand",
    })
}

#[test]
fn the_editor_s_names_are_the_aggregate_s_and_anything_else_is_dropped() {
    let Prepared { target_id, payload } = new(morlox()).expect("saved");
    assert_eq!(target_id, MORLOX);
    for (field, value) in [
        ("entityId", json!(MORLOX)),
        ("entityCampaignId", json!(VALOMBRE)),
        ("type", json!("pnj")),
        ("name", json!("Morlox")),
        ("strength", json!(8)),
        ("dexterity", json!(15)),
        ("charisma", json!(12)),
        ("armorClass", json!(13)),
        ("hpMax", json!(21)),
        ("initialPrompt", json!("Un gobelin marchand.")),
        ("actions", json!([{ "name": "Fuite agile" }])),
        ("spells", json!([])),
        ("characterEntityId", json!(MORLOX)),
        ("tokenColor", json!(DEFAULT_TOKEN_COLOR)),
        ("source", json!("dm")),
        ("entityVersionId", json!("f1")),
        ("entityCreatedAt", json!(NOW)),
        ("restoredFromVersionId", Value::Null),
    ] {
        assert_eq!(payload.get(field), Some(&value), "{field}");
    }
    // Neither the editor's subtitle nor its other keys reach the row.
    for field in ["subtitle", "stats", "ca", "pvMax", "prompt", "features"] {
        assert!(!payload.contains_key(field), "{field}");
    }
}

#[test]
fn a_dex_modifier_given_in_any_shape_is_never_written() {
    let mut entity = morlox();
    entity["dexterityModifier"] = json!(7);
    entity["dexModifier"] = json!(7);
    entity["stats"]["DEX_MOD"] = json!(7);
    let Prepared { payload, .. } = new(entity).expect("saved");
    assert_eq!(payload["dexterity"], 15);
    let text = serde_json::to_string(&payload).expect("JSON");
    assert!(!text.to_lowercase().contains("modifier"), "{text}");
    assert!(!text.contains("DEX_MOD"), "{text}");
}

#[test]
fn aliases_are_kept_as_typed_without_a_blank_or_a_duplicate_by_alias_match() {
    let Prepared { payload, .. } = new(morlox()).expect("saved");
    assert_eq!(payload["alias"], json!(["le gobelin", "le marchand"]));
    assert_eq!(
        payload["snapshot"]["aliases"],
        json!(["le gobelin", "le marchand"])
    );
}

#[test]
fn the_first_version_is_1_with_the_whole_snapshot_in_its_diff() {
    let Prepared { payload, .. } = new(morlox()).expect("saved");
    assert_eq!(payload["versionNumber"], 1);
    let snapshot = payload["snapshot"].as_object().expect("a snapshot");
    for field in ["entityId", "entityCampaignId", "updatedAt", "source"] {
        assert!(!snapshot.contains_key(field), "{field} is no sheet field");
    }
    let changed: Vec<&str> = payload["diff"]
        .as_array()
        .expect("a diff")
        .iter()
        .map(|c| c["field"].as_str().expect("a field"))
        .collect();
    let mut every: Vec<&str> = snapshot.keys().map(String::as_str).collect();
    every.sort_unstable();
    assert_eq!(changed, every);
}

#[test]
fn a_second_save_numbers_after_the_base_and_diffs_what_moved_from_its_snapshot() {
    let first = new(morlox()).expect("saved");
    let base = after(&Row::new(), &first);
    let wire = row(json!({
        "campaign": VALOMBRE,
        "entity": { "id": MORLOX, "notes": "Doit 40 po à Vesna." },
    }));
    let Prepared { payload, .. } = prepare(&wire, &base, fresh("f2", LATER)).expect("saved");
    assert_eq!(payload["versionNumber"], 2);
    assert_eq!(
        payload["diff"],
        json!([{ "field": "notes", "old": null, "new": "Doit 40 po à Vesna." }])
    );
    // A save of an existing element keeps its creation, and its colour.
    assert!(!payload.contains_key("entityCreatedAt"));
    assert!(!payload.contains_key("tokenColor"));
    assert_eq!(payload["updatedAt"], LATER);
}

#[test]
fn a_pc_or_npc_made_a_place_loses_its_sheet() {
    let mut entity = morlox();
    entity["player"] = Value::Null;
    let base = after(&Row::new(), &new(entity).expect("saved"));
    let wire = row(json!({ "entity": { "id": MORLOX, "kind": "lieu" } }));
    let prepared = prepare(&wire, &base, fresh("f2", LATER)).expect("saved");
    let left = after(&base, &prepared);
    let payload = prepared.payload;
    for field in [
        "characterEntityId",
        "dexterity",
        "armorClass",
        "tokenColor",
        "spells",
    ] {
        assert_eq!(payload.get(field), Some(&Value::Null), "{field}");
        assert!(left[field].is_null(), "{field}");
    }
    assert!(
        payload["diff"]
            .as_array()
            .expect("a diff")
            .iter()
            .any(|c| c["field"] == "dexterity" && c["new"].is_null())
    );
}

#[test]
fn a_save_the_tables_could_never_hold_is_refused() {
    let refused =
        |wire: Value, base: &Row| prepare(&row(wire), base, fresh("f1", NOW)).expect_err("refused");
    let none = Row::new();
    assert_eq!(
        refused(
            json!({ "entity": { "kind": "pnj", "name": "Morlox" } }),
            &none
        ),
        Refusal::NoCampaign
    );
    assert_eq!(
        refused(
            json!({ "campaign": VALOMBRE, "entity": { "name": "Morlox" } }),
            &none
        ),
        Refusal::NoType
    );
    // A type given that is none of the four, even on an existing element.
    assert_eq!(
        refused(
            json!({ "campaign": VALOMBRE, "entity": { "kind": "PNJ", "name": "Morlox" } }),
            &none
        ),
        Refusal::NoType
    );
    assert_eq!(
        refused(
            json!({ "campaign": VALOMBRE, "entity": { "kind": "pnj" } }),
            &none
        ),
        Refusal::NoName
    );
    // A name of blanks alone is none: AliasMatch@1 normalises it to nothing.
    assert_eq!(
        refused(
            json!({ "campaign": VALOMBRE, "entity": { "kind": "pnj", "name": " \t " } }),
            &none
        ),
        Refusal::NoName
    );
    for (wire, field) in [
        (
            json!({ "campaign": "cendres-valombre", "entity": { "kind": "pnj", "name": "Morlox" } }),
            "entityCampaignId",
        ),
        (
            json!({ "campaign": VALOMBRE, "entity": { "kind": "pj", "name": "Elowen", "player": "jou-julie" } }),
            "playerId",
        ),
        (
            json!({ "campaign": VALOMBRE, "entity": { "kind": "pj", "name": "Elowen", "portrait": "portrait.png" } }),
            "portraitMediaId",
        ),
        // Hex digits alone are not a uuid: a uuid has 36 characters.
        (
            json!({ "campaign": VALOMBRE, "entity": { "kind": "pj", "name": "Elowen", "player": "b1000000" } }),
            "playerId",
        ),
    ] {
        assert_eq!(refused(wire, &none), Refusal::NotAnId(field), "{field}");
    }
    let base = after(&none, &new(morlox()).expect("saved"));
    assert_eq!(
        refused(
            json!({ "campaign": BRUME, "entity": { "id": MORLOX, "notes": "…" } }),
            &base
        ),
        Refusal::OtherCampaign
    );
}

#[test]
fn aliases_that_are_not_a_list_are_refused_never_read_as_none() {
    let base = after(&Row::new(), &new(morlox()).expect("saved"));
    for aliases in [
        Value::Null,
        json!("le gobelin"),
        json!({ "0": "le gobelin" }),
    ] {
        let wire = json!({ "campaign": VALOMBRE, "entity": { "id": MORLOX, "aliases": aliases } });
        assert_eq!(
            prepare(&row(wire), &base, fresh("f2", NOW)),
            Err(Refusal::NotAliases),
            "{aliases}"
        );
    }
    // Left out, they are kept as they were.
    let wire = json!({ "campaign": VALOMBRE, "entity": { "id": MORLOX, "notes": "…" } });
    let kept = prepare(&row(wire), &base, fresh("f2", NOW)).expect("saved");
    assert!(!kept.payload.contains_key("alias"));
    assert_eq!(
        after(&base, &kept)["alias"],
        json!(["le gobelin", "le marchand"])
    );
}

/// The entity Elowen is saved new with, `field` set to `value`.
fn elowen_with(field: &str, value: Value) -> Value {
    let mut entity = json!({ "kind": "pj", "name": "Elowen", "stats": {} });
    if field.len() == 3 {
        entity["stats"][field] = value;
    } else {
        entity[field] = value;
    }
    entity
}

#[test]
fn a_score_the_ac_or_the_hp_max_is_refused_unless_an_integer_a_smallint_holds() {
    for (given, field) in [
        ("FOR", "strength"),
        ("DEX", "dexterity"),
        ("CON", "constitution"),
        ("INT", "intelligence"),
        ("SAG", "wisdom"),
        ("CHA", "charisma"),
        ("ca", "armorClass"),
        ("pvMax", "hpMax"),
    ] {
        for value in [
            json!("15"),
            json!(true),
            json!([15]),
            json!(15.5),
            json!(32_768),
            json!(-32_769),
        ] {
            assert_eq!(
                new(elowen_with(given, value.clone())).expect_err("refused"),
                Refusal::NotASmallint(field),
                "{given}: {value}"
            );
        }
        // The bounds, an integer written with a fraction of zero, and none.
        for value in [json!(32_767), json!(-32_768), json!(15.0), Value::Null] {
            assert!(
                new(elowen_with(given, value.clone())).is_ok(),
                "{given}: {value}"
            );
        }
    }
}

#[test]
fn an_id_that_is_not_a_string_is_not_an_id() {
    for given in ["player", "playerId"] {
        assert_eq!(
            new(elowen_with(given, json!(5))).expect_err("refused"),
            Refusal::NotAnId("playerId"),
            "{given}"
        );
    }
    // A media is a string or nothing: anything else is never written.
    for (given, field) in [("portrait", "portraitMediaId"), ("token", "tokenMediaId")] {
        let Prepared { payload, .. } = new(elowen_with(given, json!(5))).expect("saved");
        assert!(!payload.contains_key(field), "{given}");
    }
    let refused = prepare(
        &row(json!({ "entity": { "notes": "…" } })),
        &row(json!({ "entityCampaignId": 7, "type": "pj", "name": "Elowen" })),
        fresh("f1", NOW),
    )
    .expect_err("refused");
    assert_eq!(refused, Refusal::NotAnId("entityCampaignId"));
}

#[test]
fn a_nul_character_jsonb_or_text_cannot_hold_is_refused_wherever_it_hides() {
    for (given, value, field) in [
        ("spells", json!([{ "name": "Soins\u{0}" }]), "spells"),
        ("actions", json!([{ "nom\u{0}": "Fuite" }]), "actions"),
        ("features", json!(["\u{0}"]), "actions"),
        ("crop", json!({ "x": "\u{0}" }), "tokenCrop"),
        ("name", json!("Elo\u{0}wen"), "name"),
        ("notes", json!("\u{0}"), "notes"),
        ("aliases", json!(["la rôdeuse\u{0}"]), "alias"),
        ("color", json!("accent\u{0}"), "tokenColor"),
    ] {
        assert_eq!(
            new(elowen_with(given, value)).expect_err("refused"),
            Refusal::Nul(field.to_owned()),
            "{given}"
        );
    }
}

#[test]
fn an_existing_element_needs_no_campaign_kind_or_name_and_a_player_by_uuid_is_kept() {
    let mut entity = morlox();
    entity["kind"] = json!("pj");
    let base = after(&Row::new(), &new(entity).expect("saved"));
    let wire = row(json!({ "entity": { "id": MORLOX, "player": CLAIRE } }));
    let Prepared { payload, .. } = prepare(&wire, &base, fresh("f2", LATER)).expect("saved");
    assert_eq!(payload["playerId"], CLAIRE);
    assert!(!payload.contains_key("entityCampaignId"));
}

#[test]
fn an_element_s_campaign_named_in_another_case_is_its_own() {
    let base = after(&Row::new(), &new(morlox()).expect("saved"));
    let wire = row(json!({
        "campaign": VALOMBRE.to_uppercase(),
        "entity": { "id": MORLOX, "notes": "…" },
    }));
    let Prepared { payload, .. } = prepare(&wire, &base, fresh("f2", LATER)).expect("saved");
    assert!(!payload.contains_key("entityCampaignId"));
}

#[test]
fn names_is_this_data_capability_with_or_without_its_version() {
    assert!(names("campaign.saveEntity"));
    assert!(names("campaign.saveEntity@1"));
    assert!(!names("campaign.saveEntityNow"));
    assert!(!names("campaign.restoreEntityVersion@1"));
}

#[test]
fn a_refusal_says_what_the_tables_could_never_hold() {
    let told: Vec<String> = [
        Refusal::NoCampaign,
        Refusal::NoType,
        Refusal::NoName,
        Refusal::OtherCampaign,
        Refusal::NotAnId("playerId"),
        Refusal::NotASmallint("hpMax"),
        Refusal::Nul("notes".to_owned()),
        Refusal::NotAliases,
    ]
    .iter()
    .map(ToString::to_string)
    .collect();
    assert_eq!(
        told,
        [
            "un nouvel élément nomme sa campagne",
            "un élément est un pj, un pnj, un lieu ou un evenement",
            "un nouvel élément a un nom",
            "un élément ne change pas de campagne",
            "playerId n'est pas un identifiant",
            "hpMax n'est pas un entier de -32768 à 32767",
            "notes contient un caractère nul",
            "les alias sont une liste",
        ]
    );
}

#[test]
fn the_element_a_save_names_is_its_entity_s_id_or_the_payload_s() {
    let named = |wire: Value| element_id(&row(wire));
    assert_eq!(
        named(json!({ "entity": { "id": MORLOX } })),
        Some(MORLOX.to_owned())
    );
    // Under `entity`, the payload's own id is not read.
    assert_eq!(
        named(json!({ "id": CLAIRE, "entity": { "name": "Morlox" } })),
        None
    );
    assert_eq!(named(json!({ "id": MORLOX })), Some(MORLOX.to_owned()));
    for blank in [json!({ "entity": { "id": "  " } }), json!({ "id": "" })] {
        assert_eq!(named(blank), None);
    }
    assert_eq!(named(json!({ "entity": { "id": 7 } })), None);
}

/// The fields the diff from `previous` to `snapshot` reports.
fn changed(previous: Value, snapshot: Value) -> Vec<Value> {
    let object = |value: Value| -> Map<String, Value> {
        let Value::Object(map) = value else {
            panic!("a snapshot is an object");
        };
        map
    };
    diff(&object(previous), &object(snapshot))
        .into_iter()
        .map(|change| change["field"].clone())
        .collect()
}

#[test]
fn a_diff_compares_numbers_by_value_inside_lists_and_objects_too() {
    assert!(
        changed(
            json!({ "spells": [15, { "level": 1 }], "tokenCrop": { "x": 1, "y": [2] } }),
            json!({ "spells": [15.0, { "level": 1.0 }], "tokenCrop": { "x": 1.0, "y": [2.0] } }),
        )
        .is_empty()
    );
    assert_eq!(
        changed(
            json!({ "spells": [15], "tokenCrop": { "x": 1 }, "actions": [] }),
            json!({ "spells": [15, 16], "tokenCrop": { "x": 1, "y": 2 }, "actions": [] }),
        ),
        [json!("spells"), json!("tokenCrop")]
    );
    assert_eq!(
        changed(
            json!({ "tokenCrop": { "x": 1 } }),
            json!({ "tokenCrop": { "y": 1 } })
        ),
        [json!("tokenCrop")]
    );
}

#[test]
fn every_field_a_save_writes_is_one_its_manifest_touches() {
    let manifest: Value =
        serde_json::from_str(include_str!("../data-capability.json")).expect("contract F");
    let touches: Vec<&str> = manifest["touches"]
        .as_array()
        .expect("touches")
        .iter()
        .map(|touch| touch.as_str().expect("a touch"))
        .collect();
    let mut entity = morlox();
    entity["player"] = Value::Null;
    entity["portrait"] = Value::Null;
    entity["token"] = Value::Null;
    entity["crop"] = Value::Null;
    entity["color"] = json!("danger");
    entity["description"] = json!("Un gobelin.");
    entity["notes"] = json!("Doit 40 po.");
    entity["background"] = json!("Né au marché.");
    entity["behavior"] = json!("Fuyant.");
    entity["spells"] = json!([]);
    let mut base = Row::new();
    let created = new(entity).expect("saved");
    base.extend(created.payload.clone());
    // dndbeyondId and dndbeyondSyncedAt are nulled with the sheet.
    base.insert("dndbeyondId".into(), json!("db-1"));
    base.insert("dndbeyondSyncedAt".into(), json!(NOW));
    let wire = row(json!({ "entity": { "id": MORLOX, "kind": "lieu" } }));
    let placed = prepare(&wire, &base, fresh("f2", LATER)).expect("saved");
    for payload in [created.payload, placed.payload] {
        for field in payload.keys() {
            assert!(
                touches.contains(&format!("Entity.{field}").as_str()),
                "{field} is written but not touched"
            );
        }
    }
}
