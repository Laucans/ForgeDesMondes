//! Capability `session.getLastBrief`: a session's last oral brief and what its
//! analysis found, for the DM who dictates a brief.
//!
//! It reads, and only reads. The row of its persisted query
//! (`crates/session/queries/get-last-brief.graphql`, resolved by [`SQL`]) goes
//! in; the answer, in the shape of `mock/fixtures/session.getLastBrief.json`,
//! comes out: `session`, `lastRun`, `duration` (`mm:ss`), `transcript` (one
//! entry per line of the transcribed text) and `result` — `scenes`,
//! `recognized` and `missing`. Nothing is recomputed: this is what the brief
//! left in the store (`text-only-no-audio`: the text is all there is).
//!
//! The brief is the most recently recorded one of the session. Its scenes are
//! the session's scenes by rank (a scene is not tied to a brief, only to its
//! session), each with the names of its linked elements and, after them, the
//! entries still to create it holds, marked ` (à créer)` by [`SQL`]. The recognised
//! elements are the ones linked to those scenes; one is `updated` when the
//! brief wrote a version of it (`source = 'brief'`, after the brief was
//! recorded), and `change` then names the fields of that version's diff.
//! `heard` is what the transcript says of it: the text of the brief that
//! matches its name or one of its aliases, as `AliasMatch@1` normalises them
//! (case, accents, white space), `null` when the transcript names it by none.
//! The entries `missing` are the ones the brief added to the to-create list
//! and that are not created yet.
//!
//! A brief not yet analysed has no scenes, no recognised and no missing
//! element, and no `lastRun`. A session with no brief answers the same shape,
//! empty — no brief, never an error.

use std::collections::BTreeMap;

use serde_json::{Map, Value, json};

/// The Capability's identifier, as a Micro-UI cites it.
pub const CAPABILITY: &str = "session.getLastBrief";

/// What the persisted query `get-last-brief.graphql` selects: the most
/// recently recorded brief of the session bound to `$1` (compared as text, so
/// that an id which is not a uuid finds nothing instead of failing), its
/// analysis date as `YYYY-MM-DDTHH:MM:SS` text in UTC, and — once analysed —
/// the session's scenes by rank with their links, the elements linked to them
/// with what the transcript says of each and the fields every version the
/// brief wrote changed, and the entries to create the brief added, as JSON
/// arrays (empty when there is none). `heard` is cut out of the transcript
/// (in its composed form) where the name or an alias is found as whole words
/// once normalised (`alias_match_normalized`, AliasMatch@1; the punctuation
/// counts as a space), the longest first. The transcript is normalised once,
/// wrapped in two marks so that the normalisation trims none of it; a
/// normalisation that changed its length finds nothing rather than a wrong
/// cut.
pub const SQL: &str = r#"SELECT b.id::text AS id,
    b.session_id::text AS session_id,
    b.transcript,
    b.duration_seconds,
    to_char(b.analyzed_at AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS') AS analyzed_at,
    CASE WHEN b.analyzed_at IS NULL THEN '[]'::jsonb ELSE coalesce(
        (SELECT jsonb_agg(jsonb_build_object(
                'order', sc.position,
                'title', sc.title,
                'links', coalesce(
                    (SELECT jsonb_agg(l.label ORDER BY l.grp, l.pos, l.at, l.id)
                     FROM (SELECT 0 AS grp, se.position AS pos, e.created_at AS at, e.id::text AS id, e.name AS label
                           FROM scene_entity se JOIN entity e ON e.id = se.entity_id WHERE se.scene_id = sc.id
                           UNION ALL
                           SELECT 1, 0, t.created_at, t.id::text, t.name || ' (à créer)'
                           FROM entity_to_create t WHERE t.scene_id = sc.id AND t.status = 'a_creer') l),
                    '[]'::jsonb
                )) ORDER BY sc.position, sc.id)
         FROM scene sc WHERE sc.session_id = b.session_id),
        '[]'::jsonb
    ) END AS scenes,
    CASE WHEN b.analyzed_at IS NULL THEN '[]'::jsonb ELSE coalesce(
        (SELECT jsonb_agg(jsonb_build_object(
                'id', e.id::text,
                'type', e.type,
                'name', e.name,
                'heard', (SELECT substr(n.text, strpos(n.words, ' ' || t.term || ' '), char_length(t.term))
                          FROM (SELECT regexp_replace(alias_match_normalized(x.term), '[^[:alnum:]]', ' ', 'g') AS term
                                FROM (SELECT e.name AS term
                                      UNION ALL
                                      SELECT a.alias FROM entity_alias a WHERE a.entity_id = e.id) x) t
                          WHERE btrim(t.term) <> '' AND strpos(n.words, ' ' || t.term || ' ') > 0
                            AND char_length(n.words) = char_length(n.text) + 2
                          ORDER BY char_length(t.term) DESC, strpos(n.words, ' ' || t.term || ' ') LIMIT 1),
                'version', CASE WHEN EXISTS (SELECT 1 FROM entity_version ev
                                             WHERE ev.entity_id = e.id AND ev.source = 'brief' AND ev.created_at >= b.recorded_at)
                    THEN jsonb_build_object('fields', coalesce(
                        (SELECT jsonb_agg(c.field ORDER BY c.field)
                         FROM (SELECT DISTINCT d.value ->> 'field' AS field
                               FROM entity_version ev, jsonb_array_elements(ev.diff) d
                               WHERE ev.entity_id = e.id AND ev.source = 'brief' AND ev.created_at >= b.recorded_at) c),
                        '[]'::jsonb)) END
            ) ORDER BY f.ord, f.pos, e.id)
         FROM (SELECT DISTINCT ON (se.entity_id) se.entity_id, sc.position AS ord, se.position AS pos
               FROM scene_entity se JOIN scene sc ON sc.id = se.scene_id
               WHERE sc.session_id = b.session_id
               ORDER BY se.entity_id, sc.position, se.position) f
         JOIN entity e ON e.id = f.entity_id),
        '[]'::jsonb
    ) END AS recognized,
    CASE WHEN b.analyzed_at IS NULL THEN '[]'::jsonb ELSE coalesce(
        (SELECT jsonb_agg(jsonb_build_object('id', t.id::text, 'type', t.type, 'name', t.name) ORDER BY t.created_at, t.id)
         FROM entity_to_create t WHERE t.brief_id = b.id AND t.status = 'a_creer'),
        '[]'::jsonb
    ) END AS missing
FROM (SELECT * FROM oral_brief WHERE session_id::text = $1 ORDER BY recorded_at DESC, id DESC LIMIT 1) b
CROSS JOIN LATERAL (SELECT z.text,
                           regexp_replace(alias_match_normalized('·' || z.text || '·'), '[^[:alnum:]]', ' ', 'g') AS words
                    FROM (SELECT normalize(b.transcript, NFC) AS text) z) n"#;

/// The request's variables [`SQL`] binds, in the order of its `$1`, `$2`…:
/// what the registry hands the query.
pub const VARIABLES: &[&str] = &["session"];

/// `row[key]` when it is an array, `[]` when the row has none.
fn array(row: &Value, key: &str) -> Vec<Value> {
    row[key].as_array().cloned().unwrap_or_default()
}

/// `seconds` as `mm:ss`: minutes are not wrapped at the hour, a brief of an
/// hour and a quarter lasts `75:00`.
fn duration(seconds: i64) -> String {
    let seconds = seconds.max(0);
    format!("{:02}:{:02}", seconds / 60, seconds % 60)
}

/// The lines of a transcript: one per line of text, trimmed, blank ones left
/// out.
fn lines(transcript: &str) -> Vec<&str> {
    transcript
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect()
}

/// What a field of an element is called in a summary of the changes.
fn label(field: &str) -> &str {
    match field {
        "description" => "description",
        "notes" => "notes",
        "behavior" => "comportement",
        "background" => "historique",
        "aliases" => "alias",
        other => other,
    }
}

/// « Notes, comportement et alias mis à jour »: the fields a version changed,
/// by name, the first capitalised. `None` when it changed none.
fn change(fields: &[Value]) -> Option<String> {
    let labels: Vec<&str> = fields.iter().filter_map(Value::as_str).map(label).collect();
    let (last, rest) = labels.split_last()?;
    let mut said = if rest.is_empty() {
        (*last).to_owned()
    } else {
        format!("{} et {last}", rest.join(", "))
    };
    if let Some(first) = said.chars().next() {
        said = first.to_uppercase().chain(said.chars().skip(1)).collect();
    }
    Some(format!("{said} mis à jour"))
}

/// One element the brief recognised, as the DM reads it: `change` only when
/// the brief wrote a version of it.
fn recognized(row: &Value) -> Value {
    let mut element = Map::new();
    for key in ["id", "type", "name"] {
        element.insert(key.to_owned(), row[key].clone());
    }
    element.insert(
        "heard".to_owned(),
        row["heard"]
            .as_str()
            .map_or(Value::Null, |heard| json!(heard)),
    );
    let version = &row["version"];
    element.insert("updated".to_owned(), json!(version.is_object()));
    if let Some(change) = change(&array(version, "fields")) {
        element.insert("change".to_owned(), json!(change));
    }
    Value::Object(element)
}

/// The last brief of `session` among `rows` — an array of the objects [`SQL`]
/// selects, one at most — as the DM reads it. A row of another session is
/// left out; no row is a brief that is not there: the same shape, empty.
#[must_use]
pub fn answer_rows(rows: &Value, session: &str) -> Value {
    let brief = rows
        .as_array()
        .and_then(|rows| {
            rows.iter()
                .find(|row| row["session_id"].as_str() == Some(session))
        })
        .unwrap_or(&Value::Null);
    let scenes: Vec<Value> = array(brief, "scenes")
        .into_iter()
        .map(|scene| {
            json!({
                "order": scene["order"],
                "title": scene["title"],
                "links": if scene["links"].is_array() { scene["links"].clone() } else { json!([]) },
            })
        })
        .collect();
    let recognized: Vec<Value> = array(brief, "recognized").iter().map(recognized).collect();
    let missing: Vec<Value> = array(brief, "missing")
        .iter()
        .map(|entry| json!({ "id": entry["id"], "type": entry["type"], "name": entry["name"] }))
        .collect();
    json!({
        "session": session,
        "lastRun": brief["analyzed_at"].as_str().map_or(Value::Null, |at| json!(at)),
        "duration": brief["duration_seconds"].as_i64().map_or(Value::Null, |s| json!(duration(s))),
        "transcript": lines(brief["transcript"].as_str().unwrap_or_default()),
        "result": { "scenes": scenes, "recognized": recognized, "missing": missing },
    })
}

/// The answer to a read, as the registry serves it: the rows of [`SQL`] and
/// the request's variables. It reads `session`, as [`SQL`] binds it; a
/// variable absent is the empty text, which names nothing.
#[must_use]
pub fn answer(rows: &Value, variables: &BTreeMap<String, Value>) -> Value {
    answer_rows(
        rows,
        variables
            .get("session")
            .and_then(Value::as_str)
            .unwrap_or_default(),
    )
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    const S4: &str = "s4";

    fn fixture() -> Value {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../../mock/fixtures/session.getLastBrief.json");
        serde_json::from_str(&std::fs::read_to_string(path).expect("the fixture")).expect("JSON")
    }

    /// The row [`SQL`] would select for the brief of the mock-up's fixture:
    /// Morlox is the one element the brief wrote a version of.
    fn row_of_the_fixture() -> Value {
        let fixture = fixture();
        let recognized: Vec<Value> = fixture["result"]["recognized"]
            .as_array()
            .expect("an array")
            .iter()
            .map(|element| {
                let version = if element["updated"] == json!(true) {
                    json!({ "fields": ["notes", "behavior", "aliases"] })
                } else {
                    Value::Null
                };
                json!({
                    "id": element["id"], "type": element["type"], "name": element["name"],
                    "heard": element["heard"], "version": version,
                })
            })
            .collect();
        json!({
            "id": "b2",
            "session_id": S4,
            "transcript": fixture["transcript"].as_array().expect("lines").iter()
                .map(|line| line.as_str().expect("a line")).collect::<Vec<_>>().join("\n"),
            "duration_seconds": 252,
            "analyzed_at": fixture["lastRun"],
            "scenes": fixture["result"]["scenes"],
            "recognized": recognized,
            "missing": fixture["result"]["missing"],
        })
    }

    fn variables(session: &str) -> BTreeMap<String, Value> {
        BTreeMap::from([("session".to_owned(), json!(session))])
    }

    #[test]
    fn the_brief_of_the_fixture_is_answered_as_the_fixture() {
        let rows = json!([row_of_the_fixture()]);
        assert_eq!(answer_rows(&rows, S4), fixture());
    }

    #[test]
    fn the_answer_has_the_fixture_s_keys_only() {
        let answered = answer_rows(&json!([row_of_the_fixture()]), S4);
        let keys = |value: &Value| -> Vec<String> {
            value
                .as_object()
                .expect("an object")
                .keys()
                .cloned()
                .collect()
        };
        let fixture = fixture();
        assert_eq!(keys(&answered), keys(&fixture));
        assert_eq!(keys(&answered["result"]), keys(&fixture["result"]));
    }

    #[test]
    fn three_scenes_are_listed_by_order_with_their_links() {
        let answered = answer_rows(&json!([row_of_the_fixture()]), S4);
        let scenes = answered["result"]["scenes"].as_array().expect("scenes");
        let orders: Vec<&Value> = scenes.iter().map(|s| &s["order"]).collect();
        assert_eq!(orders, [1, 2, 3]);
        assert_eq!(scenes[1]["title"], "La route des Corbeaux");
        assert_eq!(scenes[1]["links"][2], "Frère Odric (à créer)");
    }

    #[test]
    fn an_updated_element_says_what_changed_and_the_others_do_not() {
        let answered = answer_rows(&json!([row_of_the_fixture()]), S4);
        let recognized = answered["result"]["recognized"].as_array().expect("array");
        let morlox = recognized
            .iter()
            .find(|e| e["id"] == "morlox")
            .expect("Morlox");
        assert_eq!(morlox["updated"], json!(true));
        assert_eq!(morlox["change"], "Notes, comportement et alias mis à jour");
        let gaspard = recognized
            .iter()
            .find(|e| e["id"] == "gaspard")
            .expect("Gaspard");
        assert_eq!(gaspard["updated"], json!(false));
        assert!(gaspard.get("change").is_none(), "{gaspard}");
    }

    #[test]
    fn an_element_the_transcript_does_not_name_was_heard_as_nothing() {
        let mut row = row_of_the_fixture();
        row["recognized"] =
            json!([{ "id": "a", "type": "pnj", "name": "A", "heard": null, "version": null }]);
        let answered = answer_rows(&json!([row.clone()]), S4);
        assert_eq!(answered["result"]["recognized"][0]["heard"], Value::Null);
        row["recognized"][0]["heard"] = json!(7);
        let answered = answer_rows(&json!([row]), S4);
        assert_eq!(answered["result"]["recognized"][0]["heard"], Value::Null);
    }

    #[test]
    fn a_version_with_no_field_is_an_update_with_no_summary() {
        let mut row = row_of_the_fixture();
        row["recognized"] = json!([{ "id": "a", "type": "pnj", "name": "A", "heard": "a", "version": { "fields": [] } }]);
        let answered = answer_rows(&json!([row]), S4);
        let element = &answered["result"]["recognized"][0];
        assert_eq!(element["updated"], json!(true));
        assert!(element.get("change").is_none(), "{element}");
    }

    #[test]
    fn the_missing_entries_are_listed_with_id_type_and_name_only() {
        let mut row = row_of_the_fixture();
        row["missing"] = json!([{ "id": "tc-1", "type": "pnj", "name": "Vesna", "excerpt": "x" }]);
        let answered = answer_rows(&json!([row]), S4);
        assert_eq!(
            answered["result"]["missing"],
            json!([{ "id": "tc-1", "type": "pnj", "name": "Vesna" }])
        );
    }

    #[test]
    fn a_duration_is_minutes_and_seconds() {
        assert_eq!(duration(252), "04:12");
        assert_eq!(duration(0), "00:00");
        assert_eq!(duration(59), "00:59");
        assert_eq!(duration(60), "01:00");
        assert_eq!(duration(4500), "75:00");
        assert_eq!(duration(3661), "61:01");
        assert_eq!(duration(-5), "00:00");
    }

    #[test]
    fn a_duration_that_is_not_a_whole_number_of_seconds_is_unknown() {
        let mut row = row_of_the_fixture();
        row["duration_seconds"] = json!("252");
        assert_eq!(answer_rows(&json!([row]), S4)["duration"], Value::Null);
    }

    #[test]
    fn the_transcript_is_one_entry_per_non_blank_line() {
        assert_eq!(
            lines("Un.\n\n  Deux.  \r\n\t\nTrois."),
            ["Un.", "Deux.", "Trois."]
        );
        assert!(lines("").is_empty());
        assert!(lines(" \n ").is_empty());
    }

    #[test]
    fn a_change_names_the_fields_in_the_order_of_the_diff() {
        let said = |fields: Value| change(fields.as_array().expect("array"));
        assert_eq!(said(json!([])), None);
        assert_eq!(said(json!(["notes"])).as_deref(), Some("Notes mis à jour"));
        assert_eq!(
            said(json!(["behavior", "background"])).as_deref(),
            Some("Comportement et historique mis à jour")
        );
        assert_eq!(
            said(json!(["description", "notes", "aliases"])).as_deref(),
            Some("Description, notes et alias mis à jour")
        );
        assert_eq!(
            said(json!(["armor_class"])).as_deref(),
            Some("Armor_class mis à jour")
        );
        assert_eq!(
            said(json!([7, "notes"])).as_deref(),
            Some("Notes mis à jour")
        );
    }

    #[test]
    fn a_brief_not_yet_analysed_has_no_last_run_and_nothing_found() {
        let row = json!({
            "id": "b3", "session_id": S4, "transcript": "Pas encore.", "duration_seconds": 5,
            "analyzed_at": null, "scenes": [], "recognized": [], "missing": [],
        });
        let answered = answer_rows(&json!([row]), S4);
        assert_eq!(answered["lastRun"], Value::Null);
        assert_eq!(answered["duration"], "00:05");
        assert_eq!(answered["transcript"], json!(["Pas encore."]));
        assert_eq!(
            answered["result"],
            json!({ "scenes": [], "recognized": [], "missing": [] })
        );
    }

    #[test]
    fn a_scene_with_no_links_lists_none() {
        let mut row = row_of_the_fixture();
        row["scenes"] =
            json!([{ "order": 1, "title": "Vide" }, { "order": 2, "title": "Faux", "links": "x" }]);
        let answered = answer_rows(&json!([row]), S4);
        assert_eq!(answered["result"]["scenes"][0]["links"], json!([]));
        assert_eq!(answered["result"]["scenes"][1]["links"], json!([]));
    }

    #[test]
    fn a_session_with_no_brief_answers_the_same_shape_empty() {
        let empty = json!({
            "session": S4, "lastRun": null, "duration": null, "transcript": [],
            "result": { "scenes": [], "recognized": [], "missing": [] },
        });
        assert_eq!(answer_rows(&json!([]), S4), empty);
        assert_eq!(answer_rows(&Value::Null, S4), empty);
    }

    #[test]
    fn a_brief_of_another_session_is_not_answered() {
        let mut other = row_of_the_fixture();
        other["session_id"] = json!("s1");
        other["transcript"] = json!("Autre.");
        let rows = json!([other, row_of_the_fixture()]);
        assert_eq!(answer_rows(&rows, S4), fixture());
        assert_eq!(answer_rows(&rows, "s1")["transcript"], json!(["Autre."]));
        assert_eq!(answer_rows(&rows, "s9")["transcript"], json!([]));
    }

    #[test]
    fn the_answer_of_a_read_is_the_one_of_the_session_the_variable_names() {
        let rows = json!([row_of_the_fixture()]);
        assert_eq!(answer(&rows, &variables(S4)), fixture());
        assert_eq!(answer(&rows, &variables("s9"))["session"], "s9");
        assert_eq!(answer(&rows, &BTreeMap::new())["session"], "");
        assert_eq!(answer(&rows, &BTreeMap::new())["transcript"], json!([]));
    }

    #[test]
    fn the_sql_is_one_select_statement_bound_to_one_session() {
        assert!(
            SQL.trim_start().to_ascii_uppercase().starts_with("SELECT"),
            "{SQL}"
        );
        assert!(!SQL.contains(';'), "a second statement in {SQL}");
        assert!(SQL.contains("WHERE session_id::text = $1"), "{SQL}");
        assert!(!SQL.contains("$2"), "{SQL}");
        assert_eq!(VARIABLES, ["session"]);
    }

    #[test]
    fn the_sql_takes_the_most_recently_recorded_brief_only() {
        assert!(
            SQL.contains("ORDER BY recorded_at DESC, id DESC LIMIT 1) b"),
            "{SQL}"
        );
    }

    #[test]
    fn the_sql_reads_what_the_analysis_left_once_the_brief_is_analysed() {
        assert_eq!(
            SQL.matches("WHEN b.analyzed_at IS NULL THEN '[]'::jsonb")
                .count(),
            3
        );
        assert!(
            SQL.contains("FROM scene sc WHERE sc.session_id = b.session_id"),
            "{SQL}"
        );
        assert!(
            SQL.contains("t.scene_id = sc.id AND t.status = 'a_creer'"),
            "{SQL}"
        );
        assert!(SQL.contains("t.name || ' (à créer)'"), "{SQL}");
        assert!(
            SQL.contains("WHERE t.brief_id = b.id AND t.status = 'a_creer'"),
            "{SQL}"
        );
        assert!(
            SQL.contains("ev.source = 'brief' AND ev.created_at >= b.recorded_at"),
            "{SQL}"
        );
        assert!(SQL.contains("alias_match_normalized"), "{SQL}");
    }

    #[test]
    fn the_sql_hears_whole_words_in_the_composed_transcript() {
        assert!(SQL.contains("normalize(b.transcript, NFC)"), "{SQL}");
        assert!(SQL.contains("'[^[:alnum:]]', ' ', 'g'"), "{SQL}");
        assert!(
            SQL.contains("strpos(n.words, ' ' || t.term || ' ')"),
            "a term is looked up between two spaces: {SQL}"
        );
        assert!(
            SQL.contains("char_length(n.words) = char_length(n.text) + 2"),
            "{SQL}"
        );
        assert!(SQL.contains("ORDER BY char_length(t.term) DESC"), "{SQL}");
        assert_eq!(
            SQL.matches("alias_match_normalized(").count(),
            2,
            "the transcript is normalised once, each term once: {SQL}"
        );
    }

    #[test]
    fn the_sql_merges_the_fields_of_every_version_the_brief_wrote() {
        assert!(
            SQL.contains("SELECT DISTINCT d.value ->> 'field' AS field"),
            "{SQL}"
        );
        assert!(SQL.contains("jsonb_agg(c.field ORDER BY c.field)"), "{SQL}");
        assert!(
            SQL.contains("CASE WHEN EXISTS (SELECT 1 FROM entity_version ev"),
            "{SQL}"
        );
    }

    #[test]
    fn the_sql_holds_no_audio_and_writes_nothing() {
        let sql = SQL.to_ascii_lowercase();
        assert!(!sql.contains("audio"), "{SQL}");
        for write in ["insert ", "update ", "delete ", "drop "] {
            assert!(!sql.contains(write), "{write} in {SQL}");
        }
    }

    #[test]
    fn the_query_file_selects_what_the_sql_reads() {
        let path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../queries/get-last-brief.graphql");
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
                "sessionId",
                "transcript",
                "durationSeconds",
                "analyzedAt",
                "scenes",
                "recognized",
                "missing"
            ]
        );
        for selected in [
            "b.id::text AS id",
            "b.session_id::text AS session_id",
            "b.transcript,",
            "b.duration_seconds,",
            "AS analyzed_at",
            "AS scenes",
            "AS recognized",
            "AS missing",
        ] {
            assert!(SQL.contains(selected), "the SQL does not select {selected}");
        }
    }

    #[test]
    fn the_manifest_names_the_capability_and_its_query() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("capability.json");
        let manifest: Value =
            serde_json::from_str(&std::fs::read_to_string(path).expect("the manifest"))
                .expect("JSON");
        assert_eq!(manifest["capability"], CAPABILITY);
        assert_eq!(manifest["system"], "session");
        assert_eq!(manifest["reads"], json!(["queries/get-last-brief.graphql"]));
        assert_eq!(manifest["input"], json!({ "session": "string" }));
    }
}
