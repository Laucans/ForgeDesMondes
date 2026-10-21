//! The proposals of every imported note the engine holds: what a validation
//! (`campaign.createEntitiesFromNote`) unchecks besides the proposals it
//! accepts — the engine reads a row by its partition, and a note's row
//! names none of its proposals.
//!
//! The aggregate's plugin records each proposal applied, over the engine's
//! memory and over a database alike (`Project::observe`); over a database,
//! its projection reads the ones the engine held before a start from its
//! rows (`ProjectPg::warm`). A proposal removed is forgotten. Notes are told
//! apart by their id whatever its case.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::RwLock;

/// Every note's proposals, by note id in lowercase.
#[derive(Debug, Default)]
pub struct Notes {
    notes: RwLock<BTreeMap<String, BTreeSet<String>>>,
}

impl Notes {
    /// `proposal`, the id of its partition, is one of `note`'s.
    pub fn record(&self, note: &str, proposal: &str) {
        self.notes
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .entry(note.to_ascii_lowercase())
            .or_default()
            .insert(proposal.to_owned());
    }

    /// `proposal` is gone, of whichever note it was.
    pub fn forget(&self, proposal: &str) {
        for proposals in self
            .notes
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .values_mut()
        {
            proposals.remove(proposal);
        }
    }

    /// `note`'s proposals, by id.
    pub fn of(&self, note: &str) -> BTreeSet<String> {
        self.notes
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .get(&note.to_ascii_lowercase())
            .cloned()
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::Notes;

    #[test]
    fn a_note_s_proposals_are_the_ones_recorded_whatever_the_note_s_case_until_forgotten() {
        let notes = Notes::default();
        notes.record("A1", "p1");
        notes.record("a1", "p2");
        notes.record("b1", "p3");
        assert_eq!(notes.of("a1").into_iter().collect::<Vec<_>>(), ["p1", "p2"]);
        notes.forget("p1");
        assert_eq!(notes.of("A1").into_iter().collect::<Vec<_>>(), ["p2"]);
        assert_eq!(notes.of("b1").into_iter().collect::<Vec<_>>(), ["p3"]);
        assert!(notes.of("c1").is_empty());
    }
}
