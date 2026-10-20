//! What a send to the player view becomes before it is submitted: the state
//! it writes on its session's diffusion row, what the server looks up for it,
//! and what is refused before it is queued.

use std::collections::BTreeMap;

use serde_json::{Value, json};
use show_on_player_view::{
    Found, NOT_AN_ILLUSTRATION, NOT_OF_THE_SESSION, Named, Refusal, named, names, prepare,
};

const VALOMBRE: &str = "c1000000-0000-4000-8000-000000000001";
const SEANCE: &str = "51000000-0000-4000-8000-00000000000a";
const SEANCE_1: &str = "51000000-0000-4000-8000-000000000001";
const PORTRAIT: &str = "d1000000-0000-4000-8000-000000000002";
const INSTANCE: &str = "b1000000-0000-4000-8000-000000000001";
const NOW: &str = "2026-10-20T20:00:00.000Z";

fn wire(payload: Value) -> BTreeMap<String, Value> {
    let Value::Object(payload) = payload else {
        panic!("a payload is an object");
    };
    payload.into_iter().collect()
}

/// The live session, the instance of `instance_session`, the media an
/// illustration of the campaign or not.
fn found(instance_session: Option<&str>, illustration: bool) -> Found {
    Found {
        session: Some(SEANCE.to_string()),
        instance_session: instance_session.map(str::to_string),
        illustration,
        held: true,
    }
}

fn battlemap() -> Value {
    json!({ "kind": "battlemap", "instance": INSTANCE, "zoom": 1.5, "panX": -120, "panY": 40 })
}

fn illustration() -> Value {
    json!({ "campaign": VALOMBRE, "kind": "illustration", "media": PORTRAIT })
}

#[test]
fn it_answers_to_its_id_with_or_without_its_version() {
    assert!(names("live.showOnPlayerView"));
    assert!(names("live.showOnPlayerView@1"));
    assert!(!names("live.showCover@1"));
    assert!(!names("live.showOnPlayerViewX"));
}

#[test]
fn a_send_names_the_campaign_and_the_instance_or_the_media_in_lower_case() {
    let upper = json!({ "campaign": VALOMBRE.to_uppercase(), "kind": "illustration", "media": PORTRAIT.to_uppercase() });
    assert_eq!(
        named(&wire(upper)),
        Ok(Named {
            campaign: Some(VALOMBRE.to_string()),
            instance: None,
            media: Some(PORTRAIT.to_string()),
        })
    );
    // The Battlemap names no campaign: the instance's is the one.
    assert_eq!(
        named(&wire(battlemap())),
        Ok(Named {
            campaign: None,
            instance: Some(INSTANCE.to_string()),
            media: None,
        })
    );
}

#[test]
fn a_battlemap_of_the_session_is_its_instance_alone_framed_as_sent() {
    let prepared = prepare(&wire(battlemap()), &found(Some(SEANCE), false), NOW).expect("sent");
    assert_eq!(prepared.target_id, SEANCE);
    assert_eq!(prepared.warning, None);
    assert_eq!(
        Value::Object(prepared.payload.into_iter().collect()),
        json!({
            "sessionId": SEANCE,
            "baseKind": "battlemap",
            "baseMediaId": null,
            "baseInstanceId": INSTANCE,
            "viewZoom": 1.5,
            "viewPanX": -120.0,
            "viewPanY": 40.0,
            "updatedAt": NOW,
        })
    );
}

#[test]
fn an_illustration_is_its_media_alone_framed_at_1_and_0_when_no_framing_is_given() {
    let prepared = prepare(&wire(illustration()), &found(None, true), NOW).expect("sent");
    assert_eq!(prepared.warning, None);
    let payload = prepared.payload;
    assert_eq!(payload["baseKind"], "illustration");
    assert_eq!(payload["baseMediaId"], PORTRAIT);
    assert_eq!(payload["baseInstanceId"], Value::Null);
    assert_eq!(
        (
            &payload["viewZoom"],
            &payload["viewPanX"],
            &payload["viewPanY"]
        ),
        (&json!(1.0), &json!(0.0), &json!(0.0))
    );
}

#[test]
fn a_send_touches_no_field_of_the_window() {
    let prepared = prepare(&wire(illustration()), &found(None, true), NOW).expect("sent");
    let fields: Vec<&str> = prepared.payload.keys().map(String::as_str).collect();
    assert_eq!(
        fields,
        [
            "baseInstanceId",
            "baseKind",
            "baseMediaId",
            "sessionId",
            "updatedAt",
            "viewPanX",
            "viewPanY",
            "viewZoom"
        ]
    );
}

#[test]
fn an_instance_of_another_session_is_left_out_for_base_coherent_to_refuse() {
    for instance_session in [Some(SEANCE_1), None] {
        let prepared =
            prepare(&wire(battlemap()), &found(instance_session, false), NOW).expect("queued");
        assert_eq!(prepared.payload["baseKind"], "battlemap");
        assert_eq!(prepared.payload["baseInstanceId"], Value::Null);
        assert_eq!(prepared.payload["baseMediaId"], Value::Null);
        assert_eq!(prepared.warning, Some(NOT_OF_THE_SESSION));
    }
}

#[test]
fn a_media_that_is_no_illustration_of_the_campaign_is_left_out_for_base_coherent_to_refuse() {
    let prepared = prepare(&wire(illustration()), &found(None, false), NOW).expect("queued");
    assert_eq!(prepared.payload["baseKind"], "illustration");
    assert_eq!(prepared.payload["baseMediaId"], Value::Null);
    assert_eq!(prepared.warning, Some(NOT_AN_ILLUSTRATION));
}

#[test]
fn the_framing_is_rounded_to_what_the_table_holds() {
    let send = json!({ "kind": "battlemap", "instance": INSTANCE, "zoom": 1.23456, "panX": 10.006, "panY": -3.333 });
    let payload = prepare(&wire(send), &found(Some(SEANCE), false), NOW)
        .expect("sent")
        .payload;
    assert_eq!(payload["viewZoom"], json!(1.235));
    assert_eq!(payload["viewPanX"], json!(10.01));
    assert_eq!(payload["viewPanY"], json!(-3.33));
}

#[test]
fn what_the_table_could_never_hold_is_refused_before_it_is_queued() {
    let at = |field: &str, value: Value| {
        let mut send = battlemap();
        send[field] = value;
        prepare(&wire(send), &found(Some(SEANCE), false), NOW).err()
    };
    assert_eq!(at("kind", json!("cover")), Some(Refusal::Kind));
    assert_eq!(at("kind", Value::Null), Some(Refusal::Kind));
    assert_eq!(
        at("instance", Value::Null),
        Some(Refusal::Missing("instance"))
    );
    assert_eq!(
        at("instance", json!("taverne")),
        Some(Refusal::NotAnId("instance"))
    );
    // A uuid's length with other characters, or a uuid's characters at
    // another length, is no uuid either.
    for id in [
        "z".repeat(36),
        INSTANCE[..35].to_string(),
        format!("{INSTANCE}0"),
    ] {
        assert_eq!(
            at("instance", json!(id)),
            Some(Refusal::NotAnId("instance")),
            "{id}"
        );
    }
    assert_eq!(
        at("campaign", json!(12)),
        Some(Refusal::NotAnId("campaign"))
    );
    for zoom in [
        json!(0),
        json!(-1),
        json!(1000),
        json!(999.9996),
        json!("2"),
    ] {
        assert_eq!(at("zoom", zoom.clone()), Some(Refusal::Zoom), "{zoom}");
    }
    assert_eq!(at("zoom", json!(999.999)), None);
    for pan in [json!(10_000_000), json!(-9_999_999.996), json!("0")] {
        assert_eq!(at("panX", pan.clone()), Some(Refusal::Pan("panX")), "{pan}");
        assert_eq!(at("panY", pan.clone()), Some(Refusal::Pan("panY")), "{pan}");
    }
    assert_eq!(at("panY", json!(-9_999_999.99)), None);
    let media = json!({ "kind": "illustration", "instance": INSTANCE });
    assert_eq!(
        prepare(&wire(media), &found(None, true), NOW).err(),
        Some(Refusal::Missing("media"))
    );
}

#[test]
fn a_send_no_live_session_shows_is_refused() {
    let none = Found {
        session: None,
        ..found(Some(SEANCE), true)
    };
    assert_eq!(
        prepare(&wire(battlemap()), &none, NOW).err(),
        Some(Refusal::NoLiveSession)
    );
    assert_eq!(
        Refusal::NoLiveSession.to_string(),
        "aucune séance n'est en cours"
    );
}

#[test]
fn an_instance_or_a_media_the_tables_do_not_hold_naming_no_campaign_is_unknown() {
    let nothing = Found::default();
    assert_eq!(
        prepare(&wire(battlemap()), &nothing, NOW).err(),
        Some(Refusal::Unknown("instance"))
    );
    let media = json!({ "kind": "illustration", "media": PORTRAIT });
    assert_eq!(
        prepare(&wire(media), &nothing, NOW).err(),
        Some(Refusal::Unknown("media"))
    );
    assert_eq!(
        Refusal::Unknown("instance").to_string(),
        "instance n'existe pas"
    );
    // A campaign named, or a row the tables hold: what is missing is the
    // live session.
    assert_eq!(
        prepare(&wire(illustration()), &nothing, NOW).err(),
        Some(Refusal::NoLiveSession)
    );
    let held = Found {
        held: true,
        ..Found::default()
    };
    assert_eq!(
        prepare(&wire(battlemap()), &held, NOW).err(),
        Some(Refusal::NoLiveSession)
    );
}
