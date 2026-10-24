//! session.addPresentCharacter's request, read and judged before anything
//! is queued: what the screen sends becomes the row of one PC present at a
//! session; a malformed request, a session there is not, or a character
//! that is no PC of the session's campaign (`present-only-pj`) is refused.

use add_present_character::{Asked, Refusal, Row, asked, prepare};
use serde_json::{Value, json};

const SEANCE: &str = "51000000-0000-4000-8000-00000000000a";
const AUTRE: &str = "51000000-0000-4000-8000-00000000000b";
const SIRIELLE: &str = "e1000000-0000-4000-8000-000000000004";

fn wire(payload: Value) -> Row {
    serde_json::from_value(payload).expect("an object")
}

fn sirielle() -> Asked {
    Asked {
        session: SEANCE.to_owned(),
        character: SIRIELLE.to_owned(),
    }
}

#[test]
fn a_request_names_its_session_by_target_or_payload_trimmed_and_lowercase() {
    let upper = format!(" {} ", SIRIELLE.to_uppercase());
    // The session as `target.id`, as SceneBoard sends it.
    assert_eq!(
        asked(&wire(json!({ "character": upper })), Some(SEANCE)),
        Ok(sirielle())
    );
    // The session in the payload: the engine names it the target too.
    assert_eq!(
        asked(
            &wire(json!({ "session": SEANCE.to_uppercase(), "character": SIRIELLE })),
            None
        ),
        Ok(sirielle())
    );
    // Both, naming the same session, in either case.
    assert_eq!(
        asked(
            &wire(json!({ "session": SEANCE, "character": SIRIELLE })),
            Some(&SEANCE.to_uppercase())
        ),
        Ok(sirielle())
    );
    assert_eq!(
        asked(
            &wire(json!({ "session": null, "character": SIRIELLE })),
            Some(SEANCE)
        ),
        Ok(sirielle())
    );
}

#[test]
fn a_malformed_request_is_refused_naming_what_is_wrong() {
    let cases = [
        (
            json!({ "character": SIRIELLE, "present": true }),
            Some(SEANCE),
            Refusal::Unknown("present".to_owned()),
        ),
        (
            json!({ "character": SIRIELLE }),
            None,
            Refusal::InvalidSession,
        ),
        (
            json!({ "character": SIRIELLE }),
            Some("seance-4"),
            Refusal::InvalidSession,
        ),
        (
            json!({ "session": "seance-4", "character": SIRIELLE }),
            Some(SEANCE),
            Refusal::InvalidSession,
        ),
        (
            json!({ "session": 4, "character": SIRIELLE }),
            None,
            Refusal::InvalidSession,
        ),
        (
            json!({ "session": AUTRE, "character": SIRIELLE }),
            Some(SEANCE),
            Refusal::OtherSession,
        ),
        (json!({}), Some(SEANCE), Refusal::InvalidCharacter),
        (
            json!({ "character": "sirielle" }),
            Some(SEANCE),
            Refusal::InvalidCharacter,
        ),
        (
            json!({ "character": [SIRIELLE] }),
            Some(SEANCE),
            Refusal::InvalidCharacter,
        ),
    ];
    for (payload, target, refusal) in cases {
        assert_eq!(
            asked(&wire(payload.clone()), target),
            Err(refusal),
            "{payload}"
        );
    }
}

#[test]
fn a_pc_of_the_campaign_is_the_row_of_one_presence_ticked_now() {
    assert_eq!(
        prepare(&sirielle(), true, true, 1_761_300_000_123),
        Ok(Row::from([
            ("sessionPresentCharacterSessionId".to_owned(), json!(SEANCE)),
            (
                "sessionPresentCharacterCharacterId".to_owned(),
                json!(SIRIELLE)
            ),
            ("addedAt".to_owned(), json!(1_761_300_000_123_u64)),
        ]))
    );
}

#[test]
fn a_session_there_is_not_or_a_character_that_is_no_pc_of_it_is_refused() {
    assert_eq!(
        prepare(&sirielle(), false, true, 0),
        Err(Refusal::UnknownSession)
    );
    // With no session, its PCs are none: the session is what is missing.
    assert_eq!(
        prepare(&sirielle(), false, false, 0),
        Err(Refusal::UnknownSession)
    );
    assert_eq!(prepare(&sirielle(), true, false, 0), Err(Refusal::NotAPc));
    assert!(Refusal::NotAPc.to_string().starts_with("present-only-pj"));
}
