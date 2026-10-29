//! `0015_battlemap_token_character_type.sql` in `crates/data/migrations/` against
//! the harness's PostgreSQL (`DATABASE_URL`): laid over a database migrated up
//! to 0014 it marks each token PC or NPC from its character's type, then
//! replayed it changes nothing; the tables refuse, whatever order two writes
//! land in, a PC that tracks HP, an NPC that does not or whose HP leave 0 to
//! its maximum (`pj-hp-not-tracked`, SQLSTATE 23514), and a PC's second token on
//! an instance (`token-roster`, 23505); over rows that already break one of its
//! rules it refuses to apply, counts them, and deletes nothing. Each test works
//! in a `ScratchDb` of its own.

use std::path::{Path, PathBuf};

use platform_scratch_db::ScratchDb;
use tokio_postgres::Client;
use tokio_postgres::error::SqlState;

const MIGRATION: &str = "0015_battlemap_token_character_type.sql";

const VALOMBRE: &str = "c1000000-0000-4000-8000-000000000001";
const SEANCE: &str = "5e000000-0000-4000-8000-000000000001";
const SCENE: &str = "5c000000-0000-4000-8000-000000000001";
const OTHER_SCENE: &str = "5c000000-0000-4000-8000-000000000002";
const GUE: &str = "d1000000-0000-4000-8000-00000000000a";
const ELOWEN: &str = "e1000000-0000-4000-8000-000000000001";
const BANDIT: &str = "e1000000-0000-4000-8000-000000000003";
const INSTANCE: &str = "b1000000-0000-4000-8000-000000000001";
const OTHER_INSTANCE: &str = "b1000000-0000-4000-8000-000000000002";
const T_ELOWEN: &str = "0a000000-0000-4000-8000-000000000001";
const T_BANDIT: &str = "0a000000-0000-4000-8000-000000000002";

fn migrations() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("migrations")
}

fn migration() -> String {
    std::fs::read_to_string(migrations().join(MIGRATION)).expect("migration 0015")
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

/// A database migrated up to 0014 — every file before 0015, in their order —
/// holding Valombre, a PC (Elowen) and an NPC (the Bandit), and two instances,
/// the first with a token of each.
async fn at_0014(label: &str) -> (ScratchDb, Client) {
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
    assert!(files.len() >= 14, "{files:?}");
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
                 ('{SCENE}', '{SEANCE}', 1, 'Le gué'),
                 ('{OTHER_SCENE}', '{SEANCE}', 2, 'La forêt');
             INSERT INTO media (id, campaign_id, kind, title, file_path, width_px, height_px) VALUES
                 ('{GUE}', '{VALOMBRE}', 'battlemap', 'Le Gué des Corbeaux', 'media/gue.png', 960, 640);
             INSERT INTO battlemap_grid (media_id, hex_size_px, offset_x, offset_y, orientation, color, opacity) VALUES
                 ('{GUE}', 30, 12, 8, 'pointe', 'text', 0.4);
             INSERT INTO entity (id, campaign_id, type, name) VALUES
                 ('{ELOWEN}', '{VALOMBRE}', 'pj', 'Elowen'),
                 ('{BANDIT}', '{VALOMBRE}', 'pnj', 'Bandit');
             INSERT INTO character (entity_id, hp_max, token_color) VALUES
                 ('{ELOWEN}', 44, 'success'),
                 ('{BANDIT}', 11, 'danger');
             INSERT INTO entity_version (entity_id, version_number, source, snapshot, diff)
                 SELECT id, 1, 'dm', '{{}}', '[]' FROM entity;
             INSERT INTO battlemap_instance (id, battlemap_media_id, session_id, scene_id) VALUES
                 ('{INSTANCE}', '{GUE}', '{SEANCE}', '{SCENE}'),
                 ('{OTHER_INSTANCE}', '{GUE}', '{SEANCE}', '{OTHER_SCENE}');
             INSERT INTO token (id, instance_id, character_id, number, name, q, r, visible,
                     hp_current, hp_max, conditions) VALUES
                 ('{T_ELOWEN}', '{INSTANCE}', '{ELOWEN}', NULL, 'Elowen', 0, 0, true, NULL, NULL, '{{}}'),
                 ('{T_BANDIT}', '{INSTANCE}', '{BANDIT}', 1, 'Bandit 1', 1, 0, true, 11, 11, '{{}}');"
        ))
        .await
        .expect("Valombre");
    (db, client)
}

/// A database at 0015.
async fn at_0015(label: &str) -> (ScratchDb, Client) {
    let (db, client) = at_0014(label).await;
    client
        .batch_execute(&migration())
        .await
        .expect("0015 applies");
    (db, client)
}

/// The SQL of a token of `character` on `instance`, numbered `number` (SQL),
/// with `hp` (SQL: `NULL, NULL` or `current, max`), naming no marker.
fn token(instance: &str, character: &str, number: &str, hp: &str) -> String {
    format!(
        "INSERT INTO token (id, instance_id, character_id, number, name, q, r, visible,
             hp_current, hp_max, conditions)
         VALUES (gen_random_uuid(), '{instance}', '{character}', {number}, 'Jeton', 0, 0, true,
             {hp}, '{{}}')"
    )
}

/// The constraint `sql` breaks, refused by the tables with `code`.
async fn refused_by(client: &Client, sql: &str, code: &SqlState) -> String {
    let error = client.batch_execute(sql).await.expect_err(sql);
    let db = error.as_db_error().expect("a database error");
    assert_eq!(db.code(), code, "{sql}: {db}");
    db.constraint().expect("a named constraint").to_owned()
}

/// What the migration lays on the `token` table: its columns, constraints and
/// indexes, as the database defines them.
async fn definitions(client: &Client) -> Vec<String> {
    client
        .query(
            "SELECT 'column ' || column_name || ' ' || data_type || ' ' || is_nullable
                 FROM information_schema.columns WHERE table_name = 'token'
             UNION ALL SELECT 'constraint ' || conname || ' ' || pg_get_constraintdef(oid)
                 FROM pg_constraint WHERE conrelid = 'token'::regclass
             UNION ALL SELECT indexdef FROM pg_indexes WHERE tablename = 'token'
             UNION ALL SELECT 'trigger ' || tgname FROM pg_trigger
                 WHERE tgrelid = 'token'::regclass AND NOT tgisinternal
             ORDER BY 1",
            &[],
        )
        .await
        .expect("the definitions")
        .iter()
        .map(|row| row.get(0))
        .collect()
}

/// The tokens, each with its marker, in a stable order.
async fn markers(client: &Client) -> Vec<String> {
    client
        .query(
            "SELECT id::text || ' ' || character_type FROM token ORDER BY id",
            &[],
        )
        .await
        .expect("the tokens")
        .iter()
        .map(|row| row.get(0))
        .collect()
}

// AC1: every token takes its character's entity.type; the column is NOT NULL
// and takes 'pj' or 'pnj' only.
#[tokio::test]
async fn it_marks_each_existing_token_with_its_character_s_type() {
    let (_db, client) = at_0014("character_type_backfill").await;
    client
        .batch_execute(&migration())
        .await
        .expect("0015 applies");
    assert_eq!(
        markers(&client).await,
        [format!("{T_ELOWEN} pj"), format!("{T_BANDIT} pnj")]
    );
    let nullable: String = client
        .query_one(
            "SELECT is_nullable FROM information_schema.columns
             WHERE table_name = 'token' AND column_name = 'character_type'",
            &[],
        )
        .await
        .expect("the column")
        .get(0);
    assert_eq!(nullable, "NO");
    // Neither another type nor none: a token of a character the tables hold
    // not yet cannot name any other.
    for kind in ["'lieu'", "'mage'", "''"] {
        assert_eq!(
            refused_by(
                &client,
                &format!(
                    "INSERT INTO token (id, instance_id, character_id, number, name, q, r, visible,
                         character_type, hp_current, hp_max, conditions)
                     VALUES (gen_random_uuid(), '{INSTANCE}', gen_random_uuid(), 2, 'Jeton', 0, 0,
                         true, {kind}, 3, 3, '{{}}')"
                ),
                &SqlState::CHECK_VIOLATION
            )
            .await,
            "token_character_type",
            "{kind}"
        );
    }
}

// AC1: a token that names no marker gets its character's type, whoever writes
// it (the projection, a fixture's SQL); one that names it keeps it.
#[tokio::test]
async fn a_token_without_a_marker_gets_its_character_s_type() {
    let (_db, client) = at_0015("character_type_default").await;
    client
        .batch_execute(&format!(
            "{}; {}",
            token(OTHER_INSTANCE, ELOWEN, "NULL", "NULL, NULL"),
            token(OTHER_INSTANCE, BANDIT, "1", "5, 5")
        ))
        .await
        .expect("tokens without a marker");
    let rows = client
        .query(
            "SELECT e.type, t.character_type FROM token t JOIN entity e ON e.id = t.character_id",
            &[],
        )
        .await
        .expect("the tokens");
    assert_eq!(rows.len(), 4);
    for row in rows {
        assert_eq!(row.get::<_, String>(0), row.get::<_, String>(1));
    }
    let explicit = client
        .query_one(
            "INSERT INTO token (id, instance_id, character_id, number, name, q, r, visible,
                 character_type, hp_current, hp_max, conditions)
             VALUES (gen_random_uuid(), $1::text::uuid, $2::text::uuid, 9, 'Jeton', 0, 0, true,
                 'pnj', 4, 4, '{}') RETURNING character_type",
            &[&OTHER_INSTANCE, &BANDIT],
        )
        .await
        .expect("a token naming its marker");
    assert_eq!(explicit.get::<_, String>(0), "pnj");
}

// AC2: replayed, 0015 changes nothing and raises no error.
#[tokio::test]
async fn it_replays_as_a_no_op() {
    let (_db, client) = at_0015("character_type_replay").await;
    let (definitions_once, markers_once) = (definitions(&client).await, markers(&client).await);
    for needle in [
        "column character_type text NO",
        "constraint token_character_type",
        "constraint token_pj_hp_not_tracked",
        "CREATE UNIQUE INDEX token_instance_pj ON public.token USING btree (instance_id, character_id) WHERE (character_type = 'pj'::text)",
        "trigger token_character_type_default",
    ] {
        assert!(
            definitions_once.iter().any(|def| def.contains(needle)),
            "{needle}: {definitions_once:#?}"
        );
    }
    client.batch_execute(&migration()).await.expect("replayed");
    client
        .batch_execute(&migration())
        .await
        .expect("replayed twice");
    assert_eq!(definitions(&client).await, definitions_once);
    assert_eq!(markers(&client).await, markers_once);
}

// AC2: the Data layer applies it after 0014, and then nothing more.
#[tokio::test]
async fn the_data_layer_applies_it_after_0014_and_again() {
    let db = ScratchDb::create("character_type_data_layer")
        .await
        .expect("DATABASE_URL names the harness's PostgreSQL");
    let mut client = db.connect().await.expect("the scratch database answers");
    let applied = data::migrate(&mut client, &migrations())
        .await
        .expect("migrated");
    assert_eq!(applied.last().map(String::as_str), Some(MIGRATION));
    let again = data::migrate(&mut client, &migrations())
        .await
        .expect("migrated again");
    assert!(again.is_empty(), "{again:?}");
}

// AC3: a PC token never carries HP — current, maximum or both — whether it
// is written or changed; the refusal is a check violation, the command's
// rejection, never a failed store.
#[tokio::test]
async fn the_tables_refuse_hp_on_a_pc_token() {
    let (_db, client) = at_0015("character_type_pj_hp").await;
    for hp in ["5, 5", "5, NULL", "NULL, 5", "0, 0"] {
        assert_eq!(
            refused_by(
                &client,
                &token(OTHER_INSTANCE, ELOWEN, "NULL", hp),
                &SqlState::CHECK_VIOLATION
            )
            .await,
            "token_pj_hp_not_tracked",
            "{hp}"
        );
    }
    for set in ["hp_current = 5", "hp_max = 5", "hp_current = 5, hp_max = 5"] {
        assert_eq!(
            refused_by(
                &client,
                &format!("UPDATE token SET {set} WHERE id = '{T_ELOWEN}'"),
                &SqlState::CHECK_VIOLATION
            )
            .await,
            "token_pj_hp_not_tracked",
            "{set}"
        );
    }
    // A marker is its character's: a PC cannot be marked an NPC to track HP,
    // nor an NPC a PC.
    for sql in [
        format!(
            "UPDATE token SET character_type = 'pnj', hp_current = 5, hp_max = 5 WHERE id = '{T_ELOWEN}'"
        ),
        format!("UPDATE token SET character_type = 'pj' WHERE id = '{T_BANDIT}'"),
        format!(
            "INSERT INTO token (id, instance_id, character_id, name, q, r, visible,
                 character_type, hp_current, hp_max, conditions)
             VALUES (gen_random_uuid(), '{OTHER_INSTANCE}', '{ELOWEN}', 'Jeton', 0, 0, true,
                 'pnj', 5, 5, '{{}}')"
        ),
    ] {
        assert_eq!(
            refused_by(&client, &sql, &SqlState::CHECK_VIOLATION).await,
            "token_character_type_matches",
            "{sql}"
        );
    }
    // A PC's token without HP is accepted.
    client
        .batch_execute(&token(OTHER_INSTANCE, ELOWEN, "NULL", "NULL, NULL"))
        .await
        .expect("a PC without HP");
}

// AC4: an NPC token always carries both HP, its current within 0 and its
// maximum.
#[tokio::test]
async fn the_tables_refuse_an_npc_token_whose_hp_are_missing_or_out_of_bounds() {
    let (_db, client) = at_0015("character_type_pnj_hp").await;
    for hp in [
        "-1, 11",
        "12, 11",
        "5, NULL",
        "NULL, 11",
        "NULL, NULL",
        "-3, -1",
    ] {
        assert_eq!(
            refused_by(
                &client,
                &token(OTHER_INSTANCE, BANDIT, "1", hp),
                &SqlState::CHECK_VIOLATION
            )
            .await,
            "token_pj_hp_not_tracked",
            "{hp}"
        );
    }
    for set in [
        "hp_current = -1",
        "hp_current = 12",
        "hp_max = NULL",
        "hp_max = 10",
        "hp_current = NULL, hp_max = NULL",
    ] {
        assert_eq!(
            refused_by(
                &client,
                &format!("UPDATE token SET {set} WHERE id = '{T_BANDIT}'"),
                &SqlState::CHECK_VIOLATION
            )
            .await,
            "token_pj_hp_not_tracked",
            "{set}"
        );
    }
    // From 0 to its maximum, both ends, are accepted.
    for hp in ["0, 11", "11, 11", "1, 1", "0, 0"] {
        client
            .batch_execute(&token(OTHER_INSTANCE, BANDIT, "NULL", hp))
            .await
            .unwrap_or_else(|e| panic!("{hp}: {e}"));
    }
}

// AC5: a PC's second token on an instance is a duplicate (23505).
#[tokio::test]
async fn the_tables_refuse_a_pc_s_second_token_on_an_instance() {
    let (_db, client) = at_0015("character_type_roster").await;
    for number in ["NULL", "1", "2"] {
        assert_eq!(
            refused_by(
                &client,
                &token(INSTANCE, ELOWEN, number, "NULL, NULL"),
                &SqlState::UNIQUE_VIOLATION
            )
            .await,
            "token_instance_pj",
            "{number}"
        );
    }
    // Moved onto an instance where the PC already stands, too.
    client
        .batch_execute(&token(OTHER_INSTANCE, ELOWEN, "NULL", "NULL, NULL"))
        .await
        .expect("the PC on another instance");
    assert_eq!(
        refused_by(
            &client,
            &format!("UPDATE token SET instance_id = '{INSTANCE}' WHERE character_id = '{ELOWEN}' AND instance_id = '{OTHER_INSTANCE}'"),
            &SqlState::UNIQUE_VIOLATION
        )
        .await,
        "token_instance_pj"
    );
}

// AC6: token-roster is per instance: the same PC stands on two.
#[tokio::test]
async fn a_pc_stands_once_on_each_instance() {
    let (_db, client) = at_0015("character_type_per_instance").await;
    client
        .batch_execute(&token(OTHER_INSTANCE, ELOWEN, "NULL", "NULL, NULL"))
        .await
        .expect("the same PC on another instance");
    let held: i64 = client
        .query_one(
            "SELECT count(*) FROM token WHERE character_id = $1::text::uuid",
            &[&ELOWEN],
        )
        .await
        .expect("a count")
        .get(0);
    assert_eq!(held, 2);
}

// AC7: an NPC may have several tokens on an instance, told apart by their
// number.
#[tokio::test]
async fn an_npc_has_several_tokens_on_an_instance() {
    let (_db, client) = at_0015("character_type_npcs").await;
    for number in ["2", "3"] {
        client
            .batch_execute(&token(INSTANCE, BANDIT, number, "11, 11"))
            .await
            .unwrap_or_else(|e| panic!("{number}: {e}"));
    }
    let held: i64 = client
        .query_one(
            "SELECT count(*) FROM token WHERE instance_id = $1::text::uuid AND character_id = $2::text::uuid",
            &[&INSTANCE, &BANDIT],
        )
        .await
        .expect("a count")
        .get(0);
    assert_eq!(held, 3);
}

// Rows that already break a rule are never deleted: 0015 refuses, counting
// them and naming the rule.
#[tokio::test]
async fn over_rows_that_break_a_rule_it_refuses_and_deletes_nothing() {
    for (label, rule, constraint, code, rows, count) in [
        (
            "character_type_broken_pc",
            "pj-hp-not-tracked: 1 token(s)",
            "token_pj_hp_not_tracked",
            SqlState::CHECK_VIOLATION,
            format!("UPDATE token SET hp_current = 7, hp_max = 7 WHERE id = '{T_ELOWEN}'"),
            2,
        ),
        (
            "character_type_broken_npc",
            "pj-hp-not-tracked: 2 token(s)",
            "token_pj_hp_not_tracked",
            SqlState::CHECK_VIOLATION,
            format!(
                "UPDATE token SET hp_current = 12 WHERE id = '{T_BANDIT}';
                 {}",
                token(INSTANCE, BANDIT, "2", "NULL, NULL")
            ),
            3,
        ),
        (
            "character_type_broken_roster",
            "token-roster: 1 PC(s)",
            "token_instance_pj",
            SqlState::UNIQUE_VIOLATION,
            token(INSTANCE, ELOWEN, "NULL", "NULL, NULL"),
            3,
        ),
    ] {
        let (_db, client) = at_0014(label).await;
        client.batch_execute(&rows).await.expect("the broken rows");
        let error = client
            .batch_execute(&migration())
            .await
            .expect_err("0015 refuses");
        let refusal = error.as_db_error().expect("a database error");
        assert_eq!(refusal.code(), &code, "{label}");
        assert_eq!(refusal.constraint(), Some(constraint), "{label}");
        assert!(refusal.message().contains(rule), "{label}: {refusal}");
        assert!(
            refusal
                .hint()
                .is_some_and(|hint| hint.contains("nothing was deleted")),
            "{label}"
        );
        let held: i64 = client
            .query_one("SELECT count(*) FROM token", &[])
            .await
            .expect("a count")
            .get(0);
        assert_eq!(held, count, "{label}: nothing deleted");
    }
}

// AC8: the token's marker is a field the aggregate declares, valid for the
// aggregate's contract (it parses as the engine reads it), and the model
// documents its column.
#[test]
fn the_aggregate_declares_the_marker_and_the_model_documents_its_column() {
    let aggregate = harness_dataguard::aggregate::Aggregate::parse(&repository(
        "crates/table/aggregates/BattlemapInstance/aggregate.json",
    ))
    .expect("an aggregate (contract I)");
    let field = aggregate
        .fields
        .get("characterType")
        .expect("characterType is declared");
    assert_eq!(
        field.kind,
        harness_dataguard::aggregate::Kind::Authoritative
    );
    for invariant in ["pj-hp-not-tracked", "token-roster"] {
        assert!(aggregate.invariant(invariant).is_some(), "{invariant}");
    }

    let model: serde_json::Value =
        serde_json::from_str(&repository("data/model.json")).expect("data/model.json");
    let token = model["tables"]
        .as_array()
        .expect("tables")
        .iter()
        .find(|table| table["name"] == "token")
        .expect("the token table");
    let column = token["fields"]
        .as_array()
        .expect("fields")
        .iter()
        .find(|field| field["name"] == "character_type")
        .expect("token.character_type is documented");
    assert_eq!(column["type"], "text");
    assert_eq!(column["nullable"], false);
}
