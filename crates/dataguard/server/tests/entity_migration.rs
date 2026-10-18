//! The Entity migration (`crates/data/migrations/0004_entity.sql`) against
//! the harness's PostgreSQL (`DATABASE_URL`): replayed over the campaign and
//! player tables it needs, it is what `data/model.json` says it is, views
//! included; its relations cascade, nullify and restrict as the aggregate
//! declares; and the tables refuse, under its id, what breaks an invariant
//! the engine cannot judge on one row — the player's campaign, the other
//! elements' aliases, the versions written. Each test works in a scratch
//! database of its own, dropped when it ends.

use std::collections::HashMap;

use serde_json::{Value, json};
use tokio_postgres::error::SqlState;
use tokio_postgres::{Client, NoTls};

const CAMPAIGN: &str = include_str!("../../../data/migrations/0002_campaign.sql");
const PLAYER: &str = include_str!("../../../data/migrations/0003_player.sql");
const MIGRATION: &str = include_str!("../../../data/migrations/0004_entity.sql");

const VALOMBRE: &str = "c1000000-0000-4000-8000-000000000001";
const BRUME: &str = "c1000000-0000-4000-8000-000000000002";
const CLAIRE: &str = "b1000000-0000-4000-8000-000000000001";
const INES: &str = "b1000000-0000-4000-8000-000000000002";
const ELOWEN: &str = "e1000000-0000-4000-8000-000000000001";
const TAVERNE: &str = "e1000000-0000-4000-8000-000000000002";
const MORLOX: &str = "e1000000-0000-4000-8000-000000000003";
const ORAGE: &str = "e1000000-0000-4000-8000-000000000004";
const PHARE: &str = "e1000000-0000-4000-8000-000000000005";

/// The tables the migration creates, in its order.
const TABLES: [&str; 4] = ["entity", "character", "entity_alias", "entity_version"];
/// The views it creates.
const VIEWS: [&str; 2] = ["detection_vocabulary_view", "campaign_overview_view"];

fn url() -> String {
    std::env::var("DATABASE_URL").expect("DATABASE_URL names the harness's PostgreSQL")
}

async fn connect(config: &tokio_postgres::Config) -> Client {
    let (client, connection) = config.connect(NoTls).await.expect("PostgreSQL");
    tokio::spawn(connection);
    client
}

/// The SQL adding version `number` of `element` from `source`, its diff
/// `diff`.
fn version(element: &str, number: u32, source: &str, diff: &Value) -> String {
    format!(
        "INSERT INTO entity_version (entity_id, version_number, source, snapshot, diff) \
         VALUES ('{element}', {number}, '{source}', '{{}}', '{diff}');"
    )
}

/// The SQL writing a change to `element` with its version, from the DM.
fn saved(element: &str, number: u32, change: &str) -> String {
    format!("{change}; {}", version(element, number, "dm", &json!([])))
}

/// A database of its own, holding the campaign and player tables, migrated
/// once. Valombre holds Elowen (Claire's PC, alias « la rodeuse »), the
/// Taverne du Sanglier (alias « la taverne »), Morlox (an NPC) and the storm
/// (an event); Brume, Inès and the lighthouse, whose alias « la rodeuse » is
/// another campaign's. Each element is at version 1.
struct Scratch {
    client: Client,
    name: String,
}

impl Scratch {
    async fn new(name: &str) -> Scratch {
        let name = format!("entity_test_{name}_{}", std::process::id());
        let config: tokio_postgres::Config = url().parse().expect("a PostgreSQL URL");
        let admin = connect(&config).await;
        for sql in [
            format!("DROP DATABASE IF EXISTS {name} WITH (FORCE)"),
            format!("CREATE DATABASE {name}"),
        ] {
            admin.batch_execute(&sql).await.expect("a scratch database");
        }
        let client = connect(config.clone().dbname(&name)).await;
        for sql in [CAMPAIGN, PLAYER, MIGRATION] {
            client.batch_execute(sql).await.expect("migrated");
        }
        let none = json!([]);
        client
            .batch_execute(&format!(
                "INSERT INTO campaign (id, name, kind) VALUES \
                 ('{VALOMBRE}', 'Les Cendres de Valombre', 'campagne'), \
                 ('{BRUME}', 'La Brume', 'oneshot'); \
                 INSERT INTO player (id, campaign_id, name) VALUES \
                 ('{CLAIRE}', '{VALOMBRE}', 'Claire'), ('{INES}', '{BRUME}', 'Inès'); \
                 INSERT INTO entity (id, campaign_id, type, name) VALUES \
                 ('{ELOWEN}', '{VALOMBRE}', 'pj', 'Elowen'), \
                 ('{TAVERNE}', '{VALOMBRE}', 'lieu', 'La Taverne du Sanglier'), \
                 ('{MORLOX}', '{VALOMBRE}', 'pnj', 'Morlox'), \
                 ('{ORAGE}', '{VALOMBRE}', 'evenement', 'L''orage de la Saint-Jean'), \
                 ('{PHARE}', '{BRUME}', 'lieu', 'Le Phare'); \
                 INSERT INTO character (entity_id, player_id, dexterity, hp_max, token_color) VALUES \
                 ('{ELOWEN}', '{CLAIRE}', 18, 24, '#2f7d5b'), ('{MORLOX}', NULL, 12, 18, '#a33'); \
                 INSERT INTO entity_alias (entity_id, alias) VALUES \
                 ('{ELOWEN}', 'la rodeuse'), ('{TAVERNE}', 'la taverne'), ('{PHARE}', 'la rodeuse'); \
                 {}{}{}{}{}",
                version(ELOWEN, 1, "dm", &none),
                version(TAVERNE, 1, "dm", &none),
                version(MORLOX, 1, "brief", &none),
                version(ORAGE, 1, "dm", &none),
                version(PHARE, 1, "dm", &none),
            ))
            .await
            .expect("two campaigns, their players and their elements");
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

    /// Runs `sql` in one transaction, which the database refuses under the
    /// invariant `id`.
    async fn refused_by(&self, sql: &str, id: &str) {
        let (_, constraint, message) = self.refused(sql).await;
        assert_eq!(
            constraint.as_deref(),
            Some(id.replace('-', "_").as_str()),
            "{sql}: {message}"
        );
        assert!(message.starts_with(&format!("{id}: ")), "{message}");
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

    /// Every row of the migration's tables, as text: what a replay must
    /// leave as it was.
    async fn snapshot(&self) -> Vec<String> {
        let mut rows = Vec::new();
        for table in TABLES {
            rows.extend(
                self.texts(&format!(
                    "SELECT row_to_json(t)::text FROM {table} t ORDER BY 1"
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

    async fn count(&self, sql: &str) -> i64 {
        self.client
            .query_one(sql, &[])
            .await
            .unwrap_or_else(|e| panic!("{sql}: {e:?}"))
            .get(0)
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

/// `relation` as `data/model.json` describes it: each field's name, type,
/// nullability and reference, in its order.
fn modelled(relation: &str) -> Vec<(String, String, bool, Option<String>)> {
    let model: Value = serde_json::from_str(include_str!("../../../../data/model.json"))
        .expect("data/model.json is JSON");
    let table = model["tables"]
        .as_array()
        .expect("tables")
        .iter()
        .find(|table| table["name"] == relation)
        .unwrap_or_else(|| panic!("data/model.json describes {relation}"));
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

#[tokio::test]
async fn the_migration_replays_as_a_no_op_over_the_campaign_and_player_tables() {
    let mut s = Scratch::new("replay").await;
    let first = s.snapshot().await;
    assert_eq!(first.len(), 5 + 2 + 3 + 5);
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
    assert!(at("0003_player.sql") < at("0004_entity.sql"), "{applied:?}");
    assert_eq!(s.snapshot().await, first);
    // A trigger dropped and created again is there once, as it was. Counted
    // on the tables this migration puts its triggers on: a later migration's
    // own are not this one's.
    assert_eq!(
        s.count(
            "SELECT count(*) FROM pg_trigger WHERE NOT tgisinternal AND tgrelid = ANY (ARRAY[\
             'entity', 'character', 'entity_alias', 'entity_version', 'player']::regclass[])"
        )
        .await,
        10
    );
    s.drop().await;
}

#[tokio::test]
async fn the_tables_and_views_are_what_data_model_json_describes() {
    let s = Scratch::new("model").await;
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
        assert_eq!(real, modelled(table), "{table}");
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
        let model: Vec<(String, String)> = modelled(view)
            .into_iter()
            .map(|(name, ty, _, references)| {
                assert_eq!(references, None, "{view}.{name}");
                (name, ty)
            })
            .collect();
        assert_eq!(real, model, "{view}");
    }
    s.drop().await;
}

#[tokio::test]
async fn each_relation_deletes_and_updates_as_the_aggregate_declares() {
    let s = Scratch::new("relations").await;
    let key = |column: &str, to: &str, delete: &str, update: &str| {
        (
            column.to_string(),
            to.to_string(),
            delete.to_string(),
            update.to_string(),
        )
    };
    // c cascade, n nullify (SET NULL), r restrict; propagate is CASCADE.
    assert_eq!(
        s.foreign_keys("entity").await,
        [key("campaign_id", "campaign.id", "c", "c")]
    );
    assert_eq!(
        s.foreign_keys("character").await,
        [
            key("entity_id", "entity.id", "c", "c"),
            key("player_id", "player.id", "n", "c"),
        ]
    );
    assert_eq!(
        s.foreign_keys("entity_alias").await,
        [key("entity_id", "entity.id", "c", "c")]
    );
    assert_eq!(
        s.foreign_keys("entity_version").await,
        [
            key("entity_id", "entity.id", "c", "r"),
            key("restored_from_version_id", "entity_version.id", "n", "r"),
        ]
    );
    s.drop().await;
}

#[tokio::test]
async fn deleting_a_campaign_deletes_its_elements_and_all_they_hold_and_theirs_alone() {
    let s = Scratch::new("cascade").await;
    s.accepted(&format!("DELETE FROM campaign WHERE id = '{VALOMBRE}'"))
        .await;
    for table in TABLES {
        let left = s
            .count(&format!(
                "SELECT count(*) FROM {table} t WHERE row_to_json(t)::text LIKE '%{PHARE}%' \
                 OR row_to_json(t)::text LIKE '%{BRUME}%'"
            ))
            .await;
        let all = s.count(&format!("SELECT count(*) FROM {table}")).await;
        assert_eq!(all, left, "{table} keeps a row of Valombre's");
        assert!(table == "character" || all > 0, "{table} lost Brume's rows");
    }
    // An element deleted takes its sheet, its aliases and its versions.
    s.accepted(&format!("DELETE FROM entity WHERE id = '{PHARE}'"))
        .await;
    for table in TABLES {
        assert_eq!(
            s.count(&format!("SELECT count(*) FROM {table}")).await,
            0,
            "{table}"
        );
    }
    s.drop().await;
}

#[tokio::test]
async fn a_campaign_s_new_id_reaches_its_elements_and_players_together() {
    let s = Scratch::new("propagate").await;
    let moved = "c1000000-0000-4000-8000-0000000000ff";
    // Claire's PC and Claire move at once: their campaign stays one.
    s.accepted(&format!(
        "UPDATE campaign SET id = '{moved}' WHERE id = '{VALOMBRE}'"
    ))
    .await;
    assert_eq!(
        s.count(&format!(
            "SELECT count(*) FROM entity WHERE campaign_id = '{moved}'"
        ))
        .await,
        4
    );
    s.drop().await;
}

#[tokio::test]
async fn a_player_s_new_id_reaches_their_pc_and_their_deletion_leaves_it_with_none() {
    let s = Scratch::new("nullify").await;
    let player = format!(
        "SELECT coalesce(player_id::text, 'none') FROM character WHERE entity_id = '{ELOWEN}'"
    );
    let moved = "b1000000-0000-4000-8000-0000000000ff";
    // The relation's doing, no one's save: no version is asked for.
    s.accepted(&format!(
        "UPDATE player SET id = '{moved}' WHERE id = '{CLAIRE}'"
    ))
    .await;
    assert_eq!(s.texts(&player).await, [moved]);
    s.accepted(&format!("DELETE FROM player WHERE id = '{moved}'"))
        .await;
    assert_eq!(s.texts(&player).await, ["none"]);
    s.drop().await;
}

#[tokio::test]
async fn an_element_with_a_history_keeps_its_id() {
    let s = Scratch::new("restrict").await;
    let (code, ..) = s
        .refused(&format!(
            "UPDATE entity SET id = gen_random_uuid() WHERE id = '{ELOWEN}'"
        ))
        .await;
    assert_eq!(code, SqlState::FOREIGN_KEY_VIOLATION);
    s.drop().await;
}

#[tokio::test]
async fn character_is_pj_or_pnj_refuses_a_sheet_for_a_place_or_an_event() {
    let s = Scratch::new("sheet_kind").await;
    for element in [TAVERNE, ORAGE] {
        s.refused_by(
            &saved(
                element,
                2,
                &format!(
                    "INSERT INTO character (entity_id, token_color) VALUES ('{element}', '#888')"
                ),
            ),
            "character-is-pj-or-pnj",
        )
        .await;
    }
    // Nor does a character become a place with its sheet.
    s.refused_by(
        &saved(
            MORLOX,
            2,
            &format!("UPDATE entity SET type = 'lieu' WHERE id = '{MORLOX}'"),
        ),
        "character-is-pj-or-pnj",
    )
    .await;
    s.drop().await;
}

#[tokio::test]
async fn character_is_pj_or_pnj_refuses_an_npc_s_player_or_another_campaign_s() {
    let s = Scratch::new("sheet_player").await;
    s.refused_by(
        &saved(
            MORLOX,
            2,
            &format!("UPDATE character SET player_id = '{CLAIRE}' WHERE entity_id = '{MORLOX}'"),
        ),
        "character-is-pj-or-pnj",
    )
    .await;
    s.refused_by(
        &saved(
            ELOWEN,
            2,
            &format!("UPDATE entity SET type = 'pnj' WHERE id = '{ELOWEN}'"),
        ),
        "character-is-pj-or-pnj",
    )
    .await;
    // Inès plays at Brume's table, not Valombre's.
    s.refused_by(
        &saved(
            ELOWEN,
            2,
            &format!("UPDATE character SET player_id = '{INES}' WHERE entity_id = '{ELOWEN}'"),
        ),
        "character-is-pj-or-pnj",
    )
    .await;
    // Nor can Claire leave for Brume while her PC stays.
    s.refused_by(
        &format!("UPDATE player SET campaign_id = '{BRUME}' WHERE id = '{CLAIRE}'"),
        "character-is-pj-or-pnj",
    )
    .await;
    // A PC turned NPC gives its player up in the same save.
    s.accepted(&saved(
        ELOWEN,
        2,
        &format!(
            "UPDATE entity SET type = 'pnj' WHERE id = '{ELOWEN}'; \
             UPDATE character SET player_id = NULL WHERE entity_id = '{ELOWEN}'"
        ),
    ))
    .await;
    s.drop().await;
}

#[tokio::test]
async fn alias_unique_in_campaign_refuses_an_alias_another_element_has_once_normalised() {
    let s = Scratch::new("alias").await;
    for alias in [
        "la Rôdeuse",
        "  LA RODEUSE\u{a0}",
        "La Taverne du Sanglier",
        "MORLOX",
    ] {
        let element = if alias == "MORLOX" { ELOWEN } else { MORLOX };
        s.refused_by(
            &saved(
                element,
                2,
                &format!(
                    "INSERT INTO entity_alias (entity_id, alias) VALUES ('{element}', '{alias}')"
                ),
            ),
            "alias-unique-in-campaign",
        )
        .await;
    }
    // Nor does a name double another element's alias.
    s.refused_by(
        &saved(
            MORLOX,
            2,
            &format!("UPDATE entity SET name = 'La Rôdeuse' WHERE id = '{MORLOX}'"),
        ),
        "alias-unique-in-campaign",
    )
    .await;
    // Nor an element moved into a campaign whose element has its alias.
    s.refused_by(
        &format!("UPDATE entity SET campaign_id = '{VALOMBRE}' WHERE id = '{PHARE}'"),
        "alias-unique-in-campaign",
    )
    .await;
    // Brume's lighthouse is « la rodeuse » too, in its own campaign; Elowen
    // may be named so twice, and a blank alias names nothing.
    s.accepted(&saved(
        ELOWEN,
        2,
        &format!(
            "INSERT INTO entity_alias (entity_id, alias) VALUES \
             ('{ELOWEN}', 'La Rôdeuse'), ('{ELOWEN}', 'Elowen des marais'), \
             ('{ELOWEN}', '  ')"
        ),
    ))
    .await;
    s.drop().await;
}

#[tokio::test]
async fn every_change_versioned_refuses_to_modify_a_version() {
    let s = Scratch::new("immutable").await;
    for change in [
        "source = 'ia'",
        "snapshot = '{\"name\": \"Elowen\"}'",
        "diff = '[{\"field\": \"name\", \"old\": null, \"new\": \"Elowen\"}]'",
        "version_number = 7",
        "created_at = now() - interval '1 day'",
    ] {
        s.refused_by(
            &format!("UPDATE entity_version SET {change} WHERE entity_id = '{ELOWEN}'"),
            "every-change-versioned",
        )
        .await;
    }
    s.drop().await;
}

#[tokio::test]
async fn every_change_versioned_refuses_a_change_with_no_version_or_out_of_turn() {
    let s = Scratch::new("versioned").await;
    for change in [
        format!("UPDATE entity SET notes = 'Cache une lettre.' WHERE id = '{ELOWEN}'"),
        format!("UPDATE character SET hp_max = 26 WHERE entity_id = '{ELOWEN}'"),
        format!("INSERT INTO entity_alias (entity_id, alias) VALUES ('{ELOWEN}', 'l''archère')"),
        format!("DELETE FROM entity_alias WHERE entity_id = '{ELOWEN}'"),
        format!("DELETE FROM character WHERE entity_id = '{MORLOX}'"),
        format!(
            "INSERT INTO entity (campaign_id, type, name) VALUES ('{VALOMBRE}', 'pnj', 'Vesna')"
        ),
    ] {
        s.refused_by(&change, "every-change-versioned").await;
    }
    // Version 3 while the last is 1, or 1 again.
    for number in [3, 1] {
        s.refused_by(
            &version(ELOWEN, number, "dm", &json!([])),
            "every-change-versioned",
        )
        .await;
    }
    s.accepted(&saved(
        ELOWEN,
        2,
        &format!("UPDATE entity SET notes = 'Cache une lettre.' WHERE id = '{ELOWEN}'"),
    ))
    .await;
    s.drop().await;
}

/// The id of `element`'s version `number`.
async fn version_id(s: &Scratch, element: &str, number: u32) -> String {
    s.texts(&format!(
        "SELECT id::text FROM entity_version WHERE entity_id = '{element}' AND version_number = {number}"
    ))
    .await
    .remove(0)
}

/// The SQL adding a restoration of `element` as version `number`, restoring
/// `restored` with the changes `diff`.
fn restoration(element: &str, number: u32, restored: &str, diff: &Value) -> String {
    format!(
        "INSERT INTO entity_version (entity_id, version_number, source, snapshot, diff, restored_from_version_id) \
         VALUES ('{element}', {number}, 'restauration', '{{}}', '{diff}', '{restored}')"
    )
}

#[tokio::test]
async fn restore_exact_rewrites_the_restored_version_s_values_and_nothing_else() {
    let s = Scratch::new("restore").await;
    // Version 2 of Elowen notes a letter and has 24 HP; version 1 is empty.
    s.accepted(&format!(
        "UPDATE entity SET notes = 'Cache une lettre.' WHERE id = '{ELOWEN}'; \
         INSERT INTO entity_version (entity_id, version_number, source, snapshot, diff) VALUES \
         ('{ELOWEN}', 2, 'dm', '{{\"name\": \"Elowen\", \"notes\": \"Cache une lettre.\", \"hpMax\": 24}}', \
          '[{{\"field\": \"notes\", \"old\": null, \"new\": \"Cache une lettre.\"}}]')"
    ))
    .await;
    let first = version_id(&s, ELOWEN, 1).await;
    let second = version_id(&s, ELOWEN, 2).await;
    let morlox = version_id(&s, MORLOX, 1).await;
    let undo = json!([{ "field": "notes", "old": "Cache une lettre.", "new": null }]);
    // A restoration names what it restores: an earlier version of its own.
    s.refused_by(&restoration(ELOWEN, 3, &morlox, &undo), "restore-exact")
        .await;
    s.refused_by(
        &format!(
            "INSERT INTO entity_version (entity_id, version_number, source, snapshot, diff) \
             VALUES ('{ELOWEN}', 3, 'restauration', '{{}}', '{undo}')"
        ),
        "restore-exact",
    )
    .await;
    // Version 1's snapshot is empty: what a restoration of it writes is a
    // null notes, not another value.
    s.refused_by(
        &restoration(
            ELOWEN,
            3,
            &first,
            &json!([{ "field": "notes", "old": "Cache une lettre.", "new": "Autre chose." }]),
        ),
        "restore-exact",
    )
    .await;
    // Undoing version 2's change restores version 1's value.
    s.accepted(&format!(
        "UPDATE entity SET notes = NULL WHERE id = '{ELOWEN}'; {}",
        restoration(ELOWEN, 3, &first, &undo)
    ))
    .await;
    // Going back to version 2 rewrites its notes and HP as they were, 24.0
    // being 24.
    s.accepted(&restoration(
        ELOWEN,
        4,
        &second,
        &json!([
            { "field": "hpMax", "old": 24, "new": 24.0 },
            { "field": "notes", "old": null, "new": "Cache une lettre." },
        ]),
    ))
    .await;
    // Only a restoration restores.
    let (code, constraint, _) = s
        .refused(&format!(
            "INSERT INTO entity_version (entity_id, version_number, source, snapshot, diff, restored_from_version_id) \
             VALUES ('{ELOWEN}', 5, 'dm', '{{}}', '[]', '{first}')"
        ))
        .await;
    assert_eq!(code, SqlState::CHECK_VIOLATION);
    assert_eq!(
        constraint.as_deref(),
        Some("entity_version_restored_by_restoration")
    );
    s.drop().await;
}

#[tokio::test]
async fn every_change_versioned_lets_a_version_go_with_its_element_alone() {
    let s = Scratch::new("version_delete").await;
    let first = version_id(&s, ELOWEN, 1).await;
    s.accepted(&restoration(ELOWEN, 2, &first, &json!([])))
        .await;
    // A version erased is a history changed: the restoration, which nothing
    // restores, as much as the version it restores.
    let restored = version_id(&s, ELOWEN, 2).await;
    for id in [&restored, &first] {
        s.refused_by(
            &format!("DELETE FROM entity_version WHERE id = '{id}'"),
            "every-change-versioned",
        )
        .await;
    }
    // The element deleted takes its versions, the restoration and the version
    // it restores (its relation's nullify) together.
    s.accepted(&format!("DELETE FROM entity WHERE id = '{ELOWEN}'"))
        .await;
    assert_eq!(
        s.count(&format!(
            "SELECT count(*) FROM entity_version WHERE entity_id = '{ELOWEN}'"
        ))
        .await,
        0
    );
    s.drop().await;
}

#[tokio::test]
async fn every_change_versioned_wants_both_elements_versioned_when_an_alias_moves() {
    let s = Scratch::new("alias_moves").await;
    let moved =
        format!("UPDATE entity_alias SET entity_id = '{MORLOX}' WHERE entity_id = '{ELOWEN}'");
    for versions in [
        String::new(),
        version(ELOWEN, 2, "dm", &json!([])),
        version(MORLOX, 2, "dm", &json!([])),
    ] {
        s.refused_by(&format!("{moved}; {versions}"), "every-change-versioned")
            .await;
    }
    s.accepted(&format!(
        "{moved}; {}{}",
        version(ELOWEN, 2, "dm", &json!([])),
        version(MORLOX, 2, "dm", &json!([]))
    ))
    .await;
    s.drop().await;
}

#[tokio::test]
async fn every_change_versioned_counts_a_version_a_savepoint_kept_and_not_one_rolled_back() {
    let s = Scratch::new("savepoint").await;
    let change = format!("UPDATE entity SET notes = 'Cache une lettre.' WHERE id = '{ELOWEN}'");
    let added = version(ELOWEN, 2, "dm", &json!([]));
    s.accepted(&format!(
        "BEGIN; {change}; SAVEPOINT save; {added} RELEASE SAVEPOINT save; COMMIT"
    ))
    .await;
    let again = version(ELOWEN, 3, "dm", &json!([]));
    s.refused_by(
        &format!("BEGIN; {change}; SAVEPOINT save; {again} ROLLBACK TO SAVEPOINT save; COMMIT"),
        "every-change-versioned",
    )
    .await;
    s.drop().await;
}

#[tokio::test]
async fn every_change_versioned_wants_each_change_to_name_its_field() {
    let s = Scratch::new("diff_field").await;
    for diff in [
        json!([{ "old": null, "new": "Elowen" }]),
        json!([{ "field": null, "old": null, "new": 1 }]),
        json!([{ "field": 3, "old": null, "new": 1 }]),
        json!(["name"]),
    ] {
        s.refused_by(
            &version(ELOWEN, 2, "dndbeyond", &diff),
            "every-change-versioned",
        )
        .await;
    }
    s.drop().await;
}

/// A version of Elowen from `source` changing `field`.
fn changing(source: &str, field: &str) -> String {
    let diff = json!([{ "field": field, "old": null, "new": 1 }]);
    version(ELOWEN, 2, source, &diff)
}

#[tokio::test]
async fn dndbeyond_resync_scope_refuses_a_resync_changing_aliases_notes_token_or_portrait() {
    let s = Scratch::new("dndbeyond").await;
    for field in [
        "aliases",
        "notes",
        "tokenColor",
        "tokenMediaId",
        "portraitMediaId",
        "name",
    ] {
        s.refused_by(&changing("dndbeyond", field), "dndbeyond-resync-scope")
            .await;
    }
    let diff = json!([
        { "field": "actions", "old": [], "new": [{ "name": "Arc long" }] },
        { "field": "armorClass", "old": 15, "new": 16 },
        { "field": "dexterity", "old": 18, "new": 19 },
        { "field": "hpMax", "old": 24, "new": 26 },
        { "field": "spells", "old": [], "new": [{ "name": "Marque du chasseur" }] },
    ]);
    s.accepted(&version(ELOWEN, 2, "dndbeyond", &diff)).await;
    s.drop().await;
}

#[tokio::test]
async fn proposal_fills_sheet_fields_refuses_a_proposal_writing_anything_else() {
    let s = Scratch::new("proposal").await;
    for field in ["notes", "spells", "aliases", "portraitMediaId", "name"] {
        s.refused_by(&changing("ia", field), "proposal-fills-sheet-fields")
            .await;
    }
    let diff = json!([
        { "field": "actions", "old": [], "new": [{ "name": "Dague" }] },
        { "field": "background", "old": null, "new": "Née dans les marais." },
        { "field": "behavior", "old": null, "new": "Taciturne." },
        { "field": "hpMax", "old": 24, "new": 22 },
        { "field": "strength", "old": null, "new": 12 },
    ]);
    s.accepted(&version(ELOWEN, 2, "ia", &diff)).await;
    s.drop().await;
}

#[tokio::test]
async fn default_token_wants_a_colour_and_a_portrait_under_a_crop() {
    let s = Scratch::new("token").await;
    for (change, code, constraint) in [
        (
            "token_crop = '{\"x\": 0.2, \"y\": 0.1, \"size\": 0.5}'",
            SqlState::CHECK_VIOLATION,
            Some("character_default_token_crop"),
        ),
        (
            "token_color = ' '",
            SqlState::CHECK_VIOLATION,
            Some("character_default_token_color"),
        ),
        ("token_color = NULL", SqlState::NOT_NULL_VIOLATION, None),
    ] {
        let (got, named, message) = s
            .refused(&saved(
                MORLOX,
                2,
                &format!("UPDATE character SET {change} WHERE entity_id = '{MORLOX}'"),
            ))
            .await;
        assert_eq!((got, named.as_deref()), (code, constraint), "{message}");
    }
    s.accepted(&saved(
        MORLOX,
        2,
        &format!(
            "UPDATE character SET portrait_media_id = gen_random_uuid(), \
             token_crop = '{{\"x\": 0.2, \"y\": 0.1, \"size\": 0.5}}' WHERE entity_id = '{MORLOX}'"
        ),
    ))
    .await;
    s.drop().await;
}

#[tokio::test]
async fn dex_modifier_derived_no_column_stores_it_and_dex_18_gives_4() {
    let s = Scratch::new("dex").await;
    let columns: Vec<String> = s
        .columns("character")
        .await
        .into_iter()
        .map(|c| c.0)
        .collect();
    assert!(
        !columns.iter().any(|c| c.contains("modifier")),
        "{columns:?}"
    );
    // Elowen's DEX 18, as stored, reads as +4 — AbilityModifier@1's case.
    let modifier: i32 = s
        .client
        .query_one(
            &format!(
                "SELECT floor((dexterity - 10) / 2.0)::int FROM character WHERE entity_id = '{ELOWEN}'"
            ),
            &[],
        )
        .await
        .expect("the sheet read")
        .get(0);
    assert_eq!(modifier, 4);
    let fixtures = platform_conformance::fixtures_of("AbilityModifier@1");
    let mut read = HashMap::new();
    for case in &fixtures.cases {
        let score = i16::try_from(case.input["score"].as_i64().expect("a score")).expect("small");
        let modifier: i32 = s
            .client
            .query_one("SELECT floor(($1::smallint - 10) / 2.0)::int", &[&score])
            .await
            .expect("computed")
            .get(0);
        read.insert(case.input.to_string(), json!({ "modifier": modifier }));
    }
    platform_conformance::check("AbilityModifier@1", |input| {
        read[&input.to_string()].clone()
    });
    s.drop().await;
}

#[tokio::test]
async fn alias_match_normalized_conforms_to_alias_match_1() {
    let s = Scratch::new("alias_match").await;
    let fixtures = platform_conformance::fixtures_of("AliasMatch@1");
    let mut answered = HashMap::new();
    for case in &fixtures.cases {
        let term = case.input["term"].as_str().expect("a term");
        let other = case.input.get("other").and_then(Value::as_str);
        // The match the trigger judges: equal normalised terms, not blank.
        let row = s
            .client
            .query_one(
                "SELECT alias_match_normalized($1), \
                 CASE WHEN $2::text IS NULL THEN NULL ELSE \
                 alias_match_normalized($1) <> '' \
                 AND alias_match_normalized($1) = alias_match_normalized($2) END",
                &[&term, &other],
            )
            .await
            .expect("normalised");
        let normalized: String = row.get(0);
        let matched: Option<bool> = row.get(1);
        answered.insert(
            case.input.to_string(),
            json!({ "normalized_term": normalized, "match": matched }),
        );
    }
    platform_conformance::check("AliasMatch@1", |input| answered[&input.to_string()].clone());
    s.drop().await;
}

#[tokio::test]
async fn detection_vocabulary_view_lists_a_campaign_s_names_and_aliases_normalised() {
    let s = Scratch::new("vocabulary").await;
    s.accepted(&saved(
        ELOWEN,
        2,
        &format!(
            "INSERT INTO entity_alias (entity_id, alias) VALUES ('{ELOWEN}', 'La Rôdeuse'), ('{ELOWEN}', ' ')"
        ),
    ))
    .await;
    let mut vocabulary = s
        .texts(&format!(
            "SELECT entity_id::text || ' ' || term || ' → ' || normalized_term \
             || coalesce(' ' || sound_id::text, '') \
             FROM detection_vocabulary_view WHERE campaign_id = '{VALOMBRE}' "
        ))
        .await;
    // Byte order, whatever the database's collation.
    vocabulary.sort();
    assert_eq!(
        vocabulary,
        [
            format!("{ELOWEN} Elowen → elowen"),
            format!("{ELOWEN} La Rôdeuse → la rodeuse"),
            format!("{ELOWEN} la rodeuse → la rodeuse"),
            format!("{TAVERNE} La Taverne du Sanglier → la taverne du sanglier"),
            format!("{TAVERNE} la taverne → la taverne"),
            format!("{MORLOX} Morlox → morlox"),
            format!("{ORAGE} L'orage de la Saint-Jean → l'orage de la saint-jean"),
        ]
    );
    s.drop().await;
}

#[tokio::test]
async fn campaign_overview_view_counts_each_campaign_s_players_characters_places_and_events() {
    let s = Scratch::new("overview").await;
    s.accepted(
        "INSERT INTO campaign (id, name, kind) VALUES ('c1000000-0000-4000-8000-000000000003', 'Vide', 'oneshot')",
    )
    .await;
    let counts = s
        .texts(
            "SELECT c.name || ' ' || o.entity_counts::text FROM campaign_overview_view o \
             JOIN campaign c ON c.id = o.campaign_id ORDER BY c.name",
        )
        .await;
    assert_eq!(
        counts,
        [
            r#"La Brume {"events": 0, "places": 1, "players": 1, "characters": 0}"#,
            r#"Les Cendres de Valombre {"events": 1, "places": 1, "players": 1, "characters": 2}"#,
            r#"Vide {"events": 0, "places": 0, "players": 0, "characters": 0}"#,
        ]
    );
    s.drop().await;
}
