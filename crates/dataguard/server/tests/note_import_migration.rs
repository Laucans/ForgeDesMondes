//! The NoteImport migration (`crates/data/migrations/0008_note_import.sql`)
//! against the harness's PostgreSQL (`DATABASE_URL`): replayed over the
//! campaign, player and entity tables it needs, it is what `data/model.json`
//! says it is; its relations cascade and nullify as the aggregate declares;
//! and the table refuses, under `no-creation-before-validation`, an element
//! created by the analysis, for an unchecked proposal, or twice. Each test
//! works in a scratch database of its own, dropped when it ends.

use serde_json::Value;
use tokio_postgres::error::SqlState;
use tokio_postgres::{Client, NoTls};

const CAMPAIGN: &str = include_str!("../../../data/migrations/0002_campaign.sql");
const PLAYER: &str = include_str!("../../../data/migrations/0003_player.sql");
const ENTITY: &str = include_str!("../../../data/migrations/0004_entity.sql");
const MIGRATION: &str = include_str!("../../../data/migrations/0008_note_import.sql");

const VALOMBRE: &str = "c1000000-0000-4000-8000-000000000001";
const BRUME: &str = "c1000000-0000-4000-8000-000000000002";
const MORLOX: &str = "e1000000-0000-4000-8000-000000000003";
const PHARE: &str = "e1000000-0000-4000-8000-000000000005";
const NOTES: &str = "a1000000-0000-4000-8000-000000000001";
const BRUME_NOTES: &str = "a1000000-0000-4000-8000-000000000002";
/// Proposed from Valombre's notes: Morlox, whom the campaign already has.
const KNOWN: &str = "f1000000-0000-4000-8000-000000000001";
/// Proposed from Valombre's notes: a new NPC, checked.
const GRIMBALD: &str = "f1000000-0000-4000-8000-000000000002";
/// Proposed from Valombre's notes: a new place, unchecked.
const MOULIN: &str = "f1000000-0000-4000-8000-000000000003";

/// The tables the migration creates, in its order.
const TABLES: [&str; 2] = ["imported_note", "note_proposal"];

fn url() -> String {
    std::env::var("DATABASE_URL").expect("DATABASE_URL names the harness's PostgreSQL")
}

async fn connect(config: &tokio_postgres::Config) -> Client {
    let (client, connection) = config.connect(NoTls).await.expect("PostgreSQL");
    tokio::spawn(connection);
    client
}

/// The SQL adding element `id` to `campaign`, with the version every element
/// is written with (0004_entity.sql, every-change-versioned).
fn element(id: &str, campaign: &str, kind: &str, name: &str) -> String {
    format!(
        "INSERT INTO entity (id, campaign_id, type, name) VALUES ('{id}', '{campaign}', '{kind}', '{name}'); \
         INSERT INTO entity_version (entity_id, version_number, source, snapshot, diff) \
         VALUES ('{id}', 1, 'dm', '{{}}', '[]');"
    )
}

/// The SQL the validation writes for `proposal`: its element, created as
/// `id`, recorded on the proposal.
fn validated(proposal: &str, id: &str, name: &str) -> String {
    format!(
        "{} UPDATE note_proposal SET created_entity_id = '{id}' WHERE id = '{proposal}';",
        element(id, VALOMBRE, "pnj", name)
    )
}

/// A database of its own, holding the campaign, player and entity tables,
/// migrated once. Valombre holds Morlox and notes analysed into three
/// proposals — Morlox again (recognised, unchecked), Grimbald (checked) and
/// the mill (unchecked); Brume, the lighthouse and notes not analysed yet.
struct Scratch {
    client: Client,
    name: String,
}

impl Scratch {
    async fn new(name: &str) -> Scratch {
        let name = format!("note_import_test_{name}_{}", std::process::id());
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
                 {}{} \
                 INSERT INTO imported_note (id, campaign_id, raw_text, analyzed_at) VALUES \
                 ('{NOTES}', '{VALOMBRE}', 'Morlox a vendu la carte à Grimbald, près du moulin.', now()), \
                 ('{BRUME_NOTES}', '{BRUME}', 'Le phare s''est éteint.', NULL); \
                 INSERT INTO note_proposal (id, note_id, type, name, excerpt, existing_entity_id, checked) VALUES \
                 ('{KNOWN}', '{NOTES}', 'pnj', 'Morlox', 'Morlox a vendu la carte', '{MORLOX}', false), \
                 ('{GRIMBALD}', '{NOTES}', 'pnj', 'Grimbald', 'à Grimbald', NULL, true), \
                 ('{MOULIN}', '{NOTES}', 'lieu', 'Le moulin', 'près du moulin', NULL, false)",
                element(MORLOX, VALOMBRE, "pnj", "Morlox"),
                element(PHARE, BRUME, "lieu", "Le Phare"),
            ))
            .await
            .expect("two campaigns, their elements, their notes and their proposals");
        Scratch { client, name }
    }

    /// Runs `sql` in one transaction, which the database refuses under the
    /// invariant `id`: the error's constraint is the id in snake case — the
    /// CHECK's name, or the one the trigger raises with, its message then
    /// starting with the id.
    async fn refused_by(&self, sql: &str, id: &str) {
        let error = self.client.batch_execute(sql).await.expect_err(sql);
        let db = error.as_db_error().expect("a refusal of the database");
        assert_eq!(db.code(), &SqlState::CHECK_VIOLATION, "{sql}");
        assert_eq!(
            db.constraint(),
            Some(id.replace('-', "_").as_str()),
            "{sql}: {}",
            db.message()
        );
    }

    async fn accepted(&self, sql: &str) {
        self.client
            .batch_execute(sql)
            .await
            .unwrap_or_else(|e| panic!("{sql}: {e:?}"));
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

    /// The element `proposal` created, `-` for none.
    async fn created(&self, proposal: &str) -> String {
        self.texts(&format!(
            "SELECT created_entity_id::text FROM note_proposal WHERE id = '{proposal}'"
        ))
        .await
        .remove(0)
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

/// `table` as `data/model.json` describes it: each field's name, type,
/// nullability and reference, in its order.
fn modelled(model: &Value, table: &str) -> Vec<(String, String, bool, Option<String>)> {
    let table = model["tables"]
        .as_array()
        .expect("tables")
        .iter()
        .find(|t| t["name"] == table)
        .unwrap_or_else(|| panic!("the model describes {table}"));
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
async fn the_migration_replays_as_a_no_op_over_the_campaign_and_entity_tables() {
    let mut s = Scratch::new("replay").await;
    let first = s.snapshot().await;
    assert_eq!(first.len(), 2 + 3);
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
    assert!(
        at("0002_campaign.sql") < at("0008_note_import.sql")
            && at("0004_entity.sql") < at("0008_note_import.sql"),
        "{applied:?}"
    );
    assert_eq!(s.snapshot().await, first);
    // The trigger dropped and created again is there once.
    assert_eq!(
        s.count(
            "SELECT count(*) FROM pg_trigger \
             WHERE tgrelid = 'note_proposal'::regclass AND NOT tgisinternal"
        )
        .await,
        1
    );
    s.drop().await;
}

#[tokio::test]
async fn the_tables_are_what_data_model_json_describes_and_the_design_kept() {
    let s = Scratch::new("model").await;
    let described = model(include_str!("../../../../data/model.json"));
    let designed = model(include_str!("../../../../mock/data/model.json"));
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
        // Moved from the dossier's design: nothing renamed, nothing dropped.
        assert_eq!(real, modelled(&designed, table), "{table}");
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
    // c cascade, n nullify (SET NULL); propagate is CASCADE.
    assert_eq!(
        s.foreign_keys("imported_note").await,
        [key("campaign_id", "campaign.id", "c", "c")]
    );
    assert_eq!(
        s.foreign_keys("note_proposal").await,
        [
            key("created_entity_id", "entity.id", "n", "c"),
            key("existing_entity_id", "entity.id", "n", "c"),
            key("note_id", "imported_note.id", "c", "c"),
        ]
    );
    s.drop().await;
}

#[tokio::test]
async fn deleting_a_campaign_deletes_its_notes_and_their_proposals_and_theirs_alone() {
    let s = Scratch::new("cascade").await;
    s.accepted(&format!("DELETE FROM campaign WHERE id = '{VALOMBRE}'"))
        .await;
    assert_eq!(s.count("SELECT count(*) FROM note_proposal").await, 0);
    assert_eq!(
        s.texts("SELECT id::text FROM imported_note").await,
        [BRUME_NOTES]
    );
    s.drop().await;
}

#[tokio::test]
async fn deleting_a_note_deletes_its_proposals() {
    let s = Scratch::new("note_cascade").await;
    s.accepted(&format!("DELETE FROM imported_note WHERE id = '{NOTES}'"))
        .await;
    assert_eq!(s.count("SELECT count(*) FROM note_proposal").await, 0);
    s.drop().await;
}

#[tokio::test]
async fn a_campaign_s_new_id_reaches_its_notes_and_a_note_s_its_proposals() {
    let s = Scratch::new("propagate").await;
    let moved = "c1000000-0000-4000-8000-0000000000ff";
    let note = "a1000000-0000-4000-8000-0000000000ff";
    s.accepted(&format!(
        "UPDATE campaign SET id = '{moved}' WHERE id = '{VALOMBRE}'; \
         UPDATE imported_note SET id = '{note}' WHERE id = '{NOTES}'"
    ))
    .await;
    assert_eq!(
        s.texts(&format!(
            "SELECT id::text FROM imported_note WHERE campaign_id = '{moved}'"
        ))
        .await,
        [note]
    );
    assert_eq!(
        s.count(&format!(
            "SELECT count(*) FROM note_proposal WHERE note_id = '{note}'"
        ))
        .await,
        3
    );
    s.drop().await;
}

#[tokio::test]
async fn deleting_an_element_leaves_the_proposals_that_name_it_with_none() {
    let s = Scratch::new("nullify").await;
    let created = "e1000000-0000-4000-8000-0000000000aa";
    s.accepted(&validated(GRIMBALD, created, "Grimbald")).await;
    s.accepted(&format!(
        "DELETE FROM entity WHERE id IN ('{MORLOX}', '{created}')"
    ))
    .await;
    assert_eq!(
        s.texts(
            "SELECT coalesce(existing_entity_id::text, '-') || ' ' || coalesce(created_entity_id::text, '-') \
             FROM note_proposal ORDER BY id"
        )
        .await,
        ["- -", "- -", "- -"]
    );
    assert_eq!(s.count("SELECT count(*) FROM note_proposal").await, 3);
    s.drop().await;
}

#[tokio::test]
async fn a_proposal_names_a_note_that_exists_its_type_its_name_and_its_excerpt() {
    let s = Scratch::new("required").await;
    for (sql, code) in [
        (
            "INSERT INTO imported_note (campaign_id, raw_text) VALUES (gen_random_uuid(), 'x')"
                .to_string(),
            SqlState::FOREIGN_KEY_VIOLATION,
        ),
        (
            format!("INSERT INTO imported_note (campaign_id) VALUES ('{VALOMBRE}')"),
            SqlState::NOT_NULL_VIOLATION,
        ),
        (
            "INSERT INTO note_proposal (note_id, type, name, excerpt, checked) \
             VALUES (gen_random_uuid(), 'pnj', 'Personne', 'x', true)"
                .to_string(),
            SqlState::FOREIGN_KEY_VIOLATION,
        ),
        (
            format!(
                "INSERT INTO note_proposal (note_id, type, name, excerpt) \
                 VALUES ('{NOTES}', 'pnj', 'Sans choix', 'x')"
            ),
            SqlState::NOT_NULL_VIOLATION,
        ),
        (
            format!(
                "INSERT INTO note_proposal (note_id, type, name, checked) \
                 VALUES ('{NOTES}', 'pnj', 'Sans extrait', true)"
            ),
            SqlState::NOT_NULL_VIOLATION,
        ),
        (
            format!(
                "INSERT INTO note_proposal (note_id, type, name, excerpt, checked) \
                 VALUES ('{NOTES}', 'objet', 'La carte', 'la carte', true)"
            ),
            SqlState::CHECK_VIOLATION,
        ),
    ] {
        let error = s.client.batch_execute(&sql).await.expect_err(&sql);
        let db = error.as_db_error().expect("a refusal of the database");
        assert_eq!(db.code(), &code, "{sql}");
    }
    // Pasted now, under an id of its own, not analysed yet.
    let row = s
        .client
        .query_one(
            &format!(
                "INSERT INTO imported_note (campaign_id, raw_text) VALUES ('{VALOMBRE}', 'Suite.') \
                 RETURNING id IS NOT NULL, created_at > now() - interval '1 minute', analyzed_at IS NULL"
            ),
            &[],
        )
        .await
        .expect("a note pasted at a campaign");
    assert!(row.get::<_, bool>(0) && row.get::<_, bool>(1) && row.get::<_, bool>(2));
    s.drop().await;
}

#[tokio::test]
async fn no_creation_before_validation_refuses_a_proposal_written_with_an_element() {
    // The analysis writes proposals, checked or not: it creates no element,
    // not even by naming one it found.
    let s = Scratch::new("analysis").await;
    for checked in ["true", "false"] {
        s.refused_by(
            &format!(
                "INSERT INTO note_proposal (note_id, type, name, excerpt, checked, created_entity_id) \
                 VALUES ('{NOTES}', 'pnj', 'Morlox', 'Morlox', {checked}, '{MORLOX}')"
            ),
            "no-creation-before-validation",
        )
        .await;
    }
    // Recognising an existing element is not creating one.
    s.accepted(&format!(
        "INSERT INTO note_proposal (note_id, type, name, excerpt, existing_entity_id, checked) \
         VALUES ('{NOTES}', 'pnj', 'Morlox', 'Morlox', '{MORLOX}', false)"
    ))
    .await;
    s.drop().await;
}

#[tokio::test]
async fn no_creation_before_validation_creates_for_a_checked_proposal_alone() {
    let s = Scratch::new("checked").await;
    let mill = "e1000000-0000-4000-8000-0000000000bb";
    s.refused_by(
        &validated(MOULIN, mill, "Le moulin"),
        "no-creation-before-validation",
    )
    .await;
    assert_eq!(s.created(MOULIN).await, "-");
    // Nothing was created either: the refusal undid the element with it.
    assert_eq!(
        s.count(&format!("SELECT count(*) FROM entity WHERE id = '{mill}'"))
            .await,
        0
    );
    // Checked by the DM, it is created.
    s.accepted(&format!(
        "UPDATE note_proposal SET checked = true WHERE id = '{MOULIN}'; {}",
        validated(MOULIN, mill, "Le moulin")
    ))
    .await;
    assert_eq!(s.created(MOULIN).await, mill);
    // And stays checked: unchecking it would leave an element nobody chose.
    s.refused_by(
        &format!("UPDATE note_proposal SET checked = false WHERE id = '{MOULIN}'"),
        "no-creation-before-validation",
    )
    .await;
    s.drop().await;
}

#[tokio::test]
async fn no_creation_before_validation_creates_a_proposal_s_element_once() {
    let s = Scratch::new("once").await;
    let first = "e1000000-0000-4000-8000-0000000000aa";
    let second = "e1000000-0000-4000-8000-0000000000ab";
    s.accepted(&validated(GRIMBALD, first, "Grimbald")).await;
    // A second validation creates nothing more for it, nor forgets the first.
    s.refused_by(
        &validated(GRIMBALD, second, "Grimbald le Second"),
        "no-creation-before-validation",
    )
    .await;
    s.refused_by(
        &format!("UPDATE note_proposal SET created_entity_id = NULL WHERE id = '{GRIMBALD}'"),
        "no-creation-before-validation",
    )
    .await;
    assert_eq!(s.created(GRIMBALD).await, first);
    // Its other fields stay the DM's to edit, as the same element.
    s.accepted(&format!(
        "UPDATE note_proposal SET name = 'Grimbald le Fourbe', created_entity_id = '{first}' \
         WHERE id = '{GRIMBALD}'"
    ))
    .await;
    s.drop().await;
}
