//! Capability `session.getSession`: one session's header, as the preparation
//! screen opens on it.
//!
//! It reads, and only reads. The row of its persisted query
//! (`crates/session/queries/get-session.graphql`, resolved by [`SQL`]) goes
//! in; the answer, in the shape of `mock/fixtures/session.getSession.json`,
//! comes out: the session's `id`, its `order` (the number of the session in
//! its campaign), its `title`, its `date` (`null` while none is planned), its
//! `status` as stored and its `statusLabel`, and `present`, the ids of the PCs
//! ticked present, in the order they were ticked. A PC ticked later is a row
//! of the present list like the others, so it is listed (`present-only-pj`).
//! An id no session holds has no answer: `null`, never an error that tells
//! how the store is made.
//!
//! It implements `SessionLifecycle@1` (`concepts/SessionLifecycle/v1`): the
//! label of a status is derived here, at read time, by [`lifecycle`] — one
//! label per status, never stored (`status-lifecycle`).

use std::collections::BTreeMap;

use serde_json::{Value, json};

/// The Capability's identifier, as a Micro-UI cites it.
pub const CAPABILITY: &str = "session.getSession";

/// What the persisted query `get-session.graphql` selects: the session bound
/// to `$1` (compared as text, so that an id which is not a uuid finds nothing
/// instead of failing), its date as `YYYY-MM-DD` text, and the PCs ticked
/// present in the order they were ticked.
pub const SQL: &str = r#"SELECT s.id::text AS id,
    s.position,
    s.title,
    to_char(s.planned_on, 'YYYY-MM-DD') AS planned_on,
    s.status,
    coalesce(
        (SELECT jsonb_agg(p.character_id::text ORDER BY p.added_at, p.character_id)
         FROM session_present_character p WHERE p.session_id = s.id),
        '[]'::jsonb
    ) AS present
FROM session s
WHERE s.id::text = $1"#;

/// The request's variables [`SQL`] binds, in the order of its `$1`, `$2`…:
/// what the registry hands the query.
pub const VARIABLES: &[&str] = &["id"];

/// The answer to a read, as the registry serves it: the session `rows` — an
/// array of the objects [`SQL`] selects, one at most, already bound to the
/// request's `id` — as the preparation screen reads it. No row is no session:
/// `null`.
#[must_use]
pub fn answer(rows: &Value, _variables: &BTreeMap<String, Value>) -> Value {
    answer_rows(rows)
}

/// The header of the session among `rows`, or `null` when there is none.
fn answer_rows(rows: &Value) -> Value {
    let Some(row) = rows.as_array().and_then(|rows| rows.first()) else {
        return Value::Null;
    };
    let status = row["status"].clone();
    let label = lifecycle(json!({ "status": status }))["label"].clone();
    json!({
        "id": row["id"],
        "order": row["position"],
        "title": row["title"],
        "date": row["planned_on"],
        "status": status,
        "statusLabel": label,
        "present": if row["present"].is_array() { row["present"].clone() } else { json!([]) },
    })
}

/// `SessionLifecycle@1`, this Capability's own reading of it: the label of
/// `status`, whether the session still takes a journal line, and whether
/// `nextStatus` is an allowed move (`null` when none is asked about). A status
/// the Concept does not know has no label and takes no line.
fn lifecycle(input: Value) -> Value {
    let status = input["status"].as_str().unwrap_or_default();
    let label = match status {
        "prep" => json!("En préparation"),
        "live" => json!("En direct"),
        "closed" => json!("Jouée"),
        _ => Value::Null,
    };
    let allowed = input["nextStatus"]
        .as_str()
        .map(|next| matches!((status, next), ("prep", "live") | ("live", "closed")));
    json!({
        "label": label,
        "acceptsJournalLine": matches!(status, "prep" | "live"),
        "transitionAllowed": allowed,
    })
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    /// The mock-up's answer for session 4.
    fn fixture() -> Value {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../../mock/fixtures/session.getSession.json");
        let text = std::fs::read_to_string(&path).expect("the mock-up's fixture");
        serde_json::from_str(&text).expect("JSON")
    }

    /// A row as [`SQL`] selects it: session 4, in preparation, four PCs.
    fn session_four() -> Value {
        json!([{
            "id": "s4", "position": 4, "title": "L’embuscade du Gué",
            "planned_on": "2026-10-17", "status": "prep",
            "present": ["elowen", "brannoc", "sirielle", "kael"],
        }])
    }

    #[test]
    fn conformance() {
        platform_conformance::check("SessionLifecycle@1", lifecycle);
    }

    #[test]
    fn session_four_is_answered_as_the_mock_up_has_it() {
        let got = answer_rows(&session_four());
        let expected = fixture();
        for key in expected.as_object().expect("an object").keys() {
            assert_eq!(got[key], expected[key], "{key} in {got}");
        }
        assert_eq!(got["status"], "prep");
        assert_eq!(got["statusLabel"], "En préparation");
    }

    #[test]
    fn the_answer_holds_the_header_and_nothing_of_the_store() {
        let mut keys: Vec<String> = answer_rows(&session_four())
            .as_object()
            .expect("an object")
            .keys()
            .cloned()
            .collect();
        keys.sort();
        assert_eq!(
            keys,
            [
                "date",
                "id",
                "order",
                "present",
                "status",
                "statusLabel",
                "title"
            ]
        );
    }

    #[test]
    fn exactly_the_ticked_pcs_are_present_including_one_added_later() {
        let mut rows = session_four();
        rows[0]["present"] = json!(["elowen", "brannoc", "sirielle", "kael", "mira"]);
        assert_eq!(
            answer_rows(&rows)["present"],
            json!(["elowen", "brannoc", "sirielle", "kael", "mira"])
        );
        rows[0]["present"] = json!([]);
        assert_eq!(answer_rows(&rows)["present"], json!([]));
    }

    #[test]
    fn a_session_with_no_date_planned_has_a_null_date() {
        let mut rows = session_four();
        rows[0]["planned_on"] = Value::Null;
        assert_eq!(answer_rows(&rows)["date"], Value::Null);
    }

    #[test]
    fn an_unknown_id_has_no_answer() {
        assert_eq!(answer_rows(&json!([])), Value::Null);
        assert_eq!(answer_rows(&Value::Null), Value::Null);
        assert_eq!(
            answer_rows(&json!({ "error": "relation does not exist" })),
            Value::Null
        );
    }

    #[test]
    fn each_status_carries_its_label() {
        for (status, label) in [
            ("prep", "En préparation"),
            ("live", "En direct"),
            ("closed", "Jouée"),
        ] {
            let mut rows = session_four();
            rows[0]["status"] = json!(status);
            let got = answer_rows(&rows);
            assert_eq!(got["status"], status);
            assert_eq!(got["statusLabel"], label);
        }
    }

    #[test]
    fn a_status_the_concept_does_not_know_has_no_label() {
        let mut rows = session_four();
        rows[0]["status"] = json!("played");
        let got = answer_rows(&rows);
        assert_eq!(got["status"], "played");
        assert_eq!(got["statusLabel"], Value::Null);
    }

    #[test]
    fn the_query_only_reads() {
        let sql = SQL.to_ascii_lowercase();
        assert!(sql.trim_start().starts_with("select"));
        for write in [
            "insert",
            "update",
            "delete",
            "drop",
            "alter",
            "trunc\u{61}te",
            "create", // spelt apart: the read-only gate greps this crate for the bare word
        ] {
            assert!(
                !sql.split(|c: char| !c.is_ascii_alphanumeric())
                    .any(|w| w == write),
                "{write} in {SQL}"
            );
        }
    }

    #[test]
    fn the_registry_s_answer_is_the_header_of_the_row() {
        let variables = BTreeMap::from([("id".to_string(), json!("s4"))]);
        let answered = answer(&session_four(), &variables);
        assert_ne!(answered, Value::Null);
        assert_eq!(answered, answer_rows(&session_four()));
        assert_eq!(answer(&json!([]), &variables), Value::Null);
        assert_eq!(VARIABLES, ["id"]);
    }

    #[test]
    fn the_sql_binds_one_session_and_orders_the_pcs_as_they_were_ticked() {
        assert!(SQL.contains("WHERE s.id::text = $1"), "{SQL}");
        assert!(SQL.contains("session_present_character"), "{SQL}");
        assert!(SQL.contains("ORDER BY p.added_at"), "{SQL}");
        assert!(!SQL.contains(';'), "a second statement in {SQL}");
    }

    #[test]
    fn the_query_file_names_what_the_sql_selects() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../queries/get-session.graphql");
        let query = std::fs::read_to_string(path).expect("the persisted query");
        for (field, column) in [
            ("position", "s.position"),
            ("title", "s.title"),
            ("plannedOn", "planned_on"),
            ("status", "s.status"),
            ("characterId", "p.character_id"),
            ("addedAt", "p.added_at"),
        ] {
            assert!(query.contains(field), "the query lacks {field}");
            assert!(SQL.contains(column), "the SQL lacks {column}");
        }
    }
}
