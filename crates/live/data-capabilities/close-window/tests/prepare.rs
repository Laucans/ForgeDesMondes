//! What `prepare` makes of a close of the floating window: the window's
//! media, x, y and width emptied together on the live session's diffusion
//! row, and the stamp — never its base nor its framing — and what is refused
//! before it is queued.

use close_window::{Refusal, Row, campaign, names, prepare};
use serde_json::{Value, json};

const VALOMBRE: &str = "c1000000-0000-4000-8000-000000000001";
const SEANCE: &str = "51000000-0000-4000-8000-000000000002";
const INSTANCE: &str = "b1000000-0000-4000-8000-000000000002";
const PORTRAIT: &str = "d1000000-0000-4000-8000-000000000002";
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

/// The live session's row showing the tavern's battlemap, framed, the
/// portrait open in its window.
fn open() -> Row {
    object(json!({
        "sessionId": SEANCE,
        "baseKind": "battlemap",
        "baseMediaId": null,
        "baseInstanceId": INSTANCE,
        "viewZoom": 1.5,
        "windowMediaId": PORTRAIT,
        "windowX": 0.6,
        "windowY": 0.12,
        "windowWidth": 0.3,
    }))
}

#[test]
fn it_names_live_close_window_with_or_without_its_version() {
    assert!(names("live.closeWindow"));
    assert!(names("live.closeWindow@1"));
    assert!(!names("live.moveWindow@1"));
    assert!(!names("live.closeWindowNow"));
}

#[test]
fn a_close_empties_the_four_window_fields_and_stamps_and_writes_nothing_else() {
    let prepared = prepare(Some(SEANCE.to_string()), &open(), NOW).expect("prepared");
    assert_eq!(prepared.target_id, SEANCE);
    // Neither the base (live.showCover's) nor the framing.
    assert_eq!(
        prepared.payload,
        object(json!({
            "windowMediaId": null,
            "windowX": null,
            "windowY": null,
            "windowWidth": null,
            "updatedAt": NOW,
        }))
    );
}

#[test]
fn a_window_already_closed_is_closed_again() {
    let mut closed = open();
    for field in ["windowMediaId", "windowX", "windowY", "windowWidth"] {
        closed.insert(field.to_string(), Value::Null);
    }
    let again = prepare(Some(SEANCE.to_string()), &closed, NOW).expect("prepared");
    let first = prepare(Some(SEANCE.to_string()), &open(), NOW).expect("prepared");
    assert_eq!(again, first);
}

#[test]
fn a_campaign_with_no_live_session_is_refused() {
    assert_eq!(prepare(None, &open(), NOW), Err(Refusal::NoLiveSession));
}

#[test]
fn a_session_nothing_was_shown_on_has_no_window_to_close() {
    for row in [Row::new(), object(json!({ "sessionId": null }))] {
        assert_eq!(
            prepare(Some(SEANCE.to_string()), &row, NOW),
            Err(Refusal::NothingShown),
            "{row:?}"
        );
    }
}

#[test]
fn the_payload_names_the_campaign_in_lower_case() {
    let wire = object(json!({ "campaign": VALOMBRE.to_uppercase() }));
    assert_eq!(campaign(&wire), Ok(VALOMBRE.to_string()));
}

#[test]
fn a_payload_naming_no_campaign_or_no_uuid_is_refused() {
    for (wire, refusal) in [
        (json!({}), Refusal::NoCampaign),
        (json!({ "campaign": null }), Refusal::NoCampaign),
        (json!({ "campaign": "valombre" }), Refusal::NotAnId),
        // A uuid's digits too few, and a uuid's length with a letter no digit.
        (json!({ "campaign": &VALOMBRE[..8] }), Refusal::NotAnId),
        (
            json!({ "campaign": VALOMBRE.replace('c', "z") }),
            Refusal::NotAnId,
        ),
        (json!({ "campaign": 7 }), Refusal::NotAnId),
    ] {
        assert_eq!(campaign(&object(wire.clone())), Err(refusal), "{wire}");
    }
}

#[test]
fn every_refusal_says_why_in_the_dm_s_words() {
    for (refusal, said) in [
        (Refusal::NoCampaign, "campaign manque"),
        (Refusal::NotAnId, "campaign n'est pas un identifiant"),
        (Refusal::NoLiveSession, "aucune séance n'est en cours"),
        (
            Refusal::NothingShown,
            "rien n'est diffusé aux joueurs : aucune fenêtre à fermer",
        ),
    ] {
        assert_eq!(refusal.to_string(), said);
    }
}

#[test]
fn the_manifest_updates_the_window_s_four_fields_under_window_fields_together() {
    let manifest = manifest();
    assert_eq!(manifest["dataCapability"], "live.closeWindow");
    assert_eq!(manifest["effect"], "update");
    assert_eq!(manifest["mode"], "overwrite");
    assert_eq!(manifest["target"]["aggregate"], "Diffusion");
    assert_eq!(
        manifest["touches"],
        json!([
            "Diffusion.windowMediaId",
            "Diffusion.windowX",
            "Diffusion.windowY",
            "Diffusion.windowWidth",
            "Diffusion.updatedAt"
        ])
    );
    assert_eq!(manifest["invariants"], json!(["window-fields-together"]));
    assert_eq!(
        manifest["payload"]
            .as_object()
            .expect("a payload")
            .keys()
            .collect::<Vec<_>>(),
        ["campaign"]
    );
}
