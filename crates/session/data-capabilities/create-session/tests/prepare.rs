//! What a new session becomes before it is submitted: the session at the
//! position it is given, in prep with its microphone off, its PCs present
//! each once — or a refusal, for what the tables could never hold.

use create_session::{Fresh, PREP, PRESENT, Refusal, Row, campaign_of, names, prepare};
use serde_json::{Value, json};

const VALOMBRE: &str = "c1000000-0000-4000-8000-000000000001";
const SEANCE: &str = "51000000-0000-4000-8000-000000000004";
const ELOWEN: &str = "e1000000-0000-4000-8000-000000000001";
const KAEL: &str = "e1000000-0000-4000-8000-000000000002";
const NOW: &str = "2026-10-20T18:00:00.000Z";

fn wire(payload: Value) -> Row {
    let Value::Object(payload) = payload else {
        panic!("a payload is an object");
    };
    payload.into_iter().collect()
}

fn fresh(position: u64) -> Fresh<'static> {
    Fresh {
        session: SEANCE,
        position,
        now: NOW,
    }
}

#[test]
fn a_new_session_is_at_its_position_in_prep_its_microphone_off_with_the_pcs_ticked() {
    let prepared = prepare(
        &wire(json!({
            "campaign": VALOMBRE.to_uppercase(),
            "title": "  La tour  ",
            "plannedOn": "2028-02-29",
            "present": [ELOWEN, KAEL, ELOWEN.to_uppercase(), KAEL],
        })),
        fresh(4),
    )
    .expect("prepared");
    assert_eq!(prepared.target_id, SEANCE);
    assert_eq!(
        Value::Object(prepared.payload.into_iter().collect()),
        json!({
            "sessionId": SEANCE,
            "sessionCampaignId": VALOMBRE,
            "sessionPosition": 4,
            "sessionTitle": "La tour",
            "plannedOn": "2028-02-29",
            "sessionStatus": PREP,
            "microphoneOn": false,
            "sessionPresentCharacterSessionId": SEANCE,
            PRESENT: [ELOWEN, KAEL],
            "addedAt": NOW,
        })
    );
}

#[test]
fn no_day_and_no_pc_given_is_a_session_planned_for_no_day_with_no_one_present() {
    for payload in [
        json!({ "campaign": VALOMBRE, "title": "Une" }),
        json!({ "campaign": VALOMBRE, "title": "Une", "plannedOn": null, "present": null }),
        json!({ "campaign": VALOMBRE, "title": "Une", "plannedOn": " ", "present": [] }),
    ] {
        let prepared = prepare(&wire(payload.clone()), fresh(1)).expect("prepared");
        assert_eq!(prepared.payload["plannedOn"], Value::Null, "{payload}");
        assert_eq!(prepared.payload[PRESENT], json!([]), "{payload}");
        assert_eq!(prepared.payload["sessionPosition"], json!(1), "{payload}");
    }
}

#[test]
fn a_session_the_tables_could_never_hold_is_refused_saying_why() {
    for (payload, refusal) in [
        (json!({ "title": "Une" }), Refusal::NoCampaign),
        (
            json!({ "campaign": "cendres-valombre", "title": "Une" }),
            Refusal::NoCampaign,
        ),
        (json!({ "campaign": VALOMBRE }), Refusal::NoTitle),
        (
            json!({ "campaign": VALOMBRE, "title": 4 }),
            Refusal::NoTitle,
        ),
        (
            json!({ "campaign": VALOMBRE, "title": " \t" }),
            Refusal::NoTitle,
        ),
        (
            json!({ "campaign": VALOMBRE, "title": "Une", "plannedOn": "2026-02-29" }),
            Refusal::NotADate,
        ),
        (
            json!({ "campaign": VALOMBRE, "title": "Une", "plannedOn": "2026-13-01" }),
            Refusal::NotADate,
        ),
        (
            json!({ "campaign": VALOMBRE, "title": "Une", "plannedOn": "7 novembre" }),
            Refusal::NotADate,
        ),
        (
            json!({ "campaign": VALOMBRE, "title": "Une", "plannedOn": 20261107 }),
            Refusal::NotADate,
        ),
        (
            json!({ "campaign": VALOMBRE, "title": "Une", "present": ELOWEN }),
            Refusal::NotPresent,
        ),
        (
            json!({ "campaign": VALOMBRE, "title": "Une", "present": [ELOWEN, null] }),
            Refusal::NotPresent,
        ),
        (
            json!({ "campaign": VALOMBRE, "title": "Une\u{0}" }),
            Refusal::Nul,
        ),
        (
            json!({ "campaign": VALOMBRE, "title": "Une", "present": ["\u{0}"] }),
            Refusal::Nul,
        ),
    ] {
        assert_eq!(
            prepare(&wire(payload.clone()), fresh(1)),
            Err(refusal),
            "{payload}"
        );
    }
}

#[test]
fn a_pc_that_is_no_id_is_kept_as_given_for_the_dataguard_to_judge() {
    let prepared = prepare(
        &wire(json!({ "campaign": VALOMBRE, "title": "Une", "present": ["Elowen", "Elowen"] })),
        fresh(1),
    )
    .expect("prepared");
    assert_eq!(prepared.payload[PRESENT], json!(["Elowen"]));
}

#[test]
fn the_capability_is_named_with_or_without_its_version_and_its_campaign_by_a_uuid() {
    assert!(names("session.createSession"));
    assert!(names("session.createSession@1"));
    assert!(!names("session.createScene@1"));
    assert_eq!(
        campaign_of(&wire(json!({ "campaign": VALOMBRE.to_uppercase() }))),
        Some(VALOMBRE.to_owned())
    );
    assert_eq!(
        campaign_of(&wire(json!({ "campaign": "cendres-valombre" }))),
        None
    );
}
