//! session.addScene's request and its judgement, without an engine: what
//! the screen sends becomes the scene's row, or is refused before anything
//! is queued.

use add_scene::{
    Asked, ENTITY_CAMPAIGN_ID, Refusal, Row, SESSION_CAMPAIGN_ID, TYPE, asked, prepare, scene_id,
};
use serde_json::{Value, json};

const VALOMBRE: &str = "c1000000-0000-4000-8000-000000000001";
const BRUME: &str = "c1000000-0000-4000-8000-000000000002";
const SEANCE: &str = "51000000-0000-4000-8000-00000000000a";
const TAVERNE: &str = "e1000000-0000-4000-8000-00000000000b";
const MOULIN: &str = "e1000000-0000-4000-8000-00000000000c";
const SCENE: &str = "5c000000-0000-4000-8000-000000000001";

fn wire(value: Value) -> Row {
    match value {
        Value::Object(row) => row.into_iter().collect(),
        _ => panic!("a payload is an object"),
    }
}

fn row(field: &str, campaign: &str) -> Row {
    Row::from([(field.to_owned(), json!(campaign))])
}

/// An element of `campaign` of type `kind`, as the store holds it.
fn element(campaign: &str, kind: &str) -> Row {
    let mut row = row(ENTITY_CAMPAIGN_ID, campaign);
    row.insert(TYPE.to_owned(), json!(kind));
    row
}

#[test]
fn a_request_names_its_session_its_title_its_notes_and_its_places_each_once() {
    let asked = asked(&wire(json!({
        "session": format!(" {} ", SEANCE.to_uppercase()),
        "title": "  L'arrivée  ",
        "notes": " Il pleut. ",
        "places": [TAVERNE.to_uppercase(), MOULIN, TAVERNE],
    })))
    .expect("asked");
    assert_eq!(
        asked,
        Asked {
            session: SEANCE.to_owned(),
            title: "L'arrivée".to_owned(),
            notes: Some(" Il pleut. ".to_owned()),
            places: vec![TAVERNE.to_owned(), MOULIN.to_owned()],
        }
    );
    // Notes and places are optional.
    let bare =
        asked_from(json!({ "session": SEANCE, "title": "Le gué", "notes": null, "places": null }));
    assert_eq!(bare.notes, None);
    assert!(bare.places.is_empty());
    assert_eq!(
        asked_from(json!({ "session": SEANCE, "title": "Le gué" })),
        bare
    );
}

fn asked_from(value: Value) -> Asked {
    asked(&wire(value)).expect("asked")
}

#[test]
fn a_malformed_request_is_refused() {
    let base = || json!({ "session": SEANCE, "title": "Le gué" });
    let with = |field: &str, value: Value| {
        let mut payload = base();
        payload[field] = value;
        payload
    };
    for (payload, refusal) in [
        (
            with("position", json!(1)),
            Refusal::Unknown("position".to_owned()),
        ),
        (json!({ "title": "Le gué" }), Refusal::UnknownSession),
        (with("session", json!("seance-4")), Refusal::UnknownSession),
        (with("session", json!(42)), Refusal::UnknownSession),
        (json!({ "session": SEANCE }), Refusal::NoTitle),
        (with("title", json!("   ")), Refusal::NoTitle),
        (with("title", json!(7)), Refusal::NoTitle),
        (with("notes", json!(["Il pleut."])), Refusal::InvalidNotes),
        (with("places", json!(TAVERNE)), Refusal::InvalidPlaces),
        (
            with("places", json!(["taverne-sanglier"])),
            Refusal::InvalidPlaces,
        ),
        (with("places", json!([TAVERNE, 3])), Refusal::InvalidPlaces),
    ] {
        assert_eq!(asked(&wire(payload.clone())), Err(refusal), "{payload}");
    }
}

#[test]
fn a_scene_writes_its_row_on_its_own_partition_and_its_rank_is_left_to_the_aggregate() {
    let asked = asked_from(json!({
        "session": SEANCE,
        "title": "L'arrivée",
        "notes": "Il pleut.",
        "places": [TAVERNE, MOULIN],
    }));
    let session = row(SESSION_CAMPAIGN_ID, &VALOMBRE.to_uppercase());
    let places = [element(VALOMBRE, "lieu"), element(VALOMBRE, "lieu")];
    assert_eq!(
        prepare(&asked, SCENE, &session, &places),
        Ok(wire(json!({
            "sceneId": SCENE,
            "sceneSessionId": SEANCE,
            "sceneTitle": "L'arrivée",
            "notes": "Il pleut.",
            "entityId": [TAVERNE, MOULIN],
        })))
    );
    // No place: no list, and no notes is a null.
    let bare = asked_from(json!({ "session": SEANCE, "title": "Le gué" }));
    assert_eq!(
        prepare(&bare, SCENE, &session, &[]),
        Ok(wire(json!({
            "sceneId": SCENE,
            "sceneSessionId": SEANCE,
            "sceneTitle": "Le gué",
            "notes": null,
        })))
    );
}

#[test]
fn a_session_there_is_not_or_a_place_of_another_campaign_is_refused() {
    let asked = asked_from(json!({ "session": SEANCE, "title": "L'arrivée", "places": [TAVERNE] }));
    let session = row(SESSION_CAMPAIGN_ID, VALOMBRE);
    assert_eq!(
        prepare(&asked, SCENE, &Row::new(), &[element(VALOMBRE, "lieu")]),
        Err(Refusal::UnknownSession)
    );
    for places in [
        // A place of Brume, an element there is not, none at all.
        vec![element(BRUME, "lieu")],
        vec![Row::new()],
        vec![],
    ] {
        assert_eq!(
            prepare(&asked, SCENE, &session, &places),
            Err(Refusal::OtherCampaign),
            "{places:?}"
        );
    }
    assert!(
        Refusal::OtherCampaign
            .to_string()
            .starts_with("scene-links-same-campaign")
    );
    // An element of the campaign that is no place: an NPC, a lieu without
    // its type.
    for place in [element(VALOMBRE, "pnj"), row(ENTITY_CAMPAIGN_ID, VALOMBRE)] {
        assert_eq!(
            prepare(&asked, SCENE, &session, &[place]),
            Err(Refusal::NotAPlace)
        );
    }
}

#[test]
fn a_scene_s_id_is_a_uuid_beginning_with_the_time_it_was_asked_at() {
    let fresh = "3F2A9C4E-1B7D-4E8F-9A6B-0C5D7E1F2A3B";
    let id = scene_id(0x0193_4f2a_1b7c, fresh);
    assert_eq!(id, "01934f2a-1b7c-73f2-8a9c-4e1b7d4e8f9a");
    assert!(aggregate_session::uuid(&id), "{id}");
    // Asked one millisecond later, whatever its random part: after it.
    assert!(scene_id(0x0193_4f2a_1b7d, "00000000-0000-4000-8000-000000000000") > id);
    // An id with too few hex digits is padded: still a uuid.
    let short = scene_id(1, "new_1");
    assert_eq!(short, "00000000-0001-7e10-8000-000000000000");
    // Only the 48 bits of the time are kept.
    assert!(scene_id(u64::MAX, fresh).starts_with("ffffffff-ffff-7"));
}
