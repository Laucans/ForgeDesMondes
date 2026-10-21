//! Capability `campaign.listEvents`: the events of a campaign, for their own
//! tab.
//!
//! It reads, and only reads. The rows of its persisted query
//! (`crates/campaign/queries/list-events.graphql`, resolved by [`SQL`]) go
//! in; the answer, in the shape of `mock/fixtures/campaign.listEvents.json`
//! — an array of `{ id, name, aliases, description }` — comes out.
//!
//! `entity.type` decides the tab: only an element of type `evenement`
//! appears. A character (`pj`, `pnj`) or a place (`lieu`) is another read,
//! and so is a session or a scene.

use std::collections::BTreeMap;

use serde_json::{Value, json};

/// The Capability's identifier, as a Micro-UI cites it.
pub const CAPABILITY: &str = "campaign.listEvents";

/// The `entity.type` of an event.
const EVENT: &str = "evenement";

/// What the persisted query `list-events.graphql` selects: the events of the
/// campaign bound to `$1` (compared as text, so that an id which is not a
/// uuid lists nothing instead of failing), in the order they were created,
/// each with its aliases as a JSON array (empty when it has none).
/// `campaign_id` and `type` are selected too, so that [`answer_rows`] keeps only
/// the campaign's own events whoever ran the statement.
pub const SQL: &str = r"SELECT e.id::text AS id,
    e.campaign_id::text AS campaign_id,
    e.type,
    e.name,
    e.description,
    COALESCE((SELECT jsonb_agg(a.alias ORDER BY a.alias)
              FROM entity_alias a
              WHERE a.entity_id = e.id), '[]'::jsonb) AS aliases
FROM entity e
WHERE e.campaign_id::text = $1 AND e.type = 'evenement'
ORDER BY e.created_at, e.name, e.id";

/// The events of `campaign` among `rows` — an array of the objects [`SQL`]
/// selects — as the tab reads them, in the order of the rows. An element of
/// another campaign, or of another type, is left out; no row is an empty,
/// well-formed answer.
#[must_use]
pub fn answer_rows(rows: &Value, campaign: &str) -> Value {
    let events: Vec<Value> = rows
        .as_array()
        .map(|rows| {
            rows.iter()
                .filter(|row| {
                    row["campaign_id"].as_str() == Some(campaign)
                        && row["type"].as_str() == Some(EVENT)
                })
                .map(|row| {
                    json!({
                        "id": row["id"],
                        "name": row["name"],
                        "aliases": row["aliases"].as_array().map_or_else(|| json!([]), |a| json!(a)),
                        "description": row["description"],
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    Value::Array(events)
}

/// The request's variables [`SQL`] binds, in the order of its `$1`, `$2`…:
/// what the registry hands the query.
pub const VARIABLES: &[&str] = &["campaign"];

/// The answer to a read, as the registry serves it: the rows of [`SQL`] and
/// the request's variables. It reads `campaign`, as [`SQL`] binds it; a
/// variable absent is the empty text, which names nothing.
#[must_use]
pub fn answer(rows: &Value, variables: &BTreeMap<String, Value>) -> Value {
    answer_rows(
        rows,
        variables
            .get("campaign")
            .and_then(Value::as_str)
            .unwrap_or_default(),
    )
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    const CENDRES: &str = "c1000000-0000-4000-8000-000000000001";
    const OTHER: &str = "c1000000-0000-4000-8000-000000000002";

    fn row(id: &str, campaign: &str, kind: &str, name: &str, aliases: &[&str]) -> Value {
        json!({
            "id": id,
            "campaign_id": campaign,
            "type": kind,
            "name": name,
            "description": format!("About {name}"),
            "aliases": aliases,
        })
    }

    fn three_events() -> Value {
        json!([
            row(
                "incendie-moulin",
                CENDRES,
                "evenement",
                "L’incendie du moulin",
                &["l’incendie", "le feu du moulin"]
            ),
            row(
                "marche-noir",
                CENDRES,
                "evenement",
                "Le marché noir de Morlox",
                &["le marché noir"]
            ),
            row(
                "embuscade-gue",
                CENDRES,
                "evenement",
                "L’embuscade du Gué",
                &["l’embuscade"]
            ),
        ])
    }

    fn fixture() -> Value {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../../mock/fixtures/campaign.listEvents.json");
        serde_json::from_str(&std::fs::read_to_string(path).expect("the fixture")).expect("JSON")
    }

    #[test]
    fn the_events_of_the_campaign_appear_with_id_name_and_aliases() {
        let listed = answer_rows(&three_events(), CENDRES);
        let key = |events: &Value| -> Vec<(Value, Value, Value)> {
            events
                .as_array()
                .expect("an array")
                .iter()
                .map(|e| (e["id"].clone(), e["name"].clone(), e["aliases"].clone()))
                .collect()
        };
        assert_eq!(key(&listed).len(), 3);
        assert_eq!(key(&listed), key(&fixture()));
    }

    #[test]
    fn each_event_shows_its_description() {
        let listed = answer_rows(&three_events(), CENDRES);
        assert_eq!(listed[0]["description"], "About L’incendie du moulin");
        let mut rows = three_events();
        rows[1]["description"] = Value::Null;
        assert_eq!(answer_rows(&rows, CENDRES)[1]["description"], Value::Null);
    }

    #[test]
    fn no_pc_npc_or_place_appears() {
        let mut rows = three_events();
        let extra = rows.as_array_mut().expect("an array");
        extra.push(row("elowen", CENDRES, "pj", "Elowen", &[]));
        extra.push(row("morlox", CENDRES, "pnj", "Morlox", &[]));
        extra.push(row("valombre", CENDRES, "lieu", "Valombre", &[]));
        let listed = answer_rows(&rows, CENDRES);
        assert_eq!(listed.as_array().expect("an array").len(), 3);
        for name in ["elowen", "morlox", "valombre"] {
            assert!(!listed.to_string().contains(name), "{name} in {listed}");
        }
    }

    #[test]
    fn an_event_of_another_campaign_does_not_appear() {
        let mut rows = three_events();
        rows.as_array_mut().expect("an array").push(row(
            "intrus",
            OTHER,
            "evenement",
            "Intrus",
            &[],
        ));
        let listed = answer_rows(&rows, CENDRES);
        assert_eq!(listed.as_array().expect("an array").len(), 3);
        assert!(!listed.to_string().contains("intrus"), "{listed}");
        assert_eq!(answer_rows(&rows, OTHER)[0]["id"], "intrus");
    }

    #[test]
    fn the_answer_holds_id_name_aliases_and_description_only() {
        let mut rows = three_events();
        rows[0]["notes"] = json!("secret");
        let listed = answer_rows(&rows, CENDRES);
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
    fn an_event_without_aliases_has_an_empty_list() {
        let rows = json!([row("e", CENDRES, "evenement", "Sans alias", &[])]);
        assert_eq!(answer_rows(&rows, CENDRES)[0]["aliases"], json!([]));
    }

    #[test]
    fn no_row_is_an_empty_well_formed_answer() {
        assert_eq!(answer_rows(&json!([]), CENDRES), json!([]));
        assert_eq!(answer_rows(&Value::Null, CENDRES), json!([]));
    }

    #[test]
    fn the_sql_is_one_select_statement_bound_to_one_campaign_and_the_event_type() {
        assert!(
            SQL.trim_start().to_ascii_uppercase().starts_with("SELECT"),
            "{SQL}"
        );
        assert!(!SQL.contains(';'), "a second statement in {SQL}");
        assert!(SQL.contains("e.campaign_id::text = $1"), "{SQL}");
        assert!(SQL.contains("e.type = 'evenement'"), "{SQL}");
    }

    #[test]
    fn the_query_file_selects_what_the_sql_reads() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../queries/list-events.graphql");
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
            ["id", "campaignId", "type", "name", "description", "aliases"]
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
            assert!(
                SQL.contains(&format!("AS {column}")) || SQL.contains(&format!("e.{column}")),
                "the query names {field}, the SQL does not select {column}"
            );
        }
    }

    #[test]
    fn the_registry_s_answer_reads_its_variable_from_the_request() {
        let rows = three_events();
        let variables = BTreeMap::from([("campaign".to_string(), json!(CENDRES))]);
        let answered = answer(&rows, &variables);
        assert_ne!(answered, Value::Null);
        assert_eq!(answered, answer_rows(&rows, CENDRES));
        assert_ne!(answer(&rows, &BTreeMap::new()), answered);
    }
}
