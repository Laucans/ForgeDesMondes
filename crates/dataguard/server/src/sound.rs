//! The invariants of the `Sound` aggregate
//! (`crates/dataguard/aggregates/Sound/aggregate.json`), which its rules
//! write in words: the engine judges them here, on the row's state after the
//! command. The `sound` table's CHECKs (`crates/data/migrations/`) judge
//! them there again, under the same ids.
//!
//! A sound stores its own volume and its tags, nothing computed from them:
//! the effective volume (volume × the campaign's master volume) is the
//! player's to compute, and the tag filter reads the tags as they are.

use std::collections::BTreeMap;
use std::collections::BTreeSet;

use harness_dataguard::command::{Effect, Entry};
use harness_dataguard::guard::Invariants;
use serde_json::Value;

const AGGREGATE: &str = "Sound";

/// One invariant: its id, and whether it holds on the state the command
/// leaves.
type Check = (&'static str, fn(&BTreeMap<String, Value>) -> bool);

const CHECKS: [Check; 2] = [
    ("sound-volume-range", sound_volume_range),
    ("tags-normalized", tags_normalized),
];

/// The invariants `plant()` binds the engine with.
pub struct Sound;

impl Invariants for Sound {
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

/// The sound's own volume, when the row has one, is a number from 0 to 1 —
/// a JSON number, or the text of one as PostgreSQL's `numeric` reads.
fn sound_volume_range(state: &BTreeMap<String, Value>) -> bool {
    let volume = match state.get("volume") {
        None => return true,
        Some(Value::Number(n)) => n.as_f64(),
        Some(Value::String(text)) => text.trim().parse::<f64>().ok(),
        Some(_) => None,
    };
    volume.is_some_and(|v| (0.0..=1.0).contains(&v))
}

/// The tags, when the row has them, are a list of texts, each in lowercase,
/// none twice.
fn tags_normalized(state: &BTreeMap<String, Value>) -> bool {
    let Some(tags) = state.get("tags") else {
        return true;
    };
    let Some(tags) = tags.as_array() else {
        return false;
    };
    let mut seen = BTreeSet::new();
    tags.iter().all(|tag| {
        tag.as_str()
            .is_some_and(|tag| tag.to_lowercase() == tag && seen.insert(tag))
    })
}

#[cfg(test)]
mod tests {
    use super::CHECKS;

    #[test]
    fn the_checks_are_the_invariants_the_aggregate_declares() {
        let aggregate: serde_json::Value =
            serde_json::from_str(include_str!("../../aggregates/Sound/aggregate.json"))
                .expect("contract I");
        let declared: Vec<&str> = aggregate["invariants"]
            .as_array()
            .expect("invariants")
            .iter()
            .map(|invariant| invariant["id"].as_str().expect("an id"))
            .collect();
        let known: Vec<&str> = CHECKS.iter().map(|(id, _)| *id).collect();
        assert_eq!(known, declared);
    }
}
