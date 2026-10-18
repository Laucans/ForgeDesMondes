//! `prepare`: the editor's payload made the command written on
//! `Entity/<id>` — or refused before it is queued.

use save_entity::{DEFAULT_TOKEN_COLOR, Fresh, Prepared, Refusal, Row, names, prepare};
use serde_json::{Value, json};

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
fn names_is_this_data_capability_with_or_without_its_version() {
    assert!(names("campaign.saveEntity"));
    assert!(names("campaign.saveEntity@1"));
    assert!(!names("campaign.saveEntityNow"));
    assert!(!names("campaign.restoreEntityVersion@1"));
}
