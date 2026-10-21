//! The versions of every element, as they were settled: what a restoration
//! (`campaign.restoreEntityVersion`) is prepared against — the engine's row
//! holds the last version alone, as its save numbered it.
//!
//! [`crate::plugin`] keeps it and shares it: its projection settles each
//! version the engine applies after the element's last one ([`History::settle`]),
//! and over PostgreSQL it starts from `entity_version` (`crate::projection`'s
//! `warm`), numbered as the tables number them, which is what the DM reads
//! (`campaign.listEntityVersions`). A removed element is forgotten. Elements
//! are told apart by their id parsed as a uuid: the same id in another case
//! is the same one.

use std::collections::BTreeMap;
use std::sync::RwLock;

use crate::aliases::{Row, parsed};
use crate::version::{Kept, settled};

/// Every element's versions, by element id, by number.
#[derive(Debug, Default)]
pub struct History {
    elements: RwLock<BTreeMap<u128, BTreeMap<u64, Kept>>>,
}

impl History {
    /// The version `row` carries, settled after `element`'s last one, is
    /// kept — unless it carries none, or one already kept.
    pub fn settle(&self, element: &str, row: &Row) {
        let Some(element) = parsed(element) else {
            return;
        };
        let mut elements = self.elements.write().unwrap_or_else(|e| e.into_inner());
        let versions = elements.entry(element).or_default();
        let id = row.get("entityVersionId").and_then(|id| id.as_str());
        if versions
            .values()
            .any(|kept| id.is_some_and(|id| kept.id.eq_ignore_ascii_case(id)))
        {
            return;
        }
        if let Some(kept) = settled(versions.values().next_back(), row) {
            versions.insert(kept.number, kept);
        }
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
    #[must_use]
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
    use serde_json::{Map, Value, json};

    use super::*;

    const MORLOX: &str = "e1000000-0000-4000-8000-000000000003";
    const BANDIT: &str = "e1000000-0000-4000-8000-000000000004";

    fn saved(version: &str, name: &str) -> Row {
        let Value::Object(row) = json!({ "entityVersionId": version, "type": "pnj", "name": name })
        else {
            panic!("a row is an object");
        };
        row.into_iter().collect()
    }

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

    fn ids(history: &History, element: &str) -> Vec<(u64, String)> {
        history
            .of(element)
            .into_iter()
            .map(|v| (v.number, v.id))
            .collect()
    }

    #[test]
    fn each_version_settled_follows_the_element_s_last_whatever_the_case_of_its_id() {
        let history = History::default();
        history.settle(MORLOX, &saved("v1", "Morlox"));
        history.settle(&MORLOX.to_uppercase(), &saved("v2", "Morlox le Vil"));
        history.settle(BANDIT, &saved("b1", "Bandit"));
        assert_eq!(
            ids(&history, MORLOX),
            [(1, "v1".to_owned()), (2, "v2".to_owned())]
        );
        let second = &history.of(MORLOX)[1];
        assert_eq!(
            second.diff,
            [json!({ "field": "name", "old": "Morlox", "new": "Morlox le Vil" })]
        );
        assert_eq!(ids(&history, BANDIT), [(1, "b1".to_owned())]);
        assert!(
            history
                .of("e1000000-0000-4000-8000-000000000009")
                .is_empty()
        );
        assert!(history.of("not-a-uuid").is_empty());
    }

    #[test]
    fn a_version_kept_already_a_row_with_none_or_an_element_that_is_no_uuid_adds_nothing() {
        let history = History::default();
        history.settle(MORLOX, &saved("v1", "Morlox"));
        history.settle(MORLOX, &saved("V1", "Morlox le Vil"));
        history.settle(MORLOX, &Row::new());
        history.settle("not-a-uuid", &saved("x1", "Morlox"));
        assert_eq!(ids(&history, MORLOX), [(1, "v1".to_owned())]);
        assert_eq!(history.of(MORLOX)[0].snapshot["name"], "Morlox");
        assert!(history.of("not-a-uuid").is_empty());
    }

    #[test]
    fn a_replacement_is_the_whole_history_and_a_forgotten_element_has_none() {
        let history = History::default();
        history.settle(MORLOX, &saved("v1", "Morlox"));
        history.replace(MORLOX, vec![kept("t1", 1, "Morlox"), kept("t3", 3, "M")]);
        assert_eq!(
            ids(&history, MORLOX),
            [(1, "t1".to_owned()), (3, "t3".to_owned())]
        );
        // Settled after the last one the tables number.
        history.settle(MORLOX, &saved("v4", "M"));
        assert_eq!(history.of(MORLOX)[2].number, 4);
        history.replace("not-a-uuid", vec![kept("t1", 1, "Morlox")]);
        assert!(history.of("not-a-uuid").is_empty());
        history.forget("not-a-uuid");
        history.forget(&MORLOX.to_uppercase());
        assert!(history.of(MORLOX).is_empty());
    }
}
