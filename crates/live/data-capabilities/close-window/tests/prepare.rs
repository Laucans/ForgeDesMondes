//! What a close of the floating window becomes before it is submitted: the
//! window's media, x, y and width emptied together on the live session's
//! diffusion row, and the stamp — never its base nor its framing — and what
//! is refused before it is queued.

use close_window::{DATA_CAPABILITY, Refusal, Row, campaign, prepare};
use serde_json::{Value, json};

const VALOMBRE: &str = "c1000000-0000-4000-8000-00000000000a";
const SEANCE: &str = "51000000-0000-4000-8000-00000000000a";
const INSTANCE: &str = "b1000000-0000-4000-8000-00000000000a";
const PORTRAIT: &str = "d1000000-0000-4000-8000-00000000000a";
const NOW: &str = "2026-10-22T20:00:00.000Z";

fn object(value: Value) -> Row {
    let Value::Object(value) = value else {
        panic!("an object");
    };
    value.into_iter().collect()
}

fn manifest() -> Value {
    serde_json::from_str(include_str!("../data-capability.json")).expect("contract F")
}

/// The live session's row showing the ford's battlemap, framed, the portrait
/// open in its window.
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

/// What every close writes: the four window fields emptied, and the stamp.
fn closing() -> Row {
    object(json!({
        "windowMediaId": null,
        "windowX": null,
        "windowY": null,
        "windowWidth": null,
        "updatedAt": NOW,
    }))
}

#[test]
fn the_manifest_updates_the_window_s_four_fields_under_window_fields_together() {
    let manifest = manifest();
    assert_eq!(manifest["dataCapability"], DATA_CAPABILITY);
    assert_eq!(manifest["effect"], "update");
    assert_eq!(manifest["target"]["aggregate"], "Diffusion");
    assert_eq!(
        manifest["touches"],
        json!([
            "Diffusion.windowMediaId",
            "Diffusion.windowX",
            "Diffusion.windowY",
            "Diffusion.windowWidth",
            "Diffusion.updatedAt",
        ])
    );
    assert_eq!(manifest["invariants"], json!(["window-fields-together"]));
    assert_eq!(manifest["mode"], "overwrite");
}

#[test]
fn a_close_empties_the_four_window_fields_and_stamps_and_writes_nothing_else() {
    let prepared = prepare(Some(SEANCE.to_string()), &open(), NOW).expect("prepared");
    assert_eq!(prepared.target_id.as_deref(), Some(SEANCE));
    // Neither the base (live.showCover's) nor the framing.
    assert_eq!(prepared.payload, closing());
    assert!(prepared.warnings.is_empty());
    assert!(prepared.then.is_empty());
}

#[test]
fn the_payload_empties_every_window_field_the_manifest_touches_and_only_those() {
    let manifest = manifest();
    let touched: Vec<String> = manifest["touches"]
        .as_array()
        .expect("touches")
        .iter()
        .map(|field| {
            field
                .as_str()
                .and_then(|field| field.strip_prefix("Diffusion."))
                .expect("a Diffusion field")
                .to_string()
        })
        .collect();
    let prepared = prepare(Some(SEANCE.to_string()), &open(), NOW).expect("prepared");
    let written: Vec<String> = prepared.payload.keys().cloned().collect();
    let mut expected = touched.clone();
    expected.sort();
    assert_eq!(written, expected);
}

#[test]
fn a_window_already_closed_is_closed_again_the_same_way() {
    let mut closed = open();
    for field in ["windowMediaId", "windowX", "windowY", "windowWidth"] {
        closed.insert(field.to_string(), Value::Null);
    }
    let again = prepare(Some(SEANCE.to_string()), &closed, NOW).expect("prepared");
    assert_eq!(again.payload, closing());
    assert_eq!(again.target_id.as_deref(), Some(SEANCE));
}

#[test]
fn a_campaign_with_no_live_session_is_refused() {
    assert_eq!(
        prepare(None, &open(), NOW).map(|p| p.payload),
        Err(Refusal::NoLiveSession)
    );
}

#[test]
fn a_session_nothing_was_shown_on_has_no_window_to_close() {
    for row in [
        Row::new(),
        object(json!({ "sessionId": null, "windowMediaId": null })),
    ] {
        assert_eq!(
            prepare(Some(SEANCE.to_string()), &row, NOW).map(|p| p.payload),
            Err(Refusal::NothingShown),
            "{row:?}"
        );
    }
}

#[test]
fn a_close_names_its_campaign_in_lower_case() {
    for named in [VALOMBRE.to_string(), VALOMBRE.to_uppercase()] {
        assert_eq!(
            campaign(&object(json!({ "campaign": named }))),
            Ok(VALOMBRE.to_string())
        );
    }
}

#[test]
fn a_close_naming_no_campaign_or_no_uuid_is_refused() {
    for (payload, refusal) in [
        (json!({}), Refusal::NoCampaign),
        (json!({ "campaign": null }), Refusal::NoCampaign),
        (json!({ "campaign": 12 }), Refusal::NotAnId),
        (json!({ "campaign": "Valombre" }), Refusal::NotAnId),
        // A uuid a character short, one a character long, one whose dash is
        // a digit, one whose digit is no hexadecimal.
        (json!({ "campaign": &VALOMBRE[1..] }), Refusal::NotAnId),
        (
            json!({ "campaign": format!("{VALOMBRE}0") }),
            Refusal::NotAnId,
        ),
        (
            json!({ "campaign": VALOMBRE.replacen('-', "0", 1) }),
            Refusal::NotAnId,
        ),
        (
            json!({ "campaign": VALOMBRE.replacen('c', "g", 1) }),
            Refusal::NotAnId,
        ),
    ] {
        assert_eq!(
            campaign(&object(payload.clone())),
            Err(refusal),
            "{payload}"
        );
    }
}

#[test]
fn every_refusal_says_why_in_french() {
    for (refusal, why) in [
        (Refusal::NoCampaign, "campaign manque"),
        (Refusal::NotAnId, "campaign n'est pas un identifiant"),
        (Refusal::NoLiveSession, "aucune séance n'est en cours"),
        (Refusal::NothingShown, "la vue joueurs n'a encore rien reçu"),
    ] {
        assert_eq!(refusal.to_string(), why);
    }
}
