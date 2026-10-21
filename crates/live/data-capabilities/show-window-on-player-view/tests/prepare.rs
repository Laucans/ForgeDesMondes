//! What opening a window becomes before it is submitted: what it names, the
//! state it writes on its session's diffusion row, and what is refused
//! before it is queued.

use serde_json::{Value, json};
use show_window_on_player_view::{
    Found, NOT_AN_ILLUSTRATION, Named, Refusal, Row, WIDTH, X, Y, named, prepare,
};

const VALOMBRE: &str = "c1000000-0000-4000-8000-00000000000a";
const PORTRAIT: &str = "d1000000-0000-4000-8000-00000000000a";
const SEANCE: &str = "51000000-0000-4000-8000-00000000000a";
const NOW: &str = "2026-10-21T20:00:00.000Z";

fn wire(payload: Value) -> Row {
    let Value::Object(payload) = payload else {
        panic!("a payload is an object");
    };
    payload.into_iter().collect()
}

fn portrait() -> Named {
    Named {
        campaign: VALOMBRE.to_string(),
        media: PORTRAIT.to_string(),
    }
}

/// The tables holding Valombre's live session and its portrait, an
/// illustration of it when `illustration`.
fn found(illustration: bool) -> Found {
    Found {
        session: Some(SEANCE.to_string()),
        diffused: true,
        held: true,
        illustration,
    }
}

#[test]
fn a_window_names_its_campaign_and_its_media_in_lower_case() {
    for (campaign, media) in [
        (VALOMBRE.to_string(), PORTRAIT.to_string()),
        (VALOMBRE.to_uppercase(), PORTRAIT.to_uppercase()),
    ] {
        assert_eq!(
            named(&wire(json!({ "campaign": campaign, "media": media }))),
            Ok(portrait())
        );
    }
}

#[test]
fn a_window_naming_no_campaign_no_media_or_no_uuid_is_refused() {
    for (payload, refusal) in [
        (json!({ "media": PORTRAIT }), Refusal::NoCampaign),
        (
            json!({ "campaign": null, "media": PORTRAIT }),
            Refusal::NoCampaign,
        ),
        (json!({ "campaign": VALOMBRE }), Refusal::NoMedia),
        (
            json!({ "campaign": VALOMBRE, "media": null }),
            Refusal::NoMedia,
        ),
        (
            json!({ "campaign": 12, "media": PORTRAIT }),
            Refusal::NotAnId("campaign"),
        ),
        (
            json!({ "campaign": VALOMBRE, "media": "Morlox" }),
            Refusal::NotAnId("media"),
        ),
        // A uuid a character short, one a character long, one whose dash
        // is a digit, one whose digit is no hexadecimal.
        (
            json!({ "campaign": VALOMBRE, "media": &PORTRAIT[1..] }),
            Refusal::NotAnId("media"),
        ),
        (
            json!({ "campaign": VALOMBRE, "media": format!("{PORTRAIT}0") }),
            Refusal::NotAnId("media"),
        ),
        (
            json!({ "campaign": VALOMBRE, "media": PORTRAIT.replacen('-', "0", 1) }),
            Refusal::NotAnId("media"),
        ),
        (
            json!({ "campaign": VALOMBRE, "media": PORTRAIT.replacen('d', "g", 1) }),
            Refusal::NotAnId("media"),
        ),
    ] {
        assert_eq!(named(&wire(payload.clone())), Err(refusal), "{payload}");
    }
}

#[test]
fn an_illustration_opens_at_its_default_place_and_width_the_base_left_as_it_is() {
    let prepared = prepare(&portrait(), &found(true), NOW).expect("prepared");
    assert_eq!(prepared.target_id, SEANCE);
    assert_eq!(prepared.warning, None);
    assert_eq!(
        Value::Object(prepared.payload.into_iter().collect()),
        json!({
            "windowMediaId": PORTRAIT,
            "windowX": X,
            "windowY": Y,
            "windowWidth": WIDTH,
            "updatedAt": NOW,
        })
    );
}

#[test]
fn the_default_window_sits_on_the_screen_with_a_width_its_height_can_follow() {
    for value in [X, Y, WIDTH] {
        assert!(value > 0.0 && value < 1.0, "{value}");
    }
    const { assert!(X + WIDTH <= 1.0, "the window ends on the screen") };
}

#[test]
fn a_session_whose_diffusion_is_not_committed_is_refused_and_nothing_is_written() {
    let undiffused = Found {
        diffused: false,
        ..found(true)
    };
    assert_eq!(
        prepare(&portrait(), &undiffused, NOW),
        Err(Refusal::NoDiffusion)
    );
}

#[test]
fn a_media_with_no_proportions_to_keep_gets_no_width_and_a_warning() {
    let prepared = prepare(&portrait(), &found(false), NOW).expect("prepared");
    assert_eq!(prepared.payload["windowWidth"], json!(0.0));
    assert_eq!(prepared.payload["windowMediaId"], json!(PORTRAIT));
    assert_eq!(prepared.warning, Some(NOT_AN_ILLUSTRATION));
}

#[test]
fn a_campaign_with_no_live_session_or_a_media_not_held_is_refused() {
    let none = Found {
        session: None,
        ..found(true)
    };
    assert_eq!(
        prepare(&portrait(), &none, NOW),
        Err(Refusal::NoLiveSession)
    );
    let unknown = Found {
        held: false,
        ..found(false)
    };
    assert_eq!(
        prepare(&portrait(), &unknown, NOW),
        Err(Refusal::UnknownMedia)
    );
}

#[test]
fn each_refusal_says_why_in_the_dm_s_words() {
    for (refusal, why) in [
        (Refusal::NoCampaign, "campaign manque"),
        (Refusal::NoMedia, "media manque"),
        (Refusal::NotAnId("media"), "media n'est pas un identifiant"),
        (Refusal::NoLiveSession, "aucune séance n'est en cours"),
        (Refusal::UnknownMedia, "ce média n'existe pas"),
        (Refusal::NoDiffusion, "la vue joueurs n'a encore rien reçu"),
    ] {
        assert_eq!(refusal.to_string(), why);
    }
}

#[test]
fn the_manifest_touches_what_a_window_changes_of_an_existing_row_and_nothing_of_the_base() {
    let manifest: Value =
        serde_json::from_str(include_str!("../data-capability.json")).expect("contract F");
    assert_eq!(
        manifest["dataCapability"],
        show_window_on_player_view::DATA_CAPABILITY
    );
    let touched: Vec<String> = prepare(&portrait(), &found(true), NOW)
        .expect("prepared")
        .payload
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
