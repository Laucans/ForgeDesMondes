//! `session.addJournalLine`'s preparation, with no engine: what the wire asks
//! for, and the fields a live session with its microphone on takes — or why
//! it takes none.

use add_journal_line::{LINE_ID, Refusal, Request, Row, SPOKEN_AT, TEXT, prepare, request};
use platform_test_kit::writes_only_touches;
use serde_json::{Value, json};

const SESSION: &str = "51000000-0000-4000-8000-000000000004";
const LINE: &str = "1a000000-0000-4000-8000-000000000001";
const NOW: u64 = 1_792_231_200_123;

fn row(value: Value) -> Row {
    value
        .as_object()
        .expect("an object")
        .clone()
        .into_iter()
        .collect()
}

fn live() -> Row {
    row(json!({ "sessionId": SESSION, "sessionStatus": "live", "microphoneOn": true }))
}

fn asked(text: &str) -> Request {
    Request {
        session: SESSION.to_owned(),
        text: text.to_owned(),
    }
}

#[test]
fn the_phrase_heard_is_read_trimmed_on_the_session_the_command_targets() {
    let wire = row(json!({ "text": "  On reprend sous le pont.  " }));
    assert_eq!(
        request(&wire, Some(&SESSION.to_ascii_uppercase())),
        Ok(asked("On reprend sous le pont."))
    );
}

#[test]
fn a_blank_phrase_no_session_or_another_field_is_refused() {
    for text in [json!(""), json!("   "), json!(null), json!(3)] {
        assert_eq!(
            request(&row(json!({ "text": text })), Some(SESSION)),
            Err(Refusal::BlankText),
            "{text}"
        );
    }
    assert_eq!(request(&Row::new(), Some(SESSION)), Err(Refusal::BlankText));
    let wire = row(json!({ "text": "x" }));
    assert_eq!(request(&wire, None), Err(Refusal::NoSession));
    assert_eq!(request(&wire, Some("nothing")), Err(Refusal::NoSession));
    assert_eq!(
        request(&row(json!({ "text": "x", "audio": "…" })), Some(SESSION)),
        Err(Refusal::UnknownField("audio".to_owned()))
    );
}

#[test]
fn text_only_no_audio_a_live_session_takes_the_line_its_id_its_time_and_its_text() {
    let fields = prepare(&asked("Morlox avoue."), &live(), LINE, NOW).expect("taken");
    assert_eq!(
        fields,
        row(json!({ LINE_ID: LINE, SPOKEN_AT: NOW, TEXT: "Morlox avoue." }))
    );
    writes_only_touches(include_str!("../data-capability.json"), &fields);
}

#[test]
fn status_lifecycle_a_session_in_prep_closed_unknown_or_its_microphone_off_takes_none() {
    let mut prep = live();
    prep.insert("sessionStatus".to_owned(), json!("prep"));
    let mut closed = live();
    closed.insert("sessionStatus".to_owned(), json!("closed"));
    closed.insert("microphoneOn".to_owned(), json!(false));
    let mut off = live();
    off.insert("microphoneOn".to_owned(), json!(false));
    for (session, refusal) in [
        (Row::new(), Refusal::UnknownSession),
        (prep, Refusal::NotLive),
        (closed, Refusal::NotLive),
        (off, Refusal::MicrophoneOff),
    ] {
        assert_eq!(
            prepare(&asked("x"), &session, LINE, NOW),
            Err(refusal.clone())
        );
        assert_eq!(refusal.not_found(), refusal == Refusal::UnknownSession);
    }
    assert!(Refusal::NotLive.to_string().starts_with("status-lifecycle"));
}
