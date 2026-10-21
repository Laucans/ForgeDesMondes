//! What a drag becomes before it is submitted: the campaign and the position
//! it names, the state it writes on its session's diffusion row, and what is
//! refused before it is queued.

use std::collections::BTreeMap;

use move_window::{Named, Refusal, named, prepare};
use serde_json::{Value, json};

const VALOMBRE: &str = "c1000000-0000-4000-8000-00000000000a";
const SEANCE: &str = "51000000-0000-4000-8000-00000000000a";
const NOW: &str = "2026-10-21T20:00:00.000Z";

fn wire(payload: Value) -> BTreeMap<String, Value> {
    let Value::Object(payload) = payload else {
        panic!("a payload is an object");
    };
    payload.into_iter().collect()
}

fn at(x: f64, y: f64) -> Named {
    Named {
        campaign: VALOMBRE.to_string(),
        x,
        y,
    }
}

#[test]
fn a_drag_names_its_campaign_in_lower_case_and_its_position() {
    for campaign in [VALOMBRE.to_string(), VALOMBRE.to_uppercase()] {
        assert_eq!(
            named(&wire(json!({ "campaign": campaign, "x": 0.25, "y": 1 }))),
            Ok(at(0.25, 1.0))
        );
    }
}

#[test]
fn a_drag_naming_no_campaign_no_uuid_or_no_position_is_refused() {
    for (payload, refusal) in [
        (json!({ "x": 0.1, "y": 0.2 }), Refusal::NoCampaign),
        (
            json!({ "campaign": null, "x": 0.1, "y": 0.2 }),
            Refusal::NoCampaign,
        ),
        (
            json!({ "campaign": 12, "x": 0.1, "y": 0.2 }),
            Refusal::NotAnId,
        ),
        (
            json!({ "campaign": "Valombre", "x": 0.1, "y": 0.2 }),
            Refusal::NotAnId,
        ),
        // A uuid a character short, one a character long, one whose dash
        // is a digit, one whose digit is no hexadecimal.
        (
            json!({ "campaign": &VALOMBRE[1..], "x": 0.1, "y": 0.2 }),
            Refusal::NotAnId,
        ),
        (
            json!({ "campaign": format!("{VALOMBRE}0"), "x": 0.1, "y": 0.2 }),
            Refusal::NotAnId,
        ),
        (
            json!({ "campaign": VALOMBRE.replacen('-', "0", 1), "x": 0.1, "y": 0.2 }),
            Refusal::NotAnId,
        ),
        (
            json!({ "campaign": VALOMBRE.replacen('c', "g", 1), "x": 0.1, "y": 0.2 }),
            Refusal::NotAnId,
        ),
        (
            json!({ "campaign": VALOMBRE, "y": 0.2 }),
            Refusal::NoPosition,
        ),
        (
            json!({ "campaign": VALOMBRE, "x": 0.1, "y": null }),
            Refusal::NoPosition,
        ),
        (
            json!({ "campaign": VALOMBRE, "x": "0.1", "y": 0.2 }),
            Refusal::NotANumber,
        ),
        (
            json!({ "campaign": VALOMBRE, "x": 0.1, "y": true }),
            Refusal::NotANumber,
        ),
    ] {
        assert_eq!(named(&wire(payload.clone())), Err(refusal), "{payload}");
    }
}

#[test]
fn a_drag_writes_x_y_and_the_stamp_alone_on_the_sessions_row() {
    let prepared = prepare(&at(0.25, 0.5), Some(SEANCE.to_string()), NOW).expect("prepared");
    assert_eq!(prepared.target_id.as_deref(), Some(SEANCE));
    // Nothing of the width, the media or the base: they stay as the row
    // holds them.
    assert_eq!(
        Value::Object(prepared.payload.into_iter().collect()),
        json!({ "windowX": 0.25, "windowY": 0.5, "updatedAt": NOW })
    );
    assert!(prepared.then.is_empty() && prepared.warnings.is_empty());
}

#[test]
fn a_position_is_brought_onto_the_screen_to_four_decimals() {
    for ((x, y), (on_x, on_y)) in [
        ((120.0, 80.0), (1.0, 1.0)),
        ((-0.5, 1.5), (0.0, 1.0)),
        ((0.0, 1.0), (0.0, 1.0)),
        // To the 4 decimals the table keeps, so the engine's row says what
        // the table does.
        ((0.123_456, 0.999_96), (0.1235, 1.0)),
        ((0.000_04, 0.250_06), (0.0, 0.2501)),
    ] {
        let prepared = prepare(&at(x, y), Some(SEANCE.to_string()), NOW).expect("prepared");
        assert_eq!(prepared.payload["windowX"], json!(on_x), "{x}");
        assert_eq!(prepared.payload["windowY"], json!(on_y), "{y}");
    }
}

#[test]
fn a_campaign_no_live_session_shows_is_refused() {
    assert_eq!(
        prepare(&at(0.1, 0.2), None, NOW).map(|_| ()),
        Err(Refusal::NoLiveSession)
    );
}

#[test]
fn each_refusal_says_why_in_the_dms_words() {
    for (refusal, said) in [
        (Refusal::NoCampaign, "campaign manque"),
        (Refusal::NotAnId, "campaign n'est pas un identifiant"),
        (Refusal::NoPosition, "x ou y manque"),
        (Refusal::NotANumber, "x ou y n'est pas un nombre"),
        (Refusal::NoLiveSession, "aucune séance n'est en cours"),
    ] {
        assert_eq!(refusal.to_string(), said);
    }
}
