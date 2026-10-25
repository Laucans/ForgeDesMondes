//! Capability `session.getPreviousSessionSummary`: what happened last time, as
//! the preparation screen opens on it.
//!
//! It reads, and only reads. The row of its persisted query
//! (`crates/session/queries/getPreviousSessionSummary.graphql`, resolved by
//! [`SQL`]) goes in; the answer, in the shape of
//! `mock/fixtures/session.getPreviousSessionSummary.json`, comes out: the closed
//! `session` (`id`, `order` — its number in the campaign —, `title`, `date`),
//! its `summary`, and `summaryMeta`, the line that says which model wrote the
//! summary and when the DM edited it.
//!
//! The session is the closed one before the session asked about, or that
//! session itself when it is closed: the statement picks it, in the campaign of
//! the session bound to `$1`. A session that is not closed has no previous
//! summary (`status-lifecycle`), so [`answer_rows`] leaves out a row whose
//! status `SessionLifecycle@1` does not close. A campaign with no closed
//! session yet has no summary: `session`, `summary` and `summaryMeta` are
//! `null`, the answer is not an error.
//!
//! The fixture's `mentions` and `threads` are not stored anywhere — the tables
//! keep the summary text and the model — so they answer empty.
//!
//! It implements `SessionLifecycle@1` (`concepts/SessionLifecycle/v1`): whether
//! a status is `closed` is derived here, by [`lifecycle`], never read.

use std::collections::BTreeMap;

use serde_json::{Value, json};

/// The Capability's identifier, as a Micro-UI cites it.
pub const CAPABILITY: &str = "session.getPreviousSessionSummary";

/// What the persisted query `getPreviousSessionSummary.graphql` selects: among
/// the closed sessions of the campaign of the session bound to `$1` (compared
/// as text, so that an id which is not a uuid finds nothing instead of
/// failing), the last one at or before its position — itself when it is
/// closed — with its date and the day of the last edit as `YYYY-MM-DD` text
/// (UTC). Two closed sessions of one position keep the later close.
pub const SQL: &str = r"SELECT s.id::text AS id,
    s.position,
    s.title,
    to_char(s.planned_on, 'YYYY-MM-DD') AS planned_on,
    s.status,
    s.summary,
    s.summary_model,
    to_char(s.summary_edited_at AT TIME ZONE 'UTC', 'YYYY-MM-DD') AS summary_edited_on
FROM session s
JOIN session asked ON asked.id::text = $1
WHERE s.campaign_id = asked.campaign_id
  AND s.status = 'closed'
  AND s.position <= asked.position
ORDER BY s.position DESC, s.closed_at DESC NULLS LAST
LIMIT 1";

/// The request's variables [`SQL`] binds, in the order of its `$1`, `$2`…:
/// what the registry hands the query.
pub const VARIABLES: &[&str] = &["session"];

/// The answer to a read, as the registry serves it: the `rows` of [`SQL`] —
/// one at most, already bound to the request's `session` — as the preparation
/// screen reads them.
#[must_use]
pub fn answer(rows: &Value, _variables: &BTreeMap<String, Value>) -> Value {
    answer_rows(rows)
}

/// The previous summary among `rows`, or the empty answer when there is no
/// closed session.
fn answer_rows(rows: &Value) -> Value {
    let Some(row) = rows
        .as_array()
        .and_then(|rows| rows.first())
        .filter(|row| row.is_object() && is_closed(&row["status"]))
    else {
        return json!({ "session": null, "summary": null, "summaryMeta": null, "mentions": [], "threads": [] });
    };
    let summary = row["summary"].as_str().filter(|s| !s.trim().is_empty());
    json!({
        "session": {
            "id": row["id"],
            "order": row["position"],
            "title": row["title"],
            "date": row["planned_on"],
        },
        "summary": summary,
        "summaryMeta": summary.and_then(|_| meta(row)),
        "mentions": [],
        "threads": [],
    })
}

/// Whether `status` is the one status that has a previous summary: `closed`,
/// the status `SessionLifecycle@1` labels `Jouée`.
fn is_closed(status: &Value) -> bool {
    lifecycle(json!({ "status": status }))["label"] == "Jouée"
}

/// The line that says where a summary comes from: the model recorded at
/// closing, and the day it was edited — `Généré à la clôture par <model>,
/// retouché le 4 oct. 2026`. Nothing recorded, no line.
fn meta(row: &Value) -> Option<String> {
    let generated = row["summary_model"]
        .as_str()
        .map(str::trim)
        .filter(|model| !model.is_empty())
        .map(|model| format!("Généré à la clôture par {model}"));
    let edited = row["summary_edited_on"].as_str().and_then(french_date);
    match (generated, edited) {
        (Some(generated), Some(edited)) => Some(format!("{generated}, retouché le {edited}")),
        (Some(generated), None) => Some(generated),
        (None, Some(edited)) => Some(format!("Retouché le {edited}")),
        (None, None) => None,
    }
}

/// `2026-10-04` as `4 oct. 2026` (`1er` for the first); `None` for anything
/// else than a calendar date.
fn french_date(day: &str) -> Option<String> {
    const MONTHS: [&str; 12] = [
        "janv.", "févr.", "mars", "avr.", "mai", "juin", "juil.", "août", "sept.", "oct.", "nov.",
        "déc.",
    ];
    let mut parts = day.splitn(3, '-');
    let year: u32 = parts.next()?.parse().ok()?;
    let number: usize = parts.next()?.parse().ok()?;
    let day: u32 = parts.next()?.parse().ok()?;
    let month = MONTHS.get(number.checked_sub(1)?)?;
    let last = match number {
        2 => 29,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    };
    if day == 0 || day > last {
        return None;
    }
    let day = if day == 1 {
        "1er".to_owned()
    } else {
        day.to_string()
    };
    Some(format!("{day} {month} {year}"))
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

    /// The mock-up's answer: session 3, closed, its summary.
    fn fixture() -> Value {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../../mock/fixtures/session.getPreviousSessionSummary.json");
        let text = std::fs::read_to_string(&path).expect("the mock-up's fixture");
        serde_json::from_str(&text).expect("JSON")
    }

    /// The row [`SQL`] selects for session 3 of the fixture: closed, summary
    /// written by the model the fixture names, edited on 4 October 2026.
    fn session_three() -> Value {
        let fixture = fixture();
        json!([{
            "id": "s3", "position": 3, "title": "Le marché noir",
            "planned_on": "2026-10-03", "status": "closed",
            "summary": fixture["summary"],
            "summary_model": "qwen2.5-14b-instruct (opencode maison)",
            "summary_edited_on": "2026-10-04",
        }])
    }

    fn nothing() -> Value {
        json!({ "session": null, "summary": null, "summaryMeta": null, "mentions": [], "threads": [] })
    }

    #[test]
    fn conformance() {
        platform_conformance::check("SessionLifecycle@1", lifecycle);
    }

    #[test]
    fn the_closed_session_is_answered_as_the_mock_up_has_it() {
        let got = answer_rows(&session_three());
        let expected = fixture();
        for key in ["session", "summary", "summaryMeta"] {
            assert_eq!(got[key], expected[key], "{key} in {got}");
        }
    }

    #[test]
    fn a_session_just_closed_has_its_summary_and_the_model_recorded_at_closing() {
        let rows = json!([{
            "id": "s4", "position": 4, "title": "L’embuscade du Gué",
            "planned_on": null, "status": "closed",
            "summary": "Le groupe a repoussé l’embuscade.",
            "summary_model": "claude-haiku-4-5-20251001",
            "summary_edited_on": null,
        }]);
        let got = answer_rows(&rows);
        assert_eq!(got["summary"], "Le groupe a repoussé l’embuscade.");
        assert_eq!(
            got["summaryMeta"],
            "Généré à la clôture par claude-haiku-4-5-20251001"
        );
        assert_eq!(
            got["session"],
            json!({ "id": "s4", "order": 4, "title": "L’embuscade du Gué", "date": null })
        );
    }

    #[test]
    fn the_answer_holds_the_summary_and_nothing_of_the_store() {
        let mut row = session_three();
        row[0]["campaign_id"] = json!("c1");
        row[0]["microphone_on"] = json!(false);
        let got = answer_rows(&row);
        let mut keys: Vec<&String> = got.as_object().expect("an object").keys().collect();
        keys.sort();
        assert_eq!(
            keys,
            ["mentions", "session", "summary", "summaryMeta", "threads"]
        );
        let mut session: Vec<&String> = got["session"]
            .as_object()
            .expect("an object")
            .keys()
            .collect();
        session.sort();
        assert_eq!(session, ["date", "id", "order", "title"]);
        assert_eq!(got["mentions"], json!([]));
        assert_eq!(got["threads"], json!([]));
    }

    #[test]
    fn no_closed_session_yet_is_no_summary_and_no_error() {
        assert_eq!(answer_rows(&json!([])), nothing());
        assert_eq!(answer_rows(&Value::Null), nothing());
        assert_eq!(answer_rows(&json!([null])), nothing());
        assert_eq!(answer_rows(&json!([{}])), nothing());
    }

    #[test]
    fn only_a_closed_session_has_a_previous_summary() {
        for status in ["prep", "live", "played", ""] {
            let mut rows = session_three();
            rows[0]["status"] = json!(status);
            assert_eq!(answer_rows(&rows), nothing(), "{status:?}");
        }
        let mut no_status = session_three();
        no_status[0]
            .as_object_mut()
            .expect("an object")
            .remove("status");
        assert_eq!(answer_rows(&no_status), nothing());
    }

    #[test]
    fn a_closed_session_with_no_summary_has_no_text_and_no_meta() {
        for summary in [json!(null), json!(""), json!("  \n")] {
            let mut rows = session_three();
            rows[0]["summary"] = summary;
            let got = answer_rows(&rows);
            assert_eq!(got["summary"], Value::Null);
            assert_eq!(got["summaryMeta"], Value::Null);
            assert_eq!(got["session"]["id"], "s3");
        }
    }

    #[test]
    fn the_meta_says_the_model_and_the_edit_when_they_are_recorded() {
        let meta_of = |model: Value, edited: Value| {
            let mut rows = session_three();
            rows[0]["summary_model"] = model;
            rows[0]["summary_edited_on"] = edited;
            answer_rows(&rows)["summaryMeta"].clone()
        };
        assert_eq!(
            meta_of(json!("m"), json!("2026-10-04")),
            "Généré à la clôture par m, retouché le 4 oct. 2026"
        );
        assert_eq!(
            meta_of(json!(" m "), json!(null)),
            "Généré à la clôture par m"
        );
        assert_eq!(
            meta_of(json!(null), json!("2026-01-01")),
            "Retouché le 1er janv. 2026"
        );
        assert_eq!(
            meta_of(json!("  "), json!("2026-12-31")),
            "Retouché le 31 déc. 2026"
        );
        assert_eq!(meta_of(json!(null), json!(null)), Value::Null);
        assert_eq!(
            meta_of(json!("m"), json!("not a date")),
            "Généré à la clôture par m"
        );
    }

    #[test]
    fn french_dates_name_every_month() {
        let months = [
            "janv.", "févr.", "mars", "avr.", "mai", "juin", "juil.", "août", "sept.", "oct.",
            "nov.", "déc.",
        ];
        for (i, month) in months.iter().enumerate() {
            assert_eq!(
                french_date(&format!("2026-{:02}-15", i + 1)),
                Some(format!("15 {month} 2026"))
            );
        }
        assert_eq!(french_date("2026-10-02").as_deref(), Some("2 oct. 2026"));
        assert_eq!(french_date("2026-04-30").as_deref(), Some("30 avr. 2026"));
        assert_eq!(french_date("2028-02-29").as_deref(), Some("29 févr. 2028"));
        assert_eq!(french_date("2026-04-30").as_deref(), Some("30 avr. 2026"));
        assert_eq!(french_date("2028-02-29").as_deref(), Some("29 févr. 2028"));
        for bad in [
            "",
            "2026",
            "2026-10",
            "2026-00-10",
            "2026-13-10",
            "2026-10-00",
            "2026-10-32",
            "2026-02-30",
            "2026-04-31",
            "2026-06-31",
            "2026-09-31",
            "2026-11-31",
            "x-y-z",
        ] {
            assert_eq!(french_date(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn the_registry_hands_the_session_to_the_statement() {
        assert_eq!(VARIABLES, ["session"]);
        assert_eq!(
            answer(&session_three(), &BTreeMap::new()),
            answer_rows(&session_three())
        );
        assert_eq!(CAPABILITY, "session.getPreviousSessionSummary");
    }

    #[test]
    fn the_statement_picks_the_closed_session_at_or_before_the_one_asked() {
        assert!(SQL.contains("FROM session s"));
        assert!(SQL.contains("s.campaign_id = asked.campaign_id"));
        assert!(SQL.contains("s.status = 'closed'"));
        assert!(SQL.contains("s.position <= asked.position"));
        assert!(SQL.contains("ORDER BY s.position DESC"));
        assert!(SQL.contains("LIMIT 1"));
        let upper = SQL.to_uppercase();
        for write in ["INSERT", "UPDATE", "DELETE"] {
            assert!(!upper.contains(write), "{write} in {SQL}");
        }
        assert!(!SQL.contains("journal"));
    }

    #[test]
    fn the_manifest_names_this_capability_and_its_query() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR"));
        let manifest: Value = serde_json::from_str(
            &std::fs::read_to_string(dir.join("capability.json")).expect("the manifest"),
        )
        .expect("JSON");
        assert_eq!(manifest["capability"], CAPABILITY);
        assert_eq!(manifest["system"], "session");
        assert_eq!(manifest["implements"], "SessionLifecycle@1");
        for read in manifest["reads"].as_array().expect("reads") {
            let file = dir.join("../..").join(read.as_str().expect("a path"));
            assert!(file.is_file(), "{}", file.display());
        }
    }
}
