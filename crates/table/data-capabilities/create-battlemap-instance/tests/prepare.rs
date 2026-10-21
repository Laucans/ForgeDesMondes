//! `prepare`: the DM's request made the instance written on
//! `BattlemapInstance/<id>` — or refused before it is queued.

use create_battlemap_instance::{
    Placement, Refusal, Request, Row, instance_id, names, prepare, request,
};
use serde_json::{Value, json};

const VALOMBRE: &str = "c1000000-0000-4000-8000-000000000001";
const BRUME: &str = "c1000000-0000-4000-8000-000000000002";
const SEANCE: &str = "5e000000-0000-4000-8000-000000000001";
const AUTRE_SEANCE: &str = "5e000000-0000-4000-8000-000000000002";
const ARRIVEE: &str = "5c000000-0000-4000-8000-000000000001";
const GUE: &str = "6d000000-0000-4000-8000-000000000001";
const NOW: &str = "2026-10-20T10:00:00Z";

fn row(value: Value) -> Row {
    let Value::Object(map) = value else {
        panic!("an object");
    };
    map.into_iter().collect()
}

fn asked() -> Request {
    Request {
        session: SEANCE.into(),
        scene: ARRIVEE.into(),
        battlemap: GUE.into(),
    }
}

/// The tavern's scene of the session, the ford's battlemap of its campaign,
/// and no instance yet.
fn placed() -> Placement {
    Placement {
        session_campaign: Some(VALOMBRE.into()),
        scene_session: Some(SEANCE.into()),
        battlemap_campaign: Some(VALOMBRE.into()),
        instance: None,
    }
}

#[test]
fn the_data_capability_is_named_with_or_without_its_version() {
    assert!(names("table.createBattlemapInstance"));
    assert!(names("table.createBattlemapInstance@1"));
    assert!(!names("table.placeTokens@1"));
    assert!(!names("table.createBattlemapInstances"));
}

#[test]
fn a_request_names_three_ids_written_as_the_tables_write_them() {
    let wire = row(json!({
        "session": SEANCE.to_uppercase(),
        "scene": format!(" {ARRIVEE} "),
        "battlemap": GUE,
    }));
    assert_eq!(request(&wire), Ok(asked()));
}

#[test]
fn a_field_that_is_no_id_is_refused_by_its_name() {
    let full = json!({ "session": SEANCE, "scene": ARRIVEE, "battlemap": GUE });
    for field in ["session", "scene", "battlemap"] {
        for bad in [Value::Null, json!("bm-gue"), json!(""), json!(7)] {
            let mut wire = full.clone();
            wire[field] = bad.clone();
            assert_eq!(
                request(&row(wire)),
                Err(Refusal::NotAnId(field)),
                "{field}: {bad}"
            );
        }
        let mut wire = row(full.clone());
        wire.remove(field);
        assert_eq!(request(&wire), Err(Refusal::NotAnId(field)), "{field}");
    }
}

#[test]
fn a_scene_s_instance_has_one_id_its_own() {
    let id = instance_id(ARRIVEE);
    assert_eq!(id, instance_id(ARRIVEE));
    assert_ne!(id, instance_id("5c000000-0000-4000-8000-000000000002"));
    assert_ne!(id, ARRIVEE);
    // A uuid, as the table's id column takes it.
    let wire = row(json!({ "session": id, "scene": id, "battlemap": id }));
    assert_eq!(request(&wire).map(|asked| asked.scene), Ok(id));
}

#[test]
fn a_creation_writes_the_instance_of_the_scene_on_its_partition() {
    let prepared = prepare(&asked(), &placed(), NOW).expect("created");
    assert_eq!(prepared.target_id, instance_id(ARRIVEE));
    assert_eq!(
        prepared.payload,
        row(json!({
            "battlemapInstanceId": instance_id(ARRIVEE),
            "battlemapMediaId": GUE,
            "sessionId": SEANCE,
            "sceneId": ARRIVEE,
            "createdAt": NOW,
        }))
    );
}

#[test]
fn the_fields_a_creation_writes_are_exactly_the_ones_its_manifest_touches() {
    let manifest: Value =
        serde_json::from_str(include_str!("../data-capability.json")).expect("contract F");
    let mut touches: Vec<String> = manifest["touches"]
        .as_array()
        .expect("touches")
        .iter()
        .map(|touch| touch.as_str().expect("a touch").to_owned())
        .collect();
    touches.sort();
    let written: Vec<String> = prepare(&asked(), &placed(), NOW)
        .expect("created")
        .payload
        .keys()
        .map(|field| format!("BattlemapInstance.{field}"))
        .collect();
    assert_eq!(written, touches);
    assert_eq!(manifest["effect"], "insert");
    assert_eq!(manifest["target"]["aggregate"], "BattlemapInstance");
}

#[test]
fn instance_isolated_a_creation_writes_nothing_of_the_battlemap_or_its_grid() {
    let payload = prepare(&asked(), &placed(), NOW).expect("created").payload;
    // The battlemap is named by its id alone: none of its own fields, nor
    // its grid's, is written.
    for field in [
        "kind",
        "title",
        "filePath",
        "hexSizePx",
        "offsetX",
        "orientation",
    ] {
        assert!(!payload.contains_key(field), "{field}");
    }
}

#[test]
fn one_instance_per_scene_a_scene_that_has_one_is_refused() {
    let placement = Placement {
        instance: Some(instance_id(ARRIVEE)),
        ..placed()
    };
    assert_eq!(
        prepare(&asked(), &placement, NOW),
        Err(Refusal::SceneHasAnInstance)
    );
}

#[test]
fn a_battlemap_of_another_campaign_than_the_session_s_is_refused() {
    let placement = Placement {
        battlemap_campaign: Some(BRUME.into()),
        ..placed()
    };
    assert_eq!(
        prepare(&asked(), &placement, NOW),
        Err(Refusal::BattlemapOfAnotherCampaign)
    );
}

#[test]
fn a_scene_of_another_session_is_refused() {
    let placement = Placement {
        scene_session: Some(AUTRE_SEANCE.into()),
        ..placed()
    };
    assert_eq!(
        prepare(&asked(), &placement, NOW),
        Err(Refusal::SceneOfAnotherSession)
    );
}

#[test]
fn what_the_tables_do_not_hold_is_refused_as_unknown() {
    for (placement, refusal) in [
        (
            Placement {
                session_campaign: None,
                ..placed()
            },
            Refusal::UnknownSession,
        ),
        (
            Placement {
                scene_session: None,
                ..placed()
            },
            Refusal::UnknownScene,
        ),
        (
            Placement {
                battlemap_campaign: None,
                ..placed()
            },
            Refusal::UnknownBattlemap,
        ),
        // Over the engine's memory the tables hold nothing.
        (Placement::default(), Refusal::UnknownSession),
    ] {
        assert_eq!(prepare(&asked(), &placement, NOW), Err(refusal));
    }
}

#[test]
fn ids_are_compared_in_any_case() {
    let placement = Placement {
        session_campaign: Some(VALOMBRE.to_uppercase()),
        scene_session: Some(SEANCE.to_uppercase()),
        battlemap_campaign: Some(VALOMBRE.into()),
        instance: None,
    };
    assert!(prepare(&asked(), &placement, NOW).is_ok());
}

#[test]
fn only_what_is_not_there_is_refused_as_not_found() {
    for refusal in [
        Refusal::UnknownSession,
        Refusal::UnknownScene,
        Refusal::UnknownBattlemap,
    ] {
        assert!(refusal.not_found(), "{refusal:?}");
    }
    for refusal in [
        Refusal::NotAnId("scene"),
        Refusal::NotTheScenesInstance,
        Refusal::SceneOfAnotherSession,
        Refusal::BattlemapOfAnotherCampaign,
        Refusal::SceneHasAnInstance,
    ] {
        assert!(!refusal.not_found(), "{refusal:?}");
    }
}

#[test]
fn every_refusal_says_why_in_words() {
    for refusal in [
        Refusal::NotAnId("scene"),
        Refusal::NotTheScenesInstance,
        Refusal::UnknownSession,
        Refusal::UnknownScene,
        Refusal::UnknownBattlemap,
        Refusal::SceneOfAnotherSession,
        Refusal::BattlemapOfAnotherCampaign,
        Refusal::SceneHasAnInstance,
    ] {
        assert!(!refusal.to_string().is_empty(), "{refusal:?}");
    }
    assert!(Refusal::NotAnId("scene").to_string().contains("scene"));
    assert!(
        Refusal::SceneHasAnInstance
            .to_string()
            .contains("one-instance-per-scene")
    );
}
