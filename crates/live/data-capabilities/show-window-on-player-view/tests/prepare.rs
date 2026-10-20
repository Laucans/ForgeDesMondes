//! What `prepare` makes of a window opened on the player view: the state of
//! the live session's diffusion row — the window's media, position and width
//! together, no height, the base as the row holds it — and what is refused
//! before it is queued.

use serde_json::{Value, json};
use show_window_on_player_view::{
    Base, Found, NOT_AN_ILLUSTRATION, Named, Refusal, Row, WIDTH, X, Y, named, names, prepare,
};

const VALOMBRE: &str = "c1000000-0000-4000-8000-000000000001";
const SEANCE: &str = "51000000-0000-4000-8000-000000000002";
/// An illustration of Valombre, 800 × 1200.
const PORTRAIT: &str = "d1000000-0000-4000-8000-000000000002";
const INSTANCE: &str = "b1000000-0000-4000-8000-000000000002";
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

fn portrait() -> Named {
    Named {
        campaign: VALOMBRE.to_string(),
        media: PORTRAIT.to_string(),
    }
}

/// What the tables say of the portrait: an illustration of Valombre, whose
/// live session is `SEANCE`.
fn illustration() -> Found {
    Found {
        session: Some(SEANCE.to_string()),
        held: true,
        illustration: true,
        base: None,
    }
}

/// The live session's row showing the tavern's battlemap instance.
fn showing_a_battlemap() -> Row {
    object(json!({
        "sessionId": SEANCE,
        "baseKind": "battlemap",
        "baseMediaId": null,
        "baseInstanceId": INSTANCE,
        "viewZoom": 1.5,
    }))
}

#[test]
fn it_names_live_show_window_on_player_view_with_or_without_its_version() {
    assert!(names("live.showWindowOnPlayerView"));
    assert!(names("live.showWindowOnPlayerView@1"));
    assert!(!names("live.showOnPlayerView"));
    assert!(!names("live.showWindowOnPlayerViews"));
}

#[test]
fn the_campaign_and_the_media_are_the_uuids_the_payload_names_in_lower_case() {
    let wire = object(json!({
        "campaign": VALOMBRE.to_ascii_uppercase(),
        "media": PORTRAIT.to_ascii_uppercase(),
    }));
    assert_eq!(named(&wire), Ok(portrait()));
}

#[test]
fn a_payload_naming_no_campaign_no_media_or_no_uuid_is_refused() {
    for (wire, refusal) in [
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
            json!({ "campaign": "valombre", "media": PORTRAIT }),
            Refusal::NotAnId,
        ),
        (
            json!({ "campaign": VALOMBRE, "media": 42 }),
            Refusal::NotAnId,
        ),
        (
            json!({ "campaign": VALOMBRE, "media": [PORTRAIT] }),
            Refusal::NotAnId,
        ),
    ] {
        assert_eq!(named(&object(wire.clone())), Err(refusal), "{wire}");
    }
}

#[test]
fn a_campaign_with_no_live_session_or_a_media_the_tables_do_not_hold_is_refused() {
    let none = Found {
        session: None,
        ..illustration()
    };
    assert_eq!(
        prepare(&portrait(), &none, &Row::new(), NOW),
        Err(Refusal::NoLiveSession)
    );
    let unknown = Found {
        held: false,
        illustration: false,
        ..illustration()
    };
    assert_eq!(
        prepare(&portrait(), &unknown, &Row::new(), NOW),
        Err(Refusal::UnknownMedia)
    );
    assert_eq!(
        Refusal::NoLiveSession.to_string(),
        "aucune séance n'est en cours"
    );
}

#[test]
fn an_illustration_opens_at_its_default_position_and_width_the_base_untouched() {
    let prepared =
        prepare(&portrait(), &illustration(), &showing_a_battlemap(), NOW).expect("prepared");
    assert_eq!(prepared.target_id, SEANCE);
    assert_eq!(prepared.warning, None);
    // window-fields-together: the media, x, y and the width all set;
    // window-proportional: no height. The base is the row's, unwritten.
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
    assert!(WIDTH > 0.0 && X + WIDTH <= 1.0 && (0.0..1.0).contains(&Y));
}

#[test]
fn a_session_with_no_row_yet_gets_one_showing_the_cover_under_the_window() {
    let prepared = prepare(&portrait(), &illustration(), &Row::new(), NOW).expect("prepared");
    assert_eq!(
        Value::Object(prepared.payload.into_iter().collect()),
        json!({
            "sessionId": SEANCE,
            "baseKind": "cover",
            "baseMediaId": null,
            "baseInstanceId": null,
            "windowMediaId": PORTRAIT,
            "windowX": X,
            "windowY": Y,
            "windowWidth": WIDTH,
            "updatedAt": NOW,
        })
    );
}

#[test]
fn a_session_the_engine_holds_no_row_of_keeps_the_base_its_table_shows() {
    let found = Found {
        base: Some(Base {
            kind: "battlemap".to_string(),
            media: None,
            instance: Some(INSTANCE.to_string()),
        }),
        ..illustration()
    };
    let prepared = prepare(&portrait(), &found, &Row::new(), NOW).expect("prepared");
    assert_eq!(prepared.payload["baseKind"], "battlemap");
    assert_eq!(prepared.payload["baseMediaId"], Value::Null);
    assert_eq!(prepared.payload["baseInstanceId"], INSTANCE);
    assert_eq!(prepared.payload["sessionId"], SEANCE);
}

#[test]
fn a_table_base_whose_illustration_is_gone_gives_way_to_the_cover() {
    // The base illustration was deleted: its id nullified, the kind kept.
    let found = Found {
        base: Some(Base {
            kind: "illustration".to_string(),
            media: None,
            instance: None,
        }),
        ..illustration()
    };
    let prepared = prepare(&portrait(), &found, &Row::new(), NOW).expect("prepared");
    assert_eq!(prepared.payload["baseKind"], "cover");
    assert_eq!(prepared.payload["baseMediaId"], Value::Null);
    assert_eq!(prepared.payload["baseInstanceId"], Value::Null);
}

#[test]
fn a_media_that_is_no_illustration_of_the_campaign_gets_no_width_and_a_warning() {
    let battlemap = Found {
        illustration: false,
        ..illustration()
    };
    let prepared = prepare(&portrait(), &battlemap, &showing_a_battlemap(), NOW).expect("queued");
    // window-proportional refuses a width its height cannot follow.
    assert_eq!(prepared.payload["windowWidth"], 0.0);
    assert_eq!(prepared.payload["windowMediaId"], PORTRAIT);
    assert_eq!(prepared.warning, Some(NOT_AN_ILLUSTRATION));
}

#[test]
fn an_open_row_writes_only_what_the_manifest_touches() {
    let prepared =
        prepare(&portrait(), &illustration(), &showing_a_battlemap(), NOW).expect("prepared");
    let mut touched: Vec<String> = manifest()["touches"]
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
    touched.sort();
    written.sort();
    assert_eq!(written, touched);
}

#[test]
fn the_manifest_upserts_the_diffusion_under_its_three_invariants() {
    let manifest = manifest();
    assert_eq!(manifest["dataCapability"], "live.showWindowOnPlayerView");
    assert_eq!(manifest["effect"], "upsert");
    assert_eq!(manifest["target"]["aggregate"], "Diffusion");
    assert_eq!(
        manifest["invariants"],
        json!([
            "window-proportional",
            "window-fields-together",
            "single-diffusion-source"
        ])
    );
    assert_eq!(
        manifest["payload"]
            .as_object()
            .expect("a payload")
            .keys()
            .collect::<Vec<_>>(),
        ["campaign", "media"]
    );
}
