//! What a new scene's wire payload becomes before it is submitted: the scene
//! on its own partition, at the rank it is given, with the places it links
//! as one list — and what the tables could never hold, refused.

use add_scene::{Fresh, Refusal, Row, names, places_of, prepare, session_of};
use serde_json::{Value, json};

const SEANCE: &str = "51000000-0000-4000-8000-000000000001";
const SCENE: &str = "5c000000-0000-4000-8000-000000000003";
const TAVERNE: &str = "e1000000-0000-4000-8000-000000000003";
const TOUR: &str = "e1000000-0000-4000-8000-000000000004";

fn wire(payload: Value) -> Row {
    let Value::Object(payload) = payload else {
        panic!("a payload is an object");
    };
    payload.into_iter().collect()
}

const FRESH: Fresh<'static> = Fresh {
    scene: SCENE,
    position: 3,
};

#[test]
fn a_scene_is_written_on_its_own_partition_at_the_rank_it_is_given() {
    let prepared = prepare(
        &wire(json!({ "session": SEANCE, "title": "  Le gué  ", "notes": "Trois bandits." })),
        FRESH,
    )
    .expect("prepared");
    assert_eq!(prepared.target_id, SCENE);
    assert_eq!(
        Value::Object(prepared.payload.into_iter().collect()),
        json!({
            "sceneId": SCENE,
            "sceneSessionId": SEANCE,
            "scenePosition": 3,
            "sceneTitle": "Le gué",
            "notes": "Trois bandits.",
        })
    );
}

#[test]
fn the_places_are_one_list_in_their_order_each_once_whatever_the_case() {
    let prepared = prepare(
        &wire(json!({
            "session": SEANCE.to_uppercase(),
            "title": "La taverne",
            "places": [TAVERNE, TOUR, TAVERNE.to_uppercase(), "taverne-sanglier"],
        })),
        FRESH,
    )
    .expect("prepared");
    let payload = prepared.payload;
    assert_eq!(payload["sceneSessionId"], SEANCE);
    assert_eq!(payload["sceneEntitySceneId"], SCENE);
    // An id that is no uuid is kept: the DataGuard judges it no element of
    // the campaign.
    assert_eq!(
        payload["entityId"],
        json!([TAVERNE, TOUR, "taverne-sanglier"])
    );
    assert_eq!(payload["notes"], Value::Null);
}

#[test]
fn a_scene_linking_nothing_writes_no_link() {
    for places in [json!([]), Value::Null] {
        let prepared = prepare(
            &wire(
                json!({ "session": SEANCE, "title": "La route", "notes": "  ", "places": places }),
            ),
            FRESH,
        )
        .expect("prepared");
        assert!(!prepared.payload.contains_key("entityId"));
        assert!(!prepared.payload.contains_key("sceneEntitySceneId"));
        assert_eq!(prepared.payload["notes"], Value::Null);
    }
}

#[test]
fn what_the_tables_could_never_hold_is_refused() {
    for (payload, refusal) in [
        (json!({ "title": "Le gué" }), Refusal::NoSession),
        (
            json!({ "session": "seance-4", "title": "Le gué" }),
            Refusal::NoSession,
        ),
        (json!({ "session": SEANCE }), Refusal::NoTitle),
        (
            json!({ "session": SEANCE, "title": "   " }),
            Refusal::NoTitle,
        ),
        (json!({ "session": SEANCE, "title": 4 }), Refusal::NoTitle),
        (
            json!({ "session": SEANCE, "title": "Le gué", "notes": ["a"] }),
            Refusal::NotNotes,
        ),
        (
            json!({ "session": SEANCE, "title": "Le gué", "places": TAVERNE }),
            Refusal::NotPlaces,
        ),
        (
            json!({ "session": SEANCE, "title": "Le gué", "places": [TAVERNE, 4] }),
            Refusal::NotPlaces,
        ),
        (
            json!({ "session": SEANCE, "title": "Le\u{0}gué" }),
            Refusal::Nul,
        ),
        (
            json!({ "session": SEANCE, "title": "Le gué", "notes": "a\u{0}" }),
            Refusal::Nul,
        ),
        (
            json!({ "session": SEANCE, "title": "Le gué", "places": ["a\u{0}"] }),
            Refusal::Nul,
        ),
    ] {
        assert_eq!(
            prepare(&wire(payload.clone()), FRESH),
            Err(refusal),
            "{payload}"
        );
    }
}

#[test]
fn the_session_and_the_places_are_read_off_the_wire_payload() {
    assert_eq!(
        session_of(&wire(json!({ "session": SEANCE.to_uppercase() }))),
        Some(SEANCE.to_owned())
    );
    assert_eq!(session_of(&wire(json!({ "session": 4 }))), None);
    assert_eq!(places_of(&wire(json!({}))), Ok(Vec::new()));
    assert!(names("session.addScene@1"));
    assert!(names("session.addScene"));
    assert!(!names("session.addSceneX@1"));
    assert!(!names("session.createSession@1"));
}

#[test]
fn every_refusal_says_why_in_the_dm_s_language() {
    for refusal in [
        Refusal::NoSession,
        Refusal::UnknownSession,
        Refusal::NoTitle,
        Refusal::NotNotes,
        Refusal::NotPlaces,
        Refusal::Nul,
    ] {
        assert!(refusal.to_string().contains("scène"), "{refusal:?}");
    }
}
