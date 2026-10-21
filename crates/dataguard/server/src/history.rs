//! The versions of every element, as they were written: what a restoration
//! (`campaign.restoreEntityVersion`) is prepared against — the engine's row
//! holds the last version alone.
//!
//! It follows the tables, as [`aggregate_entity::Aliases`] does: over a
//! database, `crate::projection` reads an element's versions from
//! `entity_version` once its projection is committed, and every element's at
//! a start — numbered as the tables number them, which is what the DM reads
//! (`campaign.listEntityVersions`). Over the engine's memory, which is the
//! tables, [`crate::versioning::Versioning`] records each version applied.
//! Either forgets an element when it is removed. Elements are told apart by
//! their id parsed as a uuid: the same id in another case is the same one.

use std::collections::BTreeMap;
use std::sync::RwLock;

use aggregate_entity::parsed;
use restore_entity_version::Kept;

/// Every element's versions, by element id, by number.
#[derive(Debug, Default)]
pub struct History {
    elements: RwLock<BTreeMap<u128, BTreeMap<u64, Kept>>>,
}

impl History {
    /// `element` has `version` too, or in its place when its number was
    /// already taken.
    pub fn record(&self, element: &str, version: Kept) {
        let Some(element) = parsed(element) else {
            return;
        };
        self.elements
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .entry(element)
            .or_default()
            .insert(version.number, version);
    }

    /// `element`'s versions are `versions`, and no other.
    pub fn replace(&self, element: &str, versions: Vec<Kept>) {
        let Some(element) = parsed(element) else {
            return;
        };
        let versions = versions.into_iter().map(|v| (v.number, v)).collect();
        self.elements
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .insert(element, versions);
    }

    /// `element` is gone.
    pub fn forget(&self, element: &str) {
        if let Some(element) = parsed(element) {
            self.elements
                .write()
                .unwrap_or_else(|e| e.into_inner())
                .remove(&element);
        }
    }

    /// `element`'s versions, oldest first; none for an element unknown.
    pub fn of(&self, element: &str) -> Vec<Kept> {
        let Some(element) = parsed(element) else {
            return Vec::new();
        };
        self.elements
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .get(&element)
            .map(|versions| versions.values().cloned().collect())
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{Map, json};

    use super::*;

    const MORLOX: &str = "e1000000-0000-4000-8000-000000000003";
    const BANDIT: &str = "e1000000-0000-4000-8000-000000000004";

    fn kept(id: &str, number: u64, name: &str) -> Kept {
        let mut snapshot = Map::new();
        snapshot.insert("name".into(), json!(name));
        Kept {
            id: id.to_owned(),
            number,
            snapshot,
            diff: Vec::new(),
        }
    }

    #[test]
    fn an_element_s_versions_are_its_own_oldest_first_whatever_the_case_of_its_id() {
        let history = History::default();
        history.record(MORLOX, kept("v2", 2, "Morlox le Vil"));
        history.record(&MORLOX.to_uppercase(), kept("v1", 1, "Morlox"));
        history.record(BANDIT, kept("b1", 1, "Bandit"));
        let ids: Vec<String> = history.of(MORLOX).into_iter().map(|v| v.id).collect();
        assert_eq!(ids, ["v1", "v2"]);
        assert_eq!(history.of(BANDIT).len(), 1);
        assert!(
            history
                .of("e1000000-0000-4000-8000-000000000009")
                .is_empty()
        );
        assert!(history.of("not-a-uuid").is_empty());
    }

    #[test]
    fn a_number_recorded_again_is_replaced_and_a_replacement_is_the_whole_history() {
        let history = History::default();
        history.record(MORLOX, kept("v1", 1, "Morlox"));
        history.record(MORLOX, kept("v1-again", 1, "Morlox"));
        assert_eq!(history.of(MORLOX)[0].id, "v1-again");
        history.replace(MORLOX, vec![kept("t1", 1, "Morlox"), kept("t2", 2, "M")]);
        let ids: Vec<String> = history.of(MORLOX).into_iter().map(|v| v.id).collect();
        assert_eq!(ids, ["t1", "t2"]);
        history.forget(MORLOX);
        assert!(history.of(MORLOX).is_empty());
    }
}
