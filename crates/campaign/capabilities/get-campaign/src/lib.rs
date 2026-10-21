//! Capability `campaign.getCampaign`: one campaign's overview.
//!
//! It reads, and only reads. The rows of its persisted query
//! (`crates/campaign/queries/get-campaign.graphql`, resolved by [`SQL`]) go
//! in; the answer, in the shape of `mock/fixtures/campaign.getCampaign.json`,
//! comes out: `id`, `name`, `type`, `cover`, `art`, `glyph`, `pitch`,
//! `createdAt`, `lastExport` and `counts`.
//!
//! The counts are derived (`campaign_overview_view`), never stored. The view
//! counts players, characters, places and events; the session, media and
//! sound counts and the next session join it with their own tables, and are
//! not in the answer until those exist.

use std::collections::BTreeMap;

use serde_json::{Map, Value, json};

/// The Capability's identifier, as a Micro-UI cites it.
pub const CAPABILITY: &str = "campaign.getCampaign";

/// The counts `campaign_overview_view` holds, in the order of the answer.
const COUNTS: [&str; 4] = ["players", "characters", "places", "events"];

/// What the persisted query `get-campaign.graphql` selects: the campaign
/// bound to `$1` (compared as text, so that an id which is not a uuid finds
/// nothing instead of failing) with its counts. The dates are the UTC day,
/// as the fixture shows them.
pub const SQL: &str = r#"SELECT c.id::text AS id,
    c.name,
    c.kind,
    c.art,
    c.glyph,
    c.pitch,
    c.cover_media_id::text AS cover_media_id,
    to_char(c.created_at AT TIME ZONE 'UTC', 'YYYY-MM-DD') AS created_at,
    to_char(c.last_exported_at AT TIME ZONE 'UTC', 'YYYY-MM-DD') AS last_exported_at,
    v.entity_counts
FROM campaign c
JOIN campaign_overview_view v ON v.campaign_id = c.id
WHERE c.id::text = $1"#;

/// The overview of `campaign` among `rows` — an array of the objects [`SQL`]
/// selects — or `None` when no row is that campaign's: an unknown id is no
/// error. A missing count is zero, as the view counts nothing for an empty
/// campaign.
#[must_use]
pub fn answer_rows(rows: &Value, campaign: &str) -> Option<Value> {
    let row = rows
        .as_array()?
        .iter()
        .find(|row| row["id"].as_str() == Some(campaign))?;
    let counts: Map<String, Value> = COUNTS
        .iter()
        .map(|key| {
            let n = row["entity_counts"][key].as_u64().unwrap_or(0);
            ((*key).to_string(), json!(n))
        })
        .collect();
    Some(json!({
        "id": row["id"],
        "name": row["name"],
        "type": row["kind"],
        "cover": row["cover_media_id"],
        "art": row["art"],
        "glyph": row["glyph"],
        "pitch": row["pitch"],
        "createdAt": row["created_at"],
        "lastExport": row["last_exported_at"],
        "counts": counts,
    }))
}

/// The request's variables [`SQL`] binds, in the order of its `$1`, `$2`…:
/// what the registry hands the query.
pub const VARIABLES: &[&str] = &["id"];

/// The answer to a read, as the registry serves it: the rows of [`SQL`] and
/// the request's variables. It reads `id`, as [`SQL`] binds it; a variable
/// absent is the empty text, which names nothing.
#[must_use]
pub fn answer(rows: &Value, variables: &BTreeMap<String, Value>) -> Value {
    answer_rows(
        rows,
        variables
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or_default(),
    )
    .unwrap_or(Value::Null)
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    const CENDRES: &str = "c1000000-0000-4000-8000-000000000001";
    const OTHER: &str = "c1000000-0000-4000-8000-000000000002";

    fn row(id: &str, name: &str, last_export: Option<&str>) -> Value {
        json!({
            "id": id,
            "name": name,
            "kind": "campagne",
            "art": "ember",
            "glyph": "V",
            "pitch": "Un bourg couvert de cendres, un moulin incendié, et la Tour d’Ébène qui tire les ficelles.",
            "cover_media_id": null,
            "created_at": "2026-08-20",
            "last_exported_at": last_export,
            "entity_counts": { "players": 4, "characters": 9, "places": 4, "events": 3 },
        })
    }

    fn fixture() -> Value {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../../mock/fixtures/campaign.getCampaign.json");
        serde_json::from_str(&std::fs::read_to_string(path).expect("the fixture")).expect("JSON")
    }

    #[test]
    fn the_answer_carries_what_the_fixture_does() {
        let rows = json!([row(CENDRES, "Les Cendres de Valombre", Some("2026-10-04"))]);
        let got = answer_rows(&rows, CENDRES).expect("found");
        let fixture = fixture();
        for key in [
            "name",
            "type",
            "art",
            "glyph",
            "pitch",
            "createdAt",
            "lastExport",
        ] {
            assert_eq!(got[key], fixture[key], "{key}");
        }
        for key in COUNTS {
            assert_eq!(got["counts"][key], fixture["counts"][key], "counts.{key}");
        }
        assert_eq!(got["id"], CENDRES);
    }

    #[test]
    fn the_last_export_is_the_stamp_of_the_row() {
        let before = answer_rows(&json!([row(CENDRES, "A", Some("2026-10-04"))]), CENDRES);
        let after = answer_rows(&json!([row(CENDRES, "A", Some("2026-10-17"))]), CENDRES);
        assert_eq!(before.expect("found")["lastExport"], "2026-10-04");
        assert_eq!(after.expect("found")["lastExport"], "2026-10-17");
    }

    #[test]
    fn a_campaign_never_exported_has_a_null_last_export() {
        let got = answer_rows(&json!([row(CENDRES, "A", None)]), CENDRES).expect("found");
        assert_eq!(got["lastExport"], Value::Null);
    }

    #[test]
    fn an_unknown_id_is_not_found_without_error() {
        let rows = json!([row(CENDRES, "A", None)]);
        assert_eq!(answer_rows(&rows, OTHER), None);
        assert_eq!(answer_rows(&json!([]), CENDRES), None);
        assert_eq!(answer_rows(&Value::Null, CENDRES), None);
    }

    #[test]
    fn only_the_campaign_asked_for_is_answered() {
        let rows = json!([row(OTHER, "Autre", None), row(CENDRES, "A", None)]);
        assert_eq!(answer_rows(&rows, CENDRES).expect("found")["name"], "A");
    }

    #[test]
    fn an_empty_campaign_counts_zero() {
        let mut empty = row(CENDRES, "A", None);
        empty["entity_counts"] = json!({});
        let got = answer_rows(&json!([empty]), CENDRES).expect("found");
        assert_eq!(
            got["counts"],
            json!({ "players": 0, "characters": 0, "places": 0, "events": 0 })
        );
    }

    #[test]
    fn the_sql_is_one_select_statement_bound_to_one_campaign() {
        assert!(
            SQL.trim_start().to_ascii_uppercase().starts_with("SELECT"),
            "{SQL}"
        );
        assert!(!SQL.contains(';'), "a second statement in {SQL}");
        assert!(SQL.contains("WHERE c.id::text = $1"), "{SQL}");
        assert!(SQL.contains("campaign_overview_view"), "{SQL}");
    }

    #[test]
    fn the_query_file_selects_what_the_sql_reads() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../queries/get-campaign.graphql");
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
            [
                "id",
                "name",
                "kind",
                "art",
                "glyph",
                "pitch",
                "coverMediaId",
                "createdAt",
                "lastExportedAt",
                "entityCounts"
            ]
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
                SQL.contains(&column),
                "the query names {field}, the SQL does not select {column}"
            );
        }
    }

    #[test]
    fn the_registry_s_answer_reads_its_variable_from_the_request() {
        let rows = json!([row(CENDRES, "Les Cendres de Valombre", Some("2026-10-04"))]);
        let variables = BTreeMap::from([("id".to_string(), json!(CENDRES))]);
        let answered = answer(&rows, &variables);
        assert_ne!(answered, Value::Null);
        assert_eq!(answered, answer_rows(&rows, CENDRES).expect("found"));
        assert_ne!(answer(&rows, &BTreeMap::new()), answered);
    }
}
