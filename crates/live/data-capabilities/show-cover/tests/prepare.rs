//! What `prepare` makes of a return to the cover: the state of the live
//! session's diffusion row — the cover, no media and no instance as its
//! base, nothing of the window nor of the framing — and what is refused
//! before it is queued.

use std::collections::BTreeMap;

use serde_json::{Value, json};
use show_cover::{Refusal, campaign, names, prepare};

const VALOMBRE: &str = "c1000000-0000-4000-8000-000000000001";
const SEANCE: &str = "51000000-0000-4000-8000-000000000002";
const NOW: &str = "2026-10-20T20:00:00.000Z";

fn wire(payload: Value) -> BTreeMap<String, Value> {
    let Value::Object(payload) = payload else {
        panic!("a payload is an object");
    };
    payload.into_iter().collect()
}

fn manifest() -> Value {
    serde_json::from_str(include_str!("../data-capability.json")).expect("contract F")
}

#[test]
fn it_names_live_show_cover_with_or_without_its_version() {
    assert!(names("live.showCover"));
    assert!(names("live.showCover@1"));
    assert!(!names("live.showOnPlayerView"));
    assert!(!names("live.showCoverage"));
}

#[test]
fn the_campaign_is_the_uuid_the_payload_names_in_lower_case() {
    let upper = VALOMBRE.to_ascii_uppercase();
    assert_eq!(
        campaign(&wire(json!({ "campaign": upper }))),
        Ok(VALOMBRE.to_string())
    );
}

#[test]
fn a_payload_naming_no_campaign_or_no_uuid_is_refused() {
    assert_eq!(campaign(&wire(json!({}))), Err(Refusal::NoCampaign));
    assert_eq!(
        campaign(&wire(json!({ "campaign": null }))),
        Err(Refusal::NoCampaign)
    );
    // A uuid's digits too few, and a uuid's length with a letter no digit.
    let short = &VALOMBRE[..8];
    let lettered = VALOMBRE.replace('c', "z");
    for not_an_id in [
        json!("valombre"),
        json!(short),
        json!(lettered),
        json!(42),
        json!([VALOMBRE]),
    ] {
        assert_eq!(
            campaign(&wire(json!({ "campaign": not_an_id }))),
            Err(Refusal::NotAnId),
            "{not_an_id}"
        );
    }
}

#[test]
fn a_campaign_with_no_live_session_is_refused() {
    assert_eq!(prepare(None, NOW), Err(Refusal::NoLiveSession));
    assert_eq!(
        Refusal::NoLiveSession.to_string(),
        "aucune séance n'est en cours"
    );
}

#[test]
fn the_cover_empties_the_base_on_the_live_session_s_row() {
    let prepared = prepare(Some(SEANCE.to_string()), NOW).expect("a live session");
    assert_eq!(prepared.target_id, SEANCE);
    // base-coherent: a cover has neither a media nor an instance;
    // single-diffusion-source: the row is the session's, the one targeted.
    assert_eq!(
        Value::Object(prepared.payload.into_iter().collect()),
        json!({
            "sessionId": SEANCE,
            "baseKind": "cover",
            "baseMediaId": null,
            "baseInstanceId": null,
            "updatedAt": NOW,
        })
    );
}

#[test]
fn the_state_written_is_what_the_manifest_touches_and_no_window_nor_framing() {
    let prepared = prepare(Some(SEANCE.to_string()), NOW).expect("a live session");
    let manifest = manifest();
    let touched: Vec<String> = manifest["touches"]
        .as_array()
        .expect("touches")
        .iter()
        .map(|field| {
            let field = field.as_str().expect("a field");
            field
                .strip_prefix("Diffusion.")
                .expect("Diffusion's")
                .to_string()
        })
        .collect();
    let mut written: Vec<String> = prepared.payload.keys().cloned().collect();
    let mut expected = touched.clone();
    written.sort();
    expected.sort();
    assert_eq!(written, expected);
    for untouched in [
        "windowMediaId",
        "windowX",
        "windowY",
        "windowWidth",
        "viewZoom",
        "viewPanX",
        "viewPanY",
    ] {
        assert!(
            !touched.iter().any(|field| field == untouched),
            "{untouched}"
        );
    }
}

#[test]
fn the_manifest_upserts_the_diffusion_under_its_two_invariants() {
    let manifest = manifest();
    assert_eq!(manifest["dataCapability"], "live.showCover");
    assert_eq!(manifest["effect"], "upsert");
    assert_eq!(manifest["target"]["aggregate"], "Diffusion");
    assert_eq!(
        manifest["invariants"],
        json!(["base-coherent", "single-diffusion-source"])
    );
}
