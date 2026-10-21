//! Capability `campaign.listPlayers`: the players of a campaign, for their
//! own tab beside the characters.
//!
//! It reads, and only reads. The rows of its persisted query
//! (`crates/campaign/queries/list-players.graphql`, resolved by [`SQL`]) go
//! in; the answer, in the shape of `mock/fixtures/campaign.listPlayers.json`
//! — an array of `{ id, name }` — comes out, each player with its `notes` too
//! (`null` when the DM wrote none).
//!
//! A player is a person at the table, not a campaign element
//! (`player-not-character`): the answer holds no sheet and no token, and the
//! characters a player plays are another read.

use std::collections::BTreeMap;

use serde_json::{Value, json};

/// The Capability's identifier, as a Micro-UI cites it.
pub const CAPABILITY: &str = "campaign.listPlayers";

/// What the persisted query `list-players.graphql` selects: the players of
/// the campaign bound to `$1` (compared as text, so that an id which is not
/// a uuid lists nobody instead of failing), by name. `campaign_id` is selected too, so that
/// [`answer_rows`] keeps only the campaign's own rows whoever ran the statement.
pub const SQL: &str = r"SELECT p.id::text AS id,
    p.campaign_id::text AS campaign_id,
    p.name,
    p.notes
FROM player p
WHERE p.campaign_id::text = $1
ORDER BY p.name, p.id";

/// The players of `campaign` among `rows` — an array of the objects [`SQL`]
/// selects — as the tab reads them, in the order of the rows. A player of
/// another campaign is left out; no row is an empty, well-formed answer.
#[must_use]
pub fn answer_rows(rows: &Value, campaign: &str) -> Value {
    let players: Vec<Value> = rows
        .as_array()
        .map(|rows| {
            rows.iter()
                .filter(|row| row["campaign_id"].as_str() == Some(campaign))
                .map(|row| {
                    json!({
                        "id": row["id"],
                        "name": row["name"],
                        "notes": row["notes"],
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    Value::Array(players)
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

    fn row(id: &str, campaign: &str, name: &str, notes: Option<&str>) -> Value {
        json!({ "id": id, "campaign_id": campaign, "name": name, "notes": notes })
    }

    fn four_players() -> Value {
        json!([
            row(
                "jou-julie",
                CENDRES,
                "Julie",
                Some("Joue Elowen, prend des notes")
            ),
            row("jou-marc", CENDRES, "Marc", None),
            row("jou-sophie", CENDRES, "Sophie", None),
            row("jou-thomas", CENDRES, "Thomas", Some("Arrive en retard")),
        ])
    }

    fn fixture() -> Value {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../../mock/fixtures/campaign.listPlayers.json");
        serde_json::from_str(&std::fs::read_to_string(path).expect("the fixture")).expect("JSON")
    }

    #[test]
    fn the_four_players_of_the_campaign_appear_with_id_and_name() {
        let listed = answer_rows(&four_players(), CENDRES);
        let pairs = |players: &Value| -> Vec<(Value, Value)> {
            players
                .as_array()
                .expect("an array")
                .iter()
                .map(|p| (p["id"].clone(), p["name"].clone()))
                .collect()
        };
        assert_eq!(pairs(&listed).len(), 4);
        assert_eq!(pairs(&listed), pairs(&fixture()));
    }

    #[test]
    fn each_player_shows_its_notes_or_null() {
        let listed = answer_rows(&four_players(), CENDRES);
        assert_eq!(listed[0]["notes"], "Joue Elowen, prend des notes");
        assert_eq!(listed[1]["notes"], Value::Null);
    }

    #[test]
    fn a_player_of_another_campaign_does_not_appear() {
        let mut rows = four_players();
        rows.as_array_mut()
            .expect("an array")
            .push(row("jou-intruse", OTHER, "Intruse", None));
        let listed = answer_rows(&rows, CENDRES);
        assert_eq!(listed.as_array().expect("an array").len(), 4);
        assert!(!listed.to_string().contains("jou-intruse"), "{listed}");
        assert_eq!(answer_rows(&rows, OTHER)[0]["id"], "jou-intruse");
    }

    #[test]
    fn a_player_is_no_character_the_answer_holds_id_name_and_notes_only() {
        let mut rows = four_players();
        rows[0]["sheet"] = json!({ "level": 5 });
        rows[0]["token"] = json!("elowen.png");
        let listed = answer_rows(&rows, CENDRES);
        let mut keys: Vec<&str> = listed[0]
            .as_object()
            .expect("an object")
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        assert_eq!(keys, ["id", "name", "notes"]);
    }

    #[test]
    fn no_row_is_an_empty_well_formed_answer() {
        assert_eq!(answer_rows(&json!([]), CENDRES), json!([]));
        assert_eq!(answer_rows(&Value::Null, CENDRES), json!([]));
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
    fn the_sql_is_bound_to_one_campaign() {
        assert!(SQL.contains("WHERE p.campaign_id::text = $1"), "{SQL}");
    }

    #[test]
    fn the_query_file_selects_what_the_sql_reads() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../queries/list-players.graphql");
        let query = std::fs::read_to_string(path).expect("the persisted query");
        let fields = query
            .lines()
            .skip_while(|l| !l.starts_with("query "))
            .skip(2)
            .map(str::trim)
            .take_while(|l| *l != "}")
            .collect::<Vec<_>>();
        assert_eq!(fields, ["id", "campaignId", "name", "notes"]);
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
                SQL.contains(&format!("p.{column}")),
                "the query names {field}, the SQL does not select it"
            );
        }
    }

    #[test]
    fn the_registry_s_answer_reads_its_variable_from_the_request() {
        let rows = four_players();
        let variables = BTreeMap::from([("campaign".to_string(), json!(CENDRES))]);
        let answered = answer(&rows, &variables);
        assert_ne!(answered, Value::Null);
        assert_eq!(answered, answer_rows(&rows, CENDRES));
        assert_ne!(answer(&rows, &BTreeMap::new()), answered);
    }
}
