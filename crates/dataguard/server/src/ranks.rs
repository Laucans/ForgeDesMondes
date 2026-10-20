//! The initiative ranks of a battlemap instance's combat in the tables
//! (`initiative_entry`, `crates/data/migrations/0010_battlemap_instance.sql`).
//!
//! A reorder (`table.reorderInitiative`) is judged against the instance's
//! open combat and its participants before it is queued: the engine judges a
//! row alone, and the combat and each of its entries are one. Which combats
//! the instance has, and which entries a combat has, are the engine's rows'
//! ([`combats`], [`entries`]) — the queue's, and each row as the queue will
//! leave it, [`crate::Rows::open_lineup`] adds. A combat is on
//! `BattlemapInstance/<its id>`, an entry on `BattlemapInstance/<its id>`;
//! one the engine tombstoned is none. Once the engine applied an entry,
//! [`rank`] writes its rank on its row: the carousel's places follow
//! (`initiative_order_view`, derived).

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

/// The rank the engine holds of the entry partition `$1` names — of every
/// entry when it is null — written on its row: the one column a reorder
/// changes (`table.reorderInitiative`), never a token's visibility
/// (`hidden-keeps-rank`) nor the combat's turn. After each apply of the
/// aggregate's partitions, and at a start, so a rank whose projection failed
/// after its apply is written again. The entry is found by its key, the id
/// its partition names; one the table does not hold yet is the start's to
/// insert, and a rank that is no number is left aside, as the DataGuard
/// refuses it.
const RANK: &str = "UPDATE initiative_entry e
    SET rank = (r.fields ->> 'rank')::numeric::integer
    FROM dataguard_rows r
    WHERE NOT r.tombstoned
        AND ($1::text IS NULL OR r.partition = $1::text)
        AND r.partition = 'BattlemapInstance/' || lower(r.fields ->> 'initiativeEntryId')
        AND e.id = CASE WHEN pg_input_is_valid(r.fields ->> 'initiativeEntryId', 'uuid')
            THEN (r.fields ->> 'initiativeEntryId')::uuid END
        AND jsonb_typeof(r.fields -> 'rank') = 'number'
        AND e.rank IS DISTINCT FROM (r.fields ->> 'rank')::numeric::integer";

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

/// Writes the rank the engine holds of the entry `partition` names — of
/// every entry, `None` — on its row, logging a failure: after each apply of
/// the aggregate's partitions, and at a start. Tables not migrated yet hold
/// none.
pub async fn rank(client: &Client, partition: Option<&str>) {
    if let Err(e) = client.execute(RANK, &[&partition]).await {
        eprintln!(
            "projection: initiative rank not written: {}",
            e.as_db_error().map_or("-", |db| db.code().code())
        );
    }
}
