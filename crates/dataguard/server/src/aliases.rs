//! The names and aliases of every element the engine has applied, by
//! campaign, normalised per AliasMatch@1: what `alias-unique-in-campaign`
//! reads to judge one element's row against the others' — the engine judges
//! a row alone, and this is the rest of its campaign.
//!
//! [`crate::versioning::Versioning`] keeps it: an element is recorded each
//! time a write of its partition is applied, forgotten when it is removed.
//! Over a database it starts from the tables (`crate::projection`), whose
//! trigger judges the same rule again.

use std::collections::BTreeMap;
use std::sync::RwLock;

use save_entity::{ALIAS, Row, normalized};
use serde_json::Value;

/// One element's terms: its campaign, its normalised name and aliases.
#[derive(Debug)]
struct Terms {
    campaign: String,
    name: String,
    aliases: Vec<String>,
}

impl Terms {
    /// `row`'s terms, when it names its campaign.
    fn of(row: &Row) -> Option<Self> {
        let campaign = row.get("entityCampaignId")?.as_str()?.to_owned();
        let name = row
            .get("name")
            .and_then(Value::as_str)
            .map(normalized)
            .unwrap_or_default();
        let aliases = row
            .get(ALIAS)
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(normalized)
            .collect();
        Some(Self {
            campaign,
            name,
            aliases,
        })
    }

    /// Each non-blank term, and whether it is an alias.
    fn each(&self) -> impl Iterator<Item = (&str, bool)> {
        std::iter::once((self.name.as_str(), false))
            .chain(self.aliases.iter().map(|alias| (alias.as_str(), true)))
            .filter(|(term, _)| !term.is_empty())
    }
}

/// The applied elements' terms, by element id.
#[derive(Debug, Default)]
pub struct Aliases {
    elements: RwLock<BTreeMap<String, Terms>>,
}

impl Aliases {
    /// `element`'s terms are now `row`'s.
    pub fn record(&self, element: &str, row: &Row) {
        let mut elements = self.elements.write().unwrap_or_else(|e| e.into_inner());
        match Terms::of(row) {
            Some(terms) => elements.insert(element.to_owned(), terms),
            None => elements.remove(element),
        };
    }

    /// `element` is gone.
    pub fn forget(&self, element: &str) {
        self.elements
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .remove(element);
    }

    /// The first term of `row`, element `element`'s, that another element
    /// of its campaign answers to: one of its aliases naming another
    /// element, by its name or an alias, or its name doubling another's
    /// alias. Two names alike are no alias's business.
    pub fn taken(&self, element: &str, row: &Row) -> Option<String> {
        let own = Terms::of(row)?;
        let elements = self.elements.read().unwrap_or_else(|e| e.into_inner());
        let others: Vec<(&str, bool)> = elements
            .iter()
            .filter(|(id, terms)| id.as_str() != element && terms.campaign == own.campaign)
            .flat_map(|(_, terms)| terms.each())
            .collect();
        own.each()
            .find(|(term, is_alias)| {
                others
                    .iter()
                    .any(|(other, other_is_alias)| other == term && (*is_alias || *other_is_alias))
            })
            .map(|(term, _)| term.to_owned())
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    const VALOMBRE: &str = "c1000000-0000-4000-8000-000000000001";
    const BRUME: &str = "c1000000-0000-4000-8000-000000000002";

    fn row(campaign: &str, name: &str, aliases: &[&str]) -> Row {
        let mut row = Row::new();
        row.insert("entityCampaignId".into(), json!(campaign));
        row.insert("name".into(), json!(name));
        row.insert(ALIAS.into(), json!(aliases));
        row
    }

    /// Valombre holds Elowen (« la Rôdeuse ») and the tavern (« la taverne »).
    fn valombre() -> Aliases {
        let aliases = Aliases::default();
        aliases.record("elowen", &row(VALOMBRE, "Elowen", &["la Rôdeuse"]));
        aliases.record(
            "taverne",
            &row(VALOMBRE, "La Taverne du Sanglier", &["la taverne"]),
        );
        aliases
    }

    #[test]
    fn an_alias_naming_another_element_of_the_campaign_is_taken() {
        let aliases = valombre();
        for (term, expected) in [
            // Another's alias, normalised.
            ("  LA RODEUSE ", "la rodeuse"),
            // Another's name.
            ("la taverne du sanglier", "la taverne du sanglier"),
        ] {
            assert_eq!(
                aliases.taken("morlox", &row(VALOMBRE, "Morlox", &["le gobelin", term])),
                Some(expected.to_owned()),
                "{term}"
            );
        }
    }

    #[test]
    fn a_name_doubling_another_s_alias_is_taken_but_not_another_s_name() {
        let aliases = valombre();
        assert_eq!(
            aliases.taken("morlox", &row(VALOMBRE, "La Taverne", &[])),
            Some("la taverne".to_owned())
        );
        assert_eq!(aliases.taken("bis", &row(VALOMBRE, "Elowen", &[])), None);
    }

    #[test]
    fn the_element_itself_another_campaign_and_a_blank_term_take_nothing() {
        let aliases = valombre();
        // Elowen saved again, her alias kept.
        assert_eq!(
            aliases.taken("elowen", &row(VALOMBRE, "Elowen", &["la rodeuse"])),
            None
        );
        assert_eq!(
            aliases.taken("phare", &row(BRUME, "Le Phare", &["la rodeuse"])),
            None
        );
        assert_eq!(
            aliases.taken("morlox", &row(VALOMBRE, "Morlox", &["  ", ""])),
            None
        );
    }

    #[test]
    fn a_forgotten_or_renamed_element_frees_its_terms() {
        let aliases = valombre();
        let morlox = row(VALOMBRE, "Morlox", &["la taverne"]);
        assert!(aliases.taken("morlox", &morlox).is_some());
        aliases.record(
            "taverne",
            &row(VALOMBRE, "La Taverne du Sanglier", &["le Sanglier"]),
        );
        assert_eq!(aliases.taken("morlox", &morlox), None);
        aliases.forget("elowen");
        assert_eq!(
            aliases.taken("morlox", &row(VALOMBRE, "Morlox", &["la rodeuse"])),
            None
        );
    }
}
