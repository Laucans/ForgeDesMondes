//! What a resync writes, made from the PC's row and the sheet D&D Beyond
//! gives, before the engine sees it: the statblock and the sync date alone,
//! with the version it adds after the row's — nothing the DM set locally.

use std::collections::BTreeMap;

use ports_character_sheets::{CharacterSheetsError, Entry};
use resync_character::{Refusal, Resync, Row, linked, prepare, statblock_of};
use serde_json::{Value, json};

mod support;
use support::{ELOWEN, actions, elowen, id, levelled_up, spells};

const ELOWEN_PC: &str = "e1000000-0000-4000-8000-000000000001";
const LONG_AGO: &str = "2026-09-26T18:12:00Z";
const NOW: &str = "2026-10-24T09:00:00.000Z";

fn fresh() -> Resync<'static> {
    Resync {
        entity: ELOWEN_PC,
        version: "f1000000-0000-4000-8000-000000000002",
        now: NOW,
    }
}

fn row(fields: Value) -> Row {
    serde_json::from_value(fields).expect("a row")
}

/// Elowen's row as an import left it at version 1: her statblock, her link,
/// and what the DM set locally — aliases, notes, token and portrait.
fn elowen_row() -> Row {
    let sheet = json!({
        "type": "pj", "name": "Elowen Ventargent", "notes": "Cherche son frère Aerin.",
        "strength": 12, "dexterity": 18, "constitution": 14, "intelligence": 10,
        "wisdom": 15, "charisma": 9, "armorClass": 16, "hpMax": 44,
        "actions": actions(), "spells": spells(),
        "portraitMediaId": "m1000000-0000-4000-8000-000000000001",
        "tokenCrop": { "zoom": 140, "x": 50, "y": 32 }, "tokenColor": "success",
        "dndbeyondId": ELOWEN.to_string(), "dndbeyondSyncedAt": LONG_AGO,
    });
    let mut snapshot = sheet.clone();
    snapshot["aliases"] = json!(["Elowen", "la rôdeuse"]);
    let mut row = row(sheet);
    for (field, value) in [
        ("entityId", json!(ELOWEN_PC)),
        (
            "entityCampaignId",
            json!("c1000000-0000-4000-8000-000000000001"),
        ),
        ("entityCreatedAt", json!(LONG_AGO)),
        ("updatedAt", json!(LONG_AGO)),
        ("alias", json!(["Elowen", "la rôdeuse"])),
        ("characterEntityId", json!(ELOWEN_PC)),
        (
            "entityVersionId",
            json!("f1000000-0000-4000-8000-000000000001"),
        ),
        ("versionNumber", json!(1)),
        ("snapshot", snapshot),
    ] {
        row.insert(field.into(), value);
    }
    row
}

/// The fields a version's diff names.
fn fields(payload: &Row) -> Vec<&str> {
    payload["diff"]
        .as_array()
        .expect("a diff")
        .iter()
        .map(|change| change["field"].as_str().expect("a field"))
        .collect()
}

#[test]
fn a_resync_after_a_level_up_writes_the_statblock_and_the_sync_date_with_the_next_version() {
    let payload = prepare(&elowen_row(), &levelled_up(), fresh()).expect("prepared");
    assert_eq!(payload["hpMax"], json!(52));
    assert_eq!(payload["wisdom"], json!(16));
    let spells: Vec<&str> = payload["spells"]
        .as_array()
        .expect("spells")
        .iter()
        .map(|spell| spell["name"].as_str().expect("a name"))
        .collect();
    assert_eq!(
        spells,
        [
            "Marque du chasseur",
            "Explorateur né amélioré",
            "Communication avec les animaux"
        ]
    );
    assert_eq!(payload["dndbeyondSyncedAt"], json!(NOW));
    assert_eq!(payload["updatedAt"], json!(NOW));
    // The version: after the row's, of source dndbeyond, of this element,
    // restoring nothing, its diff the level-up's alone.
    assert_eq!(payload["versionNumber"], json!(2));
    assert_eq!(payload["source"], json!("dndbeyond"));
    assert_eq!(payload["entityVersionId"], json!(fresh().version));
    assert_eq!(payload["entityVersionEntityId"], json!(ELOWEN_PC));
    assert_eq!(payload["entityVersionCreatedAt"], json!(NOW));
    assert_eq!(payload["restoredFromVersionId"], Value::Null);
    assert_eq!(
        fields(&payload),
        ["dndbeyondSyncedAt", "hpMax", "spells", "wisdom"]
    );
    // The snapshot keeps what the DM set locally as it was.
    let snapshot = &payload["snapshot"];
    assert_eq!(snapshot["aliases"], json!(["Elowen", "la rôdeuse"]));
    assert_eq!(snapshot["notes"], json!("Cherche son frère Aerin."));
    assert_eq!(snapshot["tokenColor"], json!("success"));
    assert_eq!(snapshot["hpMax"], json!(52));
}

#[test]
fn a_resync_writes_nothing_the_dm_set_locally_nor_the_link() {
    let payload = prepare(&elowen_row(), &levelled_up(), fresh()).expect("prepared");
    let mut written: Vec<&str> = payload.keys().map(String::as_str).collect();
    written.sort_unstable();
    assert_eq!(
        written,
        [
            "actions",
            "armorClass",
            "charisma",
            "constitution",
            "dexterity",
            "diff",
            "dndbeyondSyncedAt",
            "entityVersionCreatedAt",
            "entityVersionEntityId",
            "entityVersionId",
            "hpMax",
            "intelligence",
            "restoredFromVersionId",
            "snapshot",
            "source",
            "spells",
            "strength",
            "updatedAt",
            "versionNumber",
            "wisdom",
        ]
    );
}

#[test]
fn every_field_a_resync_writes_is_one_its_manifest_touches() {
    let manifest: Value = serde_json::from_str(resync_character::MANIFEST).expect("contract F");
    let touches: Vec<&str> = manifest["touches"]
        .as_array()
        .expect("touches")
        .iter()
        .map(|touch| {
            touch
                .as_str()
                .expect("a field")
                .trim_start_matches("Entity.")
        })
        .collect();
    let payload = prepare(&elowen_row(), &levelled_up(), fresh()).expect("prepared");
    for field in payload.keys() {
        assert!(touches.contains(&field.as_str()), "{field} is not touched");
    }
}

#[test]
fn a_resync_with_nothing_new_on_d_and_d_beyond_versions_the_sync_date_alone() {
    let payload = prepare(&elowen_row(), &elowen(), fresh()).expect("prepared");
    assert_eq!(fields(&payload), ["dndbeyondSyncedAt"]);
}

#[test]
fn a_resync_of_an_element_with_no_version_yet_is_its_first() {
    let mut base = elowen_row();
    for field in ["entityVersionId", "versionNumber", "snapshot"] {
        base.remove(field);
    }
    let payload = prepare(&base, &levelled_up(), fresh()).expect("prepared");
    assert_eq!(payload["versionNumber"], json!(1));
}

#[test]
fn the_link_is_the_id_an_import_wrote_or_a_url_and_an_element_linked_to_nothing_is_refused() {
    let mut base = elowen_row();
    assert_eq!(linked(&base), Ok(id(ELOWEN)));
    base.insert(
        "dndbeyondId".into(),
        json!(" https://www.dndbeyond.com/characters/112847390 "),
    );
    assert_eq!(linked(&base), Ok(id(ELOWEN)));
    for unlinked in [Value::Null, json!(""), json!("   "), json!(112_847_390)] {
        base.insert("dndbeyondId".into(), unlinked);
        assert_eq!(linked(&base), Err(Refusal::NotLinked));
    }
    base.remove("dndbeyondId");
    assert_eq!(linked(&base), Err(Refusal::NotLinked));
    base.insert("dndbeyondId".into(), json!("https://ddb.ac/characters/1"));
    assert!(matches!(
        linked(&base),
        Err(Refusal::Sheets(CharacterSheetsError::ForeignHost(_)))
    ));
    assert_eq!(linked(&BTreeMap::new()), Err(Refusal::NoElement));
}

#[test]
fn a_pc_linked_to_nothing_is_refused_with_a_reason_the_dm_reads() {
    let mut base = elowen_row();
    base.remove("dndbeyondId");
    let refusal = prepare(&base, &levelled_up(), fresh()).expect_err("refused");
    assert_eq!(refusal, Refusal::NotLinked);
    assert_eq!(
        refusal.to_string(),
        "ce personnage n'est pas lié à D&D Beyond : importez-le d'abord par son adresse"
    );
}

#[test]
fn a_sheet_of_another_character_than_the_linked_one_is_refused() {
    let mut other = levelled_up();
    other.id = id(112_850_214);
    let refusal = prepare(&elowen_row(), &other, fresh()).expect_err("refused");
    assert_eq!(
        refusal,
        Refusal::Sheets(CharacterSheetsError::ServiceChanged(
            "le personnage 112850214 répondu pour 112847390".into()
        ))
    );
}

#[test]
fn an_incomplete_statblock_is_refused_naming_its_unsound_fields() {
    let mut sheet = levelled_up();
    sheet.statblock.wisdom = 0;
    sheet.statblock.actions.clear();
    sheet
        .statblock
        .spells
        .push(Entry::new("Sans description", " "));
    let refusal = prepare(&elowen_row(), &sheet, fresh()).expect_err("refused");
    assert_eq!(
        refusal,
        Refusal::Incomplete(vec!["wisdom", "actions", "spells"])
    );
    assert_eq!(
        refusal.to_string(),
        "la fiche D&D Beyond n'a pas de statblock complet : wisdom, actions, spells"
    );
}

#[test]
fn a_value_the_tables_cannot_hold_is_refused() {
    let mut sheet = levelled_up();
    sheet.statblock.hp_max = 32_768;
    assert_eq!(statblock_of(&sheet), Err(Refusal::Unwritable("hpMax")));
    sheet.statblock.hp_max = 32_767;
    assert!(statblock_of(&sheet).is_ok());
    sheet.statblock.armor_class = 40_000;
    assert_eq!(statblock_of(&sheet), Err(Refusal::Unwritable("armorClass")));
    let mut sheet = levelled_up();
    sheet.statblock.spells[0].description.push('\0');
    assert_eq!(
        statblock_of(&sheet),
        Err(Refusal::Unwritable("actions ou spells"))
    );
    let mut sheet = levelled_up();
    sheet.statblock.actions[0].name.push('\0');
    assert_eq!(
        statblock_of(&sheet),
        Err(Refusal::Unwritable("actions ou spells"))
    );
}

#[test]
fn the_version_belongs_to_the_pc_resynchronised_whatever_its_row_names() {
    let mut base = elowen_row();
    base.remove("entityId");
    let payload = prepare(&base, &levelled_up(), fresh()).expect("prepared");
    assert_eq!(payload["entityVersionEntityId"], json!(ELOWEN_PC));
}
