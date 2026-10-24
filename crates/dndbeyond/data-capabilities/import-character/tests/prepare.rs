//! What an import writes, from the row as the queue leaves it and the sheet
//! D&D Beyond states: a new PC whole, a PC already there by its statblock and
//! its link alone (`dndbeyond-resync-scope`), each with the version it adds
//! (EntityVersion@1, source `dndbeyond`). A sheet an import cannot write is
//! refused.

use import_character::{DEFAULT_TOKEN_COLOR, Import, Refusal, Row, derived, prepare};
use ports_character_sheets::Entry;
use serde_json::{Value, json};

mod support;
use support::{BRANNOC, ELOWEN, elowen, id, sheet};

const VALOMBRE: &str = "c1000000-0000-4000-8000-000000000001";
const JULIE: &str = "a0000000-0000-4000-8000-000000000001";
const PC: &str = "e1000000-0000-4000-8000-000000000001";
const NOW: &str = "2026-10-24T10:00:00.000Z";

fn import<'a>(campaign: Option<&'a str>, player: Option<&'a str>) -> Import<'a> {
    Import {
        entity: PC,
        campaign,
        player,
        version: "f1000000-0000-4000-8000-000000000002",
        now: NOW,
    }
}

/// Elowen as a first import left her, with what the DM gave her since.
fn imported() -> Row {
    let created =
        prepare(&Row::new(), &elowen(), import(Some(VALOMBRE), Some(JULIE))).expect("imported");
    let mut base = created;
    base.insert("alias".into(), json!(["la rôdeuse"]));
    base.insert("notes".into(), json!("Doit une faveur au gué."));
    base.insert("tokenColor".into(), json!("success"));
    base.insert("snapshot".into(), {
        let mut snapshot = base["snapshot"].clone();
        snapshot["aliases"] = json!(["la rôdeuse"]);
        snapshot["notes"] = json!("Doit une faveur au gué.");
        snapshot["tokenColor"] = json!("success");
        snapshot
    });
    base
}

#[test]
fn a_new_pc_is_written_whole_as_a_pj_of_its_player_with_a_token_colour_and_version_1() {
    let payload =
        prepare(&Row::new(), &elowen(), import(Some(VALOMBRE), Some(JULIE))).expect("imported");
    for (field, value) in [
        ("entityId", json!(PC)),
        ("characterEntityId", json!(PC)),
        ("entityVersionEntityId", json!(PC)),
        ("entityCampaignId", json!(VALOMBRE)),
        ("type", json!("pj")),
        ("name", json!("Elowen Ventargent")),
        ("playerId", json!(JULIE)),
        ("strength", json!(12)),
        ("dexterity", json!(18)),
        ("constitution", json!(14)),
        ("intelligence", json!(10)),
        ("wisdom", json!(15)),
        ("charisma", json!(9)),
        ("armorClass", json!(16)),
        ("hpMax", json!(44)),
        ("tokenColor", json!(DEFAULT_TOKEN_COLOR)),
        ("dndbeyondId", json!(ELOWEN.to_string())),
        ("dndbeyondSyncedAt", json!(NOW)),
        ("entityCreatedAt", json!(NOW)),
        ("updatedAt", json!(NOW)),
        ("entityVersionCreatedAt", json!(NOW)),
        ("source", json!("dndbeyond")),
        ("restoredFromVersionId", Value::Null),
        ("versionNumber", json!(1)),
    ] {
        assert_eq!(payload.get(field), Some(&value), "{field}");
    }
    assert_eq!(
        payload["actions"],
        json!([{ "name": "Attaque supplémentaire",
                 "description": "Deux attaques quand elle utilise l’action Attaquer." }])
    );
    assert_eq!(payload["spells"][0]["name"], json!("Marque du chasseur"));
    // The snapshot is the PC whole, the diff every field it gained.
    let snapshot = payload["snapshot"].as_object().expect("a snapshot");
    assert_eq!(snapshot["name"], json!("Elowen Ventargent"));
    assert_eq!(snapshot["type"], json!("pj"));
    assert_eq!(snapshot["dndbeyondId"], json!(ELOWEN.to_string()));
    let diff = payload["diff"].as_array().expect("a diff");
    assert_eq!(diff.len(), snapshot.len());
    assert!(diff.iter().all(|change| change["old"].is_null()));
    // A modifier is never written (AbilityModifier@1), nor any alias.
    assert!(!payload.contains_key("dexterityModifier"));
    assert!(!payload.contains_key("alias"));
}

#[test]
fn a_new_pc_with_no_player_has_none() {
    let payload = prepare(&Row::new(), &elowen(), import(Some(VALOMBRE), None)).expect("imported");
    assert!(!payload.contains_key("playerId"));
}

#[test]
fn a_pc_already_there_takes_the_statblock_link_and_sync_date_alone_versioned_after_its_last() {
    let base = imported();
    let mut levelled = sheet(ELOWEN, "Elowen, renommée sur D&D Beyond", 52);
    levelled.statblock.wisdom = 16;
    levelled.statblock.spells.push(Entry::new(
        "Communication avec les animaux",
        "Sort de niveau 1.",
    ));
    let later = Import {
        version: "f1000000-0000-4000-8000-000000000003",
        now: "2026-10-25T10:00:00.000Z",
        // A campaign and a player named again change nothing of a PC that exists.
        ..import(Some(VALOMBRE), Some("a0000000-0000-4000-8000-000000000009"))
    };
    let payload = prepare(&base, &levelled, later).expect("imported again");
    for field in [
        "name",
        "type",
        "entityCampaignId",
        "playerId",
        "tokenColor",
        "alias",
        "notes",
        "entityCreatedAt",
    ] {
        assert!(!payload.contains_key(field), "{field} written again");
    }
    assert_eq!(payload["hpMax"], json!(52));
    assert_eq!(payload["wisdom"], json!(16));
    assert_eq!(payload["versionNumber"], json!(2));
    assert_eq!(payload["source"], json!("dndbeyond"));
    assert_eq!(
        payload["dndbeyondSyncedAt"],
        json!("2026-10-25T10:00:00.000Z")
    );
    // The snapshot keeps what the DM gave; the diff names what D&D Beyond changed.
    let snapshot = &payload["snapshot"];
    assert_eq!(snapshot["name"], json!("Elowen Ventargent"));
    assert_eq!(snapshot["aliases"], json!(["la rôdeuse"]));
    assert_eq!(snapshot["tokenColor"], json!("success"));
    let changed: Vec<&str> = payload["diff"]
        .as_array()
        .expect("a diff")
        .iter()
        .map(|change| change["field"].as_str().expect("a field"))
        .collect();
    assert_eq!(changed, ["dndbeyondSyncedAt", "hpMax", "spells", "wisdom"]);
}

#[test]
fn a_pc_restored_before_is_imported_with_no_restoration_left_on_its_version() {
    let mut base = imported();
    base.insert("source".into(), json!("restauration"));
    base.insert(
        "restoredFromVersionId".into(),
        json!("f1000000-0000-4000-8000-000000000001"),
    );
    let payload = prepare(&base, &elowen(), import(None, None)).expect("imported again");
    assert_eq!(payload["restoredFromVersionId"], Value::Null);
    assert_eq!(payload["source"], json!("dndbeyond"));
}

#[test]
fn a_new_pc_of_no_campaign_or_with_no_name_is_refused() {
    assert_eq!(
        prepare(&Row::new(), &elowen(), import(None, None)),
        Err(Refusal::NoCampaign)
    );
    assert_eq!(
        prepare(
            &Row::new(),
            &sheet(ELOWEN, " \t", 44),
            import(Some(VALOMBRE), None)
        ),
        Err(Refusal::NoName)
    );
    // A name D&D Beyond leaves blank is no matter to a PC that has one.
    assert!(prepare(&imported(), &sheet(ELOWEN, "", 44), import(None, None)).is_ok());
}

#[test]
fn a_statblock_stat_block_does_not_hold_complete_is_refused_naming_its_fields() {
    let mut empty = elowen();
    empty.statblock.actions.clear();
    empty.statblock.hp_max = 0;
    empty.statblock.charisma = 31;
    assert_eq!(
        prepare(&Row::new(), &empty, import(Some(VALOMBRE), None)),
        Err(Refusal::Incomplete(vec!["charisma", "hpMax", "actions"]))
    );
    let mut blank = elowen();
    blank
        .statblock
        .spells
        .push(Entry::new("Sans description", " "));
    assert_eq!(
        prepare(&imported(), &blank, import(None, None)),
        Err(Refusal::Incomplete(vec!["spells"]))
    );
}

#[test]
fn what_the_tables_could_never_hold_is_refused() {
    let mut huge = elowen();
    huge.statblock.hp_max = 40_000;
    assert_eq!(
        prepare(&Row::new(), &huge, import(Some(VALOMBRE), None)),
        Err(Refusal::Unwritable("hpMax"))
    );
    let mut armoured = elowen();
    armoured.statblock.armor_class = 32_768;
    assert_eq!(
        prepare(&Row::new(), &armoured, import(Some(VALOMBRE), None)),
        Err(Refusal::Unwritable("armorClass"))
    );
    let mut nul = elowen();
    nul.statblock.actions[0].description = "a\0b".into();
    assert_eq!(
        prepare(&Row::new(), &nul, import(Some(VALOMBRE), None)),
        Err(Refusal::Unwritable("actions ou spells"))
    );
    assert_eq!(
        prepare(
            &Row::new(),
            &sheet(ELOWEN, "E\0", 44),
            import(Some(VALOMBRE), None)
        ),
        Err(Refusal::Unwritable("name"))
    );
    // The largest a `smallint` holds is written.
    let mut largest = elowen();
    largest.statblock.hp_max = 32_767;
    assert!(prepare(&Row::new(), &largest, import(Some(VALOMBRE), None)).is_ok());
}

#[test]
fn a_new_pc_s_id_is_its_campaign_s_and_character_s_whatever_the_campaign_s_case() {
    let elowen = derived(VALOMBRE, id(ELOWEN));
    assert_eq!(elowen, derived(&VALOMBRE.to_uppercase(), id(ELOWEN)));
    assert_eq!(elowen.len(), 36);
    assert_eq!(elowen, elowen.to_lowercase());
    assert_ne!(elowen, derived(VALOMBRE, id(BRANNOC)));
    assert_ne!(
        elowen,
        derived("c1000000-0000-4000-8000-000000000002", id(ELOWEN))
    );
}

#[test]
fn the_refusals_speak_to_the_dm() {
    let said = [
        Refusal::NoCampaign,
        Refusal::UnknownCampaign,
        Refusal::NoElement,
        Refusal::NotAPc,
        Refusal::OtherCampaign,
        Refusal::LinkedElsewhere,
        Refusal::AlreadyImported("Elowen".into()),
        Refusal::NotAnId("player"),
        Refusal::OtherPlayer,
        Refusal::NoName,
        Refusal::Incomplete(vec!["hpMax", "actions"]),
        Refusal::Unwritable("hpMax"),
    ]
    .map(|refusal| refusal.to_string());
    assert!(said[6].contains("« Elowen »"));
    assert!(said[7].contains("player"));
    assert!(said[10].contains("hpMax, actions"));
    assert!(said[11].contains("hpMax"));
    let distinct: std::collections::HashSet<_> = said.iter().collect();
    assert_eq!(distinct.len(), said.len());
    assert_eq!(
        Refusal::Sheets(ports_character_sheets::CharacterSheetsError::NotPublic).to_string(),
        ports_character_sheets::CharacterSheetsError::NotPublic.to_string()
    );
}
