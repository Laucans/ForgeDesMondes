//! Capability `session.getSessionJournal`: a session's timestamped journal.
//!
//! It reads, and only reads. The rows of its persisted query
//! (`crates/session/queries/getSessionJournal.graphql`, resolved by [`SQL`])
//! go in; the answer, in the shape of
//! `mock/fixtures/session.getSessionJournal.json` — `{ journal: [{ t, text }] }`
//! — comes out. The fixture's `journalStats` is a figure of the session's
//! summary, not a line of its journal: another read.
//!
//! The same read serves a live session — the lines recorded so far, so the next
//! read includes a line just recorded — and a closed one, every line kept at
//! closing. The journal is transcribed text only: the query selects no other
//! column than the time and the text.

use serde_json::{Value, json};

/// The Capability's identifier, as a Micro-UI cites it.
pub const CAPABILITY: &str = "session.getSessionJournal";

/// What the persisted query `getSessionJournal.graphql` selects: the lines of
/// the session bound to `$1` (compared as text, so that an id which is not a
/// uuid lists nothing instead of failing), in spoken order — by the moment
/// they were spoken, then by id so that two lines of the same instant keep a
/// stable place. `t` is the time of day, `HH:MM`, in UTC; `session_id` is
/// selected too, so that [`answer`] keeps only the session's own lines
/// whoever ran the statement.
pub const SQL: &str = r"SELECT l.session_id::text AS session_id,
    to_char(l.spoken_at AT TIME ZONE 'UTC', 'HH24:MI') AS t,
    l.text
FROM session_journal_line l
WHERE l.session_id::text = $1
ORDER BY l.spoken_at, l.id";

/// The journal of `session` among `rows` — an array of the objects [`SQL`]
/// selects — as `{ journal: [{ t, text }] }`, in the order of the rows. A
/// line of another session is left out, and so is a row with no time or no
/// text (or only blanks): every line returned has a non-empty `t` and `text`. No row is an
/// empty, well-formed journal.
#[must_use]
pub fn answer(rows: &Value, session: &str) -> Value {
    let journal: Vec<Value> = rows
        .as_array()
        .map(|rows| {
            rows.iter()
                .filter(|row| row["session_id"].as_str() == Some(session))
                .filter_map(|row| {
                    let t = row["t"].as_str().filter(|t| !t.trim().is_empty())?;
                    let text = row["text"]
                        .as_str()
                        .filter(|text| !text.trim().is_empty())?;
                    Some(json!({ "t": t, "text": text }))
                })
                .collect()
        })
        .unwrap_or_default();
    json!({ "journal": journal })
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    const LAST: &str = "5e550000-0000-4000-8000-000000000001";
    const OTHER: &str = "5e550000-0000-4000-8000-000000000002";

    fn row(session: &str, t: &str, text: &str) -> Value {
        json!({ "session_id": session, "t": t, "text": text })
    }

    fn fixture() -> Value {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../../mock/fixtures/session.getSessionJournal.json");
        serde_json::from_str(&std::fs::read_to_string(path).expect("the fixture")).expect("JSON")
    }

    /// The fixture's lines as the rows [`SQL`] would select for them.
    fn rows_of_the_session(session: &str) -> Value {
        Value::Array(
            fixture()["journal"]
                .as_array()
                .expect("an array")
                .iter()
                .map(|line| {
                    row(
                        session,
                        line["t"].as_str().expect("t"),
                        line["text"].as_str().expect("text"),
                    )
                })
                .collect(),
        )
    }

    #[test]
    fn the_journal_of_a_session_is_the_fixtures_in_spoken_order() {
        let read = answer(&rows_of_the_session(LAST), LAST);
        assert_eq!(read["journal"], fixture()["journal"]);
        assert!(!read["journal"].as_array().expect("an array").is_empty());
    }

    #[test]
    fn a_line_carries_only_its_time_and_its_text() {
        let rows = json!([{
            "session_id": LAST,
            "t": "20:04",
            "text": "On reprend sous le pont.",
            "audio": "base64…",
            "scene_id": "x",
        }]);
        assert_eq!(
            answer(&rows, LAST),
            json!({ "journal": [{ "t": "20:04", "text": "On reprend sous le pont." }] })
        );
    }

    #[test]
    fn a_line_recorded_since_is_in_the_next_read() {
        let mut rows = rows_of_the_session(LAST);
        let before = answer(&rows, LAST)["journal"]
            .as_array()
            .expect("an array")
            .len();
        rows.as_array_mut().expect("an array").push(row(
            LAST,
            "23:20",
            "Une dernière question avant de partir.",
        ));
        let after = answer(&rows, LAST);
        let lines = after["journal"].as_array().expect("an array");
        assert_eq!(lines.len(), before + 1);
        assert_eq!(
            lines.last().expect("a line"),
            &json!({ "t": "23:20", "text": "Une dernière question avant de partir." })
        );
    }

    #[test]
    fn every_line_kept_at_closing_has_a_time_and_a_text() {
        let read = answer(&rows_of_the_session(LAST), LAST);
        for line in read["journal"].as_array().expect("an array") {
            assert!(line["t"].as_str().is_some_and(|t| !t.is_empty()), "{line}");
            assert!(
                line["text"].as_str().is_some_and(|t| !t.is_empty()),
                "{line}"
            );
        }
    }

    #[test]
    fn a_row_with_no_time_or_no_text_is_not_a_line() {
        let rows = json!([
            row(LAST, "20:04", "Gardée."),
            row(LAST, "", "Sans heure."),
            row(LAST, "20:05", ""),
            row(LAST, "20:05", "  \n"),
            { "session_id": LAST, "t": null, "text": "Null." },
            { "session_id": LAST, "t": "20:06" },
        ]);
        assert_eq!(
            answer(&rows, LAST),
            json!({ "journal": [{ "t": "20:04", "text": "Gardée." }] })
        );
    }

    #[test]
    fn a_line_of_another_session_does_not_appear() {
        let mut rows = rows_of_the_session(LAST);
        rows.as_array_mut()
            .expect("an array")
            .push(row(OTHER, "21:00", "Une autre séance."));
        let read = answer(&rows, LAST);
        assert_eq!(read["journal"], fixture()["journal"]);
        assert_eq!(
            answer(&rows, OTHER),
            json!({ "journal": [{ "t": "21:00", "text": "Une autre séance." }] })
        );
    }

    #[test]
    fn a_session_with_no_line_has_an_empty_journal() {
        assert_eq!(answer(&json!([]), LAST), json!({ "journal": [] }));
        assert_eq!(answer(&Value::Null, LAST), json!({ "journal": [] }));
    }

    #[test]
    fn the_statement_selects_text_only_in_spoken_order() {
        assert!(SQL.contains("FROM session_journal_line"));
        assert!(SQL.contains("ORDER BY l.spoken_at, l.id"));
        let upper = SQL.to_uppercase();
        for write in ["INSERT", "UPDATE", "DELETE"] {
            assert!(!upper.contains(write), "{write} in {SQL}");
        }
        assert!(!SQL.contains("audio"));
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
        for read in manifest["reads"].as_array().expect("reads") {
            let file = dir.join("../..").join(read.as_str().expect("a path"));
            assert!(file.is_file(), "{}", file.display());
        }
    }
}
