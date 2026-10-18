//! The invariants of the `Suggestion` aggregate
//! (`crates/dataguard/aggregates/Suggestion/aggregate.json`), which its rules
//! write in words: the engine judges them here, on the row's state after the
//! command. The `live_suggestion` table (`crates/data/migrations/`) keeps
//! their shape: a session, an element or a sound, the words heard and when.
//!
//! A suggestion is an element or a sound the microphone recognised during a
//! live session. What its own row can say is judged here. That its target
//! belongs to the session's campaign, and that the session was live with
//! the microphone on, read the Session aggregate, which the engine does not
//! hold beside the suggestion: the DataCapability that produces suggestions
//! reads the session before it writes one.

use std::collections::BTreeMap;

use harness_dataguard::command::{Effect, Entry};
use harness_dataguard::guard::Invariants;
use serde_json::Value;

const AGGREGATE: &str = "Suggestion";

/// The fields a suggestion stores, and nothing else: the pill's pile and
/// the history are read from them.
const FIELDS: [&str; 6] = [
    "id",
    "sessionId",
    "entityId",
    "soundId",
    "matchedText",
    "detectedAt",
];

/// One invariant: its id, and whether it holds on the state the command
/// leaves.
type Check = (&'static str, fn(&BTreeMap<String, Value>) -> bool);

const CHECKS: [Check; 3] = [
    ("suggestion-target", suggestion_target),
    ("pile-and-history-derived", pile_and_history_derived),
    ("live-only-local-audio", live_only_local_audio),
];

/// The invariants `plant()` binds the engine with.
pub struct Suggestion;

impl Invariants for Suggestion {
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
            .filter(|(_, holds)| !holds(state))
            .map(|(id, _)| (*id).to_string())
            .collect()
    }
}

/// Whether `field` holds a non-blank text in `row`.
fn text(row: &BTreeMap<String, Value>, field: &str) -> bool {
    row.get(field)
        .and_then(Value::as_str)
        .is_some_and(|text| !text.trim().is_empty())
}

/// Whether `field` holds no value in `row`: absent or `null`.
fn unset(row: &BTreeMap<String, Value>, field: &str) -> bool {
    row.get(field).is_none_or(Value::is_null)
}

/// A suggestion belongs to a session and aims at exactly one target: an
/// element or a sound, never both, never neither.
fn suggestion_target(state: &BTreeMap<String, Value>) -> bool {
    let entity = text(state, "entityId");
    let sound = text(state, "soundId");
    text(state, "sessionId")
        && (unset(state, "entityId") || entity)
        && (unset(state, "soundId") || sound)
        && entity != sound
}

/// Nothing is stored beyond the suggestion — no pile, no rank, no position
/// in the history — and its detection's instant, which orders both, is set.
fn pile_and_history_derived(state: &BTreeMap<String, Value>) -> bool {
    state.keys().all(|field| FIELDS.contains(&field.as_str())) && text(state, "detectedAt")
}

/// What the row keeps of the microphone is the words the local
/// transcription matched: a text, never a recording.
fn live_only_local_audio(state: &BTreeMap<String, Value>) -> bool {
    text(state, "matchedText")
}

#[cfg(test)]
mod tests {
    use super::{CHECKS, FIELDS};

    #[test]
    fn the_checks_and_fields_are_the_ones_the_aggregate_declares() {
        let aggregate: serde_json::Value =
            serde_json::from_str(include_str!("../../aggregates/Suggestion/aggregate.json"))
                .expect("contract I");
        let declared: Vec<&str> = aggregate["invariants"]
            .as_array()
            .expect("invariants")
            .iter()
            .map(|invariant| invariant["id"].as_str().expect("an id"))
            .collect();
        let known: Vec<&str> = CHECKS.iter().map(|(id, _)| *id).collect();
        assert_eq!(known, declared);
        let mut fields: Vec<&str> = aggregate["fields"]
            .as_object()
            .expect("fields")
            .keys()
            .map(String::as_str)
            .collect();
        let mut stored = FIELDS.to_vec();
        fields.sort_unstable();
        stored.sort_unstable();
        assert_eq!(stored, fields);
    }
}
