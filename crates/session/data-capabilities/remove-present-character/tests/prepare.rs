//! What session.removePresentCharacter makes of a request, before anything
//! is queued: its session and its PC read as uuids, the session's own
//! partition never the one deleted, and a session the tables do not hold
//! refused as not found.

use aggregate_session::{presence_id, presence_of};
use remove_present_character::{Asked, Refusal, Row, asked, prepare};
use serde_json::{Value, json};

const SEANCE: &str = "51000000-0000-4000-8000-00000000000a";
const AUTRE: &str = "51000000-0000-4000-8000-00000000000b";
const ELOWEN: &str = "e1000000-0000-4000-8000-000000000001";

fn wire(payload: Value) -> Row {
    serde_json::from_value(payload).expect("an object")
}

fn elowen_at(session: &str) -> Asked {
    Asked {
        session: session.to_owned(),
        character: ELOWEN.to_owned(),
    }
}

#[test]
fn the_session_is_the_target_s_or_the_payload_s_and_both_are_read_lowercase() {
    let upper = ELOWEN.to_uppercase();
    assert_eq!(
        asked(&wire(json!({ "character": upper })), Some(SEANCE)),
        Ok(elowen_at(SEANCE))
    );
    assert_eq!(
        asked(
            &wire(
                json!({ "session": format!(" {} ", SEANCE.to_uppercase()), "character": ELOWEN })
            ),
            None
        ),
        Ok(elowen_at(SEANCE))
    );
    // Named twice, the same session.
    assert_eq!(
        asked(
            &wire(json!({ "session": SEANCE.to_uppercase(), "character": ELOWEN })),
            Some(SEANCE)
        ),
        Ok(elowen_at(SEANCE))
    );
    // A null session is none: the target's.
    assert_eq!(
        asked(
            &wire(json!({ "session": null, "character": ELOWEN })),
            Some(SEANCE)
        ),
        Ok(elowen_at(SEANCE))
    );
}

#[test]
fn a_request_naming_no_uuid_another_session_or_another_field_is_malformed() {
    let cases = [
        (
            json!({ "character": ELOWEN }),
            None,
            Refusal::InvalidSession,
        ),
        (
            json!({ "character": ELOWEN }),
            Some("s4"),
            Refusal::InvalidSession,
        ),
        (
            json!({ "session": "s4", "character": ELOWEN }),
            Some(SEANCE),
            Refusal::InvalidSession,
        ),
        (
            json!({ "session": 4, "character": ELOWEN }),
            None,
            Refusal::InvalidSession,
        ),
        (
            json!({ "session": AUTRE, "character": ELOWEN }),
            Some(SEANCE),
            Refusal::OtherSession,
        ),
        (json!({}), Some(SEANCE), Refusal::InvalidCharacter),
        (
            json!({ "character": "elowen" }),
            Some(SEANCE),
            Refusal::InvalidCharacter,
        ),
        (
            json!({ "character": [ELOWEN] }),
            Some(SEANCE),
            Refusal::InvalidCharacter,
        ),
        (
            json!({ "character": ELOWEN, "id": ELOWEN }),
            Some(SEANCE),
            Refusal::Unknown("id".to_owned()),
        ),
    ];
    for (payload, target, refusal) in cases {
        assert_eq!(
            asked(&wire(payload.clone()), target),
            Err(refusal),
            "{payload} on {target:?}"
        );
    }
}

#[test]
fn the_partition_deleted_is_the_presence_s_never_the_session_s() {
    let asked = elowen_at(SEANCE);
    let id = prepare(&asked, true).expect("a live session");
    assert_eq!(id, format!("{SEANCE}.{ELOWEN}"));
    // The row session.addPresentCharacter ticks.
    assert_eq!(id, presence_id(SEANCE, ELOWEN));
    assert_ne!(id, SEANCE);
    // The aggregate reads the session and the PC back from it.
    assert_eq!(
        presence_of(&id),
        Some((SEANCE.to_owned(), ELOWEN.to_owned()))
    );
    assert_eq!(prepare(&asked, false), Err(Refusal::UnknownSession));
}

#[test]
fn each_refusal_says_what_is_wrong() {
    for (refusal, says) in [
        (Refusal::Unknown("id".to_owned()), "id : "),
        (Refusal::InvalidSession, "session : "),
        (Refusal::OtherSession, "session : celle que target.id"),
        (Refusal::InvalidCharacter, "character : "),
        (Refusal::UnknownSession, "séance introuvable"),
    ] {
        let message = refusal.to_string();
        assert!(message.starts_with(says), "{message}");
    }
}

#[test]
fn the_manifest_is_a_delete_on_the_session_aggregate() {
    let manifest: Value =
        serde_json::from_str(include_str!("../data-capability.json")).expect("JSON");
    assert_eq!(manifest["dataCapability"], "session.removePresentCharacter");
    assert_eq!(manifest["effect"], "delete");
    assert_eq!(manifest["target"]["aggregate"], "Session");
    assert_eq!(manifest["target"]["id"], "$session");
    assert_eq!(manifest["invariants"], json!(["present-only-pj"]));
    harness_dataguard::manifest::DataCapability::parse(include_str!("../data-capability.json"))
        .expect("contract F");
}
