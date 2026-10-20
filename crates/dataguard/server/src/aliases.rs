//! The names and aliases of every element the engine has applied, by
//! campaign, normalised per AliasMatch@1: what `alias-unique-in-campaign`
//! reads to judge one element's row against the others' — the engine judges
//! a row alone, and this is the rest of its campaign.
//!
//! It follows the tables: over a database, `crate::projection` records an
//! element once its projection is committed, and starts from what the tables
//! hold — whose trigger judges the same rule again — so a save the tables
//! refused takes no term, before a restart or after. Over the engine's
//! memory, which is the tables, [`crate::versioning::Versioning`] records an
//! element each time a write of its partition is applied. Either forgets an
//! element when it is removed. Elements and campaigns are told apart by their
//! id parsed as a uuid: the same id in another case is the same one.

use std::collections::BTreeMap;
use std::sync::RwLock;

use extract_from_notes::Known;
use save_entity::{ALIAS, Row, normalized};
use serde_json::Value;

use crate::ai_settings::uuid;

/// One element's terms: its campaign, its normalised name and aliases, and
/// both as written.
#[derive(Debug)]
struct Terms {
    campaign: u128,
    name: String,
    aliases: Vec<String>,
    written: (String, Vec<String>),
}

impl Terms {
    /// `row`'s terms, when it names its campaign by a uuid.
    fn of(row: &Row) -> Option<Self> {
        let campaign = parsed(row.get("entityCampaignId")?.as_str()?)?;
        let name = row
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        let aliases: Vec<String> = row
            .get(ALIAS)
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect();
        Some(Self {
            campaign,
            name: normalized(&name),
            aliases: aliases.iter().map(|alias| normalized(alias)).collect(),
            written: (name, aliases),
        })
    }

    /// Each non-blank term, and whether it is an alias.
    fn each(&self) -> impl Iterator<Item = (&str, bool)> {
        std::iter::once((self.name.as_str(), false))
            .chain(self.aliases.iter().map(|alias| (alias.as_str(), true)))
            .filter(|(term, _)| !term.is_empty())
    }
}

/// An id, parsed: the 128 bits a uuid written 8-4-4-4-12 in hex
/// digits, in either case, names.
pub fn parsed(id: &str) -> Option<u128> {
    uuid(id)
        .then(|| u128::from_str_radix(&id.replace('-', ""), 16).ok())
        .flatten()
}

/// `id` written as a uuid, in lowercase: 8-4-4-4-12 hex digits.
fn written(id: u128) -> String {
    let hex = format!("{id:032x}");
    format!(
        "{}-{}-{}-{}-{}",
        &hex[..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..]
    )
}

/// The applied elements' terms, by element id.
#[derive(Debug, Default)]
pub struct Aliases {
    elements: RwLock<BTreeMap<u128, Terms>>,
}

impl Aliases {
    /// `element`'s terms are now `row`'s.
    pub fn record(&self, element: &str, row: &Row) {
        let Some(element) = parsed(element) else {
            return;
        };
        let mut elements = self.elements.write().unwrap_or_else(|e| e.into_inner());
        match Terms::of(row) {
            Some(terms) => elements.insert(element, terms),
            None => elements.remove(&element),
        };
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

    /// The elements of `campaign`, its id in whatever case, with their names
    /// and aliases as written: what `ai.extractFromNotes` recognises a
    /// proposal by, and a dictation listens for. In the order of their ids.
    pub fn known(&self, campaign: &str) -> Vec<Known> {
        let Some(campaign) = parsed(campaign) else {
            return Vec::new();
        };
        let elements = self.elements.read().unwrap_or_else(|e| e.into_inner());
        elements
            .iter()
            .filter(|(_, terms)| terms.campaign == campaign)
            .map(|(id, terms)| Known {
                id: written(*id),
                name: terms.written.0.clone(),
                aliases: terms.written.1.clone(),
            })
            .collect()
    }

    /// The first term of `row`, element `element`'s, that another element
    /// of its campaign answers to: one of its aliases naming another
    /// element, by its name or an alias, or its name doubling another's
    /// alias. Two names alike are no alias's business.
    pub fn taken(&self, element: &str, row: &Row) -> Option<String> {
        let own = Terms::of(row)?;
        let element = parsed(element);
        let elements = self.elements.read().unwrap_or_else(|e| e.into_inner());
        let others: Vec<(&str, bool)> = elements
            .iter()
            .filter(|(id, terms)| Some(**id) != element && terms.campaign == own.campaign)
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
    const ELOWEN: &str = "e1000000-0000-4000-8000-000000000001";
    const TAVERNE: &str = "e1000000-0000-4000-8000-000000000002";
    const MORLOX: &str = "e1000000-0000-4000-8000-000000000003";
    const BIS: &str = "e1000000-0000-4000-8000-000000000004";
    const PHARE: &str = "e1000000-0000-4000-8000-000000000005";

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
        aliases.record(ELOWEN, &row(VALOMBRE, "Elowen", &["la Rôdeuse"]));
        aliases.record(
            TAVERNE,
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
                aliases.taken(MORLOX, &row(VALOMBRE, "Morlox", &["le gobelin", term])),
                Some(expected.to_owned()),
                "{term}"
            );
        }
    }

    #[test]
    fn a_name_doubling_another_s_alias_is_taken_but_not_another_s_name() {
        let aliases = valombre();
        assert_eq!(
            aliases.taken(MORLOX, &row(VALOMBRE, "La Taverne", &[])),
            Some("la taverne".to_owned())
        );
        assert_eq!(aliases.taken(BIS, &row(VALOMBRE, "Elowen", &[])), None);
    }

    #[test]
    fn the_element_itself_another_campaign_and_a_blank_term_take_nothing() {
        let aliases = valombre();
        // Elowen saved again, her alias kept.
        assert_eq!(
            aliases.taken(ELOWEN, &row(VALOMBRE, "Elowen", &["la rodeuse"])),
            None
        );
        assert_eq!(
            aliases.taken(PHARE, &row(BRUME, "Le Phare", &["la rodeuse"])),
            None
        );
        assert_eq!(
            aliases.taken(MORLOX, &row(VALOMBRE, "Morlox", &["  ", ""])),
            None
        );
    }

    #[test]
    fn an_id_is_parsed_as_a_uuid_or_not_at_all() {
        assert_eq!(
            parsed(ELOWEN),
            Some(0xe100_0000_0000_4000_8000_0000_0000_0001)
        );
        assert_eq!(parsed(&ELOWEN.to_uppercase()), parsed(ELOWEN));
        for id in [
            "e1000000000040008000000000000001",
            "e100000-00000-4000-8000-000000000001",
            "+1000000-0000-4000-8000-000000000001",
            "g1000000-0000-4000-8000-000000000001",
            "elowen",
        ] {
            assert_eq!(parsed(id), None, "{id}");
        }
        // An id that is none records nothing.
        let aliases = Aliases::default();
        aliases.record("elowen", &row(VALOMBRE, "Elowen", &["la rodeuse"]));
        assert_eq!(
            aliases.taken(MORLOX, &row(VALOMBRE, "Morlox", &["la rodeuse"])),
            None
        );
    }

    #[test]
    fn an_element_is_itself_whatever_the_case_of_its_id() {
        let aliases = valombre();
        assert_eq!(
            aliases.taken(
                &ELOWEN.to_uppercase(),
                &row(VALOMBRE, "Elowen", &["la rodeuse"])
            ),
            None
        );
        aliases.record(&TAVERNE.to_uppercase(), &row(VALOMBRE, "La Taverne", &[]));
        aliases.forget(TAVERNE);
        assert_eq!(
            aliases.taken(MORLOX, &row(VALOMBRE, "Morlox", &["la taverne"])),
            None
        );
    }

    #[test]
    fn a_campaign_is_itself_whatever_the_case_of_its_id() {
        let aliases = valombre();
        // The tables write a campaign's id in lower case; a save may not.
        assert_eq!(
            aliases.taken(
                MORLOX,
                &row(&VALOMBRE.to_uppercase(), "Morlox", &["la rodeuse"])
            ),
            Some("la rodeuse".to_owned())
        );
        aliases.record(BIS, &row(&BRUME.to_uppercase(), "Le Phare", &["le phare"]));
        assert_eq!(
            aliases.taken(PHARE, &row(BRUME, "Le Grand Phare", &["le phare"])),
            Some("le phare".to_owned())
        );
        // A campaign that is no uuid holds no term.
        aliases.record(BIS, &row("valombre", "Le Phare", &["le phare"]));
        assert_eq!(
            aliases.taken(PHARE, &row(BRUME, "Le Grand Phare", &["le phare"])),
            None
        );
    }

    #[test]
    fn a_campaign_s_elements_are_known_by_their_names_and_aliases_as_written() {
        let aliases = valombre();
        aliases.record(PHARE, &row(BRUME, "Le Phare", &[]));
        assert_eq!(
            aliases.known(&VALOMBRE.to_uppercase()),
            [
                Known {
                    id: ELOWEN.to_owned(),
                    name: "Elowen".to_owned(),
                    aliases: vec!["la Rôdeuse".to_owned()],
                },
                Known {
                    id: TAVERNE.to_owned(),
                    name: "La Taverne du Sanglier".to_owned(),
                    aliases: vec!["la taverne".to_owned()],
                },
            ]
        );
        aliases.forget(ELOWEN);
        assert_eq!(aliases.known(VALOMBRE).len(), 1);
        assert!(aliases.known("valombre").is_empty());
    }

    #[test]
    fn a_forgotten_or_renamed_element_frees_its_terms() {
        let aliases = valombre();
        let morlox = row(VALOMBRE, "Morlox", &["la taverne"]);
        assert!(aliases.taken(MORLOX, &morlox).is_some());
        aliases.record(
            TAVERNE,
            &row(VALOMBRE, "La Taverne du Sanglier", &["le Sanglier"]),
        );
        assert_eq!(aliases.taken(MORLOX, &morlox), None);
        aliases.forget(ELOWEN);
        assert_eq!(
            aliases.taken(MORLOX, &row(VALOMBRE, "Morlox", &["la rodeuse"])),
            None
        );
    }
}
