//! The invariants of the `Diffusion` aggregate
//! (`crates/dataguard/aggregates/Diffusion/aggregate.json`), which its rules
//! write in words: the engine judges them here, on the row's state after the
//! command — what a session's player view shows, one row per session.
//!
//! What the row itself can say is judged here. What it points to is not:
//! that the background's or the window's media is an illustration, and that
//! the background's instance is the session's own, read rows of the Media
//! and BattlemapInstance aggregates the engine does not hold beside the
//! diffusion. The `diffusion` table (`crates/data/migrations/0012_diffusion.sql`)
//! holds one row per session and stores no window height.

use std::collections::BTreeMap;

use harness_dataguard::command::{Effect, Entry};
use harness_dataguard::guard::Invariants;
use serde_json::Value;

const AGGREGATE: &str = "Diffusion";

/// The background's media and instance.
const BASE_MEDIA: &str = "baseMediaId";
const BASE_INSTANCE: &str = "baseInstanceId";

/// The media the floating window shows: the window is open when it is set.
const WINDOW_MEDIA: &str = "windowMediaId";

/// The window's position and width, in fractions of the screen.
const WINDOW: [&str; 3] = ["windowX", "windowY", "windowWidth"];

/// The height the window never stores: it follows from its width and its
/// media's proportions.
const WINDOW_HEIGHT: &str = "windowHeight";

/// One invariant: its id, and whether it holds on the state the command
/// leaves.
type Check = (&'static str, fn(&BTreeMap<String, Value>, &Entry) -> bool);

const CHECKS: [Check; 4] = [
    ("base-coherent", base_coherent),
    ("window-proportional", window_proportional),
    ("window-fields-together", window_fields_together),
    ("single-diffusion-source", single_diffusion_source),
];

/// The invariants `plant()` binds the engine with.
pub struct Diffusion;

impl Invariants for Diffusion {
    fn violated(
        &self,
        aggregate: &str,
        state: &BTreeMap<String, Value>,
        entry: &Entry,
    ) -> Vec<String> {
        // A deletion is the engine's tombstone: nothing of the row is left to
        // judge.
        if aggregate != AGGREGATE || entry.effect == Effect::Delete {
            return Vec::new();
        }
        CHECKS
            .iter()
            .filter(|(_, holds)| !holds(state, entry))
            .map(|(id, _)| (*id).to_string())
            .collect()
    }
}

/// Whether the row holds a value for `field`: a `null` is none.
fn set(state: &BTreeMap<String, Value>, field: &str) -> bool {
    state.get(field).is_some_and(|value| !value.is_null())
}

/// A finite number, as JSON writes it or as PostgreSQL's `numeric` reads
/// as text — never `inf` nor `NaN`, which Rust would parse.
fn number(value: &Value) -> Option<f64> {
    match value {
        Value::Number(n) => n.as_f64(),
        Value::String(text) => text.trim().parse::<f64>().ok(),
        _ => None,
    }
    .filter(|n| n.is_finite())
}

/// The background is one thing: a cover has neither a media nor an
/// instance, an illustration its media alone, a battlemap its instance
/// alone. A kind outside the three, or none, is no background.
fn base_coherent(state: &BTreeMap<String, Value>, _entry: &Entry) -> bool {
    let (media, instance) = (set(state, BASE_MEDIA), set(state, BASE_INSTANCE));
    match state.get("baseKind").and_then(Value::as_str) {
        Some("cover") => !media && !instance,
        Some("illustration") => media && !instance,
        Some("battlemap") => instance && !media,
        _ => false,
    }
}

/// The window keeps its media's proportions: the row stores no height, and
/// a width, when it has one, is a number above 0 the height can follow.
fn window_proportional(state: &BTreeMap<String, Value>, _entry: &Entry) -> bool {
    !set(state, WINDOW_HEIGHT)
        && state
            .get("windowWidth")
            .filter(|value| !value.is_null())
            .is_none_or(|width| number(width).is_some_and(|width| width > 0.0))
}

/// An open window — one that shows a media — has its position and its
/// width; a closed one has none of them.
fn window_fields_together(state: &BTreeMap<String, Value>, _entry: &Entry) -> bool {
    let open = set(state, WINDOW_MEDIA);
    WINDOW.iter().all(|field| set(state, field) == open)
}

/// The row is its session's: it names its session, which is the id the
/// command targets — the very text the engine keys the row by — so the
/// projected player view and the DM's tab read the one row.
fn single_diffusion_source(state: &BTreeMap<String, Value>, entry: &Entry) -> bool {
    state.get("sessionId").and_then(Value::as_str) == Some(entry.target_id())
}

#[cfg(test)]
mod tests {
    use super::{BASE_INSTANCE, BASE_MEDIA, CHECKS, WINDOW, WINDOW_HEIGHT, WINDOW_MEDIA};

    fn aggregate() -> serde_json::Value {
        serde_json::from_str(include_str!("../../aggregates/Diffusion/aggregate.json"))
            .expect("contract I")
    }

    #[test]
    fn the_checks_are_the_invariants_the_aggregate_declares() {
        let aggregate = aggregate();
        let declared: Vec<&str> = aggregate["invariants"]
            .as_array()
            .expect("invariants")
            .iter()
            .map(|invariant| invariant["id"].as_str().expect("an id"))
            .collect();
        let known: Vec<&str> = CHECKS.iter().map(|(id, _)| *id).collect();
        assert_eq!(known, declared);
    }

    #[test]
    fn the_checks_read_the_fields_the_aggregate_declares_and_no_height() {
        let aggregate = aggregate();
        let fields = aggregate["fields"].as_object().expect("fields");
        for field in [
            BASE_MEDIA,
            BASE_INSTANCE,
            WINDOW_MEDIA,
            "baseKind",
            "sessionId",
        ]
        .iter()
        .chain(WINDOW.iter())
        {
            assert!(fields.contains_key(*field), "{field}");
        }
        assert!(!fields.contains_key(WINDOW_HEIGHT));
    }
}
