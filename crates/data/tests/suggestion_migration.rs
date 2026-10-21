//! The Suggestion aggregate on its table, `live_suggestion`
//! (`crates/data/migrations/0011_suggestion.sql`), against the harness's
//! PostgreSQL (`DATABASE_URL`): the table stores the aggregate's fields and
//! nothing else, its targets are rows that exist, and each relation the
//! aggregate declares deletes and updates as it says. Each test works in a
//! scratch database of its own, migrated by the Data layer, dropped when it
//! ends.

use std::path::{Path, PathBuf};

use serde_json::Value;
use tokio_postgres::error::SqlState;
use tokio_postgres::{Client, NoTls};

const AGGREGATE: &str = include_str!("../../live/aggregates/Suggestion/aggregate.json");

const VALOMBRE: &str = "c1000000-0000-4000-8000-000000000001";
/// Valombre's NPCs.
const MORLOX: &str = "e1000000-0000-4000-8000-000000000002";
const GRIMBALD: &str = "e1000000-0000-4000-8000-000000000004";
const PLUIE: &str = "50000000-0000-4000-8000-000000000001";
/// Valombre's session 1, live with the microphone on, and session 2.
const SEANCE: &str = "51000000-0000-4000-8000-000000000001";
const SEANCE_2: &str = "51000000-0000-4000-8000-000000000002";

fn url() -> String {
    std::env::var("DATABASE_URL").expect("DATABASE_URL names the harness's PostgreSQL")
}

fn migrations() -> PathBuf {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/migrations")).to_path_buf()
}

async fn connect(config: &tokio_postgres::Config) -> Client {
    let (client, connection) = config.connect(NoTls).await.expect("PostgreSQL");
    tokio::spawn(connection);
    client
}

/// The SQL adding NPC `id` to Valombre, with the version every element is
/// written with (0004_entity.sql) and its sheet.
fn npc(id: &str, name: &str) -> String {
    format!(
        "INSERT INTO entity (id, campaign_id, type, name) VALUES ('{id}', '{VALOMBRE}', 'pnj', '{name}'); \
         INSERT INTO character (entity_id, token_color) VALUES ('{id}', '#888'); \
         INSERT INTO entity_version (entity_id, version_number, source, snapshot, diff) \
         VALUES ('{id}', 1, 'dm', '{{}}', '[]');"
    )
}

/// `aggregate.json`'s field, as its column is named.
fn column(field: &str) -> String {
    field.chars().fold(String::new(), |mut name, c| {
        if c.is_ascii_uppercase() {
            name.push('_');
        }
        name.push(c.to_ascii_lowercase());
        name
    })
}

/// A database of its own, migrated by the Data layer: Valombre, Morlox and
/// Grimbald, the rain, two sessions, and what session 1 heard — Morlox, then
/// the rain, then Grimbald; session 2 heard Morlox.
struct Scratch {
    client: Client,
    name: String,
}

impl Scratch {
    async fn new(name: &str) -> Scratch {
        let name = format!("suggestion_test_{name}_{}", std::process::id());
        let config: tokio_postgres::Config = url().parse().expect("a PostgreSQL URL");
        let admin = connect(&config).await;
        for sql in [
            format!("DROP DATABASE IF EXISTS {name} WITH (FORCE)"),
            format!("CREATE DATABASE {name}"),
        ] {
            admin.batch_execute(&sql).await.expect("a scratch database");
        }
        let mut client = connect(config.clone().dbname(&name)).await;
        data::migrate(&mut client, &migrations())
            .await
            .expect("migrated by the Data layer");
        let s = Scratch { client, name };
        s.accepted(&format!(
            "INSERT INTO campaign (id, name, kind) VALUES ('{VALOMBRE}', 'Les Cendres de Valombre', 'campagne'); \
             {}{} \
             INSERT INTO sound (id, campaign_id, name, file_path) VALUES ('{PLUIE}', '{VALOMBRE}', 'Pluie', 'pluie.ogg'); \
             INSERT INTO session (id, campaign_id, position, title, status, microphone_on) VALUES \
             ('{SEANCE}', '{VALOMBRE}', 1, 'Le Sanglier', 'live', true), \
             ('{SEANCE_2}', '{VALOMBRE}', 2, 'La Crypte', 'prep', false); \
             INSERT INTO live_suggestion (session_id, entity_id, sound_id, matched_text, detected_at) VALUES \
             ('{SEANCE}', '{MORLOX}', NULL, 'Morlox', '2026-10-18T20:00:00Z'), \
             ('{SEANCE}', NULL, '{PLUIE}', 'la pluie', '2026-10-18T20:05:00Z'), \
             ('{SEANCE}', '{GRIMBALD}', NULL, 'Grimbald', '2026-10-18T20:10:00Z'), \
             ('{SEANCE_2}', '{MORLOX}', NULL, 'le borgne', '2026-10-25T20:00:00Z')",
            npc(MORLOX, "Morlox"),
            npc(GRIMBALD, "Grimbald"),
        ))
        .await;
        s
    }

    async fn accepted(&self, sql: &str) {
        self.client
            .batch_execute(sql)
            .await
            .unwrap_or_else(|e| panic!("{sql}: {e:?}"));
    }

    /// Runs `sql`, which the database refuses with `code`.
    async fn refused(&self, sql: &str, code: &SqlState) {
        let error = self.client.batch_execute(sql).await.expect_err(sql);
        let db = error.as_db_error().expect("a refusal of the database");
        assert_eq!(db.code(), code, "{sql}: {}", db.message());
    }

    /// One text column of the rows `sql` answers.
    async fn texts(&self, sql: &str) -> Vec<String> {
        self.client
            .query(sql, &[])
            .await
            .unwrap_or_else(|e| panic!("{sql}: {e:?}"))
            .iter()
            .map(|row| row.get(0))
            .collect()
    }

    /// The words `session` heard, newest first: its history.
    async fn heard(&self, session: &str) -> Vec<String> {
        self.texts(&format!(
            "SELECT matched_text FROM live_suggestion WHERE session_id = '{session}' \
             ORDER BY detected_at DESC"
        ))
        .await
    }

    /// Drops the database. An autovacuum worker the writes woke may still be
    /// on it, which `FORCE` cannot end for a role that is not a superuser:
    /// the drop is tried again while the worker finishes.
    async fn drop(self) {
        drop(self.client);
        let config: tokio_postgres::Config = url().parse().expect("a PostgreSQL URL");
        let admin = connect(&config).await;
        let sql = format!("DROP DATABASE {} WITH (FORCE)", self.name);
        for _ in 0..20 {
            match admin.batch_execute(&sql).await {
                Ok(()) => return,
                Err(e) if e.code() == Some(&SqlState::INSUFFICIENT_PRIVILEGE) => {
                    tokio::time::sleep(std::time::Duration::from_millis(250)).await;
                }
                Err(e) => panic!("the scratch database dropped: {e:?}"),
            }
        }
        panic!("the scratch database {} is still in use", self.name);
    }
}

#[tokio::test]
async fn pile_and_history_derived_and_live_only_local_audio_the_table_stores_the_fields_and_no_recording()
 {
    let aggregate: Value = serde_json::from_str(AGGREGATE).expect("contract I");
    let mut fields: Vec<String> = aggregate["fields"]
        .as_object()
        .expect("fields")
        .keys()
        .map(|field| column(field))
        .collect();
    fields.sort_unstable();
    let s = Scratch::new("fields").await;
    let columns: Vec<(String, String)> = s
        .client
        .query(
            "SELECT a.attname::text, format_type(a.atttypid, a.atttypmod) \
             FROM pg_attribute a WHERE a.attrelid = 'live_suggestion'::regclass \
             AND a.attnum > 0 AND NOT a.attisdropped ORDER BY 1",
            &[],
        )
        .await
        .expect("the columns read")
        .iter()
        .map(|row| (row.get(0), row.get(1)))
        .collect();
    s.drop().await;
    // Nothing is stored beyond the suggestion: no pile, no rank.
    let names: Vec<&str> = columns.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(names, fields);
    // What the microphone left is the words heard, never audio.
    assert_eq!(
        columns,
        [
            ("detected_at", "timestamp with time zone"),
            ("entity_id", "uuid"),
            ("id", "uuid"),
            ("matched_text", "text"),
            ("session_id", "uuid"),
            ("sound_id", "uuid"),
        ]
        .map(|(name, ty)| (name.to_string(), ty.to_string()))
    );
}

#[tokio::test]
async fn pile_and_history_derived_both_are_read_newest_first_by_session_from_the_rows() {
    let s = Scratch::new("pile").await;
    for (minute, words) in [(15, "Morlox"), (20, "la crypte"), (25, "Grimbald")] {
        s.accepted(&format!(
            "INSERT INTO live_suggestion (session_id, entity_id, matched_text, detected_at) \
             VALUES ('{SEANCE}', '{MORLOX}', '{words}', '2026-10-18T20:{minute}:00Z')"
        ))
        .await;
    }
    let history = s.heard(SEANCE).await;
    let pile = s
        .texts(&format!(
            "SELECT matched_text FROM live_suggestion WHERE session_id = '{SEANCE}' \
             ORDER BY detected_at DESC LIMIT 5"
        ))
        .await;
    // The index the history and the pile are read through: by session,
    // newest first.
    let index = s
        .texts("SELECT pg_get_indexdef('live_suggestion_session_id'::regclass)")
        .await;
    s.drop().await;
    assert_eq!(
        history,
        [
            "Grimbald",
            "la crypte",
            "Morlox",
            "Grimbald",
            "la pluie",
            "Morlox"
        ]
    );
    assert_eq!(pile, history[..5]);
    assert!(
        index[0].ends_with("(session_id, detected_at DESC)"),
        "{index:?}"
    );
}

#[tokio::test]
async fn suggestion_target_is_a_session_and_an_element_or_a_sound_that_exist() {
    let s = Scratch::new("target").await;
    let nowhere = "00000000-0000-4000-8000-0000000000ff";
    for (session, entity, sound) in [
        (nowhere, MORLOX, "NULL"),
        (SEANCE, nowhere, "NULL"),
        (SEANCE, "NULL", nowhere),
    ] {
        let quoted = |id: &str| {
            if id == "NULL" {
                id.to_string()
            } else {
                format!("'{id}'")
            }
        };
        s.refused(
            &format!(
                "INSERT INTO live_suggestion (session_id, entity_id, sound_id, matched_text, detected_at) \
                 VALUES ('{session}', {}, {}, 'Morlox', now())",
                quoted(entity),
                quoted(sound),
            ),
            &SqlState::FOREIGN_KEY_VIOLATION,
        )
        .await;
    }
    for column in ["session_id", "matched_text", "detected_at"] {
        s.refused(
            &format!("UPDATE live_suggestion SET {column} = NULL"),
            &SqlState::NOT_NULL_VIOLATION,
        )
        .await;
    }
    s.drop().await;
}

#[tokio::test]
async fn each_relation_deletes_and_updates_as_the_aggregate_declares() {
    let aggregate: Value = serde_json::from_str(AGGREGATE).expect("contract I");
    // The relation's field, its target, and its actions as `pg_constraint`
    // codes them.
    let declared: Vec<(String, String, String, String)> = aggregate["relations"]
        .as_array()
        .expect("relations")
        .iter()
        .map(|relation| {
            let field = relation["from"]
                .as_str()
                .and_then(|from| from.strip_prefix("Suggestion."))
                .expect("a field of the aggregate");
            let code = |action: &Value| match action.as_str() {
                Some("cascade" | "propagate") => "c",
                Some("nullify") => "n",
                Some("restrict") => "r",
                other => panic!("an action the table does not map: {other:?}"),
            };
            let column = column(field);
            let target = format!("{}.id", column.trim_end_matches("_id"));
            (
                column,
                target,
                code(&relation["onDelete"]).to_string(),
                code(&relation["onUpdate"]).to_string(),
            )
        })
        .collect();
    let s = Scratch::new("relations").await;
    let mut keys: Vec<(String, String, String, String)> = s
        .client
        .query(
            "SELECT a.attname::text, c.confrelid::regclass::text || '.' || r.attname, \
             c.confdeltype::text, c.confupdtype::text \
             FROM pg_constraint c \
             JOIN pg_attribute a ON a.attrelid = c.conrelid AND a.attnum = c.conkey[1] \
             JOIN pg_attribute r ON r.attrelid = c.confrelid AND r.attnum = c.confkey[1] \
             WHERE c.conrelid = 'live_suggestion'::regclass AND c.contype = 'f'",
            &[],
        )
        .await
        .expect("the constraints read")
        .iter()
        .map(|row| (row.get(0), row.get(1), row.get(2), row.get(3)))
        .collect();
    keys.sort_unstable();
    let mut expected = declared;
    expected.sort_unstable();
    assert_eq!(keys, expected);

    // onUpdate propagate: a new id reaches the suggestions. An element's id
    // is held by its versions (0004_entity.sql), which refuse it a new one:
    // its key's action is the catalogue's alone, read above.
    let rain = "5000000f-0000-4000-8000-000000000001";
    let seance = "5100000f-0000-4000-8000-000000000001";
    s.accepted(&format!(
        "UPDATE sound SET id = '{rain}' WHERE id = '{PLUIE}'; \
         UPDATE session SET id = '{seance}' WHERE id = '{SEANCE}'"
    ))
    .await;
    assert_eq!(
        s.texts(
            "SELECT coalesce(entity_id, sound_id)::text || ' ' || session_id::text \
             FROM live_suggestion ORDER BY detected_at"
        )
        .await,
        [
            format!("{MORLOX} {seance}"),
            format!("{rain} {seance}"),
            format!("{GRIMBALD} {seance}"),
            format!("{MORLOX} {SEANCE_2}"),
        ]
    );

    // onDelete cascade: the sound's suggestion goes, then the element's in
    // every session, then the session's — and only theirs.
    s.accepted("DELETE FROM sound").await;
    assert_eq!(
        s.texts("SELECT matched_text FROM live_suggestion ORDER BY detected_at")
            .await,
        ["Morlox", "Grimbald", "le borgne"]
    );
    s.accepted(&format!("DELETE FROM entity WHERE id = '{MORLOX}'"))
        .await;
    assert_eq!(
        s.texts("SELECT matched_text FROM live_suggestion ORDER BY detected_at")
            .await,
        ["Grimbald"]
    );
    s.accepted("DELETE FROM session").await;
    assert!(
        s.texts("SELECT matched_text FROM live_suggestion")
            .await
            .is_empty()
    );
    s.drop().await;
}
