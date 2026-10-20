//! `scope` and `prepare`: the DM's « Tout arrêter » made the playbacks it
//! names, then one stop for each that still plays — or refused before
//! anything is queued.

use serde_json::{Value, json};
use stop_all::{Refusal, Row, STOPPED, Scope, names, playing, prepare, scope};

const VALOMBRE: &str = "c1000000-0000-4000-8000-000000000001";
const SEANCE: &str = "5e000000-0000-4000-8000-000000000001";
const FOULE: &str = "a0000000-0000-4000-8000-000000000001";
const PORTE: &str = "a0000000-0000-4000-8000-000000000002";
const NOW: &str = "2026-10-20T10:00:00.000Z";

fn row(value: Value) -> Row {
    let Value::Object(map) = value else {
        panic!("an object");
    };
    map.into_iter().collect()
}

#[test]
fn the_data_capability_is_named_with_or_without_its_version() {
    assert!(names("soundbox.stopAll"));
    assert!(names("soundbox.stopAll@1"));
    assert!(!names("soundbox.playSound@1"));
    assert!(!names("soundbox.stopAllSounds"));
}

#[test]
fn the_campaign_is_the_payload_s_or_the_target_s_lowercase() {
    let named = scope(&row(json!({ "campaign": VALOMBRE.to_uppercase() })), None);
    assert_eq!(
        named,
        Ok(Scope {
            campaign: VALOMBRE.into(),
            session: None
        })
    );
    assert_eq!(
        scope(&row(json!({})), Some(VALOMBRE)).map(|s| s.campaign),
        Ok(VALOMBRE.into())
    );
    // The payload's wins over the target's.
    assert_eq!(
        scope(&row(json!({ "campaign": VALOMBRE })), Some(SEANCE)).map(|s| s.campaign),
        Ok(VALOMBRE.into())
    );
}

#[test]
fn a_session_named_narrows_the_scope_to_it() {
    let named = scope(
        &row(json!({ "campaign": VALOMBRE, "session": format!(" {} ", SEANCE.to_uppercase()) })),
        None,
    );
    assert_eq!(named.map(|s| s.session), Ok(Some(SEANCE.into())));
    for none in [json!(null), json!(""), json!("  ")] {
        let named = scope(&row(json!({ "campaign": VALOMBRE, "session": none })), None);
        assert_eq!(named.map(|s| s.session), Ok(None));
    }
}

#[test]
fn a_request_naming_no_campaign_or_no_uuid_is_refused() {
    for wire in [
        json!({}),
        json!({ "campaign": "" }),
        json!({ "campaign": null }),
        json!({ "campaign": "valombre" }),
        json!({ "campaign": 1 }),
        json!({ "campaign": "c1000000-0000-4000-8000-00000000000g" }),
        json!({ "campaign": "c1000000-0000-4000-8000-0000000000011" }),
    ] {
        assert_eq!(
            scope(&row(wire.clone()), None),
            Err(Refusal::NoCampaign),
            "{wire}"
        );
    }
    assert_eq!(
        scope(&row(json!({})), Some("valombre")),
        Err(Refusal::NoCampaign)
    );
    for session in [json!("la crypte"), json!(1), json!([SEANCE])] {
        let wire = json!({ "campaign": VALOMBRE, "session": session });
        assert_eq!(
            scope(&row(wire.clone()), None),
            Err(Refusal::NotSession),
            "{wire}"
        );
    }
}

#[test]
fn a_refusal_says_why_in_the_dm_s_words() {
    assert_eq!(
        Refusal::NoCampaign.to_string(),
        "« Tout arrêter » nomme sa campagne"
    );
    assert_eq!(Refusal::NotSession.to_string(), "cette séance n'existe pas");
}

#[test]
fn a_playback_plays_until_its_row_has_a_stop_time() {
    assert!(playing(&row(json!({}))));
    assert!(playing(&row(json!({ "stoppedAt": null }))));
    assert!(!playing(&row(json!({ "stoppedAt": NOW }))));
}

#[test]
fn stop_all_stops_each_playback_still_playing_and_writes_its_stop_time_alone() {
    let running = [
        (
            FOULE.to_uppercase(),
            row(json!({ "soundId": "x", "stoppedAt": null })),
        ),
        // A stop queued ahead: it stops already.
        (
            PORTE.to_owned(),
            row(json!({ "stoppedAt": "2026-10-20T09:00:00Z" })),
        ),
    ];
    let stops = prepare(&running, NOW);
    assert_eq!(stops.len(), 1, "{stops:?}");
    assert_eq!(stops[0].target_id, FOULE);
    assert_eq!(stops[0].payload, row(json!({ STOPPED: NOW })));
}

#[test]
fn stop_all_with_nothing_playing_is_no_command() {
    assert!(prepare(&[], NOW).is_empty());
    let stopped = [(PORTE.to_owned(), row(json!({ "stoppedAt": NOW })))];
    assert!(prepare(&stopped, NOW).is_empty());
}
