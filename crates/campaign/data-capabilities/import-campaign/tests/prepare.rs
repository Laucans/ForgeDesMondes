//! `campaign.importCampaign`'s pure core, no engine: [`request`] reads and
//! checks the archive, [`copy`] and [`prepare`] make the copy — held to the
//! fixtures of `CampaignArchive@1` — and a refusal says why in words.

use std::collections::BTreeSet;

use serde_json::{Map, Value, json};

use import_campaign::{Refusal, Request, Row, copy, prepare, request, timestamp};
use platform_test_kit::{uuid, writes_only_touches};

const ID: &str = "CampaignArchive@1";
const AT: &str = "2026-10-25T12:00:00.000Z";

/// A row, from a JSON object.
fn row(value: Value) -> Row {
    serde_json::from_value(value).expect("an object")
}

/// The input of the fixtures' case whose name starts with `name`.
fn input(name: &str) -> Value {
    platform_conformance::fixtures_of(ID)
        .cases
        .into_iter()
        .find(|case| case.name.starts_with(name))
        .unwrap_or_else(|| panic!("no case starting with {name:?}"))
        .input
}

/// The archive of the fixtures' full case: players, a PJ and its sheet,
/// versions, a battlemap, a sound, a session with a scene and its journal.
fn full() -> Value {
    input("a full archive")["archive"].clone()
}

/// The command for `archive`.
fn wire(archive: Value) -> Row {
    row(json!({ "archive": archive }))
}

/// The request for `archive`, which must be well formed.
fn read(archive: Value) -> Request {
    request(&wire(archive), None).expect("a well formed archive")
}

/// `archive` with `edit` applied.
fn edited(mut archive: Value, edit: impl FnOnce(&mut Value)) -> Value {
    edit(&mut archive);
    archive
}

/// Ids as the engine draws them: uuids, each one new.
fn drawn() -> impl FnMut() -> String {
    let mut n = 0;
    move || {
        n += 1;
        uuid(0xd0, n)
    }
}

#[test]
fn conformance_the_copy_follows_the_fixtures_of_the_concept() {
    platform_conformance::check(ID, |input| {
        copy(
            &input["archive"],
            input["imported_at"].as_str().expect("a time"),
            input["new_ids"].as_object().expect("new ids"),
        )
    });
}

/// `prepare` on the fixtures' own new ids: the row the engine holds is the
/// Concept's copy, its campaign under the engine's field names.
#[test]
fn import_creates_copy_the_row_written_is_the_concepts_copy() {
    for name in [
        "a full archive",
        "an archive exported before",
        "a reference to",
    ] {
        let case = input(name);
        let request = read(case["archive"].clone());
        let mut new_ids: Vec<_> = case["new_ids"]
            .as_object()
            .expect("new ids")
            .iter()
            .filter(|(old, _)| request.ids.contains(*old))
            .map(|(old, new)| (old.clone(), new.as_str().expect("an id").to_owned()))
            .collect();
        new_ids.sort();
        let mut news = new_ids.into_iter().map(|(_, new)| new);
        let prepared = prepare(
            &request,
            case["imported_at"].as_str().expect("a time"),
            || news.next().expect("an id for every id of the archive"),
        )
        .expect("prepared");
        let expected = copy(
            &case["archive"],
            case["imported_at"].as_str().expect("a time"),
            case["new_ids"].as_object().expect("new ids"),
        );
        for section in ["players", "elements", "media", "sounds", "sessions"] {
            assert_eq!(
                prepared.payload[section], expected[section],
                "{name}: {section}"
            );
        }
        let campaign = &expected["campaign"];
        assert_eq!(prepared.target_id, campaign["id"].as_str().expect("an id"));
        for (field, column) in [
            ("id", "id"),
            ("name", "name"),
            ("kind", "kind"),
            ("pitch", "pitch"),
            ("art", "art"),
            ("glyph", "glyph"),
            ("coverMediaId", "cover_media_id"),
            ("createdAt", "created_at"),
            ("importedAt", "imported_at"),
            ("originCampaignId", "origin_campaign_id"),
            ("soundMasterVolume", "sound_master_volume"),
        ] {
            assert_eq!(prepared.payload[field], campaign[column], "{name}: {field}");
        }
        assert_eq!(
            prepared.payload.len(),
            11 + 5,
            "{name}: the campaign and its sections"
        );
        assert!(
            !prepared.payload.contains_key("lastExportedAt"),
            "{name}: never exported"
        );
    }
}

#[test]
fn import_creates_copy_the_copy_keeps_the_archives_origin_not_the_campaign_rows() {
    let archive = edited(full(), |archive| {
        archive["campaign"]["origin_campaign_id"] = json!(uuid(0xaa, 9));
    });
    let origin = archive["origin_campaign_id"].clone();
    let prepared = prepare(&read(archive), AT, drawn()).expect("prepared");
    assert_eq!(prepared.payload["originCampaignId"], origin);
    assert_eq!(prepared.payload["importedAt"], json!(AT));
    assert_ne!(
        prepared.payload["id"], origin,
        "the copy's id is new all the same"
    );
}

/// The manifest the engine judges the payload against.
const MANIFEST: &str = include_str!("../data-capability.json");

#[test]
fn the_payload_writes_only_what_the_manifest_touches() {
    let prepared = prepare(&read(full()), AT, drawn()).expect("prepared");
    writes_only_touches(MANIFEST, &prepared.payload);
}

#[test]
fn import_creates_copy_every_id_drawn_is_new_and_no_two_are_alike() {
    let request = read(full());
    let prepared = prepare(&request, AT, drawn()).expect("prepared");
    let mut ids = BTreeSet::new();
    let mut count = 0;
    let mut walk = |value: &Value| {
        let mut stack = vec![value];
        while let Some(value) = stack.pop() {
            match value {
                Value::Object(members) => {
                    if let Some(id) = members.get("id").and_then(Value::as_str) {
                        count += 1;
                        ids.insert(id.to_owned());
                    }
                    stack.extend(members.values());
                }
                Value::Array(rows) => stack.extend(rows),
                _ => {}
            }
        }
    };
    let payload = Value::Object(prepared.payload.clone().into_iter().collect());
    walk(&payload);
    assert_eq!(
        count,
        request.ids.len(),
        "one id for every id of the archive"
    );
    assert_eq!(ids.len(), count, "no two alike");
    assert!(ids.is_disjoint(&request.ids), "none is the archive's");
}

#[test]
fn import_creates_copy_ids_that_are_not_new_are_refused() {
    let request = read(full());
    let mut original = request.ids.iter().cloned();
    assert_eq!(
        prepare(&request, AT, || original.next().expect("an id")),
        Err(Refusal::IdsNotNew),
        "the archive's own ids"
    );
    assert_eq!(
        prepare(&request, AT, || uuid(0xd0, 1)),
        Err(Refusal::IdsNotNew),
        "the same id twice"
    );
    let only_campaign = read(json!({ "format": "campaign-archive", "version": 1,
        "campaign": { "id": uuid(0xc1, 1) } }));
    assert_eq!(
        prepare(&only_campaign, AT, || uuid(0xc1, 1)),
        Err(Refusal::IdsNotNew),
        "the campaign's own id"
    );
    assert_eq!(
        prepare(&request, AT, || "new_1".to_owned()),
        Err(Refusal::IdsNotUuids)
    );
}

#[test]
fn ids_are_written_in_lowercase() {
    let mut n = 0;
    let prepared = prepare(&read(full()), AT, || {
        n += 1;
        uuid(0xdd, n).to_uppercase()
    })
    .expect("prepared");
    assert_eq!(prepared.target_id, prepared.target_id.to_lowercase());
    assert_eq!(prepared.payload["id"], json!(prepared.target_id));
}

#[test]
fn an_archive_without_media_sounds_or_sessions_copies_its_campaign_players_and_elements_only() {
    let archive = json!({
        "format": "campaign-archive", "version": 1, "origin_campaign_id": uuid(0xc1, 1),
        "campaign": { "id": uuid(0xc1, 1), "name": "Valombre", "kind": "campagne",
            "cover_media_id": null, "sound_master_volume": 1 },
        "players": [{ "id": uuid(0xa1, 1), "campaign_id": uuid(0xc1, 1), "name": "Claire" }],
        "elements": [{ "id": uuid(0xe1, 1), "campaign_id": uuid(0xc1, 1), "type": "pnj",
            "name": "Le garde", "character": null }]
    });
    let prepared = prepare(&read(archive), AT, drawn()).expect("prepared");
    for section in ["media", "sounds", "sessions"] {
        assert_eq!(prepared.payload[section], json!([]), "{section}");
    }
    assert_eq!(
        prepared.payload["players"].as_array().map(Vec::len),
        Some(1)
    );
    assert_eq!(
        prepared.payload["elements"].as_array().map(Vec::len),
        Some(1)
    );
    assert_eq!(prepared.payload["elements"][0]["aliases"], json!([]));
    assert_eq!(prepared.payload["elements"][0]["versions"], json!([]));
}

#[test]
fn an_archive_reads_as_an_object_or_as_its_text() {
    let archive = full();
    let object = request(&wire(archive.clone()), None).expect("an object");
    let text = request(&wire(json!(archive.to_string())), None).expect("a text");
    assert_eq!(object, text);
    assert_eq!(object.archive["format"], json!("campaign-archive"));
}

#[test]
fn an_archive_that_is_none_is_refused() {
    for wire in [
        row(json!({})),
        row(json!({ "archive": null })),
        row(json!({ "archive": 3 })),
        row(json!({ "archive": [] })),
        row(json!({ "archive": "not json" })),
        row(json!({ "archive": "[1]" })),
    ] {
        assert_eq!(request(&wire, None), Err(Refusal::Unreadable), "{wire:?}");
    }
}

#[test]
fn an_archive_whose_format_is_not_campaign_archive_is_refused() {
    for format in [json!("campaign-export"), json!(null), json!(1)] {
        let archive = edited(full(), |archive| archive["format"] = format.clone());
        assert_eq!(
            request(&wire(archive), None),
            Err(Refusal::NotAnArchive),
            "{format}"
        );
    }
    let archive = edited(full(), |archive| {
        archive.as_object_mut().expect("an object").remove("format");
    });
    assert_eq!(request(&wire(archive), None), Err(Refusal::NotAnArchive));
}

#[test]
fn an_archive_whose_version_is_not_1_is_refused() {
    for version in [json!(2), json!(0), json!("1"), json!(1.5), json!(null)] {
        let archive = edited(full(), |archive| archive["version"] = version.clone());
        assert_eq!(
            request(&wire(archive), None),
            Err(Refusal::UnknownVersion),
            "{version}"
        );
    }
}

#[test]
fn an_archive_that_is_not_well_formed_is_refused() {
    let without_campaign = edited(full(), |archive| {
        archive
            .as_object_mut()
            .expect("an object")
            .remove("campaign");
    });
    assert_eq!(
        request(&wire(without_campaign), None),
        Err(Refusal::NoCampaign)
    );
    let campaign_not_an_object = edited(full(), |archive| archive["campaign"] = json!("c"));
    assert_eq!(
        request(&wire(campaign_not_an_object), None),
        Err(Refusal::NoCampaign)
    );

    for (edit, refusal) in [
        (
            Box::new(|a: &mut Value| a["players"] = json!({})) as Box<dyn Fn(&mut Value)>,
            Refusal::NotAList("players"),
        ),
        (
            Box::new(|a| a["elements"] = json!("x")),
            Refusal::NotAList("elements"),
        ),
        (
            Box::new(|a| a["media"] = json!(1)),
            Refusal::NotAList("media"),
        ),
        (
            Box::new(|a| a["sounds"] = json!(true)),
            Refusal::NotAList("sounds"),
        ),
        (
            Box::new(|a| a["sessions"] = json!({ "a": 1 })),
            Refusal::NotAList("sessions"),
        ),
        (
            Box::new(|a| a["elements"][0]["aliases"] = json!("x")),
            Refusal::NotAList("aliases"),
        ),
        (
            Box::new(|a| a["elements"][0]["versions"] = json!(1)),
            Refusal::NotAList("versions"),
        ),
        (
            Box::new(|a| a["elements"][0]["versions"][0]["diff"] = json!({})),
            Refusal::NotAList("diff"),
        ),
        (
            Box::new(|a| a["sessions"][0]["session_present_character"] = json!("x")),
            Refusal::NotAList("session_present_character"),
        ),
        (
            Box::new(|a| a["sessions"][0]["scenes"] = json!("x")),
            Refusal::NotAList("scenes"),
        ),
        (
            Box::new(|a| a["sessions"][0]["scenes"][0]["scene_entity"] = json!("x")),
            Refusal::NotAList("scene_entity"),
        ),
        (
            Box::new(|a| a["sessions"][0]["scenes"][0]["scene_media"] = json!("x")),
            Refusal::NotAList("scene_media"),
        ),
        (
            Box::new(|a| a["sessions"][0]["journal_lines"] = json!("x")),
            Refusal::NotAList("journal_lines"),
        ),
    ] {
        let archive = edited(full(), |archive| edit(archive));
        assert_eq!(
            request(&wire(archive), None),
            Err(refusal.clone()),
            "{refusal:?}"
        );
    }
}

#[test]
fn a_row_without_an_id_of_its_own_is_refused() {
    for (edit, what) in [
        (
            Box::new(|a: &mut Value| a["campaign"]["id"] = json!(null)) as Box<dyn Fn(&mut Value)>,
            "campaign",
        ),
        (Box::new(|a| a["players"][0]["id"] = json!(3)), "players"),
        (Box::new(|a| a["elements"][0]["id"] = json!("")), "elements"),
        (
            Box::new(|a| a["elements"][0]["versions"][0]["id"] = json!(null)),
            "versions",
        ),
        (Box::new(|a| a["media"][0] = json!("m")), "media"),
        (Box::new(|a| a["sounds"][0]["id"] = json!(null)), "sounds"),
        (
            Box::new(|a| a["sessions"][0]["id"] = json!(null)),
            "sessions",
        ),
        (
            Box::new(|a| a["sessions"][0]["scenes"][0]["id"] = json!(null)),
            "scenes",
        ),
        (
            Box::new(|a| a["sessions"][0]["journal_lines"][0]["id"] = json!(null)),
            "journal_lines",
        ),
    ] {
        let archive = edited(full(), |archive| edit(archive));
        assert_eq!(
            request(&wire(archive), None),
            Err(Refusal::NotAnId(what)),
            "{what}"
        );
    }
}

#[test]
fn an_id_held_twice_is_refused() {
    let archive = edited(full(), |archive| {
        let player = archive["players"][0]["id"].clone();
        archive["sounds"][0]["id"] = player;
    });
    let held = archive["players"][0]["id"]
        .as_str()
        .expect("an id")
        .to_owned();
    assert_eq!(
        request(&wire(archive), None),
        Err(Refusal::DuplicateId(held))
    );
}

#[test]
fn import_creates_copy_a_command_that_names_a_campaign_is_refused() {
    let original = uuid(0xc1, 1);
    assert_eq!(
        request(&wire(full()), Some(&original)),
        Err(Refusal::TargetNamed)
    );
}

#[test]
fn every_refusal_says_why_in_words_and_is_a_422() {
    for refusal in [
        Refusal::Unreadable,
        Refusal::NotAnArchive,
        Refusal::UnknownVersion,
        Refusal::NoCampaign,
        Refusal::NotAList("players"),
        Refusal::NotAnId("players"),
        Refusal::DuplicateId("x".into()),
        Refusal::TargetNamed,
        Refusal::IdsNotUuids,
        Refusal::IdsNotNew,
    ] {
        let words = refusal.to_string();
        assert!(!words.trim().is_empty(), "{refusal:?}");
        let engine = harness_dataguard::plugins::Refusal::from(refusal.clone());
        assert_eq!(
            engine.kind,
            harness_dataguard::plugins::RefusalKind::Unprocessable
        );
        assert_eq!(engine.message, words);
    }
    assert!(
        Refusal::NotAList("sessions")
            .to_string()
            .contains("sessions")
    );
    assert!(Refusal::NotAnId("media").to_string().contains("media"));
    assert!(
        Refusal::DuplicateId("abc".into())
            .to_string()
            .contains("abc")
    );
}

#[test]
fn the_time_of_the_import_is_rfc_3339_to_the_millisecond() {
    for (millis, text) in [
        (0, "1970-01-01T00:00:00.000Z"),
        (951_868_799_999, "2000-02-29T23:59:59.999Z"),
        (1_709_164_799_999, "2024-02-28T23:59:59.999Z"),
        (1_709_164_800_000, "2024-02-29T00:00:00.000Z"),
        (1_735_689_600_000, "2025-01-01T00:00:00.000Z"),
        (1_740_787_199_000, "2025-02-28T23:59:59.000Z"),
        (1_777_593_600_000, "2026-05-01T00:00:00.000Z"),
        (1_792_000_000_123, "2026-10-14T17:46:40.123Z"),
        (4_102_444_799_999, "2099-12-31T23:59:59.999Z"),
        // The centuries that are not leap years, and the one that is.
        (4_107_542_399_999, "2100-02-28T23:59:59.999Z"),
        (4_107_542_400_000, "2100-03-01T00:00:00.000Z"),
        (10_445_241_600_000, "2300-12-31T00:00:00.000Z"),
        (13_574_606_400_000, "2400-02-29T12:00:00.000Z"),
        (13_574_649_600_000, "2400-03-01T00:00:00.000Z"),
    ] {
        assert_eq!(timestamp(millis), text);
    }
}

/// A map of the archive's rows is never reordered: the copy keeps the order
/// of the archive's lists.
#[test]
fn the_copy_keeps_the_order_of_the_archive() {
    let archive = full();
    let prepared = prepare(&read(archive.clone()), AT, drawn()).expect("prepared");
    let names = |value: &Value, section: &str| -> Vec<Value> {
        value[section]
            .as_array()
            .expect("a list")
            .iter()
            .map(|row| row["name"].clone())
            .collect()
    };
    let payload = Value::Object(Map::from_iter(prepared.payload));
    for section in ["players", "elements", "sounds"] {
        assert_eq!(
            names(&payload, section),
            names(&archive, section),
            "{section}"
        );
    }
}
