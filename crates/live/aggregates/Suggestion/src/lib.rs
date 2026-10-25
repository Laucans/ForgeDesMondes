//! The invariants of the `Suggestion` aggregate
//! (`crates/live/aggregates/Suggestion/aggregate.json`), which its rules
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
//!
//! An applied write of a suggestion is a row of that table, inserted once, in
//! the transaction that applies it ([`Table`]): the pill's Capability reads
//! it from there.

use std::collections::BTreeMap;

use harness_dataguard::command::{Effect, Entry};
use harness_dataguard::plugins::{Judge, Plugin};
use serde_json::Value;

mod table;

pub use table::Table;

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

/// The aggregate's invariants as code: its judge.
pub struct Suggestion;

impl Judge for Suggestion {
    fn violated(&self, state: &BTreeMap<String, Value>, entry: &Entry) -> Vec<String> {
        // A deletion is the engine's tombstone: nothing of the row is left to
        // judge.
        if entry.effect == Effect::Delete {
            return Vec::new();
        }
        CHECKS
            .iter()
            .filter(|(_, holds)| !holds(state))
            .map(|(id, _)| (*id).to_string())
            .collect()
    }
}

/// What the aggregate plugs into the engine: its judge, and its table
/// (`live_suggestion`) written in the transaction that applies a write.
#[must_use]
pub fn plugin() -> Plugin {
    Plugin::new()
        .judge(AGGREGATE, Suggestion)
        .project_pg(AGGREGATE, Table)
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

/// Whether `id` is a uuid written 8-4-4-4-12 in hex digits, as the
/// table's keys are typed.
fn uuid(id: &str) -> bool {
    id.len() == 36
        && id.char_indices().all(|(at, c)| match at {
            8 | 13 | 18 | 23 => c == '-',
            _ => c.is_ascii_hexdigit(),
        })
}

/// Whether `field` names a row by its uuid, as the table's keys are typed.
fn id(row: &BTreeMap<String, Value>, field: &str) -> bool {
    row.get(field).and_then(Value::as_str).is_some_and(uuid)
}

/// Whether `field` holds an instant: a date, then a time — what PostgreSQL
/// reads into a `timestamptz` from ISO 8601.
fn instant(row: &BTreeMap<String, Value>, field: &str) -> bool {
    let Some(text) = row.get(field).and_then(Value::as_str) else {
        return false;
    };
    let digits = |range: std::ops::Range<usize>| {
        text.get(range)
            .is_some_and(|part| part.bytes().all(|b| b.is_ascii_digit()))
    };
    digits(0..4)
        && text.get(4..5) == Some("-")
        && digits(5..7)
        && text.get(7..8) == Some("-")
        && digits(8..10)
        && matches!(text.get(10..11), Some("T" | " "))
        && digits(11..13)
        && text.get(13..14) == Some(":")
        && digits(14..16)
}

/// A suggestion belongs to a session and aims at exactly one target: an
/// element or a sound, never both, never neither — each named by its uuid.
fn suggestion_target(state: &BTreeMap<String, Value>) -> bool {
    let entity = id(state, "entityId");
    let sound = id(state, "soundId");
    id(state, "sessionId")
        && (unset(state, "entityId") || entity)
        && (unset(state, "soundId") || sound)
        && entity != sound
}

/// Nothing is stored beyond the suggestion — no pile, no rank, no position
/// in the history: a field the aggregate does not declare holds no value —
/// and its detection's instant, which orders both, is set.
fn pile_and_history_derived(state: &BTreeMap<String, Value>) -> bool {
    state
        .iter()
        .all(|(field, value)| FIELDS.contains(&field.as_str()) || value.is_null())
        && instant(state, "detectedAt")
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
            serde_json::from_str(include_str!("../aggregate.json")).expect("contract I");
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

    #[test]
    fn a_uuid_is_written_8_4_4_4_12_in_hex_digits() {
        assert!(super::uuid("e1000000-0000-4000-8000-00000000000a"));
        assert!(super::uuid("E1000000-0000-4000-8000-00000000000A"));
        for id in [
            "e1000000000040008000000000000001",
            "e1000000-0000-4000-8000-0000000000011",
            "e100000-00000-4000-8000-000000000001",
            "e1000000-0000-4000-8000+000000000001",
            "g1000000-0000-4000-8000-000000000001",
            "",
        ] {
            assert!(!super::uuid(id), "{id}");
        }
    }
}
