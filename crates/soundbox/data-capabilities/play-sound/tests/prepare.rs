//! soundbox.playSound's request and preparation: what a trigger asks for,
//! and the playback row it starts against what the tables hold of the
//! sound.

use play_sound::{Fresh, Refusal, Request, Row, Stage, names, prepare, request};
use serde_json::{Value, json};

const PORTE: &str = "50000000-0000-4000-8000-0000000000b2";
const VALOMBRE: &str = "c1000000-0000-4000-8000-000000000001";
const AUTRE: &str = "c1000000-0000-4000-8000-000000000002";
const SEANCE: &str = "5e000000-0000-4000-8000-000000000001";
const PLAYBACK: &str = "70000000-0000-4000-8000-000000000001";
const NOW: &str = "2026-10-20T20:15:00.000Z";

fn wire(value: Value) -> Row {
    serde_json::from_value(value).expect("a row")
}

fn fresh() -> Fresh<'static> {
    Fresh {
        id: PLAYBACK,
        now: NOW,
    }
}

/// Valombre's sound, its campaign playing in its live session.
fn valombre() -> Stage {
    Stage {
        campaign: Some(VALOMBRE.to_string()),
        current: Some(SEANCE.to_string()),
        named: None,
    }
}

fn played(payload: Value) -> Request {
    request(&wire(payload), Some(PORTE)).expect("a request")
}

#[test]
fn it_names_itself_at_any_version() {
    assert!(names("soundbox.playSound"));
    assert!(names("soundbox.playSound@1"));
    assert!(!names("soundbox.stopAll@1"));
    assert!(!names("soundbox.playSoundAgain"));
}

#[test]
fn an_empty_payload_plays_the_target_from_the_soundbox_in_the_campaign_s_session() {
    // The rule test's call shape: `{}`, the sound as the command's target.
    let row = prepare(&played(json!({})), &valombre(), fresh()).expect("prepared");
    assert_eq!(
        row,
        wire(json!({
            "id": PLAYBACK,
            "sessionId": SEANCE,
            "soundId": PORTE,
            "origin": "soundbox",
            "startedAt": NOW,
        }))
    );
}

#[test]
fn simultaneous_playback_a_new_playback_plays_and_says_nothing_of_the_others() {
    // A new row, no `stoppedAt`: the DataGuard's simultaneous-playback holds,
    // and no other playback is written.
    let row = prepare(&played(json!({})), &valombre(), fresh()).expect("prepared");
    assert!(!row.contains_key("stoppedAt"), "{row:?}");
    let keys: Vec<&str> = row.keys().map(String::as_str).collect();
    assert_eq!(keys, ["id", "origin", "sessionId", "soundId", "startedAt"]);
}

#[test]
fn a_sound_from_the_pill_is_stored_with_the_pastille_s_origin() {
    let row = prepare(
        &played(json!({ "source": "suggestion" })),
        &valombre(),
        fresh(),
    )
    .expect("prepared");
    assert_eq!(row["origin"], "pastille");
    let row = prepare(
        &played(json!({ "source": "soundbox" })),
        &valombre(),
        fresh(),
    )
    .expect("prepared");
    assert_eq!(row["origin"], "soundbox");
}

#[test]
fn dm_side_only_a_source_of_the_player_view_is_carried_for_the_dataguard_to_refuse() {
    // Not refused here: the DataGuard's dm-side-only judges the origin.
    for source in [json!("player"), json!("player-view"), json!(3)] {
        let request = played(json!({ "source": source }));
        assert_eq!(request.origin, source);
        let row = prepare(&request, &valombre(), fresh()).expect("prepared");
        assert_eq!(row["origin"], source);
    }
}

#[test]
fn the_sound_is_the_command_s_target_and_a_uuid() {
    assert_eq!(
        request(&wire(json!({})), None),
        Err(Refusal::NotAnId("sound"))
    );
    assert_eq!(
        request(&wire(json!({})), Some("snd-porte")),
        Err(Refusal::NotAnId("sound"))
    );
    // A uuid's shape at its every length: neither shorter, nor longer, nor
    // with a letter that is no hex digit.
    let shorter = &PORTE[..35];
    let longer = format!("{PORTE}0");
    let not_hex = PORTE.replace('b', "g");
    for text in [shorter, &longer, &not_hex] {
        assert_eq!(
            request(&wire(json!({})), Some(text)),
            Err(Refusal::NotAnId("sound")),
            "{text}"
        );
    }
    // Lowercase, as the tables write it.
    let upper = PORTE.to_ascii_uppercase();
    let request = request(&wire(json!({})), Some(&upper)).expect("a request");
    assert_eq!(request.sound, PORTE);
}

#[test]
fn a_session_named_is_a_uuid_of_the_sound_s_campaign() {
    assert_eq!(
        request(&wire(json!({ "session": "seance-1" })), Some(PORTE)),
        Err(Refusal::NotAnId("session"))
    );
    assert_eq!(
        request(&wire(json!({ "session": 1 })), Some(PORTE)),
        Err(Refusal::NotAnId("session"))
    );
    let other = "5e000000-0000-4000-8000-000000000002";
    let named = played(json!({ "session": other.to_ascii_uppercase() }));
    assert_eq!(named.session.as_deref(), Some(other));
    // Played there, not in the campaign's current session.
    let stage = Stage {
        named: Some(VALOMBRE.to_string()),
        ..valombre()
    };
    let row = prepare(&named, &stage, fresh()).expect("prepared");
    assert_eq!(row["sessionId"], other);
    // A session the tables do not hold, or of another campaign.
    assert_eq!(
        prepare(&named, &valombre(), fresh()),
        Err(Refusal::UnknownSession)
    );
    let elsewhere = Stage {
        named: Some(AUTRE.to_string()),
        ..valombre()
    };
    assert_eq!(
        prepare(&named, &elsewhere, fresh()),
        Err(Refusal::SessionOfAnotherCampaign)
    );
    // A null session is none named.
    assert_eq!(played(json!({ "session": null })).session, None);
}

#[test]
fn a_sound_the_tables_do_not_hold_or_with_no_session_is_refused() {
    assert_eq!(
        prepare(&played(json!({})), &Stage::default(), fresh()),
        Err(Refusal::UnknownSound)
    );
    let sessionless = Stage {
        current: None,
        ..valombre()
    };
    assert_eq!(
        prepare(&played(json!({})), &sessionless, fresh()),
        Err(Refusal::NoSession)
    );
}

#[test]
fn every_refusal_says_why_in_words() {
    assert_eq!(
        Refusal::NotAnId("sound").to_string(),
        "sound n'est pas un identifiant"
    );
    assert_eq!(Refusal::UnknownSound.to_string(), "ce son n'existe pas");
    assert_eq!(
        Refusal::UnknownSession.to_string(),
        "cette séance n'existe pas"
    );
    assert_eq!(
        Refusal::SessionOfAnotherCampaign.to_string(),
        "cette séance n'est pas de la campagne du son"
    );
    assert_eq!(
        Refusal::NoSession.to_string(),
        "la campagne de ce son n'a aucune séance"
    );
}
