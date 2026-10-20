//! What `prepare` makes of a move of the floating window: the window's x and
//! y on the live session's diffusion row, brought onto the screen, and the
//! stamp — never its width nor its media — and what is refused before it is
//! queued.

use move_window::{Named, Refusal, Row, named, names, prepare};
use serde_json::{Value, json};

const VALOMBRE: &str = "c1000000-0000-4000-8000-000000000001";
const SEANCE: &str = "51000000-0000-4000-8000-000000000002";
const NOW: &str = "2026-10-20T20:00:00.000Z";

fn object(value: Value) -> Row {
    let Value::Object(value) = value else {
        panic!("an object");
    };
    value.into_iter().collect()
}

fn manifest() -> Value {
    serde_json::from_str(include_str!("../data-capability.json")).expect("contract F")
}

fn to(x: f64, y: f64) -> Named {
    Named {
        campaign: VALOMBRE.to_string(),
        x,
        y,
    }
}

#[test]
fn it_names_live_move_window_with_or_without_its_version() {
    assert!(names("live.moveWindow"));
    assert!(names("live.moveWindow@1"));
    assert!(!names("live.resizeWindow@1"));
    assert!(!names("live.moveWindowAgain"));
}

#[test]
fn a_move_writes_x_y_and_the_stamp_and_nothing_else_on_the_live_session_s_row() {
    let prepared = prepare(&to(0.12, 0.08), Some(SEANCE.to_string()), NOW).expect("prepared");
    assert_eq!(prepared.target_id, SEANCE);
    // Neither the width (live.resizeWindow's) nor the media.
    assert_eq!(
        prepared.payload,
        object(json!({ "windowX": 0.12, "windowY": 0.08, "updatedAt": NOW }))
    );
}

#[test]
fn a_position_off_the_screen_is_brought_back_to_its_edge() {
    for ((x, y), (to_x, to_y)) in [
        ((120.0, 80.0), (1.0, 1.0)),
        ((-0.5, 0.4), (0.0, 0.4)),
        ((0.0, 1.0), (0.0, 1.0)),
    ] {
        let prepared = prepare(&to(x, y), Some(SEANCE.to_string()), NOW).expect("prepared");
        assert_eq!(prepared.payload["windowX"], to_x, "{x}");
        assert_eq!(prepared.payload["windowY"], to_y, "{y}");
    }
}

#[test]
fn a_campaign_with_no_live_session_is_refused() {
    assert_eq!(
        prepare(&to(0.12, 0.08), None, NOW),
        Err(Refusal::NoLiveSession)
    );
}

#[test]
fn the_payload_names_the_campaign_in_lower_case_and_the_position() {
    let wire = object(json!({
        "campaign": VALOMBRE.to_uppercase(),
        "x": 120,
        "y": 0.08,
    }));
    assert_eq!(named(&wire), Ok(to(120.0, 0.08)));
}

#[test]
fn a_payload_naming_no_campaign_or_no_position_is_refused() {
    for (wire, refusal) in [
        (json!({ "x": 0.1, "y": 0.1 }), Refusal::NoCampaign),
        (
            json!({ "campaign": null, "x": 0.1, "y": 0.1 }),
            Refusal::NoCampaign,
        ),
        (
            json!({ "campaign": "valombre", "x": 0.1, "y": 0.1 }),
            Refusal::NotAnId,
        ),
        (
            json!({ "campaign": 7, "x": 0.1, "y": 0.1 }),
            Refusal::NotAnId,
        ),
        (
            json!({ "campaign": VALOMBRE, "y": 0.1 }),
            Refusal::NoPosition,
        ),
        (
            json!({ "campaign": VALOMBRE, "x": 0.1, "y": null }),
            Refusal::NoPosition,
        ),
        (
            json!({ "campaign": VALOMBRE, "x": "0.1", "y": 0.1 }),
            Refusal::NotANumber,
        ),
        (
            json!({ "campaign": VALOMBRE, "x": 0.1, "y": true }),
            Refusal::NotANumber,
        ),
    ] {
        assert_eq!(named(&object(wire.clone())), Err(refusal), "{wire}");
    }
}

#[test]
fn every_refusal_says_why_in_the_dm_s_words() {
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

#[test]
fn the_manifest_updates_the_window_s_position_under_its_two_invariants() {
    let manifest = manifest();
    assert_eq!(manifest["dataCapability"], "live.moveWindow");
    assert_eq!(manifest["effect"], "update");
    assert_eq!(manifest["mode"], "overwrite");
    assert_eq!(manifest["target"]["aggregate"], "Diffusion");
    assert_eq!(
        manifest["touches"],
        json!([
            "Diffusion.windowX",
            "Diffusion.windowY",
            "Diffusion.updatedAt"
        ])
    );
    assert_eq!(
        manifest["invariants"],
        json!(["window-fields-together", "single-diffusion-source"])
    );
    assert_eq!(
        manifest["payload"]
            .as_object()
            .expect("a payload")
            .keys()
            .collect::<Vec<_>>(),
        ["campaign", "x", "y"]
    );
}
