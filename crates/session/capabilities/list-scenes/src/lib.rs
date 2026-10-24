//! Capability `session.listScenes`: a session's scenes by rank, for session
//! preparation and the Médias page.
//!
//! It reads, and only reads. The rows of its persisted query
//! (`crates/session/queries/list-scenes.graphql`, resolved by [`SQL`]) go in;
//! the answer, in the shape of `mock/fixtures/session.listScenes.json` — an
//! array of `{ id, order, title, places, characters, events, media, toCreate,
//! battlemap, notes }` — comes out, by rank (`scene-order-contiguous`).
//!
//! A scene's `places`, `characters` (PCs and NPCs) and `events` are the ids of
//! the elements linked to it, split by `entity.type`. Its `media` are the ones
//! linked to the scene directly and the ones whose place is one of its places
//! (`session-media-by-place`): derived here, at read time, from
//! `media.place_id`, never stored, so a medium whose place changes shows in
//! the scenes of that place at the next read, and a medium with no place shows
//! only where it is linked directly. `session_media_view` answers the same
//! question for a whole session; a scene needs it split by scene, which the
//! view does not keep, so the rule is applied per scene. `toCreate` lists the
//! entries of the 'to create' list not yet created. A scene's `battlemap` (the
//! instance on the map) is `null` until the battlemap milestone produces it.

use std::collections::BTreeMap;

use serde_json::{Value, json};

/// The Capability's identifier, as a Micro-UI cites it.
pub const CAPABILITY: &str = "session.listScenes";

/// What the persisted query `list-scenes.graphql` selects: the scenes of the
/// session bound to `$1` (compared as text, so that an id which is not a uuid
/// lists nothing instead of failing), by rank, each with the ids of its
/// elements by kind in their display order, its media (the direct links and
/// the media of its places, each once, by upload) and its entries still to create, as JSON
/// arrays (empty when it has none). `session_id` is selected too, so that
/// [`answer`] keeps only the session's own scenes whoever ran the statement.
pub const SQL: &str = r"SELECT sc.id::text AS id,
    sc.session_id::text AS session_id,
    sc.position,
    sc.title,
    coalesce(
        (SELECT jsonb_agg(se.entity_id::text ORDER BY se.position, se.entity_id) FROM scene_entity se JOIN entity e ON e.id = se.entity_id WHERE se.scene_id = sc.id AND e.type = 'lieu'),
        '[]'::jsonb
    ) AS places,
    coalesce(
        (SELECT jsonb_agg(se.entity_id::text ORDER BY se.position, se.entity_id) FROM scene_entity se JOIN entity e ON e.id = se.entity_id WHERE se.scene_id = sc.id AND e.type IN ('pj', 'pnj')),
        '[]'::jsonb
    ) AS characters,
    coalesce(
        (SELECT jsonb_agg(se.entity_id::text ORDER BY se.position, se.entity_id) FROM scene_entity se JOIN entity e ON e.id = se.entity_id WHERE se.scene_id = sc.id AND e.type = 'evenement'),
        '[]'::jsonb
    ) AS events,
    coalesce(
        (SELECT jsonb_agg(m.id::text ORDER BY m.uploaded_at, m.id) FROM media m
         WHERE m.id IN (SELECT sm.media_id FROM scene_media sm WHERE sm.scene_id = sc.id)
            OR m.place_id IN (SELECT se.entity_id FROM scene_entity se JOIN entity pe ON pe.id = se.entity_id WHERE se.scene_id = sc.id AND pe.type = 'lieu')),
        '[]'::jsonb
    ) AS media,
    coalesce(
        (SELECT jsonb_agg(t.id::text ORDER BY t.created_at, t.id) FROM entity_to_create t WHERE t.scene_id = sc.id AND t.status = 'a_creer'),
        '[]'::jsonb
    ) AS to_create,
    sc.notes
FROM scene sc
WHERE sc.session_id::text = $1
ORDER BY sc.position, sc.id";

/// `row[key]` when it is an array, `[]` when the row has none.
fn ids(row: &Value, key: &str) -> Value {
    if row[key].is_array() {
        row[key].clone()
    } else {
        json!([])
    }
}

/// The scenes of `session` among `rows` — an array of the objects [`SQL`]
/// selects — as preparation reads them, in the order of the rows. A scene of
/// another session is left out; a scene with no element, no medium or nothing
/// to create lists `[]`, one with no notes `null`; no row is an empty,
/// well-formed answer.
#[must_use]
pub fn answer_rows(rows: &Value, session: &str) -> Value {
    let scenes: Vec<Value> = rows
        .as_array()
        .map(|rows| {
            rows.iter()
                .filter(|row| row["session_id"].as_str() == Some(session))
                .map(|row| {
                    json!({
                        "id": row["id"],
                        "order": row["position"],
                        "title": row["title"],
                        "places": ids(row, "places"),
                        "characters": ids(row, "characters"),
                        "events": ids(row, "events"),
                        "media": ids(row, "media"),
                        "toCreate": ids(row, "to_create"),
                        "battlemap": Value::Null,
                        "notes": row["notes"],
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    Value::Array(scenes)
}

/// The request's variables [`SQL`] binds, in the order of its `$1`, `$2`…:
/// what the registry hands the query.
pub const VARIABLES: &[&str] = &["session"];

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
            .join("../../../../mock/fixtures/session.listScenes.json");
        serde_json::from_str(&std::fs::read_to_string(path).expect("the fixture")).expect("JSON")
    }

    /// The row [`SQL`] would select for a scene of the mock-up's fixture.
    fn row_of(scene: &Value, session: &str) -> Value {
        json!({
            "id": scene["id"],
            "session_id": session,
            "position": scene["order"],
            "title": scene["title"],
            "places": scene["places"],
            "characters": scene["characters"],
            "events": scene["events"],
            "media": scene["media"],
            "to_create": scene["toCreate"],
            "notes": scene["notes"],
        })
    }

    fn rows_of_the_session() -> Value {
        Value::Array(
            fixture()
                .as_array()
                .expect("an array")
                .iter()
                .map(|scene| row_of(scene, S4))
                .collect(),
        )
    }

    fn ids_of(scenes: &Value) -> Vec<&str> {
        scenes
            .as_array()
            .expect("an array")
            .iter()
            .map(|s| s["id"].as_str().expect("an id"))
            .collect()
    }

    #[test]
    fn the_scenes_of_the_fixture_are_listed_by_rank() {
        let listed = answer_rows(&rows_of_the_session(), S4);
        assert_eq!(ids_of(&listed), ["sc1", "sc2", "sc3"]);
        let orders: Vec<&Value> = listed
            .as_array()
            .expect("an array")
            .iter()
            .map(|s| &s["order"])
            .collect();
        assert_eq!(orders, [1, 2, 3]);
    }

    #[test]
    fn each_scene_matches_the_fixture_but_for_its_battlemap() {
        let listed = answer_rows(&rows_of_the_session(), S4);
        for (got, expected) in listed
            .as_array()
            .expect("an array")
            .iter()
            .zip(fixture().as_array().expect("an array"))
        {
            for field in [
                "id",
                "order",
                "title",
                "places",
                "characters",
                "events",
                "media",
                "toCreate",
                "notes",
            ] {
                assert_eq!(got[field], expected[field], "{field} of {}", got["id"]);
            }
        }
    }

    #[test]
    fn the_answer_holds_the_fixture_s_keys_only() {
        let listed = answer_rows(&rows_of_the_session(), S4);
        let mut keys: Vec<&str> = listed[0]
            .as_object()
            .expect("an object")
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        let fixture = fixture();
        let mut expected: Vec<&str> = fixture[0]
            .as_object()
            .expect("an object")
            .keys()
            .map(String::as_str)
            .collect();
        expected.sort_unstable();
        assert_eq!(keys, expected);
    }

    #[test]
    fn a_battlemap_instance_is_null_until_its_milestone() {
        let mut rows = rows_of_the_session();
        rows[2]["battlemap"] = json!("bmi-gue-s4");
        let listed = answer_rows(&rows, S4);
        for scene in listed.as_array().expect("an array") {
            assert_eq!(scene["battlemap"], Value::Null, "{scene}");
        }
    }

    #[test]
    fn a_scene_of_another_session_does_not_appear() {
        let mut rows = rows_of_the_session();
        rows.as_array_mut().expect("an array").push(json!({
            "id": "intrus", "session_id": "s1", "position": 1, "title": "Intrus",
        }));
        let listed = answer_rows(&rows, S4);
        assert_eq!(ids_of(&listed).len(), 3);
        assert!(!ids_of(&listed).contains(&"intrus"), "{listed}");
        assert_eq!(ids_of(&answer_rows(&rows, "s1")), ["intrus"]);
    }

    #[test]
    fn a_scene_with_nothing_lists_empty_arrays_and_null_notes() {
        let rows = json!([{ "id": "a", "session_id": S4, "position": 1, "title": "A" }]);
        let listed = answer_rows(&rows, S4);
        for key in ["places", "characters", "events", "media", "toCreate"] {
            assert_eq!(listed[0][key], json!([]), "{key}");
        }
        assert_eq!(listed[0]["notes"], Value::Null);
    }

    #[test]
    fn no_row_is_an_empty_well_formed_answer() {
        assert_eq!(answer_rows(&json!([]), S4), json!([]));
        assert_eq!(answer_rows(&Value::Null, S4), json!([]));
    }

    #[test]
    fn the_sql_is_one_select_statement_bound_to_one_session_by_rank() {
        assert!(
            SQL.trim_start().to_ascii_uppercase().starts_with("SELECT"),
            "{SQL}"
        );
        assert!(!SQL.contains(';'), "a second statement in {SQL}");
        assert!(SQL.contains("sc.session_id::text = $1"), "{SQL}");
        assert!(SQL.contains("ORDER BY sc.position"), "{SQL}");
    }

    #[test]
    fn the_sql_splits_the_elements_of_a_scene_by_type() {
        assert!(SQL.contains("e.type = 'lieu'"), "{SQL}");
        assert!(SQL.contains("e.type IN ('pj', 'pnj')"), "{SQL}");
        assert!(SQL.contains("e.type = 'evenement'"), "{SQL}");
    }

    #[test]
    fn the_media_of_a_scene_are_its_direct_links_and_the_media_of_its_places() {
        assert!(
            SQL.contains("FROM scene_media sm WHERE sm.scene_id = sc.id"),
            "{SQL}"
        );
        assert!(
            SQL.contains(
                "m.place_id IN (SELECT se.entity_id FROM scene_entity se JOIN entity pe ON pe.id = se.entity_id WHERE se.scene_id = sc.id AND pe.type = 'lieu')"
            ),
            "{SQL}"
        );
        assert!(
            SQL.contains("OR m.place_id"),
            "an OR, so that either link attaches: {SQL}"
        );
    }

    #[test]
    fn only_the_entries_not_yet_created_are_to_create() {
        assert!(
            SQL.contains("t.scene_id = sc.id AND t.status = 'a_creer'"),
            "{SQL}"
        );
    }

    #[test]
    fn the_query_file_selects_what_the_sql_reads() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../queries/list-scenes.graphql");
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
                "position",
                "title",
                "notes",
                "places",
                "characters",
                "events",
                "media",
                "toCreate"
            ]
        );
        for (field, selected) in [
            ("id", "sc.id::text AS id"),
            ("sessionId", "sc.session_id::text AS session_id"),
            ("position", "sc.position"),
            ("title", "sc.title"),
            ("notes", "sc.notes"),
            ("places", "AS places"),
            ("characters", "AS characters"),
            ("events", "AS events"),
            ("media", "AS media"),
            ("toCreate", "AS to_create"),
        ] {
            assert!(
                SQL.contains(selected),
                "the query names {field}, the SQL does not select it"
            );
        }
    }

    #[test]
    fn the_registry_binds_the_session_variable() {
        assert_eq!(VARIABLES, ["session"]);
        assert!(SQL.contains("$1"), "{SQL}");
        assert!(!SQL.contains("$2"), "{SQL}");
    }

    #[test]
    fn the_answer_of_a_read_lists_the_session_the_variable_names() {
        let mut rows = rows_of_the_session();
        rows.as_array_mut().expect("an array").push(json!({
            "id": "intrus", "session_id": "s1", "position": 1, "title": "Intrus",
        }));
        let variables = |session: &str| BTreeMap::from([("session".to_owned(), json!(session))]);
        assert_eq!(answer(&rows, &variables(S4)), answer_rows(&rows, S4));
        assert_eq!(ids_of(&answer(&rows, &variables("s1"))), ["intrus"]);
    }

    #[test]
    fn a_read_without_its_session_variable_lists_nothing() {
        let rows = rows_of_the_session();
        assert_eq!(answer(&rows, &BTreeMap::new()), json!([]));
        let not_text = BTreeMap::from([("session".to_owned(), json!(7))]);
        assert_eq!(answer(&rows, &not_text), json!([]));
    }
}
