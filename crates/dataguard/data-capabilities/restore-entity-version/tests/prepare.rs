//! `prepare`: the DM's request made the command written on `Entity/<id>` —
//! or refused before it is queued.

use restore_entity_version::{Fresh, Kept, Refusal, Row, SOURCE, element_id, names, prepare};
use serde_json::{Map, Value, json};

const VALOMBRE: &str = "c1000000-0000-4000-8000-000000000001";
const MORLOX: &str = "e1000000-0000-4000-8000-000000000003";
const V1: &str = "f1000000-0000-4000-8000-000000000001";
const V2: &str = "f1000000-0000-4000-8000-000000000002";
const V3: &str = "f1000000-0000-4000-8000-000000000003";
const NEW: &str = "f1000000-0000-4000-8000-0000000000ff";
const NOW: &str = "2026-10-18T10:00:00Z";

fn object(value: Value) -> Map<String, Value> {
    let Value::Object(map) = value else {
        panic!("an object");
    };
    map
}

fn row(value: Value) -> Row {
    object(value).into_iter().collect()
}

fn kept(id: &str, number: u64, snapshot: Value, diff: Value) -> Kept {
    Kept {
        id: id.to_owned(),
        number,
        snapshot: object(snapshot),
        diff: diff.as_array().cloned().unwrap_or_default(),
    }
}

/// Morlox's three versions: his notes, HP and aliases moving, a place at
/// version 3.
fn history() -> Vec<Kept> {
    vec![
        kept(
            V1,
            1,
            json!({ "type": "pnj", "name": "Morlox", "notes": "Doit 40 po.", "hpMax": 21,
                    "tokenColor": "danger", "aliases": ["le gobelin"] }),
            json!([]),
        ),
        kept(
            V2,
            2,
            json!({ "type": "pnj", "name": "Morlox", "notes": "A trahi.", "hpMax": 30,
                    "tokenColor": "danger", "aliases": ["Morlo"] }),
            json!([
                { "field": "aliases", "old": ["le gobelin"], "new": ["Morlo"] },
                { "field": "hpMax", "old": 21, "new": 30 },
                { "field": "notes", "old": "Doit 40 po.", "new": "A trahi." },
            ]),
        ),
        kept(
            V3,
            3,
            json!({ "type": "lieu", "name": "Morlox", "notes": "A trahi.", "hpMax": null,
                    "tokenColor": null, "aliases": ["Morlo"] }),
            json!([]),
        ),
    ]
}

/// Morlox's row at version 2, as the engine holds it.
fn base() -> Row {
    row(json!({
        "entityId": MORLOX,
        "entityCampaignId": VALOMBRE,
        "entityCreatedAt": "2026-10-01T10:00:00Z",
        "type": "pnj",
        "name": "Morlox",
        "notes": "A trahi.",
        "hpMax": 30,
        "tokenColor": "danger",
        "characterEntityId": MORLOX,
        "alias": ["Morlo"],
        "entityVersionId": V2,
        "versionNumber": 2,
        "source": "dm",
        "snapshot": { "type": "pnj", "name": "Morlox", "notes": "A trahi.", "hpMax": 30,
                      "tokenColor": "danger", "aliases": ["Morlo"] },
        "diff": [],
    }))
}

fn fresh() -> Fresh<'static> {
    Fresh {
        version: NEW,
        now: NOW,
    }
}

fn restore(wire: Value) -> Result<Row, Refusal> {
    prepare(&row(wire), &base(), &history(), MORLOX, fresh()).map(|p| p.payload)
}

#[test]
fn the_data_capability_is_named_with_or_without_its_version() {
    assert!(names("campaign.restoreEntityVersion"));
    assert!(names("campaign.restoreEntityVersion@1"));
    assert!(!names("campaign.saveEntity@1"));
    assert!(!names("campaign.restoreEntityVersions"));
}

#[test]
fn the_element_is_the_payload_s_entity_or_id_when_it_names_one() {
    assert_eq!(
        element_id(&row(json!({ "entity": MORLOX }))),
        Some(MORLOX.into())
    );
    assert_eq!(
        element_id(&row(json!({ "id": MORLOX }))),
        Some(MORLOX.into())
    );
    assert_eq!(element_id(&row(json!({ "entity": "  " }))), None);
    assert_eq!(element_id(&row(json!({ "version": 1 }))), None);
}

#[test]
fn a_version_restored_whole_rewrites_every_field_of_its_snapshot_and_nulls_the_rest() {
    let payload = restore(json!({ "version": 1 })).expect("restored");
    assert_eq!(payload["notes"], "Doit 40 po.");
    assert_eq!(payload["hpMax"], 21);
    assert_eq!(payload["alias"], json!(["le gobelin"]));
    // What version 1 does not hold is written null, not left as it is.
    for field in [
        "description",
        "playerId",
        "spells",
        "dndbeyondId",
        "tokenCrop",
    ] {
        assert_eq!(payload[field], Value::Null, "{field}");
    }
    assert_eq!(payload["characterEntityId"], MORLOX);
    assert_eq!(payload["entityId"], MORLOX);
    assert_eq!(payload["entityAliasEntityId"], MORLOX);
    assert_eq!(payload["entityVersionEntityId"], MORLOX);
    assert_eq!(payload["updatedAt"], NOW);
    // The campaign and the creation stay the element's.
    assert!(!payload.contains_key("entityCampaignId"));
    assert!(!payload.contains_key("entityCreatedAt"));
}

#[test]
fn the_restoration_is_the_next_version_of_source_restauration_naming_the_one_restored() {
    let payload = restore(json!({ "version": 1 })).expect("restored");
    assert_eq!(payload["source"], SOURCE);
    assert_eq!(SOURCE, "restauration");
    assert_eq!(payload["restoredFromVersionId"], V1);
    assert_eq!(payload["entityVersionId"], NEW);
    assert_eq!(payload["entityVersionCreatedAt"], NOW);
    assert_eq!(payload["versionNumber"], 3);
    // Its snapshot is version 1's, every field it lacks null.
    let snapshot = payload["snapshot"].as_object().expect("a snapshot");
    for (field, value) in &history()[0].snapshot {
        assert_eq!(&snapshot[field], value, "{field}");
    }
    assert!(
        snapshot
            .iter()
            .all(|(field, value)| history()[0].snapshot.contains_key(field) || value.is_null())
    );
    // Its diff runs from the row's last snapshot, version 2's.
    assert_eq!(
        payload["diff"],
        json!([
            { "field": "aliases", "old": ["Morlo"], "new": ["le gobelin"] },
            { "field": "hpMax", "old": 30, "new": 21 },
            { "field": "notes", "old": "A trahi.", "new": "Doit 40 po." },
        ])
    );
}

#[test]
fn a_version_is_named_by_its_number_or_by_its_id_in_any_case() {
    let by_number = restore(json!({ "version": 1 })).expect("restored");
    let by_id = restore(json!({ "version": V1.to_uppercase() })).expect("restored");
    let padded = restore(json!({ "version": format!(" {V1} ") })).expect("restored");
    assert_eq!(by_number, by_id);
    assert_eq!(by_number, padded);
}

#[test]
fn a_version_restored_as_a_place_has_no_sheet() {
    let payload = restore(json!({ "version": 3 })).expect("restored");
    assert_eq!(payload["type"], "lieu");
    assert_eq!(payload["characterEntityId"], Value::Null);
    assert_eq!(payload["tokenColor"], Value::Null);
}

#[test]
fn a_snapshot_holding_no_aliases_restores_none() {
    let history = vec![kept(
        V1,
        1,
        json!({ "type": "lieu", "name": "Moulin" }),
        json!([]),
    )];
    let payload = prepare(
        &row(json!({ "version": 1 })),
        &base(),
        &history,
        MORLOX,
        fresh(),
    )
    .expect("restored")
    .payload;
    assert_eq!(payload["alias"], Value::Null);
}

#[test]
fn undoing_some_changes_of_the_next_version_sets_those_fields_alone_back() {
    // Version 2's changes: aliases (0), hpMax (1), notes (2) — undo the HP.
    let payload = restore(json!({ "version": 1, "changes": [1] })).expect("restored");
    assert_eq!(payload["hpMax"], 21);
    assert_eq!(payload["notes"], "A trahi.");
    assert_eq!(payload["alias"], json!(["Morlo"]));
    assert_eq!(payload["restoredFromVersionId"], V1);
    assert_eq!(
        payload["diff"],
        json!([{ "field": "hpMax", "old": 30, "new": 21 }])
    );
    let every = restore(json!({ "version": 1, "changes": [0, 1, 2] })).expect("restored");
    let whole = restore(json!({ "version": 1 })).expect("restored");
    assert_eq!(every["diff"], whole["diff"]);
}

#[test]
fn a_request_naming_no_element_is_refused() {
    for id in ["", "  "] {
        assert_eq!(
            prepare(
                &row(json!({ "version": 1 })),
                &base(),
                &history(),
                id,
                fresh()
            ),
            Err(Refusal::NoElement)
        );
    }
}

#[test]
fn a_request_naming_no_version_is_refused() {
    for wire in [
        json!({}),
        json!({ "version": null }),
        json!({ "version": true }),
        json!({ "version": "  " }),
        json!({ "version": [1] }),
    ] {
        assert_eq!(restore(wire.clone()), Err(Refusal::NoVersion), "{wire}");
    }
}

#[test]
fn a_version_the_element_does_not_have_is_refused() {
    for wire in [
        json!({ "version": 4 }),
        json!({ "version": 0 }),
        json!({ "version": -1 }),
        json!({ "version": 1.5 }),
        json!({ "version": "f1000000-0000-4000-8000-000000000009" }),
    ] {
        assert_eq!(
            restore(wire.clone()),
            Err(Refusal::UnknownVersion),
            "{wire}"
        );
    }
    // An element the engine does not hold — removed, or named in another
    // case than its partition's — has no version to restore.
    for base in [
        Row::new(),
        row(json!({ "entityId": MORLOX.to_uppercase() })),
    ] {
        assert_eq!(
            prepare(
                &row(json!({ "version": 1 })),
                &base,
                &history(),
                MORLOX,
                fresh()
            ),
            Err(Refusal::UnknownVersion)
        );
    }
    // Another element's history holds none of Morlox's versions.
    assert_eq!(
        prepare(&row(json!({ "version": 1 })), &base(), &[], MORLOX, fresh()),
        Err(Refusal::UnknownVersion)
    );
}

#[test]
fn changes_that_are_not_positions_of_the_next_version_are_refused() {
    for wire in [
        json!({ "version": 1, "changes": [] }),
        json!({ "version": 1, "changes": ["hpMax"] }),
        json!({ "version": 1, "changes": [-1] }),
        json!({ "version": 1, "changes": { "0": true } }),
    ] {
        assert_eq!(restore(wire.clone()), Err(Refusal::NotChanges), "{wire}");
    }
    assert_eq!(
        restore(json!({ "version": 1, "changes": [3] })),
        Err(Refusal::UnknownChange(3))
    );
    // Version 3 is the last: there is no version after it to undo.
    assert_eq!(
        restore(json!({ "version": 3, "changes": [0] })),
        Err(Refusal::UnknownChange(0))
    );
    // Version 2's diff names no field at this position.
    let mut history = history();
    history[2].diff = vec![json!({ "old": 1, "new": 2 })];
    assert_eq!(
        prepare(
            &row(json!({ "version": 2, "changes": [0] })),
            &base(),
            &history,
            MORLOX,
            fresh()
        ),
        Err(Refusal::UnknownChange(0))
    );
}

#[test]
fn every_refusal_says_why_in_words() {
    for refusal in [
        Refusal::NoElement,
        Refusal::NoVersion,
        Refusal::UnknownVersion,
        Refusal::NotChanges,
        Refusal::UnknownChange(3),
    ] {
        assert!(!refusal.to_string().is_empty(), "{refusal:?}");
    }
    assert!(Refusal::UnknownChange(3).to_string().contains('3'));
}

#[test]
fn the_version_a_row_carries_is_kept_as_it_was_written() {
    let kept = Kept::of(&base()).expect("a version");
    assert_eq!(kept.id, V2);
    assert_eq!(kept.number, 2);
    assert_eq!(kept.snapshot["notes"], "A trahi.");
    assert!(kept.diff.is_empty());
    let mut bare = base();
    bare.remove("diff");
    assert_eq!(Kept::of(&bare).map(|k| k.diff), Some(Vec::new()));
    for field in ["entityVersionId", "versionNumber", "snapshot"] {
        let mut without = base();
        without.remove(field);
        assert_eq!(Kept::of(&without), None, "{field}");
    }
}

#[test]
fn lists_and_objects_are_compared_as_json_their_numbers_by_value() {
    let snapshot = |crop: Value, actions: Value| {
        object(json!({ "name": "Morlox", "tokenCrop": crop, "actions": actions }))
    };
    let before = snapshot(json!({ "x": 10, "y": 5 }), json!([{ "dc": 15 }]));
    // The same values, written otherwise: no change.
    let same = snapshot(json!({ "x": 10.0, "y": 5 }), json!([{ "dc": 15.0 }]));
    assert_eq!(
        restore_entity_version::diff(&before, &same),
        Vec::<Value>::new()
    );
    // A value that moved, in an object or in a list, is a change of its field.
    let moved = snapshot(json!({ "x": 10, "y": 6 }), json!([{ "dc": 16 }]));
    let fields: Vec<Value> = restore_entity_version::diff(&before, &moved)
        .iter()
        .map(|change| change["field"].clone())
        .collect();
    assert_eq!(fields, [json!("actions"), json!("tokenCrop")]);
    // A key more, or an item more, is a change too.
    let grown = snapshot(
        json!({ "x": 10, "y": 5, "w": 2 }),
        json!([{ "dc": 15 }, { "dc": 15 }]),
    );
    assert_eq!(restore_entity_version::diff(&before, &grown).len(), 2);
}

#[test]
fn the_fields_a_restore_writes_are_exactly_the_ones_its_manifest_touches() {
    let manifest: Value =
        serde_json::from_str(include_str!("../data-capability.json")).expect("contract F");
    let mut touches: Vec<String> = manifest["touches"]
        .as_array()
        .expect("touches")
        .iter()
        .map(|touch| touch.as_str().expect("a touch").to_owned())
        .collect();
    touches.sort();
    for wire in [
        json!({ "version": 1 }),
        json!({ "version": 3 }),
        json!({ "version": 1, "changes": [1] }),
    ] {
        let written: Vec<String> = restore(wire)
            .expect("restored")
            .keys()
            .map(|field| format!("Entity.{field}"))
            .collect();
        assert_eq!(written, touches);
    }
}
