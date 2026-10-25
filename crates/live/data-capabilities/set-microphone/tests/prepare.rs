//! `live.setMicrophone`'s pure core, no engine: [`request`] reads the wire, [`prepare`]
//! judges the row. The microphone on moves a session in prep to live and dates it
//! once, a closed session refuses (status-lifecycle), the microphone off writes the
//! microphone alone, and the row written is the manifest's `touches` and nothing else
//! (text-only-no-audio: a boolean, a status, an instant).

use serde_json::{Value, json};

use platform_test_kit::{manifest, uuid, writes_only_touches};
use set_microphone::{Refusal, Row, prepare, request};

const MANIFEST: &str = include_str!("../data-capability.json");

/// A row, from a JSON object.
fn row(value: Value) -> Row {
    serde_json::from_value(value).expect("an object")
}

/// The session as `SESSION` answers for it: its status, its microphone, whether it
/// was ever started.
fn session(status: &str, microphone: bool, started: bool) -> Row {
    row(json!({
        "id": uuid(0x5e, 1),
        "campaign_id": uuid(0xca, 1),
        "status": status,
        "microphone_on": microphone,
        "started": started,
    }))
}

/// The microphone turned `on`, in the campaign's session.
fn asking(on: bool) -> set_microphone::Request {
    request(&row(json!({ "on": on })), None).expect("a request")
}

#[test]
fn every_refusal_says_why_in_words() {
    for refusal in [
        Refusal::NotAnId("target"),
        Refusal::NotABoolean,
        Refusal::NoSession,
        Refusal::Closed,
    ] {
        assert!(!refusal.to_string().trim().is_empty(), "{refusal:?}");
    }
    assert!(Refusal::Closed.to_string().contains("status-lifecycle"));
}

#[test]
fn only_what_is_not_there_is_a_404() {
    assert!(Refusal::NoSession.not_found());
    for refusal in [
        Refusal::NotAnId("target"),
        Refusal::NotABoolean,
        Refusal::Closed,
    ] {
        assert!(!refusal.not_found(), "{refusal:?}");
    }
}

#[test]
fn the_microphone_is_on_or_off_never_anything_else() {
    for on in [json!("true"), json!(1), json!(null)] {
        assert_eq!(
            request(&row(json!({ "on": on })), None),
            Err(Refusal::NotABoolean)
        );
    }
    assert_eq!(
        request(&row(json!({})), None),
        Err(Refusal::NotABoolean),
        "no `on` is no answer"
    );
    assert!(request(&row(json!({ "on": true })), None).expect("on").on);
    assert!(!request(&row(json!({ "on": false })), None).expect("off").on);
}

#[test]
fn a_target_is_a_session_by_its_uuid_or_nothing() {
    let wire = row(json!({ "on": true }));
    assert_eq!(request(&wire, None).expect("none").session, None);
    let id = uuid(0x5e, 1);
    assert_eq!(
        request(&wire, Some(&id.to_uppercase()))
            .expect("an id")
            .session,
        Some(id)
    );
    assert_eq!(request(&wire, Some("x")), Err(Refusal::NotAnId("target")));
}

#[test]
fn a_session_in_prep_goes_live_and_is_dated_when_the_microphone_is_turned_on() {
    let prepared = prepare(
        &asking(true),
        &session("prep", false, false),
        1_792_000_000_000,
    )
    .expect("prepared");
    assert_eq!(prepared.target_id, uuid(0x5e, 1));
    assert_eq!(prepared.campaign, uuid(0xca, 1));
    assert_eq!(
        prepared.payload,
        row(json!({
            "microphoneOn": true,
            "sessionStatus": "live",
            "startedAt": 1_792_000_000_000_u64,
        }))
    );
}

#[test]
fn a_session_already_started_keeps_its_start() {
    let prepared = prepare(
        &asking(true),
        &session("live", false, true),
        1_792_000_000_000,
    )
    .expect("prepared");
    assert_eq!(prepared.payload, row(json!({ "microphoneOn": true })));
}

#[test]
fn a_live_session_never_dated_is_dated_without_changing_its_status() {
    let prepared = prepare(&asking(true), &session("live", true, false), 5).expect("prepared");
    assert_eq!(
        prepared.payload,
        row(json!({ "microphoneOn": true, "startedAt": 5 }))
    );
}

#[test]
fn a_closed_session_refuses_the_microphone_and_writes_nothing() {
    assert_eq!(
        prepare(&asking(true), &session("closed", false, true), 5),
        Err(Refusal::Closed)
    );
}

#[test]
fn the_microphone_turned_off_writes_the_microphone_alone() {
    for (status, started) in [("prep", false), ("live", true), ("closed", true)] {
        let prepared =
            prepare(&asking(false), &session(status, true, started), 5).expect("prepared");
        assert_eq!(
            prepared.payload,
            row(json!({ "microphoneOn": false })),
            "{status}"
        );
    }
}

#[test]
fn no_session_is_unknown() {
    assert_eq!(
        prepare(&asking(true), &Row::new(), 5),
        Err(Refusal::NoSession)
    );
    // A row the query answers without its campaign or status is no session either.
    assert_eq!(
        prepare(&asking(true), &row(json!({ "id": uuid(0x5e, 1) })), 5),
        Err(Refusal::NoSession)
    );
}

#[test]
fn what_is_written_is_what_the_manifest_touches() {
    for (on, status, started) in [
        (true, "prep", false),
        (true, "live", false),
        (true, "live", true),
        (false, "live", true),
    ] {
        let prepared = prepare(&asking(on), &session(status, !on, started), 5).expect("prepared");
        writes_only_touches(MANIFEST, &prepared.payload);
    }
}

#[test]
fn the_manifest_is_an_upsert_of_the_session_declaring_what_it_listens_through() {
    let manifest = manifest(MANIFEST);
    assert_eq!(manifest.data_capability, "live.setMicrophone");
    assert_eq!(format!("{:?}", manifest.effect), "Upsert");
    assert_eq!(manifest.target.aggregate, "Session");
    assert_eq!(
        manifest.touches,
        [
            "Session.microphoneOn",
            "Session.sessionStatus",
            "Session.startedAt"
        ]
    );
    assert!(manifest.payload.contains_key("on"));
    for id in ["status-lifecycle", "text-only-no-audio"] {
        assert!(manifest.invariants.iter().any(|i| i == id), "{id}");
    }
    let json: Value = serde_json::from_str(MANIFEST).expect("json");
    assert_eq!(json["ports"], json!(["speech-to-text"]));
}
