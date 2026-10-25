//! Capability `campaign.listEntitiesToCreate`: the campaign's to-create list,
//! the NPCs, places and events its oral briefs mentioned and the campaign does
//! not hold yet.
//!
//! It reads, and only reads. The rows of its persisted query
//! (`crates/campaign/queries/list-entities-to-create.graphql`, resolved by
//! [`SQL`]) go in; the answer, in the shape the ToCreateList Micro-UI reads —
//! `{ items }`, each item as in `mock/fixtures/campaign.listEntitiesToCreate.json`
//! — comes out. Nothing is created here: the list only reflects what the
//! briefs left.
//!
//! What the store holds twice is read once. An entry is created if and only if
//! `created_entity_id` is set, so the status is derived from it and the stored
//! `status` column is not read: an entry whose created element was deleted
//! (the column is nullified) is to create again.

use std::collections::BTreeMap;

use serde_json::{Value, json};

/// The Capability's identifier, as a Micro-UI cites it.
pub const CAPABILITY: &str = "campaign.listEntitiesToCreate";

/// What the persisted query `list-entities-to-create.graphql` selects: one row
/// per entry of the campaign bound to `$1` (compared as text, so that an id
/// which is not a uuid lists nothing instead of failing), in the order of the
/// sessions, then of the entries. The session is joined for its title, and
/// only when it is the campaign's own; an entry with no session keeps a
/// `null` one. `campaign_id` is selected too, so
/// that [`answer_rows`] keeps only the campaign's own entries whoever ran the
/// statement.
pub const SQL: &str = r"SELECT e.id::text AS id,
    e.campaign_id::text AS campaign_id,
    s.id::text AS session_id,
    s.title AS session_title,
    e.type AS type,
    e.name AS name,
    e.excerpt AS excerpt,
    e.created_entity_id::text AS created_entity_id
FROM entity_to_create e
LEFT JOIN session s ON s.id = e.session_id AND s.campaign_id = e.campaign_id
WHERE e.campaign_id::text = $1
ORDER BY s.position NULLS LAST, e.created_at, e.name, e.id";

/// How the screen words an entry's type: the store's `evenement` is an
/// « événement », the other types read as they are stored.
fn kind(stored: &Value) -> Value {
    if stored == "evenement" {
        json!("événement")
    } else {
        stored.clone()
    }
}

/// An entry as the screen reads it. It is created if and only if the row
/// names the element created from it.
fn item(row: &Value) -> Value {
    let entity = row["created_entity_id"].as_str();
    json!({
        "id": row["id"],
        "type": kind(&row["type"]),
        "name": row["name"],
        "session": row["session_id"],
        "sessionTitle": row["session_title"],
        "excerpt": row["excerpt"],
        "status": if entity.is_some() { "created" } else { "todo" },
        "entity": entity,
    })
}

/// The entries of `campaign` among `rows` — an array of the objects [`SQL`]
/// selects — in the order of the rows. The rows of another campaign are
/// ignored; a campaign with no entry answers an empty list.
#[must_use]
pub fn answer_rows(rows: &Value, campaign: &str) -> Value {
    let items: Vec<Value> = rows
        .as_array()
        .map(|rows| {
            rows.iter()
                .filter(|row| row["campaign_id"].as_str() == Some(campaign))
                .map(item)
                .collect()
        })
        .unwrap_or_default();
    json!({ "items": items })
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

    const CENDRES: &str = "campagne-cendres";
    const OTHER: &str = "campagne-autre";

    fn fixture() -> Value {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../../mock/fixtures/campaign.listEntitiesToCreate.json");
        serde_json::from_str(&std::fs::read_to_string(path).expect("the fixture")).expect("JSON")
    }

    fn row(id: &str, campaign: &str, kind: &str, entity: Option<&str>) -> Value {
        json!({
            "id": id,
            "campaign_id": campaign,
            "session_id": "s4",
            "session_title": "Séance 4 — L’embuscade du Gué",
            "type": kind,
            "name": id,
            "excerpt": "…",
            "created_entity_id": entity,
        })
    }

    /// The fixture's entries as the rows [`SQL`] would select.
    fn rows() -> Value {
        Value::Array(
            fixture()
                .as_array()
                .expect("an array")
                .iter()
                .map(|item| {
                    json!({
                        "id": item["id"],
                        "campaign_id": CENDRES,
                        "session_id": item["session"],
                        "session_title": item["sessionTitle"],
                        "type": item["type"],
                        "name": item["name"],
                        "excerpt": item["excerpt"],
                        // Stored as the elements of the status: `creee` has one.
                        "created_entity_id": item["entity"],
                    })
                })
                .collect(),
        )
    }

    #[test]
    fn the_items_are_the_fixtures() {
        assert_eq!(answer_rows(&rows(), CENDRES), json!({ "items": fixture() }));
    }

    #[test]
    fn a_stored_event_reads_as_an_evenement_with_an_accent() {
        let rows = json!([
            row("a", CENDRES, "pnj", None),
            row("b", CENDRES, "lieu", None),
            row("c", CENDRES, "evenement", None),
        ]);
        let items = answer_rows(&rows, CENDRES)["items"].clone();
        let kinds: Vec<&str> = items
            .as_array()
            .expect("an array")
            .iter()
            .map(|i| i["type"].as_str().expect("a type"))
            .collect();
        assert_eq!(kinds, ["pnj", "lieu", "événement"]);
    }

    #[test]
    fn an_entry_is_created_if_and_only_if_its_entity_is_set() {
        let rows = json!([
            row("todo", CENDRES, "pnj", None),
            row("done", CENDRES, "pnj", Some("vesna")),
        ]);
        let items = answer_rows(&rows, CENDRES)["items"].clone();
        assert_eq!(items[0]["status"], "todo");
        assert_eq!(items[0]["entity"], Value::Null);
        assert_eq!(items[1]["status"], "created");
        assert_eq!(items[1]["entity"], "vesna");
    }

    #[test]
    fn a_stored_status_does_not_decide_the_status() {
        let mut stale = row("stale", CENDRES, "pnj", None);
        stale["status"] = json!("creee");
        let mut early = row("early", CENDRES, "pnj", Some("e"));
        early["status"] = json!("a_creer");
        let items = answer_rows(&json!([stale, early]), CENDRES)["items"].clone();
        assert_eq!(items[0]["status"], "todo");
        assert_eq!(items[1]["status"], "created");
    }

    #[test]
    fn an_entry_with_no_session_keeps_null_ones() {
        let mut orphan = row("orphan", CENDRES, "lieu", None);
        orphan["session_id"] = Value::Null;
        orphan["session_title"] = Value::Null;
        let items = answer_rows(&json!([orphan]), CENDRES)["items"].clone();
        assert_eq!(items[0]["session"], Value::Null);
        assert_eq!(items[0]["sessionTitle"], Value::Null);
    }

    #[test]
    fn another_campaigns_entries_do_not_appear() {
        let mut rows = rows();
        rows.as_array_mut()
            .expect("an array")
            .push(row("intrus", OTHER, "pnj", None));
        let answered = answer_rows(&rows, CENDRES);
        assert!(!answered.to_string().contains("intrus"), "{answered}");
        assert_eq!(answered["items"].as_array().expect("an array").len(), 3);
        let other = answer_rows(&rows, OTHER);
        assert_eq!(other["items"].as_array().expect("an array").len(), 1);
        assert_eq!(other["items"][0]["id"], "intrus");
    }

    #[test]
    fn a_campaign_with_no_entry_has_an_empty_list() {
        for rows in [json!([]), Value::Null, rows()] {
            assert_eq!(answer_rows(&rows, "campagne-vide"), json!({ "items": [] }));
        }
    }

    #[test]
    fn the_registry_s_answer_reads_its_variable_from_the_request() {
        let rows = rows();
        let variables = BTreeMap::from([("campaign".to_string(), json!(CENDRES))]);
        let answered = answer(&rows, &variables);
        assert_ne!(answered, json!({ "items": [] }));
        assert_eq!(answered, answer_rows(&rows, CENDRES));
        assert_eq!(answer(&rows, &BTreeMap::new()), json!({ "items": [] }));
    }
}
