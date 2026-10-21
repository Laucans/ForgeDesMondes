//! Capability `campaign.listEntityVersions`: one element's history, newest
//! first.
//!
//! It reads, and only reads. The rows of its persisted query
//! (`crates/campaign/queries/list-entity-versions.graphql`, resolved by
//! [`SQL`]) go in; the answer, in the shape of one entry of
//! `mock/fixtures/campaign.listEntityVersions.json` — `{ name, versions }`,
//! each version with `v`, `date`, `origin`, `source` and `changes` — comes
//! out, plus `restoredFrom` for a restoration.
//!
//! It implements `EntityVersion@1` (`concepts/EntityVersion/v1`): the
//! changes of a version are not read back from a stored column but derived,
//! by [`version`], from the snapshot of the version and the one it follows.
//! A version is never modified once written (`every-change-versioned`), so
//! the history only grows.

use std::collections::BTreeMap;

use serde_json::{Map, Value, json};

/// The Capability's identifier, as a Micro-UI cites it.
pub const CAPABILITY: &str = "campaign.listEntityVersions";

/// What the persisted query `list-entity-versions.graphql` selects: the
/// element bound to `$1` (compared as text, so that an id which is not a
/// uuid finds nothing instead of failing) with its versions, newest first.
/// One row per version; an element with none gives one row whose version
/// columns are null, so that it is told from an unknown element. Each
/// version carries the snapshot of the one before it (`previous_snapshot`,
/// null for the first) and the number of the version it restored
/// (`restored_from`, null unless it is a restoration). The date is the UTC
/// instant, as the fixture shows it.
pub const SQL: &str = r#"SELECT e.id::text AS id,
    e.name,
    v.version_number,
    v.source,
    v.snapshot,
    v.previous_snapshot,
    v.created_at,
    v.restored_from
FROM entity e
LEFT JOIN (
    SELECT ev.entity_id,
        ev.version_number,
        ev.source,
        ev.snapshot,
        LAG(ev.snapshot) OVER (PARTITION BY ev.entity_id ORDER BY ev.version_number) AS previous_snapshot,
        to_char(ev.created_at AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS') AS created_at,
        (SELECT r.version_number FROM entity_version r
         WHERE r.id = ev.restored_from_version_id) AS restored_from
    FROM entity_version ev
    WHERE ev.entity_id::text = $1
) v ON v.entity_id = e.id
WHERE e.id::text = $1
ORDER BY v.version_number DESC NULLS LAST"#;

/// Two JSON values are the same: numbers by value, the rest as a whole.
fn same(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(a), Value::Number(b)) => a.as_f64() == b.as_f64(),
        (Value::Array(a), Value::Array(b)) => {
            a.len() == b.len() && a.iter().zip(b).all(|(a, b)| same(a, b))
        }
        (Value::Object(a), Value::Object(b)) => {
            a.len() == b.len()
                && a.iter()
                    .all(|(key, a)| b.get(key).is_some_and(|b| same(a, b)))
        }
        (a, b) => a == b,
    }
}

/// `EntityVersion@1`: from `{ previous: { number, snapshot } | null,
/// snapshot, source }`, the `{ number, source, diff }` of the version — the
/// number goes up by one, the diff holds one `{ field, old, new }` per field
/// whose value differs between the two snapshots (an absent field is null),
/// sorted by field.
#[must_use]
pub fn version(input: &Value) -> Value {
    let (number, before) = match &input["previous"] {
        Value::Null => (1, Map::new()),
        previous => (
            previous["number"].as_u64().unwrap_or(0) + 1,
            previous["snapshot"]
                .as_object()
                .cloned()
                .unwrap_or_default(),
        ),
    };
    let after = input["snapshot"].as_object().cloned().unwrap_or_default();

    let mut fields: Vec<&String> = before.keys().chain(after.keys()).collect();
    fields.sort();
    fields.dedup();
    let diff: Vec<Value> = fields
        .into_iter()
        .filter_map(|field| {
            let old = before.get(field).unwrap_or(&Value::Null);
            let new = after.get(field).unwrap_or(&Value::Null);
            (!same(old, new)).then(|| json!({ "field": field, "old": old, "new": new }))
        })
        .collect();

    json!({ "number": number, "source": input["source"], "diff": diff })
}

/// How the screen words where a version came from: the DM's hand, the AI
/// (which also reads the DM's brief), or D&D Beyond. A restoration is the
/// DM's own act.
fn origin(source: &str) -> &'static str {
    match source {
        "ia" | "brief" => "ia",
        "dndbeyond" => "dndbeyond",
        _ => "manuelle",
    }
}

/// The history of `entity` among `rows` — an array of the objects [`SQL`]
/// selects — or `None` when no row is that element's: an unknown element is
/// no error. The versions come in the order of the rows (newest first); the
/// changes of each are derived by [`version`] from its snapshot and the one
/// before it.
#[must_use]
pub fn answer_rows(rows: &Value, entity: &str) -> Option<Value> {
    let own: Vec<&Value> = rows
        .as_array()?
        .iter()
        .filter(|row| row["id"].as_str() == Some(entity))
        .collect();
    let name = own.first()?["name"].clone();
    let versions: Vec<Value> = own
        .iter()
        .filter(|row| row["version_number"].is_u64())
        .map(|row| {
            let number = row["version_number"].as_u64().unwrap_or(0);
            let previous = if row["previous_snapshot"].is_object() {
                json!({
                    "number": number.saturating_sub(1),
                    "snapshot": row["previous_snapshot"],
                })
            } else {
                Value::Null
            };
            let source = row["source"].as_str().unwrap_or_default();
            let derived = version(&json!({
                "previous": previous,
                "snapshot": row["snapshot"],
                "source": source,
            }));
            json!({
                "v": number,
                "date": row["created_at"],
                "origin": origin(source),
                "source": source,
                "restoredFrom": row["restored_from"],
                "changes": derived["diff"],
            })
        })
        .collect();
    Some(json!({ "name": name, "versions": versions }))
}

/// The request's variables [`SQL`] binds, in the order of its `$1`, `$2`…:
/// what the registry hands the query.
pub const VARIABLES: &[&str] = &["entity"];

/// The answer to a read, as the registry serves it: the rows of [`SQL`] and
/// the request's variables. It reads `entity`, as [`SQL`] binds it; a
/// variable absent is the empty text, which names nothing.
#[must_use]
pub fn answer(rows: &Value, variables: &BTreeMap<String, Value>) -> Value {
    answer_rows(
        rows,
        variables
            .get("entity")
            .and_then(Value::as_str)
            .unwrap_or_default(),
    )
    .unwrap_or(Value::Null)
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    const MORLOX: &str = "e1000000-0000-4000-8000-000000000001";
    const OTHER: &str = "e1000000-0000-4000-8000-000000000002";

    fn snap(notes: &str, aliases: &[&str]) -> Value {
        json!({ "name": "Morlox", "type": "pnj", "notes": notes, "aliases": aliases })
    }

    #[allow(clippy::too_many_arguments)]
    fn row(
        id: &str,
        number: u64,
        source: &str,
        snapshot: &Value,
        previous: Option<&Value>,
        restored_from: Option<u64>,
    ) -> Value {
        json!({
            "id": id,
            "name": "Morlox",
            "version_number": number,
            "source": source,
            "snapshot": snapshot,
            "previous_snapshot": previous,
            "created_at": format!("2026-10-{:02}T19:32:00", 10 + number),
            "restored_from": restored_from,
        })
    }

    /// Four versions, newest first; the fourth restores the second.
    fn four_versions() -> Value {
        let s1 = snap("Doit 40 po.", &["le gobelin"]);
        let s2 = snap("Doit 40 po. Connaît le moulin.", &["le gobelin"]);
        let s3 = snap(
            "Doit 40 po. Connaît le moulin. A trahi.",
            &["le gobelin", "Morlo"],
        );
        json!([
            row(MORLOX, 4, "restauration", &s2, Some(&s3), Some(2)),
            row(MORLOX, 3, "ia", &s3, Some(&s2), None),
            row(MORLOX, 2, "dm", &s2, Some(&s1), None),
            row(MORLOX, 1, "dndbeyond", &s1, None, None),
        ])
    }

    fn fixture() -> Value {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../../mock/fixtures/campaign.listEntityVersions.json");
        serde_json::from_str(&std::fs::read_to_string(path).expect("the fixture")).expect("JSON")
    }

    #[test]
    fn conformance() {
        platform_conformance::check("EntityVersion@1", |input| version(&input));
    }

    #[test]
    fn the_versions_come_newest_first_with_number_date_origin_source_and_changes() {
        let got = answer_rows(&four_versions(), MORLOX).expect("found");
        assert_eq!(got["name"], "Morlox");
        let versions = got["versions"].as_array().expect("an array");
        let numbers: Vec<&Value> = versions.iter().map(|v| &v["v"]).collect();
        assert_eq!(numbers, [4, 3, 2, 1]);
        for v in versions {
            let mut keys: Vec<&str> = v
                .as_object()
                .expect("an object")
                .keys()
                .map(String::as_str)
                .collect();
            keys.sort_unstable();
            assert_eq!(
                keys,
                ["changes", "date", "origin", "restoredFrom", "source", "v"]
            );
        }
        assert_eq!(versions[0]["date"], "2026-10-14T19:32:00");
    }

    #[test]
    fn the_answer_has_the_keys_of_the_fixture() {
        let got = answer_rows(&four_versions(), MORLOX).expect("found");
        let fixture = fixture();
        let expected = &fixture["morlox"];
        assert_eq!(got["name"], expected["name"]);
        let (got, expected) = (&got["versions"][0], &expected["versions"][0]);
        for key in ["v", "date", "origin", "source", "changes"] {
            assert!(got.get(key).is_some(), "{key} is missing from {got}");
            assert!(expected.get(key).is_some(), "{key} is not in the fixture");
        }
        let change = &got["changes"][0];
        for key in ["field", "old", "new"] {
            assert!(change.get(key).is_some(), "{key} is missing from {change}");
        }
    }

    #[test]
    fn a_change_is_the_field_with_its_old_and_new_value() {
        let got = answer_rows(&four_versions(), MORLOX).expect("found");
        assert_eq!(
            got["versions"][1]["changes"],
            json!([
                {
                    "field": "aliases",
                    "old": ["le gobelin"],
                    "new": ["le gobelin", "Morlo"],
                },
                {
                    "field": "notes",
                    "old": "Doit 40 po. Connaît le moulin.",
                    "new": "Doit 40 po. Connaît le moulin. A trahi.",
                },
            ])
        );
    }

    #[test]
    fn the_first_version_lists_every_field_against_an_empty_snapshot() {
        let got = answer_rows(&four_versions(), MORLOX).expect("found");
        let first = &got["versions"][3]["changes"];
        let fields: Vec<&str> = first
            .as_array()
            .expect("an array")
            .iter()
            .map(|c| c["field"].as_str().expect("field"))
            .collect();
        assert_eq!(fields, ["aliases", "name", "notes", "type"]);
        assert!(
            first
                .as_array()
                .expect("an array")
                .iter()
                .all(|c| c["old"].is_null())
        );
    }

    #[test]
    fn a_restoration_shows_its_source_and_the_version_it_came_from() {
        let got = answer_rows(&four_versions(), MORLOX).expect("found");
        let restored = &got["versions"][0];
        assert_eq!(restored["source"], "restauration");
        assert_eq!(restored["restoredFrom"], 2);
        assert_eq!(got["versions"][1]["restoredFrom"], Value::Null);
        // The restoration takes the next number, and its changes are the
        // difference with the version just before it, not with the restored.
        assert_eq!(restored["v"], 4);
        assert_eq!(restored["changes"][0]["field"], "aliases");
    }

    #[test]
    fn an_object_field_is_a_change_only_when_its_content_differs() {
        let changes = |before: &Value, after: &Value| {
            version(&json!({
                "previous": { "number": 1, "snapshot": { "stats": before } },
                "snapshot": { "stats": after },
                "source": "dm",
            }))["diff"]
                .clone()
        };
        // Same keys and values (a number by value): no change.
        assert_eq!(
            changes(
                &json!({ "hp": 10, "ac": 15 }),
                &json!({ "ac": 15.0, "hp": 10 })
            ),
            json!([])
        );
        // Same size, another value or another key: a change.
        assert_eq!(
            changes(&json!({ "hp": 10 }), &json!({ "hp": 11 }))
                .as_array()
                .map(Vec::len),
            Some(1)
        );
        assert_eq!(
            changes(&json!({ "hp": 10 }), &json!({ "ac": 10 }))
                .as_array()
                .map(Vec::len),
            Some(1)
        );
        // One more key on either side: a change.
        assert_eq!(
            changes(&json!({ "hp": 10 }), &json!({ "hp": 10, "ac": 15 }))
                .as_array()
                .map(Vec::len),
            Some(1)
        );
        assert_eq!(
            changes(&json!({ "hp": 10, "ac": 15 }), &json!({ "hp": 10 }))
                .as_array()
                .map(Vec::len),
            Some(1)
        );
    }

    #[test]
    fn the_origin_is_how_the_screen_words_the_source() {
        let origins: Vec<(String, String)> = ["dm", "ia", "dndbeyond", "brief", "restauration"]
            .iter()
            .map(|s| ((*s).to_string(), origin(s).to_string()))
            .collect();
        assert_eq!(
            origins,
            [
                ("dm".to_string(), "manuelle".to_string()),
                ("ia".to_string(), "ia".to_string()),
                ("dndbeyond".to_string(), "dndbeyond".to_string()),
                ("brief".to_string(), "ia".to_string()),
                ("restauration".to_string(), "manuelle".to_string()),
            ]
        );
    }

    #[test]
    fn another_element_s_versions_do_not_appear() {
        let mut rows = four_versions();
        let s = snap("x", &[]);
        rows.as_array_mut()
            .expect("an array")
            .push(row(OTHER, 1, "dm", &s, None, None));
        let got = answer_rows(&rows, MORLOX).expect("found");
        assert_eq!(got["versions"].as_array().expect("an array").len(), 4);
        assert_eq!(
            answer_rows(&rows, OTHER).expect("found")["versions"]
                .as_array()
                .expect("an array")
                .len(),
            1
        );
    }

    #[test]
    fn an_element_never_saved_has_an_empty_history() {
        let rows = json!([{
            "id": MORLOX, "name": "Morlox", "version_number": null, "source": null,
            "snapshot": null, "previous_snapshot": null, "created_at": null,
            "restored_from": null,
        }]);
        assert_eq!(
            answer_rows(&rows, MORLOX),
            Some(json!({ "name": "Morlox", "versions": [] }))
        );
    }

    #[test]
    fn an_unknown_element_is_not_found_without_error() {
        assert_eq!(answer_rows(&four_versions(), OTHER), None);
        assert_eq!(answer_rows(&json!([]), MORLOX), None);
        assert_eq!(answer_rows(&Value::Null, MORLOX), None);
    }

    #[test]
    fn the_sql_is_one_select_statement_bound_to_one_element_newest_first() {
        assert!(
            SQL.trim_start().to_ascii_uppercase().starts_with("SELECT"),
            "{SQL}"
        );
        assert!(!SQL.contains(';'), "a second statement in {SQL}");
        assert!(SQL.contains("WHERE e.id::text = $1"), "{SQL}");
        assert!(SQL.contains("ev.entity_id::text = $1"), "{SQL}");
        assert!(SQL.contains("ORDER BY v.version_number DESC"), "{SQL}");
    }

    #[test]
    fn the_query_file_selects_what_the_sql_reads() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../queries/list-entity-versions.graphql");
        let query = std::fs::read_to_string(path).expect("the persisted query");
        let fields = query
            .lines()
            .map(str::trim)
            .skip_while(|l| *l != "versions {")
            .skip(1)
            .take_while(|l| *l != "}")
            .collect::<Vec<_>>();
        assert_eq!(
            fields,
            [
                "versionNumber",
                "source",
                "snapshot",
                "previousSnapshot",
                "createdAt",
                "restoredFrom"
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
                SQL.contains(&format!("v.{column}")),
                "the query names {field}, the SQL does not select v.{column}"
            );
        }
    }

    #[test]
    fn the_registry_s_answer_reads_its_variable_from_the_request() {
        let rows = four_versions();
        let variables = BTreeMap::from([("entity".to_string(), json!(MORLOX))]);
        let answered = answer(&rows, &variables);
        assert_ne!(answered, Value::Null);
        assert_eq!(answered, answer_rows(&rows, MORLOX).expect("found"));
        assert_ne!(answer(&rows, &BTreeMap::new()), answered);
    }
}
