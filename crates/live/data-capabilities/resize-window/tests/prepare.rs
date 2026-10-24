//! What `prepare` makes of a resize of the floating window: its width on the
//! live session's diffusion row — the largest the drawn box holds at the
//! illustration's ratio, brought between the narrowest window and the
//! screen's width — and the stamp, never its height, its position nor its
//! media; and what is refused before it is queued.

use resize_window::{
    MIN_WIDTH, NO_PROPORTIONS, Named, Proportions, Refusal, Row, Window, named, prepare,
};
use serde_json::{Value, json};

const VALOMBRE: &str = "c1000000-0000-4000-8000-000000000001";
const SEANCE: &str = "51000000-0000-4000-8000-000000000002";
const NOW: &str = "2026-10-20T20:00:00.000Z";

/// The portrait of Morlox: 800 × 1200, a 2/3 ratio.
const PORTRAIT: Window = Window::Open(Some(Proportions {
    width_px: 800,
    height_px: 1200,
}));

fn object(value: Value) -> Row {
    let Value::Object(value) = value else {
        panic!("an object");
    };
    value.into_iter().collect()
}

fn manifest() -> Value {
    serde_json::from_str(include_str!("../data-capability.json")).expect("contract F")
}

fn to(w: f64, h: Option<f64>) -> Named {
    Named {
        campaign: VALOMBRE.to_string(),
        w,
        h,
    }
}

/// The width resizing `window` to `w` × `h` writes.
fn width(w: f64, h: Option<f64>, window: Window) -> Value {
    prepare(&to(w, h), Some(SEANCE.to_string()), window, NOW)
        .expect("prepared")
        .payload["windowWidth"]
        .clone()
}

#[test]
fn a_resize_writes_the_width_and_the_stamp_and_nothing_else_on_the_live_session_s_row() {
    let prepared =
        prepare(&to(0.4, Some(0.6)), Some(SEANCE.to_string()), PORTRAIT, NOW).expect("prepared");
    assert_eq!(prepared.target_id, SEANCE);
    assert_eq!(prepared.warning, None);
    // Neither the position (live.moveWindow's), the media nor a height.
    assert_eq!(
        prepared.payload,
        object(json!({ "windowWidth": 0.4, "updatedAt": NOW }))
    );
}

#[test]
fn a_box_of_any_shape_gives_the_largest_window_of_the_image_s_ratio_it_holds() {
    for ((w, h), kept) in [
        // A square: the height leads, 0.6 × 2/3.
        ((0.6, 0.6), 0.4),
        // A box wider than the image: the height leads.
        ((0.5, 0.3), 0.2),
        // A box taller than the image: the width leads.
        ((0.2, 0.9), 0.2),
        // A box at the image's ratio: it is the window.
        ((0.3, 0.45), 0.3),
    ] {
        assert_eq!(width(w, Some(h), PORTRAIT), kept, "{w} × {h}");
    }
}

#[test]
fn the_spec_s_600_by_600_on_an_800_by_1200_illustration_keeps_2_3_at_the_screen_s_width() {
    // 600 × 600 holds 400 × 600 at 2/3, wider than the screen: the screen's
    // width, its derived height 1 × 1200 / 800.
    let width = width(600.0, Some(600.0), PORTRAIT)
        .as_f64()
        .expect("a width");
    assert_eq!(width, 1.0);
    let height = width * 1200.0 / 800.0;
    assert!((width / height - 2.0 / 3.0).abs() < 1e-12);
}

#[test]
fn with_no_h_the_width_leads() {
    assert_eq!(width(0.45, None, PORTRAIT), 0.45);
}

#[test]
fn a_width_is_brought_between_the_narrowest_window_and_the_screen_s_width() {
    for ((w, h), kept) in [
        ((0.01, Some(0.01)), MIN_WIDTH),
        ((-3.0, None), MIN_WIDTH),
        ((0.0, Some(0.5)), MIN_WIDTH),
        ((4.0, None), 1.0),
        ((1.0, Some(1.5)), 1.0),
    ] {
        assert_eq!(width(w, h, PORTRAIT), kept, "{w} × {h:?}");
    }
}

#[test]
fn a_width_is_kept_to_the_four_decimals_the_table_holds() {
    // 0.4 × 2/3 = 0.26666…
    assert_eq!(width(0.5, Some(0.4), PORTRAIT), 0.2667);
}

#[test]
fn a_window_that_is_not_open_gets_the_width_asked_for_the_data_guard_to_refuse() {
    let prepared = prepare(
        &to(0.6, Some(0.6)),
        Some(SEANCE.to_string()),
        Window::Closed,
        NOW,
    )
    .expect("prepared");
    // A width with no media: window-fields-together refuses it, not
    // window-proportional, and no warning is told.
    assert_eq!(
        prepared.payload,
        object(json!({ "windowWidth": 0.6, "updatedAt": NOW }))
    );
    assert_eq!(prepared.warning, None);
}

#[test]
fn a_window_whose_illustration_has_no_proportions_gets_no_width_and_a_warning() {
    for window in [
        Window::Open(None),
        Window::Open(Some(Proportions {
            width_px: 0,
            height_px: 1200,
        })),
        Window::Open(Some(Proportions {
            width_px: 800,
            height_px: 0,
        })),
    ] {
        let prepared =
            prepare(&to(0.4, Some(0.6)), Some(SEANCE.to_string()), window, NOW).expect("prepared");
        assert_eq!(prepared.payload["windowWidth"], 0.0, "{window:?}");
        assert_eq!(prepared.warning, Some(NO_PROPORTIONS), "{window:?}");
    }
}

#[test]
fn a_campaign_with_no_live_session_is_refused() {
    assert_eq!(
        prepare(&to(0.4, Some(0.6)), None, PORTRAIT, NOW),
        Err(Refusal::NoLiveSession)
    );
}

#[test]
fn the_payload_names_the_campaign_in_lower_case_the_width_and_maybe_the_height() {
    assert_eq!(
        named(&object(json!({
            "campaign": VALOMBRE.to_uppercase(),
            "w": 600,
            "h": 0.6,
        }))),
        Ok(to(600.0, Some(0.6)))
    );
    for wire in [
        json!({ "campaign": VALOMBRE, "w": 0.4 }),
        json!({ "campaign": VALOMBRE, "w": 0.4, "h": null }),
    ] {
        assert_eq!(named(&object(wire.clone())), Ok(to(0.4, None)), "{wire}");
    }
}

#[test]
fn a_payload_naming_no_campaign_or_no_width_is_refused() {
    for (wire, refusal) in [
        (json!({ "w": 0.4, "h": 0.6 }), Refusal::NoCampaign),
        (json!({ "campaign": null, "w": 0.4 }), Refusal::NoCampaign),
        (
            json!({ "campaign": "valombre", "w": 0.4 }),
            Refusal::NotAnId,
        ),
        // A uuid's digits too few, and a uuid's length with a letter no digit.
        (
            json!({ "campaign": &VALOMBRE[..8], "w": 0.4 }),
            Refusal::NotAnId,
        ),
        (
            json!({ "campaign": VALOMBRE.replace('c', "z"), "w": 0.4 }),
            Refusal::NotAnId,
        ),
        (json!({ "campaign": 7, "w": 0.4 }), Refusal::NotAnId),
        (json!({ "campaign": VALOMBRE, "h": 0.6 }), Refusal::NoSize),
        (
            json!({ "campaign": VALOMBRE, "w": null, "h": 0.6 }),
            Refusal::NoSize,
        ),
        (
            json!({ "campaign": VALOMBRE, "w": "0.4" }),
            Refusal::NotANumber,
        ),
        (
            json!({ "campaign": VALOMBRE, "w": 0.4, "h": true }),
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
        (Refusal::NoSize, "w manque"),
        (Refusal::NotANumber, "w ou h n'est pas un nombre"),
        (Refusal::NoLiveSession, "aucune séance n'est en cours"),
    ] {
        assert_eq!(refusal.to_string(), said);
    }
}

#[test]
fn the_manifest_updates_the_window_s_width_under_its_three_invariants() {
    let manifest = manifest();
    assert_eq!(manifest["dataCapability"], "live.resizeWindow");
    assert_eq!(manifest["effect"], "update");
    assert_eq!(manifest["mode"], "overwrite");
    assert_eq!(manifest["target"]["aggregate"], "Diffusion");
    assert_eq!(
        manifest["touches"],
        json!(["Diffusion.windowWidth", "Diffusion.updatedAt"])
    );
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
        // serde_json keeps the keys sorted.
        ["campaign", "h", "w"]
    );
}
