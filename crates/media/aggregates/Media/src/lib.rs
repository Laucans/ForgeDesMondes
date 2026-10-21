//! The invariants of the `Media` aggregate
//! (`crates/media/aggregates/Media/aggregate.json`), which its rules
//! write in words: the engine judges them here, on the row's state after the
//! command — a media and its battlemap's grid, one row to the engine. The
//! `media` and `battlemap_grid` tables (`crates/data/migrations/`) judge
//! them there again.
//!
//! What a media's own row can say is judged here. `media-place-is-lieu`
//! reads the place, an element of the Entity aggregate the engine does not
//! hold beside the media: a change of place (`media.setMediaPlace`) is judged
//! against it before it is queued (`set_media_place::prepare`), and its
//! table's constraint trigger judges it again.

use std::collections::BTreeMap;

use harness_dataguard::command::{Effect, Entry};
use harness_dataguard::plugins::{Judge, Plugin};
use serde_json::Value;

const AGGREGATE: &str = "Media";

/// The kind of media that has a grid.
const BATTLEMAP: &str = "battlemap";

/// The grid's settings, which a battlemap has from its upload.
const GRID: [&str; 6] = [
    "hexSizePx",
    "offsetX",
    "offsetY",
    "orientation",
    "color",
    "opacity",
];

/// The grid's other fields: the battlemap it sets, and the feet a hex is
/// worth.
const GRID_LINKS: [&str; 2] = ["mediaId", "feetPerHex"];

/// What a hex is worth, in feet, whatever its size.
const FEET_PER_HEX: f64 = 5.0;

/// One invariant: its id, and whether it holds on the state the command
/// leaves.
type Check = (&'static str, fn(&BTreeMap<String, Value>, &Entry) -> bool);

const CHECKS: [Check; 3] = [
    ("grid-only-for-battlemap", grid_only_for_battlemap),
    ("hex-is-five-feet", hex_is_five_feet),
    ("grid-values-range", grid_values_range),
];

/// The aggregate's invariants as code: its judge.
pub struct Media;

impl Judge for Media {
    fn violated(&self, state: &BTreeMap<String, Value>, entry: &Entry) -> Vec<String> {
        // A deletion is the engine's tombstone: nothing of the row is left to
        // judge.
        if entry.effect == Effect::Delete {
            return Vec::new();
        }
        CHECKS
            .iter()
            .filter(|(_, holds)| !holds(state, entry))
            .map(|(id, _)| (*id).to_string())
            .collect()
    }
}

/// What the aggregate plugs into the engine: its judge.
#[must_use]
pub fn plugin() -> Plugin {
    Plugin::new().judge(AGGREGATE, Media)
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

/// A battlemap has every setting of its grid, and the grid sets it, not
/// another media; any other media — or one whose kind the row does not
/// say — has none of the grid's fields.
fn grid_only_for_battlemap(state: &BTreeMap<String, Value>, entry: &Entry) -> bool {
    if state.get("kind").and_then(Value::as_str) == Some(BATTLEMAP) {
        let media = state
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or(entry.target_id());
        GRID.iter().all(|field| set(state, field))
            && state
                .get("mediaId")
                .filter(|value| !value.is_null())
                .is_none_or(|id| id.as_str().is_some_and(|id| id.eq_ignore_ascii_case(media)))
    } else {
        GRID.iter()
            .chain(GRID_LINKS.iter())
            .all(|field| !set(state, field))
    }
}

/// A hex is worth 5 feet: the row says so, or says nothing of it.
fn hex_is_five_feet(state: &BTreeMap<String, Value>, _entry: &Entry) -> bool {
    state
        .get("feetPerHex")
        .filter(|value| !value.is_null())
        .is_none_or(|feet| number(feet) == Some(FEET_PER_HEX))
}

/// A hex's size, when the row has one, is a number above 0; the grid's
/// opacity a number from 0 to 1.
fn grid_values_range(state: &BTreeMap<String, Value>, _entry: &Entry) -> bool {
    let within = |field: &str, holds: fn(f64) -> bool| {
        state
            .get(field)
            .filter(|value| !value.is_null())
            .is_none_or(|value| number(value).is_some_and(holds))
    };
    within("hexSizePx", |size| size > 0.0)
        && within("opacity", |opacity| (0.0..=1.0).contains(&opacity))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use harness_dataguard::command::Entry;
    use harness_dataguard::plugins::Judge;
    use serde_json::{Value, json};

    use super::{GRID, GRID_LINKS, Media};

    /// A command of `effect` on `aggregate`'s row.
    fn entry(aggregate: &str, effect: &str) -> Entry {
        serde_json::from_value(json!({
            "command": "c1",
            "data_capability": "test.write@1",
            "by": "dm",
            "partition": format!("{aggregate}/a1"),
            "position": 1,
            "based_on": { "version": 0 },
            "payload": {},
            "effect": effect,
            "touches": [],
            "mode": "overwrite",
            "projection": { "confirmed": {}, "pending_ahead": [] },
            "state": "queued",
        }))
        .expect("an entry")
    }

    #[test]
    fn the_judge_leaves_a_tombstone_alone() {
        // A battlemap with no grid, at an opacity out of range: what a media
        // may not be, and what another aggregate's row may well hold.
        let state: BTreeMap<String, Value> = [
            ("kind".to_string(), json!("battlemap")),
            ("opacity".to_string(), json!(2)),
        ]
        .into_iter()
        .collect();
        assert_eq!(
            Media.violated(&state, &entry("Media", "update")),
            ["grid-only-for-battlemap", "grid-values-range"]
        );
        // Another aggregate's row is never handed to this judge: the engine
        // calls the judges of the aggregate written alone.
        assert!(Media.violated(&state, &entry("Media", "delete")).is_empty());
    }

    #[test]
    fn grid_only_for_battlemap_knows_the_grid_fields_the_aggregate_declares() {
        let aggregate: serde_json::Value =
            serde_json::from_str(include_str!("../aggregate.json")).expect("contract I");
        let fields = aggregate["fields"].as_object().expect("fields");
        for field in GRID.iter().chain(GRID_LINKS.iter()) {
            assert!(fields.contains_key(*field), "{field}");
        }
    }
}
