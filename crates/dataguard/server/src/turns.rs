//! The turn of a battlemap instance's combat in the tables (`combat`,
//! `crates/data/migrations/0010_battlemap_instance.sql`).
//!
//! A « Suivant » (`table.nextTurn`) is judged against the instance's open
//! combat and its participants before it is queued: the engine judges a row
//! alone, and the combat and each of its entries are one. Which combats the
//! instance has, and which entries a combat has, are the engine's rows'
//! ([`combats`], [`entries`]) — the queue's, and each row as the queue will
//! leave it, [`crate::Rows::turn`] adds. A combat is on
//! `BattlemapInstance/<its id>`, an entry on `BattlemapInstance/<its id>`;
//! one the engine tombstoned is none. Once the engine applied a turn,
//! [`turn`] writes the combat's round and its current entry on its row.

use tokio_postgres::Client;

/// The combats the engine holds of the instance `$1`, open or ended.
const COMBATS: &str = "SELECT lower(r.fields ->> 'combatId') FROM dataguard_rows r
    WHERE r.partition LIKE 'BattlemapInstance/%' AND NOT r.tombstoned
        AND r.partition = 'BattlemapInstance/' || lower(r.fields ->> 'combatId')
        AND lower(r.fields ->> 'combatInstanceId') = lower($1::text)
    ORDER BY 1";

/// The entries the engine holds of the combat `$1`.
const ENTRIES: &str = "SELECT lower(r.fields ->> 'initiativeEntryId') FROM dataguard_rows r
    WHERE r.partition LIKE 'BattlemapInstance/%' AND NOT r.tombstoned
        AND r.partition = 'BattlemapInstance/' || lower(r.fields ->> 'initiativeEntryId')
        AND lower(r.fields ->> 'initiativeEntryCombatId') = lower($1::text)
    ORDER BY 1";

/// The round and the current entry the engine holds of the combat partition
/// `$1` names — of every combat when it is null — written on its row: the
/// two columns « Suivant » moves (`next-turn`). After each apply of the
/// aggregate's partitions, and at a start, so a turn whose projection failed
/// after its apply is written again. The current entry is an entry's key:
/// one the table does not hold yet is left as the row has it — the entry's
/// own projection points the combat at it once it lands — and none is
/// written when the engine names none.
const TURN: &str = "UPDATE combat c
    SET round = t.round,
        current_entry_id = CASE WHEN t.known THEN t.current ELSE c.current_entry_id END
    FROM (SELECT lower(r.fields ->> 'combatId') AS id,
            (r.fields ->> 'round')::numeric::integer AS round, e.id AS current,
            e.id IS NOT NULL OR r.fields ->> 'currentEntryId' IS NULL AS known
        FROM dataguard_rows r
        LEFT JOIN initiative_entry e
            ON e.id = CASE WHEN pg_input_is_valid(r.fields ->> 'currentEntryId', 'uuid')
                THEN (r.fields ->> 'currentEntryId')::uuid END
        WHERE NOT r.tombstoned AND r.partition LIKE 'BattlemapInstance/%'
            AND ($1::text IS NULL OR r.partition = $1::text)
            AND r.partition = 'BattlemapInstance/' || lower(r.fields ->> 'combatId')
            AND jsonb_typeof(r.fields -> 'round') = 'number') t
    WHERE c.id::text = t.id
        AND (c.round IS DISTINCT FROM t.round
            OR (t.known AND c.current_entry_id IS DISTINCT FROM t.current))";

/// The combats the engine holds of the instance `instance`, by id.
///
/// # Errors
///
/// The database's, when it cannot be read.
pub async fn combats(
    client: &Client,
    instance: &str,
) -> Result<Vec<String>, tokio_postgres::Error> {
    ids(client, COMBATS, instance).await
}

/// The entries the engine holds of the combat `combat`, by id.
///
/// # Errors
///
/// The database's, when it cannot be read.
pub async fn entries(client: &Client, combat: &str) -> Result<Vec<String>, tokio_postgres::Error> {
    ids(client, ENTRIES, combat).await
}

async fn ids(client: &Client, sql: &str, of: &str) -> Result<Vec<String>, tokio_postgres::Error> {
    Ok(client
        .query(sql, &[&of])
        .await?
        .iter()
        .filter_map(|row| row.get::<_, Option<String>>(0))
        .collect())
}

/// Writes the round and the current entry the engine holds of the combat
/// `partition` names — of every combat, `None` — on its row, logging a
/// failure: after each apply of the aggregate's partitions, and at a start.
/// Tables not migrated yet hold none.
pub async fn turn(client: &Client, partition: Option<&str>) {
    if let Err(e) = client.execute(TURN, &[&partition]).await {
        eprintln!(
            "projection: combat's turn not written: {}",
            e.as_db_error().map_or("-", |db| db.code().code())
        );
    }
}
