//! The invariant of the `NoteImport` aggregate
//! (`crates/campaign/aggregates/NoteImport/aggregate.json`), which its rule
//! writes in words: the engine judges here what one proposal's row and its
//! command can say, on the row's state after the command. The table
//! (`crates/data/migrations/0008_note_import.sql`) judges it again, on the
//! rows written.
//!
//! The DM's notes are analysed into proposals — an element to create, its
//! type, its name, the excerpt behind it — that the DM checks or unchecks;
//! the validation creates the element of each checked proposal and records
//! it in `createdEntityId`. The analysis writes proposals, never elements.

use std::collections::BTreeMap;

use harness_dataguard::command::{Effect, Entry};
use harness_dataguard::plugins::{Judge, Plugin};
use serde_json::Value;

const AGGREGATE: &str = "NoteImport";

/// The element a validated proposal created.
const CREATED: &str = "createdEntityId";

/// Whether the DM checked the proposal.
const CHECKED: &str = "checked";

/// The aggregate's invariants as code: its judge.
pub struct NoteImport;

impl Judge for NoteImport {
    fn violated(&self, state: &BTreeMap<String, Value>, entry: &Entry) -> Vec<String> {
        // A deletion is the engine's tombstone: nothing of the row is left to
        // judge.
        if entry.effect == Effect::Delete || no_creation_before_validation(state, entry) {
            return Vec::new();
        }
        vec!["no-creation-before-validation".to_string()]
    }
}

/// What the aggregate plugs into the engine: its judge.
#[must_use]
pub fn plugin() -> Plugin {
    Plugin::new().judge(AGGREGATE, NoteImport)
}

/// `field`'s value in `row`, an absent field and a null being the same.
fn value<'a>(row: &'a BTreeMap<String, Value>, field: &str) -> &'a Value {
    row.get(field).unwrap_or(&Value::Null)
}

/// The analysis creates no element: a proposal is written with none. Only
/// the validation creates one, for a checked proposal — which stays checked
/// — that has none yet: a proposal that created its element never creates
/// another, nor forgets the one it created.
fn no_creation_before_validation(state: &BTreeMap<String, Value>, entry: &Entry) -> bool {
    let created = value(state, CREATED);
    let before = value(&entry.projection.confirmed, CREATED);
    // Written now: an insert, or an upsert of a row the engine does not hold
    // — an update never is, though the row it reads is empty when the command
    // is queued.
    let born = entry.effect == Effect::Insert
        || (entry.effect == Effect::Upsert && entry.projection.confirmed.is_empty());
    let written = !born || created.is_null();
    let checked = created.is_null() || value(state, CHECKED) == &Value::Bool(true);
    let once = before.is_null() || created == before;
    written && checked && once
}

#[cfg(test)]
mod tests {
    use super::{CHECKED, CREATED};

    #[test]
    fn no_creation_before_validation_knows_the_fields_the_aggregate_declares() {
        let aggregate: serde_json::Value =
            serde_json::from_str(include_str!("../aggregate.json")).expect("contract I");
        let fields = aggregate["fields"].as_object().expect("fields");
        for field in [CREATED, CHECKED] {
            assert!(fields.contains_key(field), "{field} is not declared");
        }
        let invariants: Vec<&str> = aggregate["invariants"]
            .as_array()
            .expect("invariants")
            .iter()
            .map(|invariant| invariant["id"].as_str().expect("an id"))
            .collect();
        assert_eq!(invariants, ["no-creation-before-validation"]);
    }
}
