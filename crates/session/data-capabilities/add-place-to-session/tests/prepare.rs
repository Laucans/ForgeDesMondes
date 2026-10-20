//! session.addPlaceToSession's preparation: the wire's `{session, scene?,
//! place}` made the link the command writes, on its own partition, ranked
//! after the scene's last element.

use std::collections::BTreeMap;

use add_place_to_session::{
    Asked, LIEU, PLACE, POSITION, Refusal, Row, SCENE, Scene, asked, link_id, link_of, names,
    prepare,
};
use serde_json::{Value, json};

const SEANCE_4: &str = "51000000-0000-4000-8000-000000000004";
const ARRIVEE: &str = "5c000000-0000-4000-8000-000000000001";
const TAVERNE: &str = "e1000000-0000-4000-8000-000000000003";
const TOUR: &str = "e1000000-0000-4000-8000-000000000004";
const DONJON: &str = "e1000000-0000-4000-8000-000000000005";

fn wire(payload: Value) -> Row {
    let Value::Object(payload) = payload else {
        panic!("a payload is an object");
    };
    payload.into_iter().collect()
}

fn the_tavern() -> Asked {
    Asked {
        session: SEANCE_4.into(),
        scene: Some(ARRIVEE.into()),
        place: TAVERNE.into(),
    }
}

/// The arrival, linking `links` already.
fn arrival(links: &[(&str, u64)]) -> Scene {
    Scene {
        id: ARRIVEE.to_uppercase(),
        links: links
            .iter()
            .map(|(id, rank)| ((*id).to_owned(), *rank))
            .collect::<BTreeMap<_, _>>(),
    }
}

#[test]
fn it_names_itself_with_or_without_its_version() {
    assert!(names("session.addPlaceToSession"));
    assert!(names("session.addPlaceToSession@1"));
    assert!(!names("session.addScene"));
    assert!(!names("session.addPlaceToSessions"));
}

#[test]
fn the_wire_asks_a_session_a_place_and_maybe_a_scene_in_lowercase() {
    assert_eq!(
        asked(&wire(json!({
            "session": format!(" {} ", SEANCE_4.to_uppercase()),
            "scene": ARRIVEE.to_uppercase(),
            "place": TAVERNE,
        }))),
        Ok(the_tavern())
    );
    for scene in [json!(null), json!("  "), json!(4)] {
        assert_eq!(
            asked(&wire(
                json!({ "session": SEANCE_4, "scene": scene, "place": TAVERNE })
            )),
            Ok(Asked {
                scene: None,
                ..the_tavern()
            }),
            "{scene}"
        );
    }
}

#[test]
fn a_link_naming_no_session_or_no_place_is_refused() {
    for (payload, refusal) in [
        (json!({ "place": TAVERNE }), Refusal::NoSession),
        (
            json!({ "session": " ", "place": TAVERNE }),
            Refusal::NoSession,
        ),
        (json!({ "session": SEANCE_4 }), Refusal::NoPlace),
        (json!({ "session": SEANCE_4, "place": 3 }), Refusal::NoPlace),
    ] {
        assert_eq!(asked(&wire(payload.clone())), Err(refusal), "{payload}");
    }
}

#[test]
fn the_place_joins_its_scene_after_the_last_element_on_the_link_s_own_partition() {
    let prepared = prepare(
        &the_tavern(),
        Some(&arrival(&[(TOUR, 1), (DONJON, 3)])),
        Some(LIEU),
    )
    .expect("prepared");
    assert_eq!(
        prepared.target_id,
        format!("{SEANCE_4}:{ARRIVEE}:{TAVERNE}")
    );
    assert_eq!(
        prepared.payload,
        wire(json!({ SCENE: ARRIVEE, PLACE: TAVERNE, POSITION: 4 }))
    );
    // The scene's first element, its type unknown yet: the DataGuard judges.
    let prepared = prepare(&the_tavern(), Some(&arrival(&[])), None).expect("prepared");
    assert_eq!(prepared.payload[POSITION], json!(1));
}

#[test]
fn a_place_the_scene_links_already_keeps_its_rank_and_its_one_link() {
    let first = prepare(&the_tavern(), Some(&arrival(&[(TOUR, 1)])), Some(LIEU)).expect("first");
    let again = prepare(
        &the_tavern(),
        Some(&arrival(&[(TAVERNE, 2), (TOUR, 1), (DONJON, 3)])),
        Some(LIEU),
    )
    .expect("again");
    assert_eq!(again.target_id, first.target_id, "the same link");
    assert_eq!(again.payload[POSITION], json!(2));
}

#[test]
fn a_session_with_no_scene_takes_no_place_and_a_pnj_is_no_place() {
    assert_eq!(
        prepare(&the_tavern(), None, Some(LIEU)),
        Err(Refusal::NoScene)
    );
    for kind in ["pnj", "pj", "evenement"] {
        assert_eq!(
            prepare(&the_tavern(), Some(&arrival(&[])), Some(kind)),
            Err(Refusal::NotAPlace),
            "{kind}"
        );
    }
}

#[test]
fn a_link_s_id_names_its_session_its_scene_and_its_place() {
    let id = link_id(&SEANCE_4.to_uppercase(), ARRIVEE, TAVERNE);
    assert_eq!(link_of(&id), Some((SEANCE_4, ARRIVEE, TAVERNE)));
    for id in [
        SEANCE_4.to_owned(),
        format!("{SEANCE_4}:{ARRIVEE}"),
        format!("{SEANCE_4}::{TAVERNE}"),
        format!("{SEANCE_4}:{ARRIVEE}:{TAVERNE}:{TOUR}"),
    ] {
        assert_eq!(link_of(&id), None, "{id}");
    }
}

#[test]
fn a_refusal_says_why_in_the_dm_s_words() {
    assert!(Refusal::NoScene.to_string().contains("scène"));
    assert!(Refusal::NotAPlace.to_string().contains("lieu"));
    assert!(Refusal::NoSession.to_string().contains("séance"));
    assert!(Refusal::NoPlace.to_string().contains("lieu"));
}
