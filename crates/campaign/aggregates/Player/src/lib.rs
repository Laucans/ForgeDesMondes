//! The invariant of the `Player` aggregate
//! (`crates/campaign/aggregates/Player/aggregate.json`), which its rule
//! writes in words: the engine judges it here, on the row's state after the
//! command. The `player` table (`crates/data/migrations/`) has no column for
//! anything else either.
//!
//! A player is a real person at the table, not one of the campaign's
//! elements: a name and notes, never a sheet or a token. Their PCs point to
//! them (`character.player_id`, the Entity aggregate's), never the other way
//! round, so a player's row names no character either.

use std::collections::BTreeMap;

use harness_dataguard::command::{Effect, Entry};
use harness_dataguard::plugins::{Judge, Plugin};
use serde_json::Value;

const AGGREGATE: &str = "Player";

/// What a player is: the fields `aggregate.json` declares, and nothing else.
const FIELDS: [&str; 5] = ["id", "campaignId", "name", "notes", "createdAt"];

/// The aggregate's invariants as code: its judge.
pub struct Player;

impl Judge for Player {
    fn violated(&self, state: &BTreeMap<String, Value>, entry: &Entry) -> Vec<String> {
        // A deletion is the engine's tombstone: nothing of the row is left to
        // judge.
        if entry.effect == Effect::Delete || player_not_character(state, entry) {
            return Vec::new();
        }
        vec!["player-not-character".to_string()]
    }
}

/// What the aggregate plugs into the engine: its judge.
#[must_use]
pub fn plugin() -> Plugin {
    Plugin::new().judge(AGGREGATE, Player)
}

/// A player's row holds what a player is and nothing of a campaign element —
/// no sheet, no token, no link to a PC — and no command writes such a field,
/// in its payload or its `touches` — a touch of another aggregate's field
/// least of all.
fn player_not_character(state: &BTreeMap<String, Value>, entry: &Entry) -> bool {
    let touched = entry.touches.iter().map(|touch| {
        touch
            .strip_prefix(AGGREGATE)
            .and_then(|field| field.strip_prefix('.'))
            .unwrap_or(touch)
    });
    state
        .keys()
        .chain(entry.payload.keys())
        .map(String::as_str)
        .chain(touched)
        .all(|field| FIELDS.contains(&field))
}

#[cfg(test)]
mod tests {
    use super::FIELDS;

    #[test]
    fn player_not_character_knows_the_fields_the_aggregate_declares() {
        let aggregate: serde_json::Value =
            serde_json::from_str(include_str!("../aggregate.json")).expect("contract I");
        let mut declared: Vec<&str> = aggregate["fields"]
            .as_object()
            .expect("fields")
            .keys()
            .map(String::as_str)
            .collect();
        let mut known = FIELDS.to_vec();
        declared.sort_unstable();
        known.sort_unstable();
        assert_eq!(known, declared);
    }
}
