//! `0014_battlemap_instance_uniques.sql` in `crates/data/migrations/`
//! against the harness's PostgreSQL (`DATABASE_URL`): laid over a database
//! migrated up to 0013, then replayed, it changes nothing; over rows that
//! already break one of its indexes it refuses to apply, names the invariant,
//! and deletes nothing. Each test works in a `ScratchDb` of its own.

use std::path::{Path, PathBuf};

use platform_scratch_db::ScratchDb;
use tokio_postgres::Client;
use tokio_postgres::error::SqlState;

const MIGRATION: &str = "0014_battlemap_instance_uniques.sql";

const VALOMBRE: &str = "c1000000-0000-4000-8000-000000000001";
const SEANCE: &str = "5e000000-0000-4000-8000-000000000001";
const SCENE: &str = "5c000000-0000-4000-8000-000000000001";
const GUE: &str = "d1000000-0000-4000-8000-00000000000a";
const BANDIT: &str = "e1000000-0000-4000-8000-000000000003";
const INSTANCE: &str = "b1000000-0000-4000-8000-000000000001";

fn migrations() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("migrations")
}

/// A database migrated up to 0013 — every file before 0014, in their order —
/// holding Valombre, its scene, battlemap and an NPC, and one instance.
async fn at_0013(label: &str) -> (ScratchDb, Client) {
    let db = ScratchDb::create(label)
        .await
        .expect("DATABASE_URL names the harness's PostgreSQL");
    let client = db.connect().await.expect("the scratch database answers");
    let mut files: Vec<_> = std::fs::read_dir(migrations())
        .expect("the migrations")
        .map(|file| file.expect("a file").path())
        .filter(|path| path.file_name().is_some_and(|name| name < MIGRATION))
        .collect();
    files.sort();
    assert!(files.len() >= 13, "{files:?}");
    for file in files {
        client
            .batch_execute(&std::fs::read_to_string(&file).expect("a migration"))
            .await
            .unwrap_or_else(|e| panic!("{} applies: {e}", file.display()));
    }
    client
        .batch_execute(&format!(
            "INSERT INTO campaign (id, name, kind) VALUES
                 ('{VALOMBRE}', 'Les Cendres de Valombre', 'campagne');
             INSERT INTO session (id, campaign_id, position, title, status, microphone_on) VALUES
                 ('{SEANCE}', '{VALOMBRE}', 1, 'L''embuscade du Gué', 'live', false);
             INSERT INTO scene (id, session_id, position, title) VALUES
                 ('{SCENE}', '{SEANCE}', 1, 'Le gué');
             INSERT INTO media (id, campaign_id, kind, title, file_path, width_px, height_px) VALUES
                 ('{GUE}', '{VALOMBRE}', 'battlemap', 'Le Gué des Corbeaux', 'media/gue.png', 960, 640);
             INSERT INTO battlemap_grid (media_id, hex_size_px, offset_x, offset_y, orientation, color, opacity) VALUES
                 ('{GUE}', 30, 12, 8, 'pointe', 'text', 0.4);
             INSERT INTO entity (id, campaign_id, type, name) VALUES
                 ('{BANDIT}', '{VALOMBRE}', 'pnj', 'Bandit');
             INSERT INTO character (entity_id, hp_max, token_color) VALUES ('{BANDIT}', 11, 'danger');
             INSERT INTO entity_version (entity_id, version_number, source, snapshot, diff)
                 SELECT id, 1, 'dm', '{{}}', '[]' FROM entity;
             INSERT INTO battlemap_instance (id, battlemap_media_id, session_id, scene_id)
                 VALUES ('{INSTANCE}', '{GUE}', '{SEANCE}', '{SCENE}');"
        ))
        .await
        .expect("Valombre");
    (db, client)
}

fn migration() -> String {
    std::fs::read_to_string(migrations().join(MIGRATION)).expect("migration 0014")
}

/// The indexes of the three tables, with their definitions.
async fn indexes(client: &Client) -> Vec<String> {
    client
        .query(
            "SELECT indexdef FROM pg_indexes
             WHERE tablename IN ('battlemap_instance', 'token', 'combat') ORDER BY indexname",
            &[],
        )
        .await
        .expect("the indexes")
        .iter()
        .map(|row| row.get(0))
        .collect()
}

/// How many rows the three tables hold.
async fn held(client: &Client) -> i64 {
    client
        .query_one(
            "SELECT (SELECT count(*) FROM battlemap_instance)
                  + (SELECT count(*) FROM combat) + (SELECT count(*) FROM token)",
            &[],
        )
        .await
        .expect("a count")
        .get(0)
}

/// The text of a file of the repository, from `crates/data`.
fn repository(path: &str) -> String {
    std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(path),
    )
    .unwrap_or_else(|e| panic!("{path}: {e}"))
}

// AC2: 0014 follows 0010's conventions: a header saying why, replayable
// statements, a comment naming the invariant above each index.
#[test]
fn it_follows_the_conventions_of_0010() {
    let ten = repository("crates/data/migrations/0010_battlemap_instance.sql");
    let text = migration();
    for file in [&ten, &text] {
        assert!(file.starts_with("-- "), "a header comment first");
    }
    let header: Vec<&str> = text.lines().take_while(|l| l.starts_with("--")).collect();
    assert!(
        header.iter().any(|l| l.contains("Replays as a no-op")),
        "the header says it replays as a no-op, as 0010's does"
    );
    for (index, invariant) in [
        (
            "battlemap_instance_scene_id_unique",
            "one-instance-per-scene",
        ),
        ("combat_one_active", "one-active-combat"),
        ("token_instance_character_number", "multi-drop-numbering"),
    ] {
        let statement = format!("CREATE UNIQUE INDEX IF NOT EXISTS {index}\n");
        let at = text
            .find(&statement)
            .unwrap_or_else(|| panic!("{index} IF NOT EXISTS"));
        let comment: Vec<&str> = text[..at]
            .lines()
            .rev()
            .skip_while(|l| l.is_empty())
            .take_while(|l| l.starts_with("--"))
            .collect();
        assert!(
            comment.iter().any(|l| l.contains(invariant)) || comment.join(" ").contains(invariant),
            "{index}: the comment above it names {invariant}: {comment:?}"
        );
    }
    assert_eq!(text.matches("CREATE UNIQUE INDEX").count(), 3);
    assert!(!text.contains("DELETE"), "it never deletes a row");
}

// AC3: data/model.json lists each index on its table, naming the invariant
// it enforces, and names the migration.
#[test]
fn the_data_model_lists_each_index_and_its_invariant() {
    let model: serde_json::Value =
        serde_json::from_str(&repository("data/model.json")).expect("a data model is JSON");
    for (table, field, index, invariants) in [
        (
            "battlemap_instance",
            "scene_id",
            "battlemap_instance_scene_id_unique",
            vec!["one-instance-per-scene"],
        ),
        (
            "combat",
            "ended_at",
            "combat_one_active",
            vec!["one-active-combat"],
        ),
        (
            "token",
            "number",
            "token_instance_character_number",
            vec!["multi-drop-numbering", "token-roster"],
        ),
    ] {
        let table = model["tables"]
            .as_array()
            .expect("tables")
            .iter()
            .find(|t| t["name"] == table)
            .unwrap_or_else(|| panic!("{table} is in the model"));
        let field = table["fields"]
            .as_array()
            .expect("fields")
            .iter()
            .find(|f| f["name"] == field)
            .unwrap_or_else(|| panic!("{field} is in the model"));
        let technical = field["technical"].to_string();
        assert!(technical.contains(index), "{index}: {technical}");
        assert!(technical.contains(MIGRATION), "{index}: {technical}");
        for invariant in invariants {
            assert!(technical.contains(invariant), "{index}: {invariant}");
        }
    }
}

// AC1: replayed, 0014 succeeds and changes nothing.
#[tokio::test]
async fn replayed_it_changes_nothing() {
    let (_db, client) = at_0013("uniques_replay").await;
    let before = indexes(&client).await;
    client.batch_execute(&migration()).await.expect("applied");
    let once = indexes(&client).await;
    assert_eq!(once.len(), before.len() + 3, "its three indexes: {once:#?}");
    for (index, columns) in [
        ("battlemap_instance_scene_id_unique", "(scene_id)"),
        (
            "combat_one_active",
            "(instance_id) WHERE (ended_at IS NULL)",
        ),
        (
            "token_instance_character_number",
            "(instance_id, character_id, number)",
        ),
    ] {
        assert!(
            once.iter().any(|def| def.starts_with("CREATE UNIQUE INDEX")
                && def.contains(index)
                && def.ends_with(columns)),
            "{index}: {once:#?}"
        );
    }
    client.batch_execute(&migration()).await.expect("replayed");
    assert_eq!(indexes(&client).await, once);
}

// AC1: the Data layer applies it over the files before it, and again.
#[tokio::test]
async fn the_data_layer_applies_it_after_0013_and_again() {
    let db = ScratchDb::create("uniques_data_layer")
        .await
        .expect("DATABASE_URL names the harness's PostgreSQL");
    let mut client = db.connect().await.expect("the scratch database answers");
    let applied = data::migrate(&mut client, &migrations())
        .await
        .expect("migrated");
    assert!(applied.iter().any(|file| file == MIGRATION), "{applied:?}");
    let again = data::migrate(&mut client, &migrations())
        .await
        .expect("migrated again");
    assert!(again.is_empty(), "{again:?}");
}

// The rows it cannot index are never deleted: it refuses, naming the invariant.
#[tokio::test]
async fn over_rows_that_break_an_index_it_refuses_and_deletes_nothing() {
    for (label, invariant, constraint, rows) in [
        (
            "uniques_scene",
            "one-instance-per-scene",
            "battlemap_instance_scene_id_unique",
            format!(
                "INSERT INTO battlemap_instance (id, battlemap_media_id, session_id, scene_id)
                 VALUES ('b1000000-0000-4000-8000-000000000002', '{GUE}', '{SEANCE}', '{SCENE}')"
            ),
        ),
        (
            "uniques_combat",
            "one-active-combat",
            "combat_one_active",
            format!(
                "INSERT INTO combat (instance_id, round, started_at)
                 VALUES ('{INSTANCE}', 1, now()), ('{INSTANCE}', 1, now())"
            ),
        ),
        (
            "uniques_token",
            "multi-drop-numbering",
            "token_instance_character_number",
            format!(
                "INSERT INTO token (instance_id, character_id, number, name, q, r, visible, conditions)
                 VALUES ('{INSTANCE}', '{BANDIT}', 1, 'Bandit 1', 0, 0, true, '{{}}'),
                        ('{INSTANCE}', '{BANDIT}', 1, 'Bandit 1', 1, 0, true, '{{}}')"
            ),
        ),
    ] {
        let (_db, client) = at_0013(label).await;
        client.batch_execute(&rows).await.expect("duplicates");
        let before = held(&client).await;
        let refused = client
            .batch_execute(&migration())
            .await
            .expect_err("0014 refuses duplicates");
        let db = refused.as_db_error().expect("a database error");
        assert_eq!(db.code(), &SqlState::UNIQUE_VIOLATION, "{label}: {db}");
        assert_eq!(db.constraint(), Some(constraint), "{label}");
        assert!(db.message().starts_with(invariant), "{label}: {db}");
        assert_eq!(held(&client).await, before, "{label}: nothing deleted");
        assert!(
            !indexes(&client)
                .await
                .iter()
                .any(|def| def.contains(constraint)),
            "{label}: no index is left half-made"
        );
    }
}
