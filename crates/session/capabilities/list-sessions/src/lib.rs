//! Capability `session.listSessions`: a campaign's sessions in order, for the
//! campaign page.
//!
//! It reads, and only reads. The rows of its persisted query
//! (`crates/session/queries/list-sessions.graphql`, resolved by [`SQL`]) go
//! in; the answer, in the shape of `mock/fixtures/session.listSessions.json`
//! — an array of `{ id, order, title, date, status, present, summary }` —
//! comes out, with the `label` of the status beside it.
//!
//! The status is the one `SessionLifecycle@1` defines — `prep`, `live` or
//! `closed` — and its label is the Concept's, `En préparation`, `En direct`
//! or `Jouée`, so that a session reads the same everywhere it is listed. The
//! mock-up's `played` is the label's `Jouée` of a `closed` session, not a
//! fourth status.

use serde_json::{Value, json};

/// The Capability's identifier, as a Micro-UI cites it.
pub const CAPABILITY: &str = "session.listSessions";

/// What the persisted query `list-sessions.graphql` selects: the sessions of
/// the campaign bound to `$1` (compared as text, so that an id which is not a
/// uuid lists nobody instead of failing), by position, each with the ids of
/// its present PCs as a JSON array, in the order they were ticked (empty when
/// none is). `campaign_id` is selected too, so that [`answer`] keeps only the
/// campaign's own sessions whoever ran the statement.
pub const SQL: &str = r"SELECT s.id::text AS id,
    s.campaign_id::text AS campaign_id,
    s.position,
    s.title,
    s.planned_on::text AS planned_on,
    s.status,
    coalesce(
        (SELECT jsonb_agg(p.character_id::text ORDER BY p.added_at, p.character_id) FROM session_present_character p WHERE p.session_id = s.id),
        '[]'::jsonb
    ) AS present,
    s.summary
FROM session s
WHERE s.campaign_id::text = $1
ORDER BY s.position, s.id";

/// The label `SessionLifecycle@1` gives `status`, `null` for a status it does
/// not know.
fn label(status: &str) -> Value {
    match status {
        "prep" => json!("En préparation"),
        "live" => json!("En direct"),
        "closed" => json!("Jouée"),
        _ => Value::Null,
    }
}

/// The sessions of `campaign` among `rows` — an array of the objects [`SQL`]
/// selects — as the campaign page reads them, in the order of the rows. A
/// session of another campaign is left out; one with no present PC lists
/// `[]`, one with no date or no summary `null`; no row is an empty,
/// well-formed answer.
#[must_use]
pub fn answer(rows: &Value, campaign: &str) -> Value {
    let sessions: Vec<Value> = rows
        .as_array()
        .map(|rows| {
            rows.iter()
                .filter(|row| row["campaign_id"].as_str() == Some(campaign))
                .map(|row| {
                    json!({
                        "id": row["id"],
                        "order": row["position"],
                        "title": row["title"],
                        "date": row["planned_on"],
                        "status": row["status"],
                        "label": label(row["status"].as_str().unwrap_or_default()),
                        "present": if row["present"].is_array() { row["present"].clone() } else { json!([]) },
                        "summary": row["summary"],
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    Value::Array(sessions)
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    /// `SessionLifecycle@1` as the Concept states it: what `input.status` (and, when asked, the move to
    /// `input.nextStatus`) allows — the status's label, whether it still takes a
    /// journal line, and whether the move is allowed (`null` without one). A
    /// status the Concept does not know has no label, takes no journal line and
    /// moves nowhere.
    fn lifecycle(input: &Value) -> Value {
        let status = input["status"].as_str().unwrap_or_default();
        let transition_allowed = input["nextStatus"].as_str().map_or(Value::Null, |next| {
            json!(matches!(
                (status, next),
                ("prep", "live") | ("live", "closed")
            ))
        });
        json!({
            "label": label(status),
            "acceptsJournalLine": matches!(status, "prep" | "live"),
            "transitionAllowed": transition_allowed,
        })
    }

    const CENDRES: &str = "c1000000-0000-4000-8000-000000000001";
    const OTHER: &str = "c1000000-0000-4000-8000-000000000002";

    fn fixture() -> Value {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../../mock/fixtures/session.listSessions.json");
        serde_json::from_str(&std::fs::read_to_string(path).expect("the fixture")).expect("JSON")
    }

    /// The row [`SQL`] would select for a session of the mock-up's fixture:
    /// its `played` is the `closed` status of the Concept.
    fn row_of(session: &Value, campaign: &str) -> Value {
        let status = match session["status"].as_str() {
            Some("played") => "closed",
            Some(other) => other,
            None => "",
        };
        json!({
            "id": session["id"],
            "campaign_id": campaign,
            "position": session["order"],
            "title": session["title"],
            "planned_on": session["date"],
            "status": status,
            "present": session["present"],
            "summary": session["summary"],
        })
    }

    fn rows_of_the_campaign() -> Value {
        Value::Array(
            fixture()
                .as_array()
                .expect("an array")
                .iter()
                .map(|session| row_of(session, CENDRES))
                .collect(),
        )
    }

    fn ids(sessions: &Value) -> Vec<&str> {
        sessions
            .as_array()
            .expect("an array")
            .iter()
            .map(|s| s["id"].as_str().expect("an id"))
            .collect()
    }

    #[test]
    fn conformance() {
        platform_conformance::check("SessionLifecycle@1", |input| lifecycle(&input));
    }

    #[test]
    fn the_sessions_of_the_fixture_are_listed_in_the_order_of_the_rows() {
        let listed = answer(&rows_of_the_campaign(), CENDRES);
        assert_eq!(ids(&listed), ["s1", "s2", "s3", "s4"]);
        assert_eq!(ids(&listed), ids(&fixture()));
    }

    #[test]
    fn each_session_carries_the_fields_of_the_fixture() {
        let listed = answer(&rows_of_the_campaign(), CENDRES);
        for (got, expected) in listed
            .as_array()
            .expect("an array")
            .iter()
            .zip(fixture().as_array().expect("an array"))
        {
            for field in ["id", "order", "title", "date", "present", "summary"] {
                assert_eq!(got[field], expected[field], "{field} of {}", got["id"]);
            }
        }
    }

    #[test]
    fn the_answer_holds_the_fixture_s_keys_and_the_label_only() {
        let listed = answer(&rows_of_the_campaign(), CENDRES);
        let mut keys: Vec<&str> = listed[0]
            .as_object()
            .expect("an object")
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            [
                "date", "id", "label", "order", "present", "status", "summary", "title"
            ]
        );
    }

    #[test]
    fn each_status_is_listed_with_the_label_of_the_concept() {
        let rows = json!([
            { "id": "a", "campaign_id": CENDRES, "position": 1, "title": "A", "status": "prep", "present": [] },
            { "id": "b", "campaign_id": CENDRES, "position": 2, "title": "B", "status": "live", "present": [] },
            { "id": "c", "campaign_id": CENDRES, "position": 3, "title": "C", "status": "closed", "present": [] },
        ]);
        let listed = answer(&rows, CENDRES);
        let labels: Vec<(&str, &str)> = listed
            .as_array()
            .expect("an array")
            .iter()
            .map(|s| {
                (
                    s["status"].as_str().expect("a status"),
                    s["label"].as_str().expect("a label"),
                )
            })
            .collect();
        assert_eq!(
            labels,
            [
                ("prep", "En préparation"),
                ("live", "En direct"),
                ("closed", "Jouée")
            ]
        );
    }

    #[test]
    fn the_listed_label_is_the_one_the_concept_fixtures_give() {
        let fixtures = platform_conformance::fixtures_of("SessionLifecycle@1");
        for case in fixtures
            .cases
            .iter()
            .filter(|c| c.input["nextStatus"].is_null())
        {
            let rows = json!([{
                "id": "x", "campaign_id": CENDRES, "position": 1, "title": "X",
                "status": case.input["status"], "present": [],
            }]);
            assert_eq!(
                answer(&rows, CENDRES)[0]["label"],
                case.output["label"],
                "{}",
                case.name
            );
        }
    }

    #[test]
    fn a_closed_session_shows_its_summary() {
        let listed = answer(&rows_of_the_campaign(), CENDRES);
        let closed = &listed[2];
        assert_eq!(closed["status"], "closed");
        assert_eq!(closed["summary"], fixture()[2]["summary"]);
        assert!(closed["summary"].as_str().is_some_and(|s| !s.is_empty()));
    }

    #[test]
    fn a_session_of_another_campaign_does_not_appear() {
        let mut rows = rows_of_the_campaign();
        rows.as_array_mut().expect("an array").push(json!({
            "id": "intrus", "campaign_id": OTHER, "position": 1, "title": "Intrus",
            "status": "prep", "present": [],
        }));
        let listed = answer(&rows, CENDRES);
        assert_eq!(ids(&listed).len(), 4);
        assert!(!ids(&listed).contains(&"intrus"), "{listed}");
        assert_eq!(ids(&answer(&rows, OTHER)), ["intrus"]);
    }

    #[test]
    fn no_present_pc_no_date_and_no_summary_list_an_empty_array_and_nulls() {
        let rows = json!([{
            "id": "a", "campaign_id": CENDRES, "position": 1, "title": "A", "status": "prep",
        }]);
        let listed = answer(&rows, CENDRES);
        assert_eq!(listed[0]["present"], json!([]));
        assert_eq!(listed[0]["date"], Value::Null);
        assert_eq!(listed[0]["summary"], Value::Null);
    }

    #[test]
    fn a_status_the_concept_does_not_know_has_no_label() {
        let rows = json!([{
            "id": "a", "campaign_id": CENDRES, "position": 1, "title": "A", "status": "played",
            "present": [],
        }]);
        assert_eq!(answer(&rows, CENDRES)[0]["label"], Value::Null);
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
    fn the_sql_is_bound_to_one_campaign_and_ordered_by_position() {
        assert!(SQL.contains("s.campaign_id::text = $1"), "{SQL}");
        assert!(SQL.contains("ORDER BY s.position"), "{SQL}");
    }

    #[test]
    fn the_query_file_selects_what_the_sql_reads() {
        let path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../queries/list-sessions.graphql");
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
                "campaignId",
                "position",
                "title",
                "plannedOn",
                "status",
                "present",
                "summary"
            ]
        );
        for (field, selected) in [
            ("id", "s.id::text AS id"),
            ("campaignId", "s.campaign_id::text AS campaign_id"),
            ("position", "s.position"),
            ("title", "s.title"),
            ("plannedOn", "s.planned_on::text AS planned_on"),
            ("status", "s.status"),
            ("present", "AS present"),
            ("summary", "s.summary"),
        ] {
            assert!(
                SQL.contains(selected),
                "the query names {field}, the SQL does not select it"
            );
        }
    }
}
