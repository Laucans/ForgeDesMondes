//! session.addPlaceToSession's request and its judgement, without an
//! engine: what the screen sends becomes the row of the scene the place
//! joins, or is refused before anything is queued.

use add_place_to_session::{
    Asked, ENTITY_CAMPAIGN_ID, Refusal, Row, SESSION_CAMPAIGN_ID, TYPE, asked, prepare,
};
use serde_json::{Value, json};

const VALOMBRE: &str = "c1000000-0000-4000-8000-000000000001";
const BRUME: &str = "c1000000-0000-4000-8000-000000000002";
const SEANCE: &str = "51000000-0000-4000-8000-000000000004";
const AUTRE: &str = "51000000-0000-4000-8000-000000000005";
const SCENE: &str = "5c000000-0000-4000-8000-000000000001";
const TAVERNE: &str = "e1000000-0000-4000-8000-000000000002";
const MOULIN: &str = "e1000000-0000-4000-8000-000000000005";

fn wire(value: Value) -> Row {
    match value {
        Value::Object(row) => row.into_iter().collect(),
        _ => panic!("a payload is an object"),
    }
}

fn asked_from(value: Value) -> Asked {
    asked(&wire(value)).expect("asked")
}

/// Valombre's session, as the store holds it.
fn session() -> Row {
    wire(json!({ SESSION_CAMPAIGN_ID: VALOMBRE, "sessionStatus": "prep" }))
}

/// A scene of `session` linking `links`, as the store holds it.
fn scene(session: &str, links: Value) -> Row {
    wire(json!({
        "sceneId": SCENE.to_uppercase(),
        "sceneSessionId": session,
        "sceneTitle": "Le bourg",
        "notes": "Il pleut.",
        "entityId": links,
    }))
}

/// An element of `campaign` of type `kind`, as the store holds it.
fn element(campaign: &str, kind: &str) -> Row {
    wire(json!({ ENTITY_CAMPAIGN_ID: campaign, TYPE: kind }))
}

fn tavern() -> Asked {
    Asked {
        session: SEANCE.to_owned(),
        scene: Some(SCENE.to_owned()),
        place: TAVERNE.to_owned(),
    }
}

#[test]
fn a_request_names_its_session_its_place_and_its_scene_when_it_names_one() {
    assert_eq!(
        asked_from(json!({
            "session": format!(" {} ", SEANCE.to_uppercase()),
            "scene": SCENE.to_uppercase(),
            "place": format!("{TAVERNE} "),
        })),
        tavern()
    );
    // The scene is optional: none is the session's first.
    let first = Asked {
        scene: None,
        ..tavern()
    };
    assert_eq!(
        asked_from(json!({ "session": SEANCE, "place": TAVERNE })),
        first
    );
    assert_eq!(
        asked_from(json!({ "session": SEANCE, "scene": null, "place": TAVERNE })),
        first
    );
}

#[test]
fn a_request_with_another_field_or_ids_that_are_no_uuids_is_refused() {
    let refusal = |value| asked(&wire(value)).expect_err("refused");
    assert_eq!(
        refusal(json!({ "session": SEANCE, "place": TAVERNE, "media": TAVERNE })),
        Refusal::Unknown("media".to_owned())
    );
    for session in [json!(null), json!("séance 4"), json!(4)] {
        assert_eq!(
            refusal(json!({ "session": session, "place": TAVERNE })),
            Refusal::UnknownSession
        );
    }
    for scene in [json!("la première"), json!(1), json!("")] {
        assert_eq!(
            refusal(json!({ "session": SEANCE, "scene": scene, "place": TAVERNE })),
            Refusal::UnknownScene
        );
    }
    for place in [json!(null), json!("la taverne")] {
        assert_eq!(
            refusal(json!({ "session": SEANCE, "place": place })),
            Refusal::InvalidPlace
        );
    }
    assert_eq!(refusal(json!({ "session": SEANCE })), Refusal::InvalidPlace);
}

#[test]
fn the_place_joins_the_scene_after_its_last_element_its_title_and_notes_kept() {
    let row = prepare(
        &tavern(),
        &session(),
        &scene(SEANCE, json!([MOULIN.to_uppercase()])),
        &element(VALOMBRE, "lieu"),
    )
    .expect("prepared");
    assert_eq!(
        Value::Object(row.into_iter().collect()),
        json!({
            "sceneId": SCENE,
            "sceneSessionId": SEANCE,
            "sceneTitle": "Le bourg",
            "notes": "Il pleut.",
            "entityId": [MOULIN, TAVERNE],
        })
    );
    // A scene that links nothing yet, nor holds notes: the place is its first.
    let mut bare = scene(SEANCE, json!(null));
    bare.remove("notes");
    let row = prepare(&tavern(), &session(), &bare, &element(VALOMBRE, "lieu")).expect("prepared");
    assert_eq!(row["entityId"], json!([TAVERNE]));
    assert_eq!(row["notes"], Value::Null);
}

#[test]
fn a_place_the_scene_links_already_keeps_its_rank_a_single_link_remaining() {
    let row = prepare(
        &tavern(),
        &session(),
        &scene(SEANCE, json!([TAVERNE.to_uppercase(), MOULIN])),
        &element(VALOMBRE, "lieu"),
    )
    .expect("prepared");
    assert_eq!(row["entityId"], json!([TAVERNE, MOULIN]));
}

#[test]
fn a_session_or_a_scene_the_store_does_not_hold_is_refused() {
    let place = element(VALOMBRE, "lieu");
    let ours = scene(SEANCE, json!([]));
    assert_eq!(
        prepare(&tavern(), &Row::new(), &ours, &place),
        Err(Refusal::UnknownSession)
    );
    assert_eq!(
        prepare(&tavern(), &session(), &Row::new(), &place),
        Err(Refusal::UnknownScene)
    );
    // No scene named, and the session has none.
    let first = Asked {
        scene: None,
        ..tavern()
    };
    assert_eq!(
        prepare(&first, &session(), &Row::new(), &place),
        Err(Refusal::NoScene)
    );
}

#[test]
fn scene_links_same_campaign_refuses_a_scene_of_another_session_or_a_place_of_another_campaign() {
    let place = element(VALOMBRE, "lieu");
    for other in [scene(AUTRE, json!([])), {
        let mut row = scene(SEANCE, json!([]));
        row.remove("sceneSessionId");
        row
    }] {
        let refusal = prepare(&tavern(), &session(), &other, &place).expect_err("refused");
        assert_eq!(refusal, Refusal::OtherSession);
        assert!(
            refusal.to_string().starts_with("scene-links-same-campaign"),
            "{refusal}"
        );
    }
    let ours = scene(SEANCE, json!([]));
    // A place of Brume, or one the store does not hold.
    for stranger in [element(BRUME, "lieu"), Row::new()] {
        let refusal = prepare(&tavern(), &session(), &ours, &stranger).expect_err("refused");
        assert_eq!(refusal, Refusal::OtherCampaign);
        assert!(
            refusal.to_string().starts_with("scene-links-same-campaign"),
            "{refusal}"
        );
    }
    // An element of Valombre that is no « lieu ».
    assert_eq!(
        prepare(&tavern(), &session(), &ours, &element(VALOMBRE, "pnj")),
        Err(Refusal::NotAPlace)
    );
}

#[test]
fn every_refusal_says_what_was_wrong() {
    for (refusal, said) in [
        (
            Refusal::Unknown("media".to_owned()),
            "media : un lieu rejoint une scène par session, scene et place",
        ),
        (Refusal::UnknownSession, "séance introuvable"),
        (Refusal::UnknownScene, "scène introuvable"),
        (Refusal::NoScene, "la séance n'a pas encore de scène"),
        (Refusal::InvalidPlace, "place : l'identifiant d'un lieu"),
        (Refusal::NotAPlace, "place : seul un lieu de la campagne"),
    ] {
        assert_eq!(refusal.to_string(), said);
    }
}
