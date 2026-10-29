//! The rest of the Data layer's schema — `0006_session.sql` and
//! `0009_sound_playback.sql` to `0012_diffusion.sql` in
//! `crates/data/migrations/` — against the harness's PostgreSQL
//! (`DATABASE_URL`): laid over a database the earlier migrations built, then
//! replayed, they change nothing; their tables and views are what
//! `data/model.json` says, as the dossier designed them; each table they
//! reference exists before them; their relations cascade, nullify and hold
//! as the aggregates declare; their generated columns and their views
//! compute what the design says. The aggregates' invariants are not theirs.
//! Each test works in a scratch database of its own, dropped when it ends.

use std::path::{Path, PathBuf};

use serde_json::Value;
use tokio_postgres::error::SqlState;
use tokio_postgres::{Client, NoTls};

/// The migrations this schema adds, in their order.
const MIGRATIONS: [&str; 5] = [
    "0006_session.sql",
    "0009_sound_playback.sql",
    "0010_battlemap_instance.sql",
    "0011_suggestion.sql",
    "0012_diffusion.sql",
];

/// The migrations that build on them, which the Data layer applies after
/// them: they are not "earlier" than them.
const LATER: [&str; 2] = [
    "0014_battlemap_instance_uniques.sql",
    "0015_battlemap_token_character_type.sql",
];

/// The columns a later migration adds to those tables, which the dossier's
/// design (`mock/data/model.json`, never edited) does not have: the token's
/// PC/NPC marker (0015).
const ADDED_AFTER_THE_DESIGN: [(&str, &str); 1] = [("token", "character_type")];

/// The tables they create, in their order.
const TABLES: [&str; 15] = [
    "session",
    "session_present_character",
    "scene",
    "scene_entity",
    "scene_media",
    "oral_brief",
    "entity_to_create",
    "session_journal_line",
    "sound_playback",
    "battlemap_instance",
    "token",
    "combat",
    "initiative_entry",
    "live_suggestion",
    "diffusion",
];

/// The views they create.
const VIEWS: [&str; 3] = [
    "session_roster_view",
    "session_media_view",
    "initiative_order_view",
];

const VALOMBRE: &str = "c1000000-0000-4000-8000-000000000001";
const BRUME: &str = "c1000000-0000-4000-8000-000000000002";
/// Valombre's PC.
const ELOWEN: &str = "e1000000-0000-4000-8000-000000000001";
/// Valombre's NPC.
const MORLOX: &str = "e1000000-0000-4000-8000-000000000002";
/// Valombre's place.
const TAVERNE: &str = "e1000000-0000-4000-8000-000000000003";
/// Valombre's battlemap, set at the Taverne.
const CARTE: &str = "d1000000-0000-4000-8000-000000000001";
/// Valombre's illustration, at no place.
const PORTRAIT: &str = "d1000000-0000-4000-8000-000000000002";
const PLUIE: &str = "50000000-0000-4000-8000-000000000001";
/// Valombre's session 1.
const SEANCE: &str = "51000000-0000-4000-8000-000000000001";
/// Brume's session 1, which holds nothing.
const BRUME_SEANCE: &str = "51000000-0000-4000-8000-000000000002";
/// The tavern scene: the Taverne and Morlox.
const ARRIVEE: &str = "5c000000-0000-4000-8000-000000000001";
/// The ambush scene: Morlox and the portrait.
const EMBUSCADE: &str = "5c000000-0000-4000-8000-000000000002";
const BRIEF: &str = "b0000000-0000-4000-8000-000000000001";
const A_CREER: &str = "ac000000-0000-4000-8000-000000000001";
const INSTANCE: &str = "b1000000-0000-4000-8000-000000000001";
/// Elowen's token: a PC's, HP not tracked.
const JETON_ELOWEN: &str = "70000000-0000-4000-8000-000000000001";
/// Morlox 1: 7 HP of 7, shown.
const MORLOX_1: &str = "70000000-0000-4000-8000-000000000002";
/// Morlox 2: 0 HP of 7, hidden.
const MORLOX_2: &str = "70000000-0000-4000-8000-000000000003";
const COMBAT: &str = "c0000000-0000-4000-8000-000000000001";
/// Morlox 2's entry, rank 1.
const ENTREE_MORLOX_2: &str = "1e000000-0000-4000-8000-000000000001";
/// Elowen's entry, rank 2: the current one.
const ENTREE_ELOWEN: &str = "1e000000-0000-4000-8000-000000000002";
/// Morlox 1's entry, rank 3.
const ENTREE_MORLOX_1: &str = "1e000000-0000-4000-8000-000000000003";

fn url() -> String {
    std::env::var("DATABASE_URL").expect("DATABASE_URL names the harness's PostgreSQL")
}

fn migrations() -> PathBuf {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/migrations")).to_path_buf()
}

fn migration(file: &str) -> String {
    std::fs::read_to_string(migrations().join(file)).expect("a migration")
}

async fn connect(config: &tokio_postgres::Config) -> Client {
    let (client, connection) = config.connect(NoTls).await.expect("PostgreSQL");
    tokio::spawn(connection);
    client
}

/// The SQL adding element `id` to Valombre, with the version every element
/// is written with (0004_entity.sql, every-change-versioned), and its sheet
/// for a PC or an NPC.
fn element(id: &str, kind: &str, name: &str) -> String {
    let sheet = if kind == "lieu" {
        String::new()
    } else {
        format!("INSERT INTO character (entity_id, token_color) VALUES ('{id}', '#888');")
    };
    format!(
        "INSERT INTO entity (id, campaign_id, type, name) VALUES ('{id}', '{VALOMBRE}', '{kind}', '{name}'); \
         {sheet} \
         INSERT INTO entity_version (entity_id, version_number, source, snapshot, diff) \
         VALUES ('{id}', 1, 'dm', '{{}}', '[]');"
    )
}

/// The SQL of token `id` of `character`, `hp` its HP (SQL: `NULL, NULL` or
/// `current, max`).
fn token(id: &str, character: &str, name: &str, visible: bool, hp: &str) -> String {
    format!(
        "INSERT INTO token (id, instance_id, character_id, name, q, r, visible, hp_current, hp_max, conditions) \
         VALUES ('{id}', '{INSTANCE}', '{character}', '{name}', 0, 0, {visible}, {hp}, '{{}}');"
    )
}

/// A database of its own.
struct Scratch {
    client: Client,
    name: String,
}

impl Scratch {
    /// Empty: no table yet.
    async fn empty(name: &str) -> Scratch {
        let name = format!("data_schema_test_{name}_{}", std::process::id());
        let config: tokio_postgres::Config = url().parse().expect("a PostgreSQL URL");
        let admin = connect(&config).await;
        for sql in [
            format!("DROP DATABASE IF EXISTS {name} WITH (FORCE)"),
            format!("CREATE DATABASE {name}"),
        ] {
            admin.batch_execute(&sql).await.expect("a scratch database");
        }
        let client = connect(config.clone().dbname(&name)).await;
        Scratch { client, name }
    }

    /// Migrated by the Data layer, and holding a row in every table of the
    /// schema: Valombre's session 1, its PC present, its two scenes and
    /// their links, a brief and what it left to create, a journal line, a
    /// sound playing, the tavern's battlemap instance with three tokens in
    /// combat, a suggestion and the player view. Brume has an empty session.
    async fn new(name: &str) -> Scratch {
        let mut s = Scratch::empty(name).await;
        data::migrate(&mut s.client, &migrations())
            .await
            .expect("migrated by the Data layer");
        s.seed().await;
        s
    }

    async fn seed(&self) {
        self.accepted(&format!(
            "INSERT INTO campaign (id, name, kind) VALUES \
             ('{VALOMBRE}', 'Les Cendres de Valombre', 'campagne'), ('{BRUME}', 'La Brume', 'oneshot'); \
             {}{}{} \
             INSERT INTO media (id, campaign_id, kind, title, file_path, width_px, height_px, place_id) VALUES \
             ('{CARTE}', '{VALOMBRE}', 'battlemap', 'Taverne', 'carte.png', 2048, 1536, '{TAVERNE}'), \
             ('{PORTRAIT}', '{VALOMBRE}', 'illustration', 'Morlox', 'morlox.png', 800, 1200, NULL); \
             INSERT INTO battlemap_grid (media_id, hex_size_px, offset_x, offset_y, orientation, color, opacity) \
             VALUES ('{CARTE}', 64, 0, 0, 'pointe', '#202020', 0.4); \
             INSERT INTO sound (id, campaign_id, name, file_path) VALUES ('{PLUIE}', '{VALOMBRE}', 'Pluie', 'pluie.ogg'); \
             INSERT INTO session (id, campaign_id, position, title, status, microphone_on) VALUES \
             ('{SEANCE}', '{VALOMBRE}', 1, 'Le Sanglier', 'live', true), \
             ('{BRUME_SEANCE}', '{BRUME}', 1, 'Le Phare', 'prep', false); \
             INSERT INTO session_present_character (session_id, character_id) VALUES ('{SEANCE}', '{ELOWEN}'); \
             INSERT INTO scene (id, session_id, position, title) VALUES \
             ('{ARRIVEE}', '{SEANCE}', 1, 'Arrivée'), ('{EMBUSCADE}', '{SEANCE}', 2, 'Embuscade'); \
             INSERT INTO scene_entity (scene_id, entity_id, position) VALUES \
             ('{ARRIVEE}', '{TAVERNE}', 1), ('{ARRIVEE}', '{MORLOX}', 2), ('{EMBUSCADE}', '{MORLOX}', 1); \
             INSERT INTO scene_media (scene_id, media_id) VALUES ('{EMBUSCADE}', '{PORTRAIT}'); \
             INSERT INTO oral_brief (id, session_id, transcript, duration_seconds, recorded_at) \
             VALUES ('{BRIEF}', '{SEANCE}', 'Grimbald attend à la taverne.', 42, now()); \
             INSERT INTO entity_to_create (id, campaign_id, session_id, brief_id, scene_id, type, name, excerpt, status) \
             VALUES ('{A_CREER}', '{VALOMBRE}', '{SEANCE}', '{BRIEF}', '{ARRIVEE}', 'pnj', 'Grimbald', 'Grimbald attend', 'a_creer'); \
             INSERT INTO session_journal_line (session_id, scene_id, spoken_at, text) \
             VALUES ('{SEANCE}', '{ARRIVEE}', now(), 'Elowen entre.'); \
             INSERT INTO sound_playback (session_id, sound_id, origin, started_at) \
             VALUES ('{SEANCE}', '{PLUIE}', 'soundbox', now()); \
             INSERT INTO battlemap_instance (id, battlemap_media_id, session_id, scene_id) \
             VALUES ('{INSTANCE}', '{CARTE}', '{SEANCE}', '{ARRIVEE}'); \
             {}{}{} \
             INSERT INTO combat (id, instance_id, round, started_at) VALUES ('{COMBAT}', '{INSTANCE}', 1, now()); \
             INSERT INTO initiative_entry (id, combat_id, token_id, roll, initiative, dex_score, tiebreak, rank) VALUES \
             ('{ENTREE_MORLOX_2}', '{COMBAT}', '{MORLOX_2}', 18, 19, 12, 0.5, 1), \
             ('{ENTREE_ELOWEN}', '{COMBAT}', '{JETON_ELOWEN}', NULL, 15, 14, 0.25, 2), \
             ('{ENTREE_MORLOX_1}', '{COMBAT}', '{MORLOX_1}', 3, 4, 12, 0.75, 3); \
             UPDATE combat SET current_entry_id = '{ENTREE_ELOWEN}' WHERE id = '{COMBAT}'; \
             INSERT INTO live_suggestion (session_id, entity_id, matched_text, detected_at) \
             VALUES ('{SEANCE}', '{MORLOX}', 'Morlox', now()); \
             INSERT INTO diffusion (session_id, base_kind, base_instance_id, window_media_id, \
             window_x, window_y, window_width, view_zoom, view_pan_x, view_pan_y) \
             VALUES ('{SEANCE}', 'battlemap', '{INSTANCE}', '{PORTRAIT}', 0.1, 0.1, 0.3, 1, 0, 0)",
            element(ELOWEN, "pj", "Elowen"),
            element(MORLOX, "pnj", "Morlox"),
            element(TAVERNE, "lieu", "La Taverne du Sanglier"),
            token(JETON_ELOWEN, ELOWEN, "Elowen", true, "NULL, NULL"),
            token(MORLOX_1, MORLOX, "Morlox 1", true, "7, 7"),
            token(MORLOX_2, MORLOX, "Morlox 2", false, "0, 7"),
        ))
        .await;
    }

    async fn accepted(&self, sql: &str) {
        self.client
            .batch_execute(sql)
            .await
            .unwrap_or_else(|e| panic!("{sql}: {e:?}"));
    }

    /// Runs `sql` in one transaction, which the database refuses with `code`.
    async fn refused(&self, sql: &str, code: &SqlState) {
        let error = self
            .client
            .batch_execute(&format!("BEGIN; {sql}; COMMIT"))
            .await
            .expect_err(sql);
        let db = error.as_db_error().expect("a refusal of the database");
        assert_eq!(db.code(), code, "{sql}: {}", db.message());
        self.accepted("ROLLBACK").await;
    }

    /// One text column of the rows `sql` answers, a null as `-`.
    async fn texts(&self, sql: &str) -> Vec<String> {
        self.client
            .query(sql, &[])
            .await
            .unwrap_or_else(|e| panic!("{sql}: {e:?}"))
            .iter()
            .map(|row| {
                row.get::<_, Option<String>>(0)
                    .unwrap_or_else(|| "-".to_string())
            })
            .collect()
    }

    async fn count(&self, sql: &str) -> i64 {
        self.client
            .query_one(sql, &[])
            .await
            .unwrap_or_else(|e| panic!("{sql}: {e:?}"))
            .get(0)
    }

    /// Every row of the schema's tables, as text: what a replay must leave
    /// as it was.
    async fn snapshot(&self) -> Vec<String> {
        let mut rows = Vec::new();
        for table in TABLES {
            rows.extend(
                self.texts(&format!(
                    "SELECT '{table} ' || row_to_json(t)::text FROM {table} t ORDER BY 1"
                ))
                .await,
            );
        }
        rows
    }

    /// The relation's columns: name, type and nullability, in its order.
    async fn columns(&self, relation: &str) -> Vec<(String, String, bool)> {
        self.client
            .query(
                "SELECT a.attname, format_type(a.atttypid, a.atttypmod), NOT a.attnotnull \
                 FROM pg_attribute a WHERE a.attrelid = $1::text::regclass \
                 AND a.attnum > 0 AND NOT a.attisdropped ORDER BY a.attnum",
                &[&relation],
            )
            .await
            .expect("the columns read")
            .iter()
            .map(|row| {
                let ty: String = row.get(1);
                // As the model writes it, pg_dump's own alias.
                let ty = ty.replace("timestamp with time zone", "timestamptz");
                (row.get(0), ty, row.get(2))
            })
            .collect()
    }

    /// The table's foreign keys: column, referenced `table.column`, the
    /// actions on delete and on update as `pg_constraint` codes them, and
    /// whether the check waits for the commit.
    async fn foreign_keys(&self, table: &str) -> Vec<(String, String, String, String, bool)> {
        self.client
            .query(
                "SELECT a.attname, c.confrelid::regclass::text || '.' || r.attname, \
                 c.confdeltype::text, c.confupdtype::text, c.condeferred \
                 FROM pg_constraint c \
                 JOIN pg_attribute a ON a.attrelid = c.conrelid AND a.attnum = c.conkey[1] \
                 JOIN pg_attribute r ON r.attrelid = c.confrelid AND r.attnum = c.confkey[1] \
                 WHERE c.conrelid = $1::text::regclass AND c.contype = 'f' ORDER BY 1",
                &[&table],
            )
            .await
            .expect("the constraints read")
            .iter()
            .map(|row| {
                // `character` is a keyword, which regclass quotes.
                let to: String = row.get(1);
                (
                    row.get(0),
                    to.replace('"', ""),
                    row.get(2),
                    row.get(3),
                    row.get(4),
                )
            })
            .collect()
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

/// `relation` as a data model describes it: each field's name, type,
/// nullability and reference, in its order.
fn modelled(model: &Value, relation: &str) -> Vec<(String, String, bool, Option<String>)> {
    let table = model["tables"]
        .as_array()
        .expect("tables")
        .iter()
        .find(|t| t["name"] == relation)
        .unwrap_or_else(|| panic!("the model describes {relation}"));
    table["fields"]
        .as_array()
        .expect("fields")
        .iter()
        .map(|field| {
            (
                field["name"].as_str().expect("a name").to_string(),
                field["type"].as_str().expect("a type").to_string(),
                field["nullable"].as_bool().expect("a nullability"),
                field["references"].as_str().map(str::to_string),
            )
        })
        .collect()
}

fn model(text: &str) -> Value {
    serde_json::from_str(text).expect("a data model is JSON")
}

#[tokio::test]
async fn the_five_migrations_fill_in_a_database_the_earlier_ones_built_and_replay_as_a_no_op() {
    // A database migrated before them: every other file, through the Data
    // layer, which records each.
    let mut s = Scratch::empty("replay").await;
    let before = std::env::temp_dir().join(format!("data_schema_before_{}", std::process::id()));
    // A folder a failed run left under the same pid is emptied first.
    let _ = std::fs::remove_dir_all(&before);
    std::fs::create_dir_all(&before).expect("a folder of the earlier migrations");
    for entry in std::fs::read_dir(migrations()).expect("the migrations") {
        let path = entry.expect("a migration").path();
        let file = path
            .file_name()
            .expect("a name")
            .to_string_lossy()
            .into_owned();
        if file.ends_with(".sql")
            && !MIGRATIONS.contains(&file.as_str())
            && !LATER.contains(&file.as_str())
        {
            std::fs::copy(&path, before.join(&file)).expect("copied");
        }
    }
    let earlier = data::migrate(&mut s.client, &before).await;
    std::fs::remove_dir_all(&before).expect("the folder removed");
    let earlier = earlier.expect("the earlier migrations");
    assert!(!earlier.is_empty());
    // The Data layer fills in the five, in file order, the gap at 0006
    // included, and what builds on them after.
    let applied = data::migrate(&mut s.client, &migrations())
        .await
        .expect("the five migrations");
    assert_eq!(applied, [MIGRATIONS.as_slice(), LATER.as_slice()].concat());
    s.seed().await;
    let first = s.snapshot().await;
    assert_eq!(
        first.len(),
        2 + 1 + 2 + 3 + 1 + 1 + 1 + 1 + 1 + 1 + 3 + 1 + 3 + 1 + 1
    );
    // Replayed as the rules' runtime replays them with psql, twice.
    for _ in 0..2 {
        for file in MIGRATIONS {
            s.client
                .batch_execute(&migration(file))
                .await
                .unwrap_or_else(|e| panic!("{file} replayed: {e:?}"));
        }
        assert_eq!(s.snapshot().await, first);
    }
    // The foreign key closing the cycle, dropped and added again, is there
    // once.
    assert_eq!(
        s.count(
            "SELECT count(*) FROM pg_constraint \
             WHERE conrelid = 'combat'::regclass AND contype = 'f'"
        )
        .await,
        2
    );
    assert!(
        data::migrate(&mut s.client, &migrations())
            .await
            .expect("migrated again")
            .is_empty()
    );
    s.drop().await;
}

#[tokio::test]
async fn each_table_a_migration_references_exists_before_it_applies() {
    // Applied one file at a time on an empty database, each file applies,
    // and each table is born in the first file that has it.
    let s = Scratch::empty("order").await;
    let mut files: Vec<String> = std::fs::read_dir(migrations())
        .expect("the migrations")
        .map(|e| {
            e.expect("a migration")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .filter(|f| f.ends_with(".sql"))
        .collect();
    files.sort();
    let mut born = Vec::new();
    for (at, file) in files.iter().enumerate() {
        s.client
            .batch_execute(&migration(file))
            .await
            .unwrap_or_else(|e| panic!("{file} applies after the files before it: {e:?}"));
        for table in s
            .texts(
                "SELECT c.relname::text FROM pg_class c \
                 JOIN pg_namespace n ON n.oid = c.relnamespace \
                 WHERE n.nspname = 'public' AND c.relkind IN ('r', 'v')",
            )
            .await
        {
            if !born.iter().any(|(t, _)| *t == table) {
                born.push((table, at));
            }
        }
    }
    let at = |table: &str| {
        born.iter()
            .find(|(t, _)| t == table)
            .map(|(_, at)| *at)
            .unwrap_or_else(|| panic!("{table} is created"))
    };
    for table in TABLES {
        assert_eq!(
            files[at(table)],
            *MIGRATIONS
                .iter()
                .find(|m| migration(m).contains(&format!("CREATE TABLE IF NOT EXISTS {table} (")))
                .unwrap_or_else(|| panic!("{table} is one of the five migrations'")),
            "{table}"
        );
        for (column, to, ..) in s.foreign_keys(table).await {
            let referenced = to.split('.').next().expect("a table");
            assert!(
                at(referenced) <= at(table),
                "{table}.{column} references {to}, created after it"
            );
        }
    }
    s.drop().await;
}

#[tokio::test]
async fn the_tables_and_views_are_what_data_model_json_describes_and_the_design_kept() {
    let s = Scratch::new("model").await;
    let described = model(include_str!("../../../data/model.json"));
    let designed = model(include_str!("../../../mock/data/model.json"));
    for table in TABLES {
        let keys = s.foreign_keys(table).await;
        let real: Vec<(String, String, bool, Option<String>)> = s
            .columns(table)
            .await
            .into_iter()
            .map(|(name, ty, nullable)| {
                let references = keys
                    .iter()
                    .find(|(column, ..)| *column == name)
                    .map(|(_, to, ..)| to.clone());
                (name, ty, nullable, references)
            })
            .collect();
        assert_eq!(real, modelled(&described, table), "{table}");
        // Copied from the dossier's design: nothing renamed, nothing dropped.
        // A column a later migration adds is the model's alone.
        let kept: Vec<_> = real
            .iter()
            .filter(|(column, ..)| !ADDED_AFTER_THE_DESIGN.contains(&(table, column.as_str())))
            .cloned()
            .collect();
        assert_eq!(kept, modelled(&designed, table), "{table}");
    }
    // A view's columns are all nullable to PostgreSQL: the model says what
    // the view guarantees, its names and types are the database's.
    for view in VIEWS {
        let real: Vec<(String, String)> = s
            .columns(view)
            .await
            .into_iter()
            .map(|(name, ty, _)| (name, ty))
            .collect();
        for model in [&described, &designed] {
            let fields: Vec<(String, String)> = modelled(model, view)
                .into_iter()
                .map(|(name, ty, _, references)| {
                    assert_eq!(references, None, "{view}.{name}");
                    (name, ty)
                })
                .collect();
            assert_eq!(real, fields, "{view}");
        }
    }
    s.drop().await;
}

#[tokio::test]
async fn each_relation_deletes_and_updates_as_the_aggregate_declares() {
    let s = Scratch::new("relations").await;
    // c cascade, n nullify (SET NULL), r restrict; propagate is CASCADE.
    // Restrict and review are `a` (NO ACTION), checked at the commit.
    let key = |column: &str, to: &str, delete: &str, update: &str| {
        let held = delete == "a";
        (
            column.to_string(),
            to.to_string(),
            delete.to_string(),
            update.to_string(),
            held,
        )
    };
    let expected = [
        ("session", vec![key("campaign_id", "campaign.id", "c", "c")]),
        (
            "session_present_character",
            vec![
                key("character_id", "character.entity_id", "c", "c"),
                key("session_id", "session.id", "c", "c"),
            ],
        ),
        ("scene", vec![key("session_id", "session.id", "c", "c")]),
        (
            "scene_entity",
            vec![
                key("entity_id", "entity.id", "c", "c"),
                key("scene_id", "scene.id", "c", "c"),
            ],
        ),
        (
            "scene_media",
            vec![
                key("media_id", "media.id", "c", "c"),
                key("scene_id", "scene.id", "c", "c"),
            ],
        ),
        (
            "oral_brief",
            vec![key("session_id", "session.id", "c", "c")],
        ),
        (
            "entity_to_create",
            vec![
                key("brief_id", "oral_brief.id", "n", "c"),
                key("campaign_id", "campaign.id", "c", "c"),
                key("created_entity_id", "entity.id", "n", "c"),
                key("scene_id", "scene.id", "n", "c"),
                key("session_id", "session.id", "c", "c"),
            ],
        ),
        (
            "session_journal_line",
            vec![
                key("scene_id", "scene.id", "n", "c"),
                key("session_id", "session.id", "c", "r"),
            ],
        ),
        (
            "sound_playback",
            vec![
                key("session_id", "session.id", "c", "c"),
                key("sound_id", "sound.id", "c", "c"),
            ],
        ),
        (
            "battlemap_instance",
            vec![
                key("battlemap_media_id", "media.id", "a", "c"),
                key("scene_id", "scene.id", "c", "c"),
                key("session_id", "session.id", "c", "c"),
            ],
        ),
        (
            "token",
            vec![
                key("character_id", "character.entity_id", "a", "c"),
                key("instance_id", "battlemap_instance.id", "c", "c"),
            ],
        ),
        (
            "combat",
            vec![
                key("current_entry_id", "initiative_entry.id", "n", "c"),
                key("instance_id", "battlemap_instance.id", "c", "c"),
            ],
        ),
        (
            "initiative_entry",
            vec![
                key("combat_id", "combat.id", "c", "c"),
                key("token_id", "token.id", "c", "c"),
            ],
        ),
        (
            "live_suggestion",
            vec![
                key("entity_id", "entity.id", "c", "c"),
                key("session_id", "session.id", "c", "c"),
                key("sound_id", "sound.id", "c", "c"),
            ],
        ),
        (
            "diffusion",
            vec![
                key("base_instance_id", "battlemap_instance.id", "n", "c"),
                key("base_media_id", "media.id", "n", "c"),
                key("session_id", "session.id", "c", "c"),
                key("window_media_id", "media.id", "n", "c"),
            ],
        ),
    ];
    assert_eq!(expected.len(), TABLES.len());
    for (table, keys) in expected {
        assert_eq!(s.foreign_keys(table).await, keys, "{table}");
    }
    s.drop().await;
}

#[tokio::test]
async fn deleting_a_campaign_deletes_its_sessions_and_all_they_hold_and_theirs_alone() {
    // Its media and its elements go with it, battlemap and characters on the
    // map included: the instance and its tokens are gone by the commit.
    let s = Scratch::new("cascade").await;
    s.accepted(&format!("DELETE FROM campaign WHERE id = '{VALOMBRE}'"))
        .await;
    let mut left = Vec::new();
    for table in TABLES {
        left.push((
            table,
            s.count(&format!("SELECT count(*) FROM {table}")).await,
        ));
    }
    let expected: Vec<(&str, i64)> = TABLES
        .iter()
        .map(|t| (*t, i64::from(*t == "session")))
        .collect();
    assert_eq!(left, expected);
    assert_eq!(
        s.texts("SELECT id::text FROM session").await,
        [BRUME_SEANCE]
    );
    s.drop().await;
}

#[tokio::test]
async fn deleting_a_session_deletes_what_it_holds() {
    let s = Scratch::new("session_cascade").await;
    s.accepted(&format!("DELETE FROM session WHERE id = '{SEANCE}'"))
        .await;
    for table in TABLES {
        let left = s.count(&format!("SELECT count(*) FROM {table}")).await;
        assert_eq!(left, i64::from(table == "session"), "{table}");
    }
    // The campaign's own rows stay.
    assert_eq!(
        s.count("SELECT count(*) FROM media").await
            + s.count("SELECT count(*) FROM entity").await
            + s.count("SELECT count(*) FROM sound").await,
        2 + 3 + 1
    );
    s.drop().await;
}

#[tokio::test]
async fn a_battlemap_or_a_character_still_on_a_map_is_kept_until_the_map_goes() {
    let s = Scratch::new("held").await;
    s.refused(
        &format!("DELETE FROM media WHERE id = '{CARTE}'"),
        &SqlState::FOREIGN_KEY_VIOLATION,
    )
    .await;
    s.refused(
        &format!("DELETE FROM entity WHERE id = '{MORLOX}'"),
        &SqlState::FOREIGN_KEY_VIOLATION,
    )
    .await;
    // Checked at the commit: the map removed in the same transaction, both
    // go.
    s.accepted(&format!(
        "BEGIN; DELETE FROM media WHERE id = '{CARTE}'; DELETE FROM entity WHERE id = '{MORLOX}'; \
         DELETE FROM battlemap_instance WHERE id = '{INSTANCE}'; COMMIT"
    ))
    .await;
    assert_eq!(s.count("SELECT count(*) FROM token").await, 0);
    // The scenes lost their links to Morlox, the session its media.
    assert_eq!(s.count("SELECT count(*) FROM scene_entity").await, 1);
    assert_eq!(s.count("SELECT count(*) FROM diffusion").await, 1);
    s.drop().await;
}

#[tokio::test]
async fn what_points_to_a_deleted_row_it_does_not_belong_to_is_left_with_none() {
    let s = Scratch::new("nullify").await;
    s.accepted(&format!(
        "DELETE FROM initiative_entry WHERE id = '{ENTREE_ELOWEN}'; \
         DELETE FROM oral_brief WHERE id = '{BRIEF}'; \
         DELETE FROM scene WHERE id = '{ARRIVEE}'"
    ))
    .await;
    // The scene took the instance with it, and the combat.
    assert_eq!(s.count("SELECT count(*) FROM combat").await, 0);
    assert_eq!(
        s.texts(
            "SELECT coalesce(brief_id::text, '-') || ' ' || coalesce(scene_id::text, '-') \
             FROM entity_to_create"
        )
        .await,
        ["- -"]
    );
    assert_eq!(
        s.texts("SELECT coalesce(scene_id::text, '-') FROM session_journal_line")
            .await,
        ["-"]
    );
    assert_eq!(
        s.texts(
            "SELECT base_kind || ' ' || coalesce(base_instance_id::text, '-') \
             || ' ' || coalesce(window_media_id::text, '-') FROM diffusion"
        )
        .await,
        [format!("battlemap - {PORTRAIT}")]
    );
    s.drop().await;
}

#[tokio::test]
async fn the_current_entry_s_deletion_leaves_the_combat_with_none() {
    let s = Scratch::new("current").await;
    s.accepted(&format!("DELETE FROM token WHERE id = '{JETON_ELOWEN}'"))
        .await;
    assert_eq!(
        s.texts("SELECT coalesce(current_entry_id::text, '-') FROM combat")
            .await,
        ["-"]
    );
    assert_eq!(s.count("SELECT count(*) FROM initiative_entry").await, 2);
    s.drop().await;
}

#[tokio::test]
async fn a_new_id_reaches_every_row_that_points_to_it_but_a_session_s_journal_keeps_it() {
    let s = Scratch::new("propagate").await;
    let moved = "51000000-0000-4000-8000-0000000000ff";
    // onUpdate restrict: a session with a journal keeps its id.
    s.refused(
        &format!("UPDATE session SET id = '{moved}' WHERE id = '{SEANCE}'"),
        &SqlState::FOREIGN_KEY_VIOLATION,
    )
    .await;
    s.accepted(&format!(
        "DELETE FROM session_journal_line; \
         UPDATE session SET id = '{moved}' WHERE id = '{SEANCE}'"
    ))
    .await;
    for table in [
        "session_present_character",
        "scene",
        "oral_brief",
        "entity_to_create",
        "sound_playback",
        "battlemap_instance",
        "live_suggestion",
        "diffusion",
    ] {
        assert_eq!(
            s.count(&format!(
                "SELECT count(*) FROM {table} WHERE session_id <> '{moved}'"
            ))
            .await,
            0,
            "{table}"
        );
    }
    let entry = "1e000000-0000-4000-8000-0000000000ff";
    s.accepted(&format!(
        "UPDATE initiative_entry SET id = '{entry}' WHERE id = '{ENTREE_ELOWEN}'"
    ))
    .await;
    assert_eq!(
        s.texts("SELECT current_entry_id::text FROM combat").await,
        [entry]
    );
    s.drop().await;
}

#[tokio::test]
async fn a_token_s_death_band_and_initial_are_generated_from_its_hp_and_name() {
    let s = Scratch::new("generated").await;
    let state = |id: &str| {
        format!(
            "SELECT dead::text || ' ' || coalesce(health_band, '-') || ' ' || label_initial \
             FROM token WHERE id = '{id}'"
        )
    };
    assert_eq!(s.texts(&state(JETON_ELOWEN)).await, ["false - E"]);
    assert_eq!(s.texts(&state(MORLOX_2)).await, ["true mort M"]);
    for (hp, expected) in [
        ("7", "false indemne M"),
        ("4", "false blessé M"),
        ("3", "false en sang M"),
        ("1", "false en sang M"),
        ("0", "true mort M"),
    ] {
        s.accepted(&format!(
            "UPDATE token SET hp_max = 7, hp_current = {hp} WHERE id = '{MORLOX_1}'"
        ))
        .await;
        assert_eq!(s.texts(&state(MORLOX_1)).await, [expected], "{hp} HP of 7");
    }
    // Half of an even maximum is « en sang », one more « blessé ».
    for (hp, expected) in [("4", "false en sang M"), ("5", "false blessé M")] {
        s.accepted(&format!(
            "UPDATE token SET hp_max = 8, hp_current = {hp} WHERE id = '{MORLOX_1}'"
        ))
        .await;
        assert_eq!(s.texts(&state(MORLOX_1)).await, [expected], "{hp} HP of 8");
    }
    s.accepted(&format!(
        "UPDATE token SET name = 'Grimbald' WHERE id = '{MORLOX_1}'"
    ))
    .await;
    assert_eq!(s.texts(&state(MORLOX_1)).await, ["false blessé G"]);
    // Never written.
    for column in [
        "dead = false",
        "health_band = 'indemne'",
        "label_initial = 'X'",
    ] {
        s.refused(
            &format!("UPDATE token SET {column} WHERE id = '{MORLOX_1}'"),
            &SqlState::GENERATED_ALWAYS,
        )
        .await;
    }
    s.drop().await;
}

#[tokio::test]
async fn session_roster_view_offers_the_present_pcs_and_the_npcs_of_its_scenes() {
    let s = Scratch::new("roster").await;
    let roster = || {
        s.texts(
            "SELECT session_id || ' ' || character_id || ' ' || via \
             FROM session_roster_view ORDER BY 1",
        )
    };
    // Morlox once though in two scenes; the Taverne, a place, never.
    let mut expected = vec![
        format!("{SEANCE} {ELOWEN} present"),
        format!("{SEANCE} {MORLOX} scene"),
    ];
    expected.sort();
    assert_eq!(roster().await, expected);
    s.accepted(&format!(
        "DELETE FROM scene_entity WHERE entity_id = '{MORLOX}'; \
         DELETE FROM session_present_character"
    ))
    .await;
    assert!(roster().await.is_empty());
    s.drop().await;
}

#[tokio::test]
async fn session_media_view_follows_the_scenes_media_and_their_places() {
    let s = Scratch::new("media").await;
    let media = || {
        s.texts(
            "SELECT session_id || ' ' || media_id || ' ' || coalesce(via_place_id::text, '-') \
             FROM session_media_view ORDER BY 1",
        )
    };
    let mut expected = vec![
        format!("{SEANCE} {PORTRAIT} -"),
        format!("{SEANCE} {CARTE} {TAVERNE}"),
    ];
    expected.sort();
    assert_eq!(media().await, expected);
    // Never stored: the battlemap moved to no place leaves the session.
    s.accepted(&format!(
        "UPDATE media SET place_id = NULL WHERE id = '{CARTE}'"
    ))
    .await;
    assert_eq!(media().await, [format!("{SEANCE} {PORTRAIT} -")]);
    // Linked to a scene and at a place of another, a media is there once,
    // as a direct link.
    s.accepted(&format!(
        "UPDATE media SET place_id = '{TAVERNE}' WHERE id = '{PORTRAIT}'"
    ))
    .await;
    assert_eq!(media().await, [format!("{SEANCE} {PORTRAIT} -")]);
    s.accepted(&format!(
        "DELETE FROM scene_media WHERE media_id = '{PORTRAIT}'"
    ))
    .await;
    assert_eq!(media().await, [format!("{SEANCE} {PORTRAIT} {TAVERNE}")]);
    s.drop().await;
}

#[tokio::test]
async fn initiative_order_view_orders_by_rank_flags_the_hidden_and_skips_the_dead() {
    let s = Scratch::new("initiative").await;
    let order = || {
        s.texts(
            "SELECT entry_id || ' ' || order_index || ' ' || shown_to_players || ' ' || skipped \
             FROM initiative_order_view ORDER BY order_index",
        )
    };
    assert_eq!(
        order().await,
        [
            format!("{ENTREE_MORLOX_2} 1 false true"),
            format!("{ENTREE_ELOWEN} 2 true false"),
            format!("{ENTREE_MORLOX_1} 3 true false"),
        ]
    );
    // Revealed and healed, Morlox 2 keeps its place; a new rank moves it.
    s.accepted(&format!(
        "UPDATE token SET visible = true, hp_current = 5 WHERE id = '{MORLOX_2}'; \
         UPDATE initiative_entry SET rank = 4 WHERE id = '{ENTREE_MORLOX_2}'"
    ))
    .await;
    assert_eq!(
        order().await,
        [
            format!("{ENTREE_ELOWEN} 1 true false"),
            format!("{ENTREE_MORLOX_1} 2 true false"),
            format!("{ENTREE_MORLOX_2} 3 true false"),
        ]
    );
    s.drop().await;
}

#[tokio::test]
async fn a_value_outside_its_set_is_refused() {
    let s = Scratch::new("values").await;
    for sql in [
        format!("UPDATE session SET status = 'paused' WHERE id = '{SEANCE}'"),
        format!("UPDATE entity_to_create SET type = 'pj' WHERE id = '{A_CREER}'"),
        format!("UPDATE entity_to_create SET status = 'ignoree' WHERE id = '{A_CREER}'"),
        "UPDATE sound_playback SET origin = 'joueur'".to_string(),
        "UPDATE diffusion SET base_kind = 'video'".to_string(),
    ] {
        s.refused(&sql, &SqlState::CHECK_VIOLATION).await;
    }
    s.drop().await;
}
