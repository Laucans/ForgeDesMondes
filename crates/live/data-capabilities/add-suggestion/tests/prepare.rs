//! `live.addSuggestion`'s pure core, no engine: [`request`] reads the wire,
//! [`prepare`] judges the session's row and writes the suggestion's. The
//! refusals say why in words, only an unknown session is a 404, the target
//! is exactly one of an element and a sound, the matched text is never blank,
//! the instant is the clock's and the row holds nothing else.

use platform_test_kit::uuid;
use serde_json::{Value, json};

use add_suggestion::{Refusal, Request, Row, Target, prepare, request};

/// A row, from a JSON object.
fn row(value: Value) -> Row {
    serde_json::from_value(value).expect("an object")
}

/// The suggestion's row, the session, an element and a sound of the house.
fn suggestion() -> String {
    uuid(0x5a, 1)
}
fn session() -> String {
    uuid(0x5e, 1)
}
fn entity() -> String {
    uuid(0xe1, 1)
}
fn sound() -> String {
    uuid(0x50, 1)
}

/// The wire of a suggestion of the element, with `extra` fields over it.
fn wire(extra: Value) -> Row {
    let mut wire = row(json!({
        "sessionId": session(),
        "entityId": entity(),
        "matchedText": "la taverne",
    }));
    wire.extend(row(extra));
    wire
}

/// What the query answers for a live session with the microphone on and
/// both its element and its sound in its campaign.
fn live() -> Row {
    row(json!({
        "status": "live",
        "microphone_on": true,
        "entity_in_campaign": true,
        "sound_in_campaign": true,
    }))
}

/// `live()` with `fields` over it.
fn session_with(fields: Value) -> Row {
    let mut session = live();
    session.extend(row(fields));
    session
}

fn asks_for_entity() -> Request {
    request(&wire(json!({})), None, &suggestion()).expect("a request")
}

fn asks_for_sound() -> Request {
    let wire = wire(json!({ "entityId": null, "soundId": sound() }));
    request(&wire, None, &suggestion()).expect("a request")
}

#[test]
fn every_refusal_says_why_in_words() {
    for refusal in [
        Refusal::NotAnId("sessionId"),
        Refusal::NamedRow,
        Refusal::OneTarget,
        Refusal::BlankText,
        Refusal::UnknownSession,
        Refusal::NotLive,
        Refusal::MicrophoneOff,
        Refusal::OutsideCampaign("sound"),
    ] {
        assert!(!refusal.to_string().trim().is_empty(), "{refusal:?}");
    }
    assert!(Refusal::NotAnId("soundId").to_string().contains("soundId"));
    assert!(
        Refusal::OutsideCampaign("sound")
            .to_string()
            .contains("sound")
    );
}

#[test]
fn only_an_unknown_session_is_a_404() {
    assert!(Refusal::UnknownSession.not_found());
    for refusal in [
        Refusal::NotAnId("sessionId"),
        Refusal::NamedRow,
        Refusal::OneTarget,
        Refusal::BlankText,
        Refusal::NotLive,
        Refusal::MicrophoneOff,
        Refusal::OutsideCampaign("element"),
    ] {
        assert!(!refusal.not_found(), "{refusal:?}");
    }
}

#[test]
fn a_request_for_an_element_reads_the_wire() {
    assert_eq!(
        asks_for_entity(),
        Request {
            id: suggestion(),
            session_id: session(),
            target: Target::Entity(entity()),
            matched_text: "la taverne".into(),
        }
    );
}

#[test]
fn a_request_for_a_sound_reads_the_wire() {
    assert_eq!(asks_for_sound().target, Target::Sound(sound()));
}

#[test]
fn the_ids_are_written_lowercase_and_the_words_trimmed() {
    let wire = wire(json!({
        "sessionId": session().to_uppercase(),
        "entityId": entity().to_uppercase(),
        "matchedText": "  la taverne \n",
    }));
    let read = request(&wire, None, &suggestion()).expect("a request");
    assert_eq!(read.session_id, session());
    assert_eq!(read.target, Target::Entity(entity()));
    assert_eq!(read.matched_text, "la taverne");
}

#[test]
fn a_payload_naming_both_an_entity_and_a_sound_is_refused() {
    let both = wire(json!({ "soundId": sound() }));
    assert_eq!(request(&both, None, &suggestion()), Err(Refusal::OneTarget));
}

#[test]
fn a_payload_naming_neither_is_refused() {
    for neither in [
        wire(json!({ "entityId": null })),
        wire(json!({ "entityId": null, "soundId": null })),
    ] {
        assert_eq!(
            request(&neither, None, &suggestion()),
            Err(Refusal::OneTarget)
        );
    }
    let mut absent = wire(json!({}));
    absent.remove("entityId");
    assert_eq!(
        request(&absent, None, &suggestion()),
        Err(Refusal::OneTarget)
    );
}

#[test]
fn a_blank_matched_text_is_refused() {
    for blank in [json!(""), json!("   \t"), json!(null), json!(7)] {
        let wire = wire(json!({ "matchedText": blank }));
        assert_eq!(
            request(&wire, None, &suggestion()),
            Err(Refusal::BlankText),
            "{wire:?}"
        );
    }
    let mut absent = wire(json!({}));
    absent.remove("matchedText");
    assert_eq!(
        request(&absent, None, &suggestion()),
        Err(Refusal::BlankText)
    );
}

#[test]
fn an_id_that_is_no_uuid_is_refused() {
    for (field, expected) in [
        ("sessionId", "sessionId"),
        ("entityId", "entityId"),
        ("soundId", "soundId"),
    ] {
        let bad = if field == "soundId" {
            wire(json!({ "entityId": null, "soundId": "x" }))
        } else {
            wire(json!({ field: "x" }))
        };
        assert_eq!(
            request(&bad, None, &suggestion()),
            Err(Refusal::NotAnId(expected))
        );
    }
    let numeric = wire(json!({ "sessionId": 3 }));
    assert_eq!(
        request(&numeric, None, &suggestion()),
        Err(Refusal::NotAnId("sessionId"))
    );
}

#[test]
fn a_missing_session_is_refused() {
    let mut wire = wire(json!({}));
    wire.remove("sessionId");
    assert_eq!(
        request(&wire, None, &suggestion()),
        Err(Refusal::NotAnId("sessionId"))
    );
}

#[test]
fn a_command_that_names_its_row_is_refused() {
    assert_eq!(
        request(&wire(json!({})), Some(&suggestion()), &suggestion()),
        Err(Refusal::NamedRow)
    );
}

#[test]
fn a_live_session_with_the_microphone_on_takes_an_element() {
    let written = prepare(&asks_for_entity(), &live(), 1_792_000_000_000).expect("written");
    assert_eq!(written.target_id, suggestion());
    assert_eq!(
        written.payload,
        row(json!({
            "id": suggestion(),
            "sessionId": session(),
            "entityId": entity(),
            "matchedText": "la taverne",
            "detectedAt": "2026-10-14T17:46:40.000Z",
        }))
    );
}

#[test]
fn a_live_session_with_the_microphone_on_takes_a_sound() {
    let written = prepare(&asks_for_sound(), &live(), 1_792_000_000_000).expect("written");
    assert_eq!(
        written.payload,
        row(json!({
            "id": suggestion(),
            "sessionId": session(),
            "soundId": sound(),
            "matchedText": "la taverne",
            "detectedAt": "2026-10-14T17:46:40.000Z",
        }))
    );
}

#[test]
fn the_detection_is_stamped_with_the_clock_it_is_given() {
    let at = |millis| prepare(&asks_for_entity(), &live(), millis).expect("written");
    assert_eq!(
        at(1_792_000_123_456).payload["detectedAt"],
        "2026-10-14T17:48:43.456Z"
    );
    assert_ne!(at(1).payload["detectedAt"], at(2).payload["detectedAt"]);
}

#[test]
fn an_unknown_session_is_unknown() {
    assert_eq!(
        prepare(&asks_for_entity(), &Row::new(), 0),
        Err(Refusal::UnknownSession)
    );
}

#[test]
fn a_session_in_prep_or_closed_is_refused() {
    for status in ["prep", "closed", ""] {
        let session = session_with(json!({ "status": status }));
        assert_eq!(
            prepare(&asks_for_entity(), &session, 0),
            Err(Refusal::NotLive),
            "{status}"
        );
    }
    let mut no_status = live();
    no_status.remove("status");
    assert_eq!(
        prepare(&asks_for_entity(), &no_status, 0),
        Err(Refusal::NotLive)
    );
}

#[test]
fn a_session_with_the_microphone_off_is_refused() {
    for off in [json!(false), json!(null), json!("true")] {
        let session = session_with(json!({ "microphone_on": off }));
        assert_eq!(
            prepare(&asks_for_entity(), &session, 0),
            Err(Refusal::MicrophoneOff),
            "{session:?}"
        );
    }
    let mut absent = live();
    absent.remove("microphone_on");
    assert_eq!(
        prepare(&asks_for_sound(), &absent, 0),
        Err(Refusal::MicrophoneOff)
    );
}

#[test]
fn an_element_of_another_campaign_is_refused() {
    // Only the element's flag counts for an element, only the sound's for a
    // sound.
    let session = session_with(json!({ "entity_in_campaign": false }));
    assert_eq!(
        prepare(&asks_for_entity(), &session, 0),
        Err(Refusal::OutsideCampaign("element"))
    );
    assert!(prepare(&asks_for_sound(), &session, 0).is_ok());
}

#[test]
fn a_sound_of_another_campaign_is_refused() {
    let session = session_with(json!({ "sound_in_campaign": false }));
    assert_eq!(
        prepare(&asks_for_sound(), &session, 0),
        Err(Refusal::OutsideCampaign("sound"))
    );
    assert!(prepare(&asks_for_entity(), &session, 0).is_ok());
}

#[test]
fn a_target_the_query_did_not_vouch_for_is_refused() {
    let mut session = live();
    session.remove("entity_in_campaign");
    assert_eq!(
        prepare(&asks_for_entity(), &session, 0),
        Err(Refusal::OutsideCampaign("element"))
    );
}

#[test]
fn the_instant_is_iso_8601_in_utc_to_the_millisecond() {
    // (milliseconds since the epoch, the instant): the epoch, a second
    // before a new year, a leap day, the day after it, a century year that
    // is not a leap year and one that is, the eve of a month's end.
    for (millis, instant) in [
        (0, "1970-01-01T00:00:00.000Z"),
        (946_684_799_999, "1999-12-31T23:59:59.999Z"),
        (951_782_400_000, "2000-02-29T00:00:00.000Z"),
        (951_868_800_000, "2000-03-01T00:00:00.000Z"),
        (1_709_164_800_000, "2024-02-29T00:00:00.000Z"),
        (1_709_251_200_000, "2024-03-01T00:00:00.000Z"),
        (1_798_675_199_000, "2026-12-30T23:59:59.000Z"),
        (4_107_456_000_000, "2100-02-28T00:00:00.000Z"),
        (4_107_542_400_000, "2100-03-01T00:00:00.000Z"),
        (1_792_000_000_000, "2026-10-14T17:46:40.000Z"),
        (1_792_003_723_004, "2026-10-14T18:48:43.004Z"),
    ] {
        let written = prepare(&asks_for_entity(), &live(), millis).expect("written");
        assert_eq!(written.payload["detectedAt"], instant, "{millis}");
    }
}
