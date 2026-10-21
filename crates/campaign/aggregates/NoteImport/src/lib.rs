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
//!
//! Its writes reach the product's tables in the transaction that applies
//! them (`projection`), and every proposal applied is recorded among its
//! note's ([`Notes`]), which the plugin shares for a validation to read: the
//! engine reads a row by its partition, and a note's row names none of its
//! proposals. Its two DataCapabilities — the analysis (`ai.extractFromNotes`)
//! and the validation (`campaign.createEntitiesFromNote`) — each submit
//! more than one command: the first one's `Prepare` asks the engine for the
//! others ([`Announced`]).

use std::collections::BTreeMap;
use std::sync::Arc;

use harness_dataguard::command::{Effect, Entry};
use harness_dataguard::plugins::{Judge, Plugin, Project};
use harness_dataguard::store::Write;
use serde_json::Value;

mod announced;
mod notes;
mod projection;
mod time;

pub use announced::Announced;
pub use notes::Notes;
pub use time::timestamp;

/// The aggregate's manifest (contract I), as the engine parses it.
pub const MANIFEST: &str = include_str!("../aggregate.json");

const AGGREGATE: &str = "NoteImport";

/// The partitions of the aggregate: `NoteImport/<note or proposal id>`.
const PARTITION: &str = "NoteImport/";

/// The proposal's field naming its note.
const NOTE: &str = "noteId";

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

/// What the aggregate plugs into the engine: its judge; what follows an
/// applied write — the proposal recorded among its note's, over the
/// engine's memory and a database alike, and over a database its tables,
/// in the apply transaction — and the [`Notes`] it keeps, shared.
#[must_use]
pub fn plugin() -> Plugin {
    let notes = Arc::new(Notes::default());
    Plugin::new()
        .judge(AGGREGATE, NoteImport)
        .project(AGGREGATE, Noting(notes.clone()))
        .project_pg(
            AGGREGATE,
            projection::Tables {
                notes: notes.clone(),
            },
        )
        .share(notes)
}

/// Records each proposal applied among its note's, and forgets one removed.
struct Noting(Arc<Notes>);

impl Project for Noting {
    fn observe(&self, write: &Write) {
        let Some(id) = write.partition.strip_prefix(PARTITION) else {
            return;
        };
        if write.tombstone {
            self.0.forget(id);
        } else if let Some(note) = write.values.get(NOTE).and_then(Value::as_str) {
            self.0.record(note, id);
        }
    }
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
    use serde_json::json;

    use super::*;

    fn write(partition: &str, values: Value, tombstone: bool) -> Write {
        Write {
            partition: partition.to_owned(),
            values: serde_json::from_value(values).expect("an object"),
            tombstone,
        }
    }

    #[test]
    fn an_applied_proposal_is_recorded_among_its_note_s_until_it_is_removed() {
        let notes = Arc::new(Notes::default());
        let noting = Noting(notes.clone());
        noting.observe(&write(
            "NoteImport/p1",
            json!({ "noteProposalId": "p1", "noteId": "n1" }),
            false,
        ));
        // The note's own row, a row of no note, another aggregate's: none.
        noting.observe(&write(
            "NoteImport/n1",
            json!({ "importedNoteId": "n1" }),
            false,
        ));
        noting.observe(&write("NoteImport/p2", json!({ "noteId": 3 }), false));
        noting.observe(&write("Entity/p3", json!({ "noteId": "n1" }), false));
        assert_eq!(notes.of("n1").into_iter().collect::<Vec<_>>(), ["p1"]);
        noting.observe(&write("Entity/p1", json!({}), true));
        assert_eq!(notes.of("n1").len(), 1, "another aggregate's removal");
        noting.observe(&write("NoteImport/p1", json!({}), true));
        assert!(notes.of("n1").is_empty());
    }

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
