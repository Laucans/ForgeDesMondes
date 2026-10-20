//! The Media migration (`crates/data/migrations/0005_media.sql`) against the
//! harness's PostgreSQL (`DATABASE_URL`): replayed over the campaign, player
//! and entity tables it needs, it is what `data/model.json` says it is —
//! the foreign keys it gives the campaign's cover and the sheet's portrait
//! and token included; its relations cascade and nullify as the aggregates
//! declare; and the tables refuse, under its id, what breaks one of the
//! Media aggregate's invariants. Each test works in a scratch database of
//! its own, dropped when it ends.

use serde_json::Value;
use tokio_postgres::error::SqlState;
use tokio_postgres::{Client, NoTls};

const CAMPAIGN: &str = include_str!("../../../data/migrations/0002_campaign.sql");
const PLAYER: &str = include_str!("../../../data/migrations/0003_player.sql");
const ENTITY: &str = include_str!("../../../data/migrations/0004_entity.sql");
const MIGRATION: &str = include_str!("../../../data/migrations/0005_media.sql");

const VALOMBRE: &str = "c1000000-0000-4000-8000-000000000001";
const BRUME: &str = "c1000000-0000-4000-8000-000000000002";
const ELOWEN: &str = "e1000000-0000-4000-8000-000000000001";
const TAVERNE: &str = "e1000000-0000-4000-8000-000000000002";
const MORLOX: &str = "e1000000-0000-4000-8000-000000000003";
const PHARE: &str = "e1000000-0000-4000-8000-000000000004";
const CARTE: &str = "a1000000-0000-4000-8000-000000000001";
const PORTRAIT: &str = "a1000000-0000-4000-8000-000000000002";
const JETON: &str = "a1000000-0000-4000-8000-000000000003";
const ECUEIL: &str = "a1000000-0000-4000-8000-000000000004";

/// The tables the migration creates, in its order.
const TABLES: [&str; 2] = ["media", "battlemap_grid"];
/// The older tables it gives a foreign key to media.id.
const POINTING: [&str; 2] = ["campaign", "character"];
/// The triggers it creates, and on how many tables each.
const TRIGGERS: [(&str, i64); 3] = [
    ("grid_only_for_battlemap", 2),
    ("media_place_is_lieu", 2),
    ("token_crop_follows_portrait", 1),
];

fn url() -> String {
    std::env::var("DATABASE_URL").expect("DATABASE_URL names the harness's PostgreSQL")
}

async fn connect(config: &tokio_postgres::Config) -> Client {
    let (client, connection) = config.connect(NoTls).await.expect("PostgreSQL");
    tokio::spawn(connection);
    client
}

/// The SQL adding version `number` of `element`, from the DM.
fn version(element: &str, number: u32) -> String {
    format!(
        "INSERT INTO entity_version (entity_id, version_number, source, snapshot, diff) \
         VALUES ('{element}', {number}, 'dm', '{{}}', '[]');"
    )
}

/// The SQL of a grid for `media`, `hex` pixels a hex, at `opacity`.
fn grid(media: &str, hex: &str, opacity: &str) -> String {
    format!(
        "INSERT INTO battlemap_grid (media_id, hex_size_px, offset_x, offset_y, orientation, color, opacity) \
         VALUES ('{media}', {hex}, 12.5, 0, 'pointe', '#202020', {opacity});"
    )
}

/// The SQL of a media of `campaign`, of `kind`, at `place` (SQL: `NULL` or
/// a quoted id).
fn media(id: &str, campaign: &str, kind: &str, place: &str) -> String {
    format!(
        "INSERT INTO media (id, campaign_id, kind, title, file_path, width_px, height_px, place_id) \
         VALUES ('{id}', '{campaign}', '{kind}', 'Carte', 'media/{id}.png', 2048, 1536, {place});"
    )
}

/// A database of its own, holding the campaign, player and entity tables,
/// migrated once. Valombre holds Elowen (a PC), the Taverne du Sanglier (a
/// place) and Morlox (an NPC); its battlemap of the tavern, with its grid,
/// is set at the Taverne, and its two illustrations are Elowen's portrait
/// and token and the campaign's cover. Brume holds the lighthouse and its
/// reef's battlemap. Each element is at version 1.
struct Scratch {
    client: Client,
    name: String,
}

impl Scratch {
    async fn new(name: &str) -> Scratch {
        let name = format!("media_test_{name}_{}", std::process::id());
        let config: tokio_postgres::Config = url().parse().expect("a PostgreSQL URL");
        let admin = connect(&config).await;
        for sql in [
            format!("DROP DATABASE IF EXISTS {name} WITH (FORCE)"),
            format!("CREATE DATABASE {name}"),
        ] {
            admin.batch_execute(&sql).await.expect("a scratch database");
        }
        let client = connect(config.clone().dbname(&name)).await;
        for sql in [CAMPAIGN, PLAYER, ENTITY, MIGRATION] {
            client.batch_execute(sql).await.expect("migrated");
        }
        client
            .batch_execute(&format!(
                "INSERT INTO campaign (id, name, kind) VALUES \
                 ('{VALOMBRE}', 'Les Cendres de Valombre', 'campagne'), \
                 ('{BRUME}', 'La Brume', 'oneshot'); \
                 INSERT INTO entity (id, campaign_id, type, name) VALUES \
                 ('{ELOWEN}', '{VALOMBRE}', 'pj', 'Elowen'), \
                 ('{TAVERNE}', '{VALOMBRE}', 'lieu', 'La Taverne du Sanglier'), \
                 ('{MORLOX}', '{VALOMBRE}', 'pnj', 'Morlox'), \
                 ('{PHARE}', '{BRUME}', 'lieu', 'Le Phare'); \
                 {}{}{}{}{}{} \
                 INSERT INTO character (entity_id, portrait_media_id, token_media_id, token_color) \
                 VALUES ('{ELOWEN}', '{PORTRAIT}', '{JETON}', '#2f7d5b'); \
                 {}{}{}{} \
                 UPDATE campaign SET cover_media_id = '{PORTRAIT}' WHERE id = '{VALOMBRE}'",
                media(CARTE, VALOMBRE, "battlemap", &format!("'{TAVERNE}'")),
                grid(CARTE, "64", "0.4"),
                media(PORTRAIT, VALOMBRE, "illustration", "NULL"),
                media(JETON, VALOMBRE, "illustration", "NULL"),
                media(ECUEIL, BRUME, "battlemap", &format!("'{PHARE}'")),
                grid(ECUEIL, "48.5", "1"),
                version(ELOWEN, 1),
                version(TAVERNE, 1),
                version(MORLOX, 1),
                version(PHARE, 1),
            ))
            .await
            .expect("two campaigns, their elements and their media");
        Scratch { client, name }
    }

    /// Runs `sql` in one transaction, which the database refuses: the code
    /// and the constraint of its refusal, and its message.
    async fn refused(&self, sql: &str) -> (SqlState, Option<String>, String) {
        let error = self.client.batch_execute(sql).await.expect_err(sql);
        let db = error.as_db_error().expect("a refusal of the database");
        (
            db.code().clone(),
            db.constraint().map(str::to_string),
            db.message().to_string(),
        )
    }

    /// Runs `sql` in one transaction, which a constraint trigger refuses
    /// under the invariant `id`.
    async fn refused_by(&self, sql: &str, id: &str) {
        let (code, constraint, message) = self.refused(sql).await;
        assert_eq!(code, SqlState::CHECK_VIOLATION, "{sql}: {message}");
        assert_eq!(
            constraint.as_deref(),
            Some(id.replace('-', "_").as_str()),
            "{sql}: {message}"
        );
        assert!(message.starts_with(&format!("{id}: ")), "{message}");
    }

    /// Runs `sql` in one transaction, which a CHECK named `constraint`
    /// refuses.
    async fn refused_by_check(&self, sql: &str, constraint: &str) {
        let (code, named, message) = self.refused(sql).await;
        assert_eq!(
            (code, named.as_deref()),
            (SqlState::CHECK_VIOLATION, Some(constraint)),
            "{sql}: {message}"
        );
    }

    async fn accepted(&self, sql: &str) {
        self.client
            .batch_execute(sql)
            .await
            .unwrap_or_else(|e| panic!("{sql}: {e:?}"));
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

    async fn count(&self, sql: &str) -> i64 {
        self.client
            .query_one(sql, &[])
            .await
            .unwrap_or_else(|e| panic!("{sql}: {e:?}"))
            .get(0)
    }

    /// Every row of the migration's tables and of those it points to
    /// media.id, as text: what a replay must leave as it was.
    async fn snapshot(&self) -> Vec<String> {
        let mut rows = Vec::new();
        for table in TABLES.iter().chain(POINTING.iter()) {
            rows.extend(
                self.texts(&format!(
                    "SELECT row_to_json(t)::text FROM {table} t ORDER BY 1"
                ))
                .await,
            );
        }
        rows
    }

    /// The table's columns: name, type and nullability, in its order.
    async fn columns(&self, table: &str) -> Vec<(String, String, bool)> {
        self.client
            .query(
                "SELECT a.attname, format_type(a.atttypid, a.atttypmod), NOT a.attnotnull \
                 FROM pg_attribute a WHERE a.attrelid = $1::text::regclass \
                 AND a.attnum > 0 AND NOT a.attisdropped ORDER BY a.attnum",
                &[&table],
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

    /// The table's foreign keys: column, referenced `table.column`, and the
    /// actions on delete and on update, as `pg_constraint` codes them.
    async fn foreign_keys(&self, table: &str) -> Vec<(String, String, String, String)> {
        self.client
            .query(
                "SELECT a.attname, c.confrelid::regclass::text || '.' || r.attname, \
                 c.confdeltype::text, c.confupdtype::text \
                 FROM pg_constraint c \
                 JOIN pg_attribute a ON a.attrelid = c.conrelid AND a.attnum = c.conkey[1] \
                 JOIN pg_attribute r ON r.attrelid = c.confrelid AND r.attnum = c.confkey[1] \
                 WHERE c.conrelid = $1::text::regclass AND c.contype = 'f' ORDER BY 1",
                &[&table],
            )
            .await
            .expect("the constraints read")
            .iter()
            .map(|row| (row.get(0), row.get(1), row.get(2), row.get(3)))
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

/// `table` as `data/model.json` describes it: each field's name, type,
/// nullability and reference, in its order.
fn modelled(table: &str) -> Vec<(String, String, bool, Option<String>)> {
    let model: Value = serde_json::from_str(include_str!("../../../../data/model.json"))
        .expect("data/model.json is JSON");
    model["tables"]
        .as_array()
        .expect("tables")
        .iter()
        .find(|t| t["name"] == table)
        .unwrap_or_else(|| panic!("data/model.json describes {table}"))["fields"]
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

/// A foreign key as [`Scratch::foreign_keys`] reads it: c cascade, n
/// nullify (SET NULL); propagate is CASCADE.
fn key(column: &str, to: &str, delete: &str, update: &str) -> (String, String, String, String) {
    (
        column.to_string(),
        to.to_string(),
        delete.to_string(),
        update.to_string(),
    )
}

#[tokio::test]
async fn the_migration_replays_as_a_no_op_over_the_campaign_player_and_entity_tables() {
    let mut s = Scratch::new("replay").await;
    let first = s.snapshot().await;
    assert_eq!(first.len(), 4 + 2 + 2 + 1);
    for _ in 0..2 {
        s.client
            .batch_execute(MIGRATION)
            .await
            .expect("replayed as psql replays it");
        assert_eq!(s.snapshot().await, first);
    }
    // The server's applier after the rules' runtime's psql, which records
    // nothing: it runs every file again, in order, and nothing moves.
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../data/migrations");
    let applied = data::migrate(&mut s.client, std::path::Path::new(dir))
        .await
        .expect("replayed by the Data layer");
    let at = |file: &str| {
        applied
            .iter()
            .position(|f| f == file)
            .unwrap_or_else(|| panic!("{file} is applied: {applied:?}"))
    };
    assert!(at("0004_entity.sql") < at("0005_media.sql"), "{applied:?}");
    assert_eq!(s.snapshot().await, first);
    // A trigger or a foreign key dropped and created again is there once.
    for (trigger, tables) in TRIGGERS {
        assert_eq!(
            s.count(&format!(
                "SELECT count(*) FROM pg_trigger WHERE tgname = '{trigger}'"
            ))
            .await,
            tables,
            "{trigger}"
        );
    }
    for table in POINTING {
        assert_eq!(
            s.count(&format!(
                "SELECT count(*) FROM pg_constraint WHERE contype = 'f' \
                 AND conrelid = '{table}'::regclass AND confrelid = 'media'::regclass"
            ))
            .await,
            if table == "campaign" { 1 } else { 2 },
            "{table}"
        );
    }
    s.drop().await;
}

#[tokio::test]
async fn the_tables_and_the_foreign_keys_to_media_are_what_data_model_json_describes() {
    let s = Scratch::new("model").await;
    for table in TABLES.iter().chain(POINTING.iter()) {
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
        assert_eq!(real, modelled(table), "{table}");
    }
    s.drop().await;
}

#[tokio::test]
async fn each_relation_deletes_and_updates_as_the_aggregates_declare() {
    let s = Scratch::new("relations").await;
    assert_eq!(
        s.foreign_keys("media").await,
        [
            key("campaign_id", "campaign.id", "c", "c"),
            key("place_id", "entity.id", "n", "c"),
        ]
    );
    assert_eq!(
        s.foreign_keys("battlemap_grid").await,
        [key("media_id", "media.id", "c", "c")]
    );
    assert_eq!(
        s.foreign_keys("campaign").await,
        [key("cover_media_id", "media.id", "n", "c")]
    );
    assert_eq!(
        s.foreign_keys("character").await,
        [
            key("entity_id", "entity.id", "c", "c"),
            key("player_id", "player.id", "n", "c"),
            key("portrait_media_id", "media.id", "n", "c"),
            key("token_media_id", "media.id", "n", "c"),
        ]
    );
    s.drop().await;
}

#[tokio::test]
async fn deleting_a_campaign_deletes_its_media_and_their_grids_and_theirs_alone() {
    let s = Scratch::new("cascade").await;
    // Elowen's token is a crop of her portrait, which is Valombre's cover
    // too: the campaign still goes whole.
    s.accepted(&format!(
        "UPDATE character SET token_crop = '{{\"x\": 0.2}}' WHERE entity_id = '{ELOWEN}'; {}",
        version(ELOWEN, 2)
    ))
    .await;
    s.accepted(&format!("DELETE FROM campaign WHERE id = '{VALOMBRE}'"))
        .await;
    assert_eq!(s.texts("SELECT id::text FROM media").await, [ECUEIL]);
    assert_eq!(
        s.texts("SELECT media_id::text FROM battlemap_grid").await,
        [ECUEIL]
    );
    // A battlemap deleted takes its grid.
    s.accepted(&format!("DELETE FROM media WHERE id = '{ECUEIL}'"))
        .await;
    assert_eq!(s.count("SELECT count(*) FROM battlemap_grid").await, 0);
    s.drop().await;
}

#[tokio::test]
async fn a_campaign_s_new_id_reaches_its_media_and_their_place_with_them() {
    let s = Scratch::new("propagate").await;
    let moved = "c1000000-0000-4000-8000-0000000000ff";
    // The tavern and its battlemap move at once: media-place-is-lieu, judged
    // at commit, finds them in one campaign still.
    s.accepted(&format!(
        "UPDATE campaign SET id = '{moved}' WHERE id = '{VALOMBRE}'"
    ))
    .await;
    assert_eq!(
        s.count(&format!(
            "SELECT count(*) FROM media WHERE campaign_id = '{moved}'"
        ))
        .await,
        3
    );
    s.drop().await;
}

#[tokio::test]
async fn a_media_s_new_id_reaches_its_grid_the_cover_and_the_sheet() {
    let s = Scratch::new("media_id").await;
    let carte = "a1000000-0000-4000-8000-0000000000f1";
    let portrait = "a1000000-0000-4000-8000-0000000000f2";
    // The relations' doing, no one's save of Elowen: no version is asked for.
    s.accepted(&format!(
        "UPDATE media SET id = '{carte}' WHERE id = '{CARTE}'; \
         UPDATE media SET id = '{portrait}' WHERE id = '{PORTRAIT}'"
    ))
    .await;
    assert_eq!(
        s.texts("SELECT media_id::text FROM battlemap_grid ORDER BY 1")
            .await,
        [ECUEIL, carte]
    );
    assert_eq!(
        s.texts(&format!(
            "SELECT cover_media_id::text FROM campaign WHERE id = '{VALOMBRE}' \
             UNION ALL SELECT portrait_media_id::text FROM character"
        ))
        .await,
        [portrait, portrait]
    );
    s.drop().await;
}

#[tokio::test]
async fn deleting_a_media_leaves_the_cover_portrait_and_token_with_none() {
    let s = Scratch::new("nullify").await;
    let pointing = "SELECT coalesce(cover_media_id::text, 'none') FROM campaign \
                    WHERE id = 'c1000000-0000-4000-8000-000000000001' \
                    UNION ALL SELECT coalesce(portrait_media_id::text, 'none') || ' ' || \
                    coalesce(token_media_id::text, 'none') FROM character";
    // The relations' doing, no one's save of Elowen: no version is asked for.
    s.accepted(&format!("DELETE FROM media WHERE id = '{JETON}'"))
        .await;
    assert_eq!(
        s.texts(pointing).await,
        [PORTRAIT.to_string(), format!("{PORTRAIT} none")]
    );
    s.accepted(&format!("DELETE FROM media WHERE id = '{PORTRAIT}'"))
        .await;
    assert_eq!(s.texts(pointing).await, ["none", "none none"]);
    s.drop().await;
}

#[tokio::test]
async fn every_change_versioned_still_wants_a_version_of_a_portrait_or_token_set_by_hand() {
    let s = Scratch::new("by_hand").await;
    // A media that exists, given or taken by hand, is the DM's save.
    for change in [
        format!("token_media_id = '{PORTRAIT}'"),
        format!("portrait_media_id = '{JETON}'"),
        "token_media_id = NULL".to_string(),
        "portrait_media_id = NULL".to_string(),
    ] {
        s.refused_by(
            &format!("UPDATE character SET {change} WHERE entity_id = '{ELOWEN}'"),
            "every-change-versioned",
        )
        .await;
    }
    // Nor does a media's deletion excuse the rest of what its transaction
    // writes to the sheet — beside it, or in the same row's write.
    for write in [
        "UPDATE character SET hp_max = 30",
        "UPDATE character SET hp_max = 30, token_media_id = 'a1000000-0000-4000-8000-000000000002'",
    ] {
        s.refused_by(
            &format!(
                "{write} WHERE entity_id = '{ELOWEN}'; DELETE FROM media WHERE id = '{JETON}'"
            ),
            "every-change-versioned",
        )
        .await;
    }
    // A write that changes nothing is a write all the same.
    s.refused_by(
        &format!(
            "UPDATE character SET token_media_id = token_media_id WHERE entity_id = '{ELOWEN}'"
        ),
        "every-change-versioned",
    )
    .await;
    s.accepted(&format!(
        "UPDATE character SET token_media_id = NULL WHERE entity_id = '{ELOWEN}'; {}",
        version(ELOWEN, 2)
    ))
    .await;
    // A token given to a sheet that had none.
    s.refused_by(
        &format!("UPDATE character SET token_media_id = '{JETON}' WHERE entity_id = '{ELOWEN}'"),
        "every-change-versioned",
    )
    .await;
    s.drop().await;
}

#[tokio::test]
async fn default_token_drops_the_crop_with_the_portrait_its_media_took_away() {
    let s = Scratch::new("crop").await;
    let crop = format!(
        "SELECT coalesce(token_crop::text, 'none') FROM character WHERE entity_id = '{ELOWEN}'"
    );
    s.accepted(&format!(
        "UPDATE character SET token_crop = '{{\"x\": 0.2}}' WHERE entity_id = '{ELOWEN}'; {}",
        version(ELOWEN, 2)
    ))
    .await;
    // Its token's own media going leaves the crop of the portrait as it is.
    s.accepted(&format!("DELETE FROM media WHERE id = '{JETON}'"))
        .await;
    assert_eq!(s.texts(&crop).await, ["{\"x\": 0.2}"]);
    // A portrait cleared by hand under its crop is still refused.
    s.refused_by_check(
        &format!(
            "UPDATE character SET portrait_media_id = NULL WHERE entity_id = '{ELOWEN}'; {}",
            version(ELOWEN, 3)
        ),
        "character_default_token_crop",
    )
    .await;
    // Its media deleted, the portrait goes with its crop: the token is the
    // disc of its colour.
    s.accepted(&format!("DELETE FROM media WHERE id = '{PORTRAIT}'"))
        .await;
    assert_eq!(s.texts(&crop).await, ["none"]);
    s.drop().await;
}

#[tokio::test]
async fn deleting_a_place_leaves_its_media_with_none() {
    let s = Scratch::new("place").await;
    s.accepted(&format!("DELETE FROM entity WHERE id = '{TAVERNE}'"))
        .await;
    assert_eq!(
        s.texts(&format!(
            "SELECT coalesce(place_id::text, 'none') FROM media WHERE id = '{CARTE}'"
        ))
        .await,
        ["none"]
    );
    s.drop().await;
}

#[tokio::test]
async fn grid_only_for_battlemap_wants_a_battlemap_uploaded_with_its_grid() {
    let s = Scratch::new("grid_upload").await;
    let carte = "a1000000-0000-4000-8000-0000000000f1";
    s.refused_by(
        &media(carte, VALOMBRE, "battlemap", "NULL"),
        "grid-only-for-battlemap",
    )
    .await;
    // Its grid in the same transaction, it is uploaded.
    s.accepted(&format!(
        "{}{}",
        media(carte, VALOMBRE, "battlemap", "NULL"),
        grid(carte, "70", "0.5")
    ))
    .await;
    // Its grid leaves it only with it.
    s.refused_by(
        &format!("DELETE FROM battlemap_grid WHERE media_id = '{carte}'"),
        "grid-only-for-battlemap",
    )
    .await;
    // Nor moves to another media, leaving this one without.
    s.refused_by(
        &format!("UPDATE battlemap_grid SET media_id = '{PORTRAIT}' WHERE media_id = '{carte}'"),
        "grid-only-for-battlemap",
    )
    .await;
    s.drop().await;
}

#[tokio::test]
async fn grid_only_for_battlemap_refuses_a_grid_to_any_other_media() {
    let s = Scratch::new("grid_other").await;
    s.refused_by(&grid(PORTRAIT, "64", "0.4"), "grid-only-for-battlemap")
        .await;
    // A battlemap made an illustration keeps a grid it may not have.
    s.refused_by(
        &format!("UPDATE media SET kind = 'illustration' WHERE id = '{CARTE}'"),
        "grid-only-for-battlemap",
    )
    .await;
    // An illustration made a battlemap has none yet.
    s.refused_by(
        &format!("UPDATE media SET kind = 'battlemap' WHERE id = '{PORTRAIT}'"),
        "grid-only-for-battlemap",
    )
    .await;
    // Both changed together, the kinds and the grids agree.
    s.accepted(&format!(
        "DELETE FROM battlemap_grid WHERE media_id = '{CARTE}'; \
         UPDATE media SET kind = 'illustration' WHERE id = '{CARTE}'; \
         UPDATE media SET kind = 'battlemap' WHERE id = '{PORTRAIT}'; {}",
        grid(PORTRAIT, "64", "0.4")
    ))
    .await;
    // Only two kinds of media.
    s.refused_by_check(
        &format!("UPDATE media SET kind = 'son' WHERE id = '{JETON}'"),
        "media_kind",
    )
    .await;
    s.drop().await;
}

#[tokio::test]
async fn hex_is_five_feet_whatever_the_hex_s_size_offset_or_orientation() {
    let s = Scratch::new("five_feet").await;
    s.accepted(&format!(
        "UPDATE battlemap_grid SET hex_size_px = 133.33, offset_x = -40, offset_y = 7.25, \
         orientation = 'plat' WHERE media_id = '{CARTE}'"
    ))
    .await;
    assert_eq!(
        s.texts("SELECT feet_per_hex::text FROM battlemap_grid ORDER BY media_id")
            .await,
        ["5", "5"]
    );
    // Generated: no one writes it, not even 5.
    for feet in ["10", "5"] {
        let (code, _, message) = s
            .refused(&format!(
                "UPDATE battlemap_grid SET feet_per_hex = {feet} WHERE media_id = '{CARTE}'"
            ))
            .await;
        assert_eq!(code, SqlState::GENERATED_ALWAYS, "{message}");
    }
    // And its CHECK says so under the invariant's id.
    assert_eq!(
        s.texts(
            "SELECT pg_get_constraintdef(oid) FROM pg_constraint \
             WHERE conrelid = 'battlemap_grid'::regclass AND conname = 'hex_is_five_feet'"
        )
        .await,
        ["CHECK ((feet_per_hex = 5))"]
    );
    s.drop().await;
}

#[tokio::test]
async fn grid_values_range_wants_a_hex_above_0_and_an_opacity_from_0_to_1() {
    let s = Scratch::new("range").await;
    for change in [
        "hex_size_px = 0",
        "hex_size_px = -12",
        "opacity = 1.01",
        "opacity = -0.01",
    ] {
        s.refused_by_check(
            &format!("UPDATE battlemap_grid SET {change} WHERE media_id = '{CARTE}'"),
            "grid_values_range",
        )
        .await;
    }
    for change in ["hex_size_px = 0.01, opacity = 0", "opacity = 1"] {
        s.accepted(&format!(
            "UPDATE battlemap_grid SET {change} WHERE media_id = '{CARTE}'"
        ))
        .await;
    }
    s.refused_by_check(
        &format!("UPDATE battlemap_grid SET orientation = 'oblique' WHERE media_id = '{CARTE}'"),
        "battlemap_grid_orientation",
    )
    .await;
    s.drop().await;
}

#[tokio::test]
async fn media_place_is_lieu_refuses_a_place_that_is_no_lieu_of_the_campaign() {
    let s = Scratch::new("place_kind").await;
    for place in [MORLOX, ELOWEN, PHARE] {
        s.refused_by(
            &format!("UPDATE media SET place_id = '{place}' WHERE id = '{PORTRAIT}'"),
            "media-place-is-lieu",
        )
        .await;
    }
    // A media moved to another campaign leaves its place behind.
    s.refused_by(
        &format!("UPDATE media SET campaign_id = '{BRUME}' WHERE id = '{CARTE}'"),
        "media-place-is-lieu",
    )
    .await;
    s.accepted(&format!(
        "UPDATE media SET campaign_id = '{BRUME}', place_id = '{PHARE}' WHERE id = '{CARTE}'"
    ))
    .await;
    s.accepted(&format!(
        "UPDATE media SET place_id = NULL WHERE id = '{ECUEIL}'"
    ))
    .await;
    s.drop().await;
}

#[tokio::test]
async fn media_place_is_lieu_refuses_a_lieu_made_otherwise_under_its_media() {
    let s = Scratch::new("place_change").await;
    s.refused_by(
        &format!(
            "UPDATE entity SET type = 'evenement' WHERE id = '{TAVERNE}'; {}",
            version(TAVERNE, 2)
        ),
        "media-place-is-lieu",
    )
    .await;
    s.refused_by(
        &format!(
            "UPDATE entity SET campaign_id = '{BRUME}' WHERE id = '{TAVERNE}'; {}",
            version(TAVERNE, 2)
        ),
        "media-place-is-lieu",
    )
    .await;
    // A lieu no media is set at changes freely.
    s.accepted(&format!(
        "UPDATE media SET place_id = NULL WHERE id = '{CARTE}'; \
         UPDATE entity SET type = 'evenement' WHERE id = '{TAVERNE}'; {}",
        version(TAVERNE, 2)
    ))
    .await;
    s.drop().await;
}
