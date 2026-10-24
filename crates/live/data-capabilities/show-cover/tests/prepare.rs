//! What a return to the cover becomes before it is submitted: the campaign
//! it names, the state it writes on its session's diffusion row, and what is
//! refused before it is queued.

use std::collections::BTreeMap;

use serde_json::{Value, json};
use show_cover::{Refusal, campaign, prepare};

const VALOMBRE: &str = "c1000000-0000-4000-8000-00000000000a";
const SEANCE: &str = "51000000-0000-4000-8000-00000000000a";
const NOW: &str = "2026-10-21T20:00:00.000Z";

fn wire(payload: Value) -> BTreeMap<String, Value> {
    let Value::Object(payload) = payload else {
        panic!("a payload is an object");
    };
    payload.into_iter().collect()
}

#[test]
fn a_return_names_its_campaign_in_lower_case() {
    for named in [VALOMBRE.to_string(), VALOMBRE.to_uppercase()] {
        assert_eq!(
            campaign(&wire(json!({ "campaign": named }))),
            Ok(VALOMBRE.to_string())
        );
    }
}

#[test]
fn a_return_naming_no_campaign_or_no_uuid_is_refused() {
    for (payload, refusal) in [
        (json!({}), Refusal::NoCampaign),
        (json!({ "campaign": null }), Refusal::NoCampaign),
        (json!({ "campaign": 12 }), Refusal::NotAnId),
        (json!({ "campaign": "Valombre" }), Refusal::NotAnId),
        // A uuid a character short, one a character long, one whose dash
        // is a digit, one whose digit is no hexadecimal.
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
        assert_eq!(campaign(&wire(payload.clone())), Err(refusal), "{payload}");
    }
}

#[test]
fn the_cover_is_written_with_neither_media_nor_instance_and_nothing_of_the_window() {
    let written = prepare(Some(SEANCE.to_string()), NOW).expect("prepared");
    assert_eq!(
        Value::Object(written.into_iter().collect()),
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
fn a_campaign_with_no_live_session_has_no_cover_to_show() {
    assert_eq!(prepare(None, NOW), Err(Refusal::NoLiveSession));
}

#[test]
fn each_refusal_says_why_in_the_dm_s_words() {
    for (refusal, why) in [
        (Refusal::NoCampaign, "campaign manque"),
        (Refusal::NotAnId, "campaign n'est pas un identifiant"),
        (Refusal::NoLiveSession, "aucune séance n'est en cours"),
    ] {
        assert_eq!(refusal.to_string(), why);
    }
}

#[test]
fn the_manifest_touches_what_the_cover_writes_and_nothing_of_the_window() {
    let manifest: Value =
        serde_json::from_str(include_str!("../data-capability.json")).expect("contract F");
    assert_eq!(manifest["dataCapability"], show_cover::DATA_CAPABILITY);
    let touched: Vec<String> = prepare(Some(SEANCE.to_string()), NOW)
        .expect("prepared")
        .into_keys()
        .map(|field| format!("Diffusion.{field}"))
        .collect();
    let mut declared: Vec<String> = manifest["touches"]
        .as_array()
        .expect("touches")
        .iter()
        .map(|field| field.as_str().expect("a field").to_string())
        .collect();
    declared.sort();
    assert_eq!(touched, declared);
}
