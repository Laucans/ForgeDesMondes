//! Capability `campaign.listPlaces`: the places of a campaign, for their own
//! tab.
//!
//! It reads, and only reads. The rows of its persisted query
//! (`crates/campaign/queries/list-places.graphql`, resolved by [`SQL`]) go
//! in; the answer, in the shape of `mock/fixtures/campaign.listPlaces.json`
//! — an array of `{ id, name, aliases, description }` — comes out. The
//! fixture's `illustration` and `art` belong to the media of a place, and its
//! `characters` to the scenes: other reads, from other milestones.
//!
//! `entity.type` decides the tab: only an element of type `lieu` is a place.
//! No PC, NPC or event shows there.

use serde_json::{Value, json};

/// The Capability's identifier, as a Micro-UI cites it.
pub const CAPABILITY: &str = "campaign.listPlaces";

/// The `entity.type` of a place.
const PLACE: &str = "lieu";

/// What the persisted query `list-places.graphql` selects: the places of the
/// campaign bound to `$1` (compared as text, so that an id which is not a
/// uuid lists nobody instead of failing), by name, each with its aliases as
/// a JSON array (empty when it has none). `campaign_id` and `type` are
/// selected too, so that [`answer`] keeps only the campaign's own places
/// whoever ran the statement.
pub const SQL: &str = r"SELECT e.id::text AS id,
    e.campaign_id::text AS campaign_id,
    e.type,
    e.name,
    coalesce(
        (SELECT jsonb_agg(a.alias ORDER BY a.alias, a.id) FROM entity_alias a WHERE a.entity_id = e.id),
        '[]'::jsonb
    ) AS aliases,
    e.description
FROM entity e
WHERE e.campaign_id::text = $1 AND e.type = 'lieu'
ORDER BY e.name, e.id";

/// The places of `campaign` among `rows` — an array of the objects [`SQL`]
/// selects — as the tab reads them, in the order of the rows. An element of
/// another type or of another campaign is left out; a place with no alias
/// lists `[]`, one with no description `null`; no row is an empty,
/// well-formed answer.
#[must_use]
pub fn answer(rows: &Value, campaign: &str) -> Value {
    let places: Vec<Value> = rows
        .as_array()
        .map(|rows| {
            rows.iter()
                .filter(|row| {
                    row["campaign_id"].as_str() == Some(campaign)
                        && row["type"].as_str() == Some(PLACE)
                })
                .map(|row| {
                    json!({
                        "id": row["id"],
                        "name": row["name"],
                        "aliases": if row["aliases"].is_array() { row["aliases"].clone() } else { json!([]) },
                        "description": row["description"],
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    Value::Array(places)
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    const CENDRES: &str = "c1000000-0000-4000-8000-000000000001";
    const OTHER: &str = "c1000000-0000-4000-8000-000000000002";

    fn row(
        id: &str,
        campaign: &str,
        kind: &str,
        name: &str,
        aliases: &[&str],
        description: Option<&str>,
    ) -> Value {
        json!({
            "id": id,
            "campaign_id": campaign,
            "type": kind,
            "name": name,
            "aliases": aliases,
            "description": description,
        })
    }

    fn fixture() -> Value {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../../mock/fixtures/campaign.listPlaces.json");
        serde_json::from_str(&std::fs::read_to_string(path).expect("the fixture")).expect("JSON")
    }

    /// The fixture's places as the rows [`SQL`] would select for them, plus
    /// the elements of the other tabs that live in the same campaign.
    fn rows_of_the_campaign() -> Value {
        let mut rows: Vec<Value> = fixture()
            .as_array()
            .expect("an array")
            .iter()
            .map(|place| {
                json!({
                    "id": place["id"],
                    "campaign_id": CENDRES,
                    "type": "lieu",
                    "name": place["name"],
                    "aliases": place["aliases"],
                    "description": place["description"],
                })
            })
            .collect();
        rows.push(row(
            "gaspard",
            CENDRES,
            "pnj",
            "Gaspard",
            &["le tavernier"],
            None,
        ));
        rows.push(row(
            "elowen",
            CENDRES,
            "pj",
            "Elowen",
            &["la rôdeuse"],
            Some("Rôdeuse"),
        ));
        rows.push(row(
            "incendie",
            CENDRES,
            "evenement",
            "L’incendie",
            &[],
            None,
        ));
        Value::Array(rows)
    }

    fn ids(places: &Value) -> Vec<&str> {
        places
            .as_array()
            .expect("an array")
            .iter()
            .map(|p| p["id"].as_str().expect("an id"))
            .collect()
    }

    #[test]
    fn the_tavern_appears_with_its_aliases_and_description() {
        let listed = answer(&rows_of_the_campaign(), CENDRES);
        let tavern = listed
            .as_array()
            .expect("an array")
            .iter()
            .find(|p| p["id"] == "taverne-sanglier")
            .expect("taverne-sanglier");
        let expected = fixture()[0].clone();
        assert_eq!(tavern["name"], expected["name"]);
        assert_eq!(tavern["aliases"], json!(["la taverne", "le Sanglier"]));
        assert_eq!(tavern["description"], expected["description"]);
    }

    #[test]
    fn the_four_places_of_the_fixture_are_listed_in_the_order_of_the_rows() {
        let listed = answer(&rows_of_the_campaign(), CENDRES);
        assert_eq!(ids(&listed), ids(&fixture()));
    }

    #[test]
    fn a_pc_an_npc_or_an_event_does_not_appear() {
        let listed = answer(&rows_of_the_campaign(), CENDRES);
        for other_tab in ["gaspard", "elowen", "incendie"] {
            assert!(
                !ids(&listed).contains(&other_tab),
                "{other_tab} in {listed}"
            );
        }
        assert_eq!(ids(&listed).len(), 4);
    }

    #[test]
    fn a_place_of_another_campaign_does_not_appear() {
        let mut rows = rows_of_the_campaign();
        rows.as_array_mut().expect("an array").push(row(
            "lieu-intrus",
            OTHER,
            "lieu",
            "Intrus",
            &[],
            None,
        ));
        let listed = answer(&rows, CENDRES);
        assert_eq!(ids(&listed).len(), 4);
        assert!(!ids(&listed).contains(&"lieu-intrus"), "{listed}");
        assert_eq!(ids(&answer(&rows, OTHER)), ["lieu-intrus"]);
    }

    #[test]
    fn a_place_with_no_alias_lists_an_empty_array_and_no_description_is_null() {
        let rows = json!([row("cave", CENDRES, "lieu", "La cave", &[], None)]);
        let listed = answer(&rows, CENDRES);
        assert_eq!(listed[0]["aliases"], json!([]));
        assert_eq!(listed[0]["description"], Value::Null);
        let no_aliases_column = json!([{
            "id": "cave", "campaign_id": CENDRES, "type": "lieu", "name": "La cave",
        }]);
        assert_eq!(answer(&no_aliases_column, CENDRES)[0]["aliases"], json!([]));
    }

    #[test]
    fn the_answer_holds_id_name_aliases_and_description_only() {
        let mut rows = rows_of_the_campaign();
        rows[0]["notes"] = json!("secret du DM");
        rows[0]["illustration"] = json!("ill-salle-sanglier");
        let listed = answer(&rows, CENDRES);
        let mut keys: Vec<&str> = listed[0]
            .as_object()
            .expect("an object")
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        assert_eq!(keys, ["aliases", "description", "id", "name"]);
    }

    #[test]
    fn no_row_is_an_empty_well_formed_answer() {
        assert_eq!(answer(&json!([]), CENDRES), json!([]));
        assert_eq!(answer(&Value::Null, CENDRES), json!([]));
    }

    #[test]
    fn the_sql_is_one_select_statement() {
        assert!(
            SQL.trim_start().to_ascii_uppercase().starts_with("SELECT"),
            "{SQL}"
        );
        assert!(!SQL.contains(';'), "a second statement in {SQL}");
    }

    #[test]
    fn the_sql_is_bound_to_one_campaign_and_to_the_places() {
        assert!(SQL.contains("e.campaign_id::text = $1"), "{SQL}");
        assert!(SQL.contains(&format!("e.type = '{PLACE}'")), "{SQL}");
    }

    #[test]
    fn the_query_file_selects_what_the_sql_reads() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../queries/list-places.graphql");
        let query = std::fs::read_to_string(path).expect("the persisted query");
        let fields = query
            .lines()
            .skip_while(|l| !l.starts_with("query "))
            .skip(2)
            .map(str::trim)
            .take_while(|l| *l != "}")
            .collect::<Vec<_>>();
        assert_eq!(
            fields,
            ["id", "campaignId", "type", "name", "aliases", "description"]
        );
        for field in fields {
            let column: String = field
                .chars()
                .flat_map(|c| {
                    if c.is_ascii_uppercase() {
                        vec!['_', c.to_ascii_lowercase()]
                    } else {
                        vec![c]
                    }
                })
                .collect();
            let selected = if field == "aliases" {
                SQL.contains("AS aliases")
            } else {
                SQL.contains(&format!("e.{column}"))
            };
            assert!(
                selected,
                "the query names {field}, the SQL does not select it"
            );
        }
    }
}
