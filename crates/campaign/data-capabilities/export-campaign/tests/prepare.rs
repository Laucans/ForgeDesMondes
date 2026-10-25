//! campaign.exportCampaign's preparation: what the tables hold of a campaign
//! made one archive, and the stamp — the one field the command writes.

use export_campaign::{
    FORMAT, FORMAT_VERSION, Refusal, Row, campaign_id, file_name, names, prepare,
};
use serde_json::{Value, json};

const VALOMBRE: &str = "c1000000-0000-4000-8000-000000000001";
const NOW: &str = "2026-10-18T09:30:00.123Z";

/// Valombre as the tables hold it: Claire, Elowen her PC, a place.
fn tables() -> Value {
    json!({
        "campaign": {
            "id": VALOMBRE,
            "name": "Les Cendres de Valombre",
            "kind": "campagne",
            "last_exported_at": null,
        },
        "players": [{ "id": "b1000000-0000-4000-8000-000000000001", "name": "Claire" }],
        "elements": [
            {
                "id": "e1",
                "type": "pj",
                "character": { "player_id": "b1000000-0000-4000-8000-000000000001", "hp_max": 44 },
                "aliases": ["Elowen"],
                "versions": [{ "version_number": 1, "snapshot": {}, "diff": [] }],
            },
            { "id": "e2", "type": "lieu", "character": null, "aliases": [], "versions": [] },
        ],
        "media": [{
            "id": "d1",
            "kind": "battlemap",
            "battlemap_grid": { "media_id": "d1", "hex_size_px": 48, "feet_per_hex": 5 },
        }],
        "sounds": [{ "id": "a1", "name": "Pluie", "tags": ["météo"], "volume": 0.5 }],
        "sessions": [{
            "id": "5e1",
            "session_present_character": [{ "session_id": "5e1", "character_id": "e1" }],
            "scenes": [{
                "id": "5c1",
                "scene_entity": [{ "scene_id": "5c1", "entity_id": "e2", "position": 0 }],
                "scene_media": [{ "scene_id": "5c1", "media_id": "d1" }],
            }],
            "journal_lines": [{ "id": "1a", "session_id": "5e1", "scene_id": "5c1", "text": "Elle entre." }],
        }],
    })
}

fn wire(payload: Value) -> Row {
    let Value::Object(payload) = payload else {
        panic!("a payload is an object");
    };
    payload.into_iter().collect()
}

#[test]
fn the_payload_stamps_the_export_s_time_and_writes_nothing_else() {
    let prepared = prepare(&tables(), NOW).expect("prepared");
    assert_eq!(prepared.target_id, VALOMBRE);
    assert_eq!(prepared.payload, wire(json!({ "lastExportedAt": NOW })));
}

#[test]
fn the_archive_holds_every_section_of_the_campaign_as_the_tables_do() {
    let archive = prepare(&tables(), NOW).expect("prepared").archive;
    let tables = tables();
    assert_eq!(
        archive.content,
        json!({
            "format": FORMAT,
            "version": FORMAT_VERSION,
            "origin_campaign_id": VALOMBRE,
            "exported_at": NOW,
            "campaign": tables["campaign"],
            "players": tables["players"],
            "elements": tables["elements"],
            "media": tables["media"],
            "sounds": tables["sounds"],
            "sessions": tables["sessions"],
        })
    );
    assert_eq!(
        archive.file,
        "les-cendres-de-valombre-2026-10-18T09-30-00-123Z.json"
    );
}

#[test]
fn a_campaign_named_in_capitals_is_its_one_partition() {
    let mut tables = tables();
    tables["campaign"]["id"] = json!(VALOMBRE.to_uppercase());
    let prepared = prepare(&tables, NOW).expect("prepared");
    assert_eq!(prepared.target_id, VALOMBRE);
    assert_eq!(prepared.archive.content["origin_campaign_id"], VALOMBRE);
}

#[test]
fn no_campaign_or_rows_that_are_no_list_are_refused() {
    let mut no_id = tables();
    no_id["campaign"]["id"] = json!("  ");
    let mut no_players = tables();
    no_players["players"] = Value::Null;
    let mut no_elements = tables();
    no_elements["elements"] = json!({});
    // The sections an archive gained with media, sounds and sessions.
    let [no_media, no_sounds, no_sessions] = ["media", "sounds", "sessions"].map(|section| {
        let mut broken = tables();
        broken[section] = json!("none");
        broken
    });
    for (tables, refusal) in [
        (json!({}), Refusal::NoCampaign),
        (
            json!({ "campaign": null, "players": [], "elements": [] }),
            Refusal::NoCampaign,
        ),
        (no_id, Refusal::NoCampaign),
        (no_players, Refusal::Unreadable("players")),
        (no_elements, Refusal::Unreadable("elements")),
        (no_media, Refusal::Unreadable("media")),
        (no_sounds, Refusal::Unreadable("sounds")),
        (no_sessions, Refusal::Unreadable("sessions")),
    ] {
        assert_eq!(prepare(&tables, NOW), Err(refusal.clone()), "{tables}");
    }
    let mut missing = tables();
    missing.as_object_mut().expect("tables").remove("sounds");
    assert_eq!(
        prepare(&missing, NOW),
        Err(Refusal::Unreadable("sounds")),
        "an absent list is not an empty one"
    );
    assert_eq!(
        Refusal::NoCampaign.to_string(),
        "cette campagne n'existe pas"
    );
    assert_eq!(
        Refusal::Unreadable("players").to_string(),
        "l'archive n'a pas pu être construite : players illisibles"
    );
}

#[test]
fn a_file_is_named_after_its_campaign_and_the_time_to_the_millisecond() {
    for (name, now, file) in [
        (
            "La Brume",
            "2026-10-18T09:30:00Z",
            "la-brume-2026-10-18T09-30-00Z.json",
        ),
        (
            "  L'Œil d'Ambre — Acte II !  ",
            NOW,
            "l-œil-d-ambre-acte-ii-2026-10-18T09-30-00-123Z.json",
        ),
        ("", NOW, "campagne-2026-10-18T09-30-00-123Z.json"),
        ("--- ", NOW, "campagne-2026-10-18T09-30-00-123Z.json"),
    ] {
        assert_eq!(file_name(name, now), file, "{name}");
    }
}

#[test]
fn the_campaign_a_payload_names_is_its_id_campaign_id_or_campaign() {
    for payload in [
        json!({ "id": VALOMBRE }),
        json!({ "campaignId": VALOMBRE }),
        json!({ "campaign": format!(" {VALOMBRE} ") }),
    ] {
        assert_eq!(campaign_id(&wire(payload)), Some(VALOMBRE.to_owned()));
    }
    for payload in [json!({}), json!({ "id": "" }), json!({ "id": 7 })] {
        assert_eq!(campaign_id(&wire(payload.clone())), None, "{payload}");
    }
}

#[test]
fn only_this_data_capability_is_named_with_or_without_its_version() {
    assert!(names("campaign.exportCampaign"));
    assert!(names("campaign.exportCampaign@1"));
    assert!(!names("campaign.importCampaign@1"));
    assert!(!names("campaign.exportCampaigns"));
}

#[test]
fn a_campaign_with_no_media_sounds_or_sessions_exports_them_as_empty_lists() {
    let mut empty = tables();
    for section in ["media", "sounds", "sessions"] {
        empty[section] = json!([]);
    }
    let content = prepare(&empty, NOW).expect("prepared").archive.content;
    for section in ["media", "sounds", "sessions"] {
        assert_eq!(content[section], json!([]), "{section}");
    }
}

#[test]
fn the_tables_of_the_concept_s_archives_come_out_as_the_archives_they_are() {
    let fixtures = platform_conformance::fixtures_of("CampaignArchive@1");
    let mut full = 0;
    for case in &fixtures.cases {
        let archive = &case.input["archive"];
        let mut tables = archive.clone();
        let content = tables.as_object_mut().expect("an archive");
        for header in ["format", "version", "origin_campaign_id", "exported_at"] {
            content.remove(header);
        }
        // An archive made before media, sounds and sessions joined it.
        let mut expected = archive.clone();
        let mut whole = true;
        for section in ["media", "sounds", "sessions"] {
            if archive.get(section).is_none() {
                tables[section] = json!([]);
                expected[section] = json!([]);
                whole = false;
            }
        }
        full += usize::from(whole);
        let now = archive["exported_at"].as_str().expect("an export time");
        let exported = prepare(&tables, now).expect(&case.name);
        assert_eq!(exported.archive.content, expected, "{}", case.name);
        assert_eq!(exported.payload, wire(json!({ "lastExportedAt": now })));
    }
    assert!(
        full >= 3,
        "the fixtures hold archives with all their sections"
    );
}
