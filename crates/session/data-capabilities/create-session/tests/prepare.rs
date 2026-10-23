//! session.createSession's request, read and judged before anything is
//! queued: what the screen sends becomes the row of a session in prep, its
//! microphone off, ticking the PCs of its campaign; a malformed request, a
//! campaign there is not, a oneshot's second session
//! (`oneshot-single-session`) or a character that is no PC of the campaign
//! (`present-only-pj`) is refused.

use create_session::{Asked, Campaign, Refusal, Row, asked, prepare};
use serde_json::{Value, json};

const VALOMBRE: &str = "c1000000-0000-4000-8000-000000000001";
const ELOWEN: &str = "e1000000-0000-4000-8000-000000000001";
const KAEL: &str = "e1000000-0000-4000-8000-000000000002";
const SEANCE: &str = "51000000-0000-4000-8000-00000000000a";

fn wire(payload: Value) -> Row {
    serde_json::from_value(payload).expect("an object")
}

fn campagne(sessions: u64) -> Campaign {
    Campaign {
        kind: "campagne".to_owned(),
        sessions,
    }
}

fn oneshot(sessions: u64) -> Campaign {
    Campaign {
        kind: "oneshot".to_owned(),
        sessions,
    }
}

#[test]
fn a_request_is_read_trimmed_lowercase_and_each_pc_once() {
    assert_eq!(
        asked(&wire(json!({
            "campaign": format!(" {} ", VALOMBRE.to_uppercase()),
            "title": "  La séance 4 ",
            "plannedOn": " 2026-11-05 ",
            "present": [KAEL.to_uppercase(), ELOWEN, KAEL],
        }))),
        Ok(Asked {
            campaign: VALOMBRE.to_owned(),
            title: "La séance 4".to_owned(),
            planned_on: Some("2026-11-05".to_owned()),
            present: vec![KAEL.to_owned(), ELOWEN.to_owned()],
        })
    );
    // No day, no PCs: none.
    for payload in [
        json!({ "campaign": "cendres-valombre", "title": "La suite" }),
        json!({ "campaign": "cendres-valombre", "title": "La suite", "plannedOn": null, "present": null }),
    ] {
        assert_eq!(
            asked(&wire(payload)),
            Ok(Asked {
                campaign: "cendres-valombre".to_owned(),
                title: "La suite".to_owned(),
                planned_on: None,
                present: Vec::new(),
            })
        );
    }
}

#[test]
fn a_malformed_request_is_refused_naming_what_is_wrong() {
    let base = json!({ "campaign": VALOMBRE, "title": "La suite" });
    let with = |field: &str, value: Value| {
        let mut payload = base.clone();
        payload[field] = value;
        wire(payload)
    };
    let cases = [
        (
            with("status", json!("live")),
            Refusal::Unknown("status".to_owned()),
        ),
        (with("campaign", json!("  ")), Refusal::NoCampaign),
        (with("campaign", json!(4)), Refusal::NoCampaign),
        (wire(json!({ "title": "La suite" })), Refusal::NoCampaign),
        (with("title", json!(" ")), Refusal::NoTitle),
        (with("title", json!(4)), Refusal::NoTitle),
        (wire(json!({ "campaign": VALOMBRE })), Refusal::NoTitle),
        (with("plannedOn", json!("5 novembre")), Refusal::InvalidDate),
        (with("plannedOn", json!("2026/11/05")), Refusal::InvalidDate),
        (with("plannedOn", json!("2026-11-5")), Refusal::InvalidDate),
        (with("plannedOn", json!("2026-11-0x")), Refusal::InvalidDate),
        (with("plannedOn", json!(20_261_105)), Refusal::InvalidDate),
        (with("present", json!(ELOWEN)), Refusal::InvalidPresent),
        (
            with("present", json!({ "id": ELOWEN })),
            Refusal::InvalidPresent,
        ),
        (
            with("present", json!([ELOWEN, ""])),
            Refusal::InvalidPresent,
        ),
        (with("present", json!([ELOWEN, 4])), Refusal::InvalidPresent),
    ];
    for (payload, refusal) in cases {
        assert_eq!(asked(&payload), Err(refusal), "{payload:?}");
    }
}

#[test]
fn a_day_is_one_of_the_calendar() {
    let on = |day: &str| {
        asked(&wire(
            json!({ "campaign": VALOMBRE, "title": "La suite", "plannedOn": day }),
        ))
        .map(|asked| asked.planned_on)
    };
    for day in [
        "2026-01-31",
        "2026-03-31",
        "2026-05-31",
        "2026-07-31",
        "2026-08-31",
        "2026-10-31",
        "2026-12-31",
        "2026-04-30",
        "2026-06-30",
        "2026-09-30",
        "2026-11-30",
        "2026-02-28",
        "2024-02-29",
        "2000-02-29",
        "2026-11-01",
    ] {
        assert_eq!(on(day), Ok(Some(day.to_owned())), "{day}");
    }
    for day in [
        "2026-01-32",
        "2026-04-31",
        "2026-06-31",
        "2026-09-31",
        "2026-11-31",
        "2026-02-29",
        "1900-02-29",
        "2024-02-30",
        "2026-13-01",
        "2026-00-10",
        "2026-11-00",
    ] {
        assert_eq!(on(day), Err(Refusal::InvalidDate), "{day}");
    }
}

#[test]
fn status_lifecycle_a_session_starts_in_prep_its_microphone_off_ticking_its_pcs() {
    let request = asked(&wire(json!({
        "campaign": VALOMBRE,
        "title": "La séance 4",
        "plannedOn": "2026-11-05",
        "present": [ELOWEN, KAEL],
    })))
    .expect("read");
    let row = prepare(
        &request,
        SEANCE,
        Some(&campagne(3)),
        &[KAEL.to_owned(), ELOWEN.to_owned()],
    )
    .expect("prepared");
    assert_eq!(
        Value::Object(row.into_iter().collect()),
        json!({
            "sessionId": SEANCE,
            "sessionCampaignId": VALOMBRE,
            "sessionTitle": "La séance 4",
            "plannedOn": "2026-11-05",
            "sessionStatus": "prep",
            "microphoneOn": false,
            "sessionPresentCharacterCharacterId": [ELOWEN, KAEL],
        })
    );
    // No day: null; no PCs: an empty list, so none is present.
    let request = asked(&wire(json!({ "campaign": VALOMBRE, "title": "La suite" }))).expect("read");
    let row = prepare(&request, SEANCE, Some(&campagne(0)), &[]).expect("prepared");
    assert_eq!(row["plannedOn"], Value::Null);
    assert_eq!(row["sessionPresentCharacterCharacterId"], json!([]));
}

#[test]
fn a_campaign_there_is_not_is_refused_as_unknown() {
    let request = asked(&wire(
        json!({ "campaign": "nulle-part", "title": "La suite" }),
    ))
    .expect("read");
    assert_eq!(
        prepare(&request, SEANCE, None, &[]),
        Err(Refusal::UnknownCampaign)
    );
    assert_eq!(Refusal::UnknownCampaign.to_string(), "campagne introuvable");
}

#[test]
fn oneshot_single_session_refuses_a_second_session_of_a_oneshot() {
    let request = asked(&wire(json!({ "campaign": VALOMBRE, "title": "Encore" }))).expect("read");
    assert_eq!(
        prepare(&request, SEANCE, Some(&oneshot(1)), &[]),
        Err(Refusal::OneshotHasItsSession)
    );
    assert!(
        Refusal::OneshotHasItsSession
            .to_string()
            .starts_with("oneshot-single-session")
    );
    // A oneshot with none yet takes its one; a campagne takes any number.
    assert!(prepare(&request, SEANCE, Some(&oneshot(0)), &[]).is_ok());
    assert!(prepare(&request, SEANCE, Some(&campagne(12)), &[]).is_ok());
}

#[test]
fn present_only_pj_refuses_a_character_that_is_no_pc_of_the_campaign() {
    let request = asked(&wire(json!({
        "campaign": VALOMBRE,
        "title": "La suite",
        "present": [ELOWEN, KAEL],
    })))
    .expect("read");
    // Kael is no PC of the campaign the tables hold: an NPC, a PC of
    // another campaign, a slug the tables do not know.
    assert_eq!(
        prepare(&request, SEANCE, Some(&campagne(3)), &[ELOWEN.to_owned()]),
        Err(Refusal::NotAPc)
    );
    assert!(Refusal::NotAPc.to_string().starts_with("present-only-pj"));
}

#[test]
fn every_refusal_says_what_is_wrong_in_words() {
    for (refusal, words) in [
        (Refusal::Unknown("status".to_owned()), "status :"),
        (Refusal::NoCampaign, "campaign :"),
        (Refusal::NoTitle, "title :"),
        (Refusal::InvalidDate, "plannedOn :"),
        (Refusal::InvalidPresent, "present :"),
    ] {
        assert!(refusal.to_string().starts_with(words), "{refusal}");
    }
}
