//! The invariants of the `SoundPlayback` aggregate
//! (`crates/dataguard/aggregates/SoundPlayback/aggregate.json`), which its
//! rules write in words: the engine judges here what one playback's row and
//! its command can say, on the row's state after the command. The
//! `sound_playback` table (`crates/data/migrations/0009_sound_playback.sql`)
//! holds them there: no uniqueness keeps two playbacks of a session apart,
//! and its origins are the DM's surfaces alone.
//!
//! A playback is one sound playing during a session, from the soundbox or
//! from a suggestion's pastille, until it stops — on its own or with every
//! other one of the session, by « Tout arrêter ».

use std::collections::BTreeMap;

use harness_dataguard::command::{Effect, Entry};
use harness_dataguard::guard::Invariants;
use serde_json::Value;

const AGGREGATE: &str = "SoundPlayback";

/// When the playback stopped; none while it plays.
const STOPPED: &str = "stoppedAt";

/// The DM's surface it was started from.
const ORIGIN: &str = "origin";

/// The surfaces a playback is started from: both the DM view's.
const DM_SURFACES: [&str; 2] = ["soundbox", "pastille"];

/// One invariant: its id, and whether it holds on the state the command
/// leaves, given the entry — whose `projection.confirmed` is the row as the
/// store held it when the command was queued (empty while the engine judges
/// it at enqueue), not as it is when the command is applied.
type Check = (&'static str, fn(&BTreeMap<String, Value>, &Entry) -> bool);

const CHECKS: [Check; 3] = [
    ("simultaneous-playback", simultaneous_playback),
    ("stop-all", stop_all),
    ("dm-side-only", dm_side_only),
];

/// The invariants `plant()` binds the engine with.
pub struct SoundPlayback;

impl Invariants for SoundPlayback {
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

/// `field`'s value in `row`, an absent field and a null being the same.
fn value<'a>(row: &'a BTreeMap<String, Value>, field: &str) -> &'a Value {
    row.get(field).unwrap_or(&Value::Null)
}

/// A sound triggered adds a playback that plays: written now, it has no
/// `stoppedAt`. Nothing else is asked of it — another playback of the
/// session, of the same sound even, playing or not, is no reason to refuse
/// it, and adding it stops none.
fn simultaneous_playback(state: &BTreeMap<String, Value>, entry: &Entry) -> bool {
    // Written now: an insert. An upsert's row cannot be told new from the
    // entry, whose projection is empty at enqueue and stale at apply time.
    entry.effect != Effect::Insert || value(state, STOPPED).is_null()
}

/// « Tout arrêter » stops the playbacks still playing: one that stopped
/// keeps the instant it stopped at, neither restarted nor stopped again.
///
/// The stop it had is the one the row had when the command was queued: a
/// stop is never undone, so a playback stopped then is stopped still. A
/// second stop queued behind a first one not yet applied saw none — that
/// is the queue's to catch (`mode: confirm_on_stale`), not this judge's.
fn stop_all(state: &BTreeMap<String, Value>, entry: &Entry) -> bool {
    let before = value(&entry.projection.confirmed, STOPPED);
    before.is_null() || value(state, STOPPED) == before
}

/// A playback is the DM view's: started from the soundbox or a pastille,
/// never from the player view — nor, added now, from nowhere said. A command
/// queued behind its playback's insert sees no row yet: it is judged on the
/// origin it says, if any.
fn dm_side_only(state: &BTreeMap<String, Value>, entry: &Entry) -> bool {
    match state.get(ORIGIN) {
        None => entry.effect != Effect::Insert,
        Some(origin) => origin
            .as_str()
            .is_some_and(|origin| DM_SURFACES.contains(&origin)),
    }
}

#[cfg(test)]
mod tests {
    use super::{CHECKS, ORIGIN, STOPPED};

    fn aggregate() -> serde_json::Value {
        serde_json::from_str(include_str!(
            "../../aggregates/SoundPlayback/aggregate.json"
        ))
        .expect("contract I")
    }

    #[test]
    fn the_checks_are_the_invariants_the_aggregate_declares() {
        let declared: Vec<String> = aggregate()["invariants"]
            .as_array()
            .expect("invariants")
            .iter()
            .map(|invariant| invariant["id"].as_str().expect("an id").to_string())
            .collect();
        let known: Vec<&str> = CHECKS.iter().map(|(id, _)| *id).collect();
        assert_eq!(known, declared);
    }

    #[test]
    fn the_checks_know_the_fields_the_aggregate_declares() {
        let aggregate = aggregate();
        let fields = aggregate["fields"].as_object().expect("fields");
        for field in [STOPPED, ORIGIN] {
            assert!(fields.contains_key(field), "{field} is not declared");
        }
    }
}
