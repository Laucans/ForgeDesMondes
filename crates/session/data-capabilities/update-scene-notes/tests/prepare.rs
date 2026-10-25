//! `session.updateSceneNotes`'s pure core, no engine: [`request`] reads the
//! wire — a scene, its session, notes that are a text — and [`prepare`]
//! judges the scene's row: a scene the store does not hold is unknown, one
//! of another session is refused, and the row written changes the notes
//! alone.

use serde_json::{Value, json};

use update_scene_notes::{Refusal, Row, prepare, request};

/// A row, from a JSON object.
fn row(value: Value) -> Row {
    serde_json::from_value(value).expect("an object")
}

const SCENE: &str = "0c000000-0000-4000-8000-000000000001";
const SESSION: &str = "0b000000-0000-4000-8000-000000000001";
const OTHER_SESSION: &str = "0b000000-0000-4000-8000-000000000002";
const PLACE: &str = "0e000000-0000-4000-8000-000000000001";
const EVENT: &str = "0e000000-0000-4000-8000-000000000002";

/// The wire payload of the command.
fn wire(notes: Value) -> Row {
    row(json!({ "session": SESSION, "scene": SCENE, "notes": notes }))
}

/// A scene of `SESSION`, as the engine holds it.
fn scene() -> Row {
    row(json!({
        "sceneId": SCENE,
        "sceneSessionId": SESSION,
        "sceneTitle": "La taverne",
        "notes": "avant",
        "entityId": [PLACE, EVENT],
    }))
}

#[test]
fn every_refusal_says_why_in_words() {
    for refusal in [
        Refusal::NotAnId("scene"),
        Refusal::InvalidNotes,
        Refusal::Unknown,
        Refusal::OtherSession,
    ] {
        assert!(!refusal.to_string().trim().is_empty(), "{refusal:?}");
    }
    assert!(Refusal::NotAnId("scene").to_string().starts_with("scene"));
}

#[test]
fn only_what_is_not_there_is_a_404() {
    assert!(Refusal::Unknown.not_found());
    for refusal in [
        Refusal::NotAnId("scene"),
        Refusal::InvalidNotes,
        Refusal::OtherSession,
    ] {
        assert!(!refusal.not_found(), "{refusal:?}");
    }
}

#[test]
fn a_scene_and_a_session_that_are_no_uuid_are_refused() {
    let mut bad = wire(json!("x"));
    bad.insert("scene".into(), json!("x"));
    assert_eq!(request(&bad, None), Err(Refusal::NotAnId("scene")));
    let mut bad = wire(json!("x"));
    bad.insert("session".into(), json!("x"));
    assert_eq!(request(&bad, None), Err(Refusal::NotAnId("session")));
    let mut bad = wire(json!("x"));
    bad.remove("session");
    assert_eq!(request(&bad, None), Err(Refusal::NotAnId("session")));
    let mut bad = wire(json!("x"));
    bad.remove("scene");
    assert_eq!(request(&bad, None), Err(Refusal::NotAnId("scene")));
}

#[test]
fn the_scene_is_the_target_the_engine_names_else_the_payloads() {
    let read = request(&wire(json!("x")), None).expect("a request");
    assert_eq!(read.id, SCENE);
    assert_eq!(read.session, SESSION);
    let mut no_scene = wire(json!("x"));
    no_scene.remove("scene");
    let read = request(&no_scene, Some(&SCENE.to_uppercase())).expect("the target");
    assert_eq!(read.id, SCENE, "lowercase, as the tables write it");
    let read = request(&wire(json!("x")), Some(PLACE)).expect("the target wins");
    assert_eq!(read.id, PLACE);
}

#[test]
fn notes_are_a_text_kept_as_written_blank_included() {
    for notes in ["des notes", "", "   ", " a\nb "] {
        let read = request(&wire(json!(notes)), None).expect("a text");
        assert_eq!(read.notes, notes);
    }
}

#[test]
fn notes_that_are_no_text_are_refused() {
    for notes in [json!(null), json!(3), json!(true), json!(["a"]), json!({})] {
        assert_eq!(
            request(&wire(notes.clone()), None),
            Err(Refusal::InvalidNotes),
            "{notes}"
        );
    }
    let mut none = wire(json!("x"));
    none.remove("notes");
    assert_eq!(request(&none, None), Err(Refusal::InvalidNotes));
}

#[test]
fn a_row_that_is_not_a_scene_is_unknown() {
    let read = request(&wire(json!("x")), None).expect("a request");
    assert_eq!(prepare(&read, &Row::new()), Err(Refusal::Unknown));
    let sessions = row(json!({ "sessionId": SESSION, "sessionTitle": "Séance" }));
    assert_eq!(prepare(&read, &sessions), Err(Refusal::Unknown));
}

#[test]
fn a_scene_of_another_session_is_refused() {
    let read = request(&wire(json!("x")), None).expect("a request");
    let mut elsewhere = scene();
    elsewhere.insert("sceneSessionId".into(), json!(OTHER_SESSION));
    assert_eq!(prepare(&read, &elsewhere), Err(Refusal::OtherSession));
    let mut orphan = scene();
    orphan.remove("sceneSessionId");
    assert_eq!(prepare(&read, &orphan), Err(Refusal::OtherSession));
}

#[test]
fn the_notes_replace_the_scene_s_and_nothing_else_changes() {
    let read = request(&wire(json!("après")), None).expect("a request");
    let prepared = prepare(&read, &scene()).expect("the scene is the session's");
    assert_eq!(prepared.target_id, SCENE);
    assert_eq!(
        prepared.payload,
        row(json!({
            "sceneId": SCENE,
            "sceneSessionId": SESSION,
            "sceneTitle": "La taverne",
            "entityId": [PLACE, EVENT],
            "notes": "après",
        }))
    );
}

#[test]
fn a_blank_text_replaces_the_notes_and_is_not_null() {
    let read = request(&wire(json!("")), None).expect("a request");
    let prepared = prepare(&read, &scene()).expect("the scene is the session's");
    assert_eq!(prepared.payload["notes"], json!(""));
}

#[test]
fn a_scene_with_no_list_of_links_links_none() {
    let read = request(&wire(json!("x")), None).expect("a request");
    let mut bare = scene();
    bare.remove("entityId");
    let prepared = prepare(&read, &bare).expect("the scene is the session's");
    assert_eq!(prepared.payload["entityId"], json!([]));
}

#[test]
fn the_session_of_the_row_is_read_whatever_its_case() {
    let read = request(&wire(json!("x")), None).expect("a request");
    let mut upper = scene();
    upper.insert("sceneSessionId".into(), json!(SESSION.to_uppercase()));
    let prepared = prepare(&read, &upper).expect("the same session");
    assert_eq!(prepared.payload["sceneSessionId"], json!(SESSION));
}
