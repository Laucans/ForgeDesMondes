//! session.reorderScenes's preparation: the wire's `{session, order}` made
//! the order the command writes on the session's partition.

use reorder_scenes::{ORDER, Refusal, Row, names, prepare};
use serde_json::{Value, json};

const SEANCE: &str = "51000000-0000-4000-8000-000000000001";
const A: &str = "5c000000-0000-4000-8000-00000000000a";
const B: &str = "5c000000-0000-4000-8000-00000000000b";
const C: &str = "5c000000-0000-4000-8000-00000000000c";

fn wire(payload: Value) -> Row {
    let Value::Object(payload) = payload else {
        panic!("a payload is an object");
    };
    payload.into_iter().collect()
}

#[test]
fn it_names_itself_with_or_without_its_version() {
    assert!(names("session.reorderScenes"));
    assert!(names("session.reorderScenes@1"));
    assert!(!names("session.addScene"));
    assert!(!names("session.reorderScene"));
}

#[test]
fn the_order_is_written_whole_on_the_session_s_partition_in_its_order() {
    let prepared =
        prepare(&wire(json!({ "session": SEANCE, "order": [C, A, B] }))).expect("prepared");
    assert_eq!(prepared.target_id, SEANCE);
    assert_eq!(prepared.payload, wire(json!({ ORDER: [C, A, B] })));
}

#[test]
fn the_ids_are_trimmed_and_lowercase_and_kept_as_given_repeats_included() {
    let prepared = prepare(&wire(json!({
        "session": format!(" {} ", SEANCE.to_uppercase()),
        "order": [C.to_uppercase(), format!(" {A}"), C],
        "extra": "ignored",
    })))
    .expect("prepared");
    assert_eq!(prepared.target_id, SEANCE);
    // A repeat is the DataGuard's to reject (scene-order-contiguous), never
    // smoothed away here.
    assert_eq!(prepared.payload, wire(json!({ ORDER: [C, A, C] })));
}

#[test]
fn a_reorder_naming_no_session_is_refused() {
    for payload in [
        json!({ "order": [A] }),
        json!({ "session": null, "order": [A] }),
        json!({ "session": "   ", "order": [A] }),
        json!({ "session": 51, "order": [A] }),
        json!({ "session": "a\u{0}b", "order": [A] }),
    ] {
        assert_eq!(
            prepare(&wire(payload.clone())),
            Err(Refusal::NoSession),
            "{payload}"
        );
    }
}

#[test]
fn a_reorder_with_no_list_of_scene_ids_is_refused() {
    for payload in [
        json!({ "session": SEANCE }),
        json!({ "session": SEANCE, "order": null }),
        json!({ "session": SEANCE, "order": [] }),
        json!({ "session": SEANCE, "order": A }),
        json!({ "session": SEANCE, "order": { "1": A } }),
        json!({ "session": SEANCE, "order": [A, 2] }),
        json!({ "session": SEANCE, "order": [A, null] }),
        json!({ "session": SEANCE, "order": [A, " "] }),
        json!({ "session": SEANCE, "order": [A, "b\u{0}"] }),
    ] {
        assert_eq!(
            prepare(&wire(payload.clone())),
            Err(Refusal::NoOrder),
            "{payload}"
        );
    }
}

#[test]
fn each_refusal_says_why_in_words() {
    assert_eq!(
        Refusal::NoSession.to_string(),
        "un réordonnancement nomme sa séance"
    );
    assert_eq!(
        Refusal::NoOrder.to_string(),
        "un réordonnancement donne la liste des scènes de la séance"
    );
}
