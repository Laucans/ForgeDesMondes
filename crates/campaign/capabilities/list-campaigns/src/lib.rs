//! Capability `campaign.listCampaigns`: the campaigns and oneshots of the
//! home page.
//!
//! It reads, and only reads. The rows of its persisted query
//! (`crates/campaign/queries/list-campaigns.graphql`, resolved by [`SQL`]) go
//! in; the answer, in the shape of `mock/fixtures/campaign.listCampaigns.json`
//! — an array of `{ id, name, type, art, glyph, pitch, sessions, lastPlayed,
//! next }` — comes out, in the order of the rows.
//!
//! No account is asked and none is answered: a campaign belongs to no owner,
//! so an item holds neither `account` nor `owner`, whatever a row carries.
//!
//! `sessions`, `lastPlayed` and `next` are counted from the sessions, never
//! stored (`campaign_overview_view`). The Session aggregate's table does not
//! exist yet and the view holds no session column: they are `null`, an
//! unknown count, never a made-up zero. A row that carries them
//! (`session_count`, `last_played`, `next_session`) is answered as it says.

use std::collections::BTreeMap;

use serde_json::{Value, json};

/// The Capability's identifier, as a Micro-UI cites it.
pub const CAPABILITY: &str = "campaign.listCampaigns";

/// What the persisted query `list-campaigns.graphql` selects: every campaign,
/// the newest first. `kind` is the `type` of the answer.
pub const SQL: &str = r"SELECT c.id::text AS id,
    c.name,
    c.kind,
    c.art,
    c.glyph,
    c.pitch,
    c.created_at
FROM campaign c
ORDER BY c.created_at DESC, c.name, c.id";

/// The campaigns among `rows` — an array of the objects [`SQL`] selects — as
/// the home page reads them, in the order of the rows. No row is an empty,
/// well-formed answer.
#[must_use]
pub fn answer_rows(rows: &Value) -> Value {
    let campaigns: Vec<Value> = rows
        .as_array()
        .map(|rows| {
            rows.iter()
                .map(|row| {
                    json!({
                        "id": row["id"],
                        "name": row["name"],
                        "type": row["kind"],
                        "art": row["art"],
                        "glyph": row["glyph"],
                        "pitch": row["pitch"],
                        "sessions": row["session_count"],
                        "lastPlayed": row["last_played"],
                        "next": row["next_session"],
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    Value::Array(campaigns)
}

/// The request's variables [`SQL`] binds, in the order of its `$1`, `$2`…:
/// what the registry hands the query.
pub const VARIABLES: &[&str] = &[];

/// The answer to a read, as the registry serves it: the rows of [`SQL`] and
/// the request's variables, of which it reads none.
#[must_use]
pub fn answer(rows: &Value, _variables: &BTreeMap<String, Value>) -> Value {
    answer_rows(rows)
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    fn row(id: &str, name: &str, kind: &str, art: &str) -> Value {
        json!({
            "id": id, "name": name, "kind": kind, "art": art,
            "glyph": name[..1], "pitch": "Un pitch",
            "created_at": "2026-01-01T00:00:00Z",
        })
    }

    fn three() -> Value {
        json!([
            row("cendres-valombre", "Les Cendres", "campagne", "ember"),
            row("nuit-carnaval", "La Nuit", "oneshot", "dusk"),
            row("heritiers-ebene", "Les Héritiers", "campagne", "night"),
        ])
    }

    fn fixture() -> Value {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../../mock/fixtures/campaign.listCampaigns.json");
        serde_json::from_str(&std::fs::read_to_string(path).expect("the fixture")).expect("JSON")
    }

    #[test]
    fn two_campaigns_and_a_oneshot_all_appear_with_their_cover() {
        let listed = answer_rows(&three());
        let items = listed.as_array().expect("an array");
        assert_eq!(items.len(), 3);
        let types: Vec<&Value> = items.iter().map(|c| &c["type"]).collect();
        assert_eq!(types, ["campagne", "oneshot", "campagne"]);
        assert!(
            items.iter().all(|c| c["art"].as_str().is_some()),
            "{listed}"
        );
    }

    #[test]
    fn an_item_has_the_keys_of_the_fixture() {
        let keys = |v: &Value| -> Vec<String> {
            let mut keys: Vec<String> = v[0]
                .as_object()
                .expect("an object")
                .keys()
                .cloned()
                .collect();
            keys.sort_unstable();
            keys
        };
        assert_eq!(keys(&answer_rows(&three())), keys(&fixture()));
    }

    #[test]
    fn no_item_carries_an_account_or_an_owner() {
        let mut rows = three();
        rows[0]["account"] = json!("laurent");
        rows[1]["owner"] = json!("laurent");
        let listed = answer_rows(&rows);
        for item in listed.as_array().expect("an array") {
            assert!(
                item.get("account").is_none() && item.get("owner").is_none(),
                "{item}"
            );
        }
    }

    #[test]
    fn sessions_are_unknown_until_they_are_counted_never_zero() {
        let listed = answer_rows(&three());
        assert_eq!(listed[1]["sessions"], Value::Null);
        assert_eq!(listed[1]["lastPlayed"], Value::Null);
        assert_eq!(listed[1]["next"], Value::Null);
    }

    #[test]
    fn a_counted_session_is_answered_as_it_is() {
        let mut rows = three();
        rows[1]["session_count"] = json!(1);
        rows[1]["last_played"] = json!("2026-07-11");
        rows[0]["next_session"] = json!("2026-10-17");
        let listed = answer_rows(&rows);
        assert_eq!(listed[1]["sessions"], 1);
        assert_eq!(listed[1]["lastPlayed"], "2026-07-11");
        assert_eq!(listed[0]["next"], "2026-10-17");
    }

    #[test]
    fn the_order_of_the_rows_is_kept() {
        let listed = answer_rows(&three());
        let ids: Vec<&Value> = listed
            .as_array()
            .expect("an array")
            .iter()
            .map(|c| &c["id"])
            .collect();
        assert_eq!(
            ids,
            ["cendres-valombre", "nuit-carnaval", "heritiers-ebene"]
        );
    }

    #[test]
    fn no_row_is_an_empty_well_formed_answer() {
        assert_eq!(answer_rows(&json!([])), json!([]));
        assert_eq!(answer_rows(&Value::Null), json!([]));
    }

    #[test]
    fn the_sql_is_one_select_statement_that_writes_nothing() {
        assert!(
            SQL.trim_start().to_ascii_uppercase().starts_with("SELECT"),
            "{SQL}"
        );
        assert!(!SQL.contains(';'), "a second statement in {SQL}");
    }

    #[test]
    fn the_query_file_selects_what_the_sql_reads() {
        let path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../queries/list-campaigns.graphql");
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
            ["id", "name", "kind", "art", "glyph", "pitch", "createdAt"]
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
                SQL.contains(&format!("c.{column}")),
                "the query names {field}, the SQL does not select it"
            );
        }
    }

    #[test]
    fn the_registry_s_answer_is_the_rows_answer() {
        let rows = three();
        let answered = answer(&rows, &BTreeMap::new());
        assert_ne!(answered, Value::Null);
        assert_eq!(answered, answer_rows(&rows));
    }
}
