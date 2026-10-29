//! Capability `live.listSuggestions`: what the suggestion pill shows.
//!
//! It reads, and only reads. The rows of its persisted query
//! (`crates/live/queries/list-suggestions.graphql`, resolved by [`SQL`]) go in —
//! every suggestion of the campaign's live session, newest first, each
//! resolved to the element or the sound it names; the answer comes out:
//! `suggestions` and `history`. It deliberately does not take the flat shape
//! of `mock/fixtures/live.listSuggestions.json` (one list, `heard`, no time).
//!
//! Nothing is stored beyond the suggestions (pile-and-history-derived): the
//! pill's stack is the first [`PILE`] rows, the history is all of them, both
//! by `detected_at` descending. It produces no suggestion — `live.setMicrophone`
//! does.

use std::collections::BTreeMap;

use serde_json::{Value, json};

/// The Capability's identifier, as a Micro-UI cites it.
pub const CAPABILITY: &str = "live.listSuggestions";

/// How many suggestions the pill's stack holds: the most recent ones.
const PILE: usize = 5;

/// What the persisted query `list-suggestions.graphql` selects: the live
/// session of the campaign bound to `$1` — of two, the last one planned, as
/// the DataCapabilities that write find it, a session the engine deleted (its
/// tombstone) being none; the last one started, among the campaigns', when `$1`
/// is the empty text or null (a request without the `campaign` variable) — and its suggestions, newest first,
/// each with the element or the sound it names and the words that were heard.
/// A row that names not exactly one of them, or one of another campaign, breaks
/// suggestion-target and is left out; so is a suggestion, an element or a sound
/// the engine deleted. The campaign is compared as text, so
/// that an id which is not a uuid finds nothing instead of failing.
pub const SQL: &str = r#"WITH live AS (
    SELECT s.id, s.campaign_id
    FROM session s
    WHERE s.status = 'live' AND (coalesce($1, '') = '' OR s.campaign_id::text = $1)
        AND NOT EXISTS (SELECT 1 FROM dataguard_rows r
            WHERE r.partition = 'Session/' || s.id::text AND r.tombstoned)
    ORDER BY CASE WHEN coalesce($1, '') = '' THEN s.started_at END DESC NULLS LAST, s.position DESC, s.id
    LIMIT 1
)
SELECT
    coalesce(g.entity_id, g.sound_id)::text AS id,
    CASE WHEN g.entity_id IS NOT NULL THEN 'entity' ELSE 'sound' END AS kind,
    e.type AS type,
    coalesce(e.name, so.name) AS name,
    g.matched_text AS via,
    to_char(g.detected_at AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.MS"Z"') AS detected_at
FROM live_suggestion g
JOIN live ON live.id = g.session_id
LEFT JOIN entity e ON e.id = g.entity_id AND e.campaign_id = live.campaign_id
    AND NOT EXISTS (SELECT 1 FROM dataguard_rows r
        WHERE r.partition = 'Entity/' || e.id::text AND r.tombstoned)
LEFT JOIN sound so ON so.id = g.sound_id AND so.campaign_id = live.campaign_id
    AND NOT EXISTS (SELECT 1 FROM dataguard_rows r
        WHERE r.partition = 'Sound/' || so.id::text AND r.tombstoned)
WHERE num_nonnulls(g.entity_id, g.sound_id) = 1
    AND num_nonnulls(e.id, so.id) = 1
    AND NOT EXISTS (SELECT 1 FROM dataguard_rows r
        WHERE r.partition = 'Suggestion/' || g.id::text AND r.tombstoned)
ORDER BY g.detected_at DESC, g.id"#;

/// The request's variables [`SQL`] binds, in the order of its `$1`: the
/// campaign.
pub const VARIABLES: &[&str] = &["campaign"];

/// How the screen words an element's type: the store's `evenement` is an
/// « événement », the other types read as they are stored.
fn kind_of(stored: &Value) -> Value {
    if stored == "evenement" {
        json!("événement")
    } else {
        stored.clone()
    }
}

/// A suggestion as the pill reads it: what it names (`id`, `kind`, `type` for
/// an element only, `name`), the words heard (`via`) and when (`detectedAt`).
fn item(row: &Value) -> Value {
    let mut item = json!({
        "id": row["id"],
        "kind": row["kind"],
        "name": row["name"],
        "via": row["via"],
        "detectedAt": row["detected_at"],
    });
    if row["kind"] == "entity" {
        item["type"] = kind_of(&row["type"]);
    }
    item
}

/// The pill's `{ suggestions, history }` out of `rows` — the objects [`SQL`]
/// selects, newest first. No row (no live session, or none for the campaign,
/// or no suggestion yet) leaves both empty.
#[must_use]
pub fn answer_rows(rows: &Value) -> Value {
    let history: Vec<Value> = rows
        .as_array()
        .map(|rows| rows.iter().map(item).collect())
        .unwrap_or_default();
    json!({
        "suggestions": history.iter().take(PILE).collect::<Vec<_>>(),
        "history": history,
    })
}

/// The answer to a read, as the registry serves it: the rows of [`SQL`]; the
/// variables bind the query and do not shape the answer.
#[must_use]
pub fn answer(rows: &Value, _variables: &BTreeMap<String, Value>) -> Value {
    answer_rows(rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entity_row(n: u32, stored_type: &str) -> Value {
        json!({
            "id": format!("e{n}"), "kind": "entity", "type": stored_type,
            "name": format!("Nom {n}"), "via": format!("mot {n}"),
            "detected_at": format!("2026-01-01T00:00:{:02}.000Z", 59 - n),
        })
    }

    fn sound_row(n: u32) -> Value {
        json!({
            "id": format!("s{n}"), "kind": "sound", "type": null,
            "name": format!("Son {n}"), "via": format!("bruit {n}"),
            "detected_at": format!("2026-01-01T00:00:{:02}.000Z", 59 - n),
        })
    }

    fn ids(items: &Value) -> Vec<&str> {
        items
            .as_array()
            .expect("an array")
            .iter()
            .map(|i| i["id"].as_str().expect("an id"))
            .collect()
    }

    #[test]
    fn the_stack_holds_the_five_most_recent_and_the_history_all_of_them() {
        let rows: Vec<Value> = (0..8).map(|n| entity_row(n, "pnj")).collect();
        let got = answer_rows(&json!(rows));
        assert_eq!(ids(&got["suggestions"]), ["e0", "e1", "e2", "e3", "e4"]);
        assert_eq!(
            ids(&got["history"]),
            ["e0", "e1", "e2", "e3", "e4", "e5", "e6", "e7"]
        );
    }

    #[test]
    fn under_five_suggestions_the_stack_and_the_history_are_the_same() {
        let rows = json!([entity_row(0, "lieu"), sound_row(1)]);
        let got = answer_rows(&rows);
        assert_eq!(ids(&got["suggestions"]), ["e0", "s1"]);
        assert_eq!(got["suggestions"], got["history"]);
    }

    #[test]
    fn exactly_five_suggestions_fill_the_stack() {
        let rows: Vec<Value> = (0..5).map(|n| entity_row(n, "pnj")).collect();
        let got = answer_rows(&json!(rows));
        assert_eq!(got["suggestions"].as_array().map(Vec::len), Some(5));
        assert_eq!(got["history"].as_array().map(Vec::len), Some(5));
    }

    #[test]
    fn an_element_carries_its_id_kind_type_name_via_and_time() {
        let got = answer_rows(&json!([entity_row(3, "pnj")]));
        assert_eq!(
            got["history"][0],
            json!({
                "id": "e3", "kind": "entity", "type": "pnj", "name": "Nom 3",
                "via": "mot 3", "detectedAt": "2026-01-01T00:00:56.000Z",
            })
        );
    }

    #[test]
    fn a_sound_carries_no_type() {
        let got = answer_rows(&json!([sound_row(2)]));
        let sound = &got["history"][0];
        assert_eq!(
            *sound,
            json!({
                "id": "s2", "kind": "sound", "name": "Son 2",
                "via": "bruit 2", "detectedAt": "2026-01-01T00:00:57.000Z",
            })
        );
        assert!(sound.get("type").is_none(), "a sound has no type");
    }

    #[test]
    fn a_stored_evenement_reads_with_an_accent_and_the_others_as_stored() {
        let rows = json!([
            entity_row(0, "evenement"),
            entity_row(1, "lieu"),
            entity_row(2, "pj"),
        ]);
        let got = answer_rows(&rows);
        let types: Vec<&str> = got["history"]
            .as_array()
            .expect("an array")
            .iter()
            .map(|i| i["type"].as_str().expect("a type"))
            .collect();
        assert_eq!(types, ["événement", "lieu", "pj"]);
    }

    #[test]
    fn no_row_leaves_both_empty() {
        for rows in [json!([]), Value::Null, json!({})] {
            assert_eq!(
                answer_rows(&rows),
                json!({ "suggestions": [], "history": [] }),
                "{rows}"
            );
        }
    }

    #[test]
    fn the_variables_do_not_shape_the_answer() {
        let rows = json!([entity_row(0, "pnj")]);
        let variables = BTreeMap::from([("campaign".to_string(), json!("c"))]);
        assert_eq!(answer(&rows, &variables), answer_rows(&rows));
        assert_eq!(VARIABLES, ["campaign"]);
        assert_eq!(CAPABILITY, "live.listSuggestions");
    }
}
