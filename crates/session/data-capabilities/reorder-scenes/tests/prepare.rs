//! session.reorderScenes as a function of its request and its session's
//! scenes: what it asks ([`asked`]), and the order's row it writes, or why
//! it may not ([`ordered`]) — an order that omits a scene, repeats one or
//! names a scene of another session breaks `scene-order-contiguous`.

use reorder_scenes::{Asked, Refusal, Row, asked, ordered};
use serde_json::{Value, json};

const SEANCE: &str = "51000000-0000-4000-8000-000000000004";
const A: &str = "5c000000-0000-4000-8000-00000000000a";
const B: &str = "5c000000-0000-4000-8000-00000000000b";
const C: &str = "5c000000-0000-4000-8000-00000000000c";
/// Session 5's scene.
const X: &str = "5c000000-0000-4000-8000-0000000000ff";

fn wire(payload: Value) -> Row {
    let Value::Object(payload) = payload else {
        panic!("a payload is an object");
    };
    payload.into_iter().collect()
}

fn reorder(order: &[&str]) -> Asked {
    Asked {
        session: SEANCE.to_owned(),
        order: order.iter().map(|id| (*id).to_owned()).collect(),
    }
}

fn scenes() -> Vec<String> {
    [A, B, C].iter().map(|id| (*id).to_owned()).collect()
}

#[test]
fn a_request_names_its_session_and_its_scenes_lowercase_trimmed_in_order() {
    assert_eq!(
        asked(&wire(json!({
            "session": format!(" {} ", SEANCE.to_uppercase()),
            "order": [C.to_uppercase(), A, format!("{B} ")],
        }))),
        Ok(reorder(&[C, A, B]))
    );
}

#[test]
fn a_request_without_a_session_or_an_order_of_scene_ids_is_refused() {
    for (payload, refusal) in [
        (json!({ "order": [A] }), Refusal::UnknownSession),
        (
            json!({ "session": "seance-4", "order": [A] }),
            Refusal::UnknownSession,
        ),
        (
            json!({ "session": 4, "order": [A] }),
            Refusal::UnknownSession,
        ),
        (json!({ "session": SEANCE }), Refusal::NoOrder),
        (json!({ "session": SEANCE, "order": [] }), Refusal::NoOrder),
        (json!({ "session": SEANCE, "order": A }), Refusal::NoOrder),
        (
            json!({ "session": SEANCE, "order": [A, "la-route"] }),
            Refusal::NoOrder,
        ),
        // One digit short, or one too many: no uuid, however well placed
        // its dashes.
        (
            json!({ "session": &SEANCE[..35], "order": [A] }),
            Refusal::UnknownSession,
        ),
        // A scene order's own id is no session's.
        (
            json!({ "session": "51000000-0000-c000-8000-000000000004", "order": [A] }),
            Refusal::UnknownSession,
        ),
        (
            json!({ "session": SEANCE, "order": [A, format!("{B}0")] }),
            Refusal::NoOrder,
        ),
        (
            json!({ "session": SEANCE, "order": [A, 2] }),
            Refusal::NoOrder,
        ),
        (
            json!({ "session": SEANCE, "order": [A], "positions": [1] }),
            Refusal::Unknown("positions".to_owned()),
        ),
    ] {
        assert_eq!(asked(&wire(payload.clone())), Err(refusal), "{payload}");
    }
}

#[test]
fn an_order_naming_a_scene_twice_is_refused_whatever_its_case() {
    assert_eq!(
        asked(&wire(
            json!({ "session": SEANCE, "order": [C, A, B, A.to_uppercase()] })
        )),
        Err(Refusal::Repeated(A.to_owned()))
    );
}

#[test]
fn an_order_of_every_scene_of_the_session_once_writes_their_ids_in_its_order() {
    assert_eq!(
        ordered(&reorder(&[C, A, B]), &scenes()),
        Ok(Row::from([("scenePosition".to_owned(), json!([C, A, B]))]))
    );
}

#[test]
fn an_order_omitting_a_scene_or_naming_another_session_s_is_refused() {
    assert_eq!(
        ordered(&reorder(&[C, A]), &scenes()),
        Err(Refusal::Omitted(B.to_owned()))
    );
    assert_eq!(
        ordered(&reorder(&[C, A, B, X]), &scenes()),
        Err(Refusal::Foreign(X.to_owned()))
    );
    assert_eq!(
        ordered(&reorder(&[C, X, B]), &scenes()),
        Err(Refusal::Foreign(X.to_owned()))
    );
}

#[test]
fn a_refusal_of_the_order_names_the_invariant_it_breaks() {
    for refusal in [
        Refusal::Repeated(A.to_owned()),
        Refusal::Omitted(A.to_owned()),
        Refusal::Foreign(A.to_owned()),
    ] {
        let said = refusal.to_string();
        assert!(said.starts_with("scene-order-contiguous : "), "{said}");
        assert!(said.contains(A), "{said}");
    }
    for refusal in [
        Refusal::Unknown("positions".to_owned()),
        Refusal::UnknownSession,
        Refusal::NoOrder,
    ] {
        let said = refusal.to_string();
        assert!(!said.contains("scene-order-contiguous"), "{said}");
        assert!(!said.is_empty());
    }
    assert!(
        Refusal::Unknown("positions".to_owned())
            .to_string()
            .starts_with("positions : ")
    );
}
