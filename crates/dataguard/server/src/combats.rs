//! The combat of a battlemap instance in the tables (`combat` and
//! `initiative_entry`, `crates/data/migrations/0010_battlemap_instance.sql`).
//!
//! A start (`table.startCombat`) is judged against what the tables hold of
//! the instance before it is queued ([`lineup`]): the engine judges a row
//! alone, and the combat and each of its entries are one. The tokens are the
//! table's and the engine's rows' together — a token applied whose
//! projection failed is one too — and the queue's, which
//! [`crate::Rows::lineup`] adds, with each character's kind and DEX; the
//! combats not ended are the table's and the engine's, which
//! [`crate::Rows::lineup`] reads again as the queue will leave them. A token
//! or a combat the engine tombstoned is none.
//!
//! The engine applies the partitions of a run in the order of their names,
//! not of their commands: an entry may land before its combat, or before a
//! token still queued when the combat started. Once the engine applied a
//! combat on `BattlemapInstance/<its id>`, [`project`] inserts its row; an
//! entry is inserted once the table holds its combat and its token
//! ([`enter`]) — at its own apply, its combat's or its token's, whichever
//! comes last. A row the table holds already is left as it is. The combat's
//! current entry is an entry's key: [`current`] points it at the entry the
//! engine names once the table holds that entry — after each entry
//! inserted, after each apply of a combat, and at a start.
//!
//! An end (`table.endCombat`) is judged against the combats of the instance
//! not ended ([`open`]), which [`crate::Rows::open_combat`] reads again as
//! the queue will leave them. Once the engine applied it on the combat's
//! partition, [`end`] writes the combat's `ended_at` and its current entry —
//! none —, and no other column: its entries, the instance's tokens, their
//! HP and their conditions stay as they are.
//!
//! A participant taken out of the combat (`table.removeParticipant`) is a
//! tombstone on its entry's partition: [`leave`] takes its row out of the
//! table. A reset of the instance (`table.resetBattlemap`) tombstones its
//! combats too: [`wipe`] takes each out of the table, and its entries with
//! it (`initiativeEntryCombatId`, cascade).

use start_combat::{Lineup, Participant};
use tokio_postgres::Client;

/// Whether the tables hold the instance `$1`.
const INSTANCE: &str = "SELECT EXISTS (SELECT 1 FROM battlemap_instance WHERE id = $1::text::uuid)";

/// The tokens of the instance `$1` — in the table, in the engine's rows, or
/// queued (`$2` their ids, `$3` their characters) — with their character,
/// its kind and its DEX score — a PC or an NPC with its sheet alone —, by
/// id: the order the ties keep.
const TOKENS: &str = "WITH placed AS (
        SELECT t.id::text AS token, t.character_id::text AS character FROM token t
        WHERE t.instance_id = $1::text::uuid
        UNION
        SELECT lower(r.fields ->> 'tokenId'), lower(r.fields ->> 'characterId')
        FROM dataguard_rows r
        WHERE r.partition LIKE 'BattlemapInstance/%' AND NOT r.tombstoned
            AND r.partition = 'BattlemapInstance/' || lower(r.fields ->> 'tokenId')
            AND lower(r.fields ->> 'tokenInstanceId') = $1::text
        UNION
        SELECT lower(q.token), lower(q.character)
        FROM unnest($2::text[], $3::text[]) AS q(token, character)
    )
    SELECT p.token, p.character, e.type, c.dexterity::bigint
    FROM placed p
    LEFT JOIN (entity e JOIN character c ON c.entity_id = e.id)
        ON e.id = CASE WHEN pg_input_is_valid(p.character, 'uuid') THEN p.character::uuid END
            AND e.type IN ('pj', 'pnj')
    WHERE NOT EXISTS (SELECT 1 FROM dataguard_rows r
        WHERE r.partition = 'BattlemapInstance/' || p.token AND r.tombstoned)
    ORDER BY p.token";

/// The combats of the instance `$1` the table holds with no end, and the
/// ones the engine holds of it with none: which are still open is the
/// engine's row's to tell, as the queue will leave it. An end, once written,
/// is kept (`one-active-combat`).
const COMBATS: &str = "SELECT c.id::text FROM combat c
    WHERE c.instance_id = $1::text::uuid AND c.ended_at IS NULL
        AND NOT EXISTS (SELECT 1 FROM dataguard_rows r
            WHERE r.partition = 'BattlemapInstance/' || c.id::text AND r.tombstoned)
    UNION
    SELECT lower(r.fields ->> 'combatId') FROM dataguard_rows r
    WHERE r.partition LIKE 'BattlemapInstance/%' AND NOT r.tombstoned
        AND r.partition = 'BattlemapInstance/' || lower(r.fields ->> 'combatId')
        AND lower(r.fields ->> 'combatInstanceId') = $1::text
        AND r.fields ->> 'endedAt' IS NULL";

/// The combat the engine holds on partition `$1`, inserted when the table
/// holds it not, its current entry once the table holds that entry. Its row
/// is the partition's own: its id is the one the partition names.
const PROJECT: &str = "INSERT INTO combat
        (id, instance_id, round, current_entry_id, started_at, ended_at)
    SELECT (r.fields ->> 'combatId')::uuid, (r.fields ->> 'combatInstanceId')::uuid,
        (r.fields ->> 'round')::numeric::integer,
        (SELECT e.id FROM initiative_entry e
            WHERE e.id::text = lower(r.fields ->> 'currentEntryId')),
        (r.fields ->> 'startedAt')::timestamptz, (r.fields ->> 'endedAt')::timestamptz
    FROM dataguard_rows r
    WHERE r.partition = $1::text AND NOT r.tombstoned
        AND r.partition = 'BattlemapInstance/' || lower(r.fields ->> 'combatId')
        AND NOT EXISTS (SELECT 1 FROM combat c WHERE c.id = (r.fields ->> 'combatId')::uuid)";

/// The entries the engine holds on partition `$1`, of the combat `$2` or of
/// the token `$3`, inserted when the table holds them not and holds their
/// combat and their token: the combats they are of.
const ENTER: &str = "INSERT INTO initiative_entry
        (id, combat_id, token_id, roll, initiative, dex_score, tiebreak, rank)
    SELECT (r.fields ->> 'initiativeEntryId')::uuid,
        (r.fields ->> 'initiativeEntryCombatId')::uuid,
        (r.fields ->> 'initiativeEntryTokenId')::uuid, (r.fields ->> 'roll')::numeric::smallint,
        (r.fields ->> 'initiative')::numeric::smallint,
        (r.fields ->> 'dexScore')::numeric::smallint,
        (r.fields ->> 'tiebreak')::double precision, (r.fields ->> 'rank')::numeric::integer
    FROM dataguard_rows r
    WHERE r.partition LIKE 'BattlemapInstance/%' AND NOT r.tombstoned
        AND r.partition = 'BattlemapInstance/' || lower(r.fields ->> 'initiativeEntryId')
        AND (r.partition = $1::text
            OR lower(r.fields ->> 'initiativeEntryCombatId') = lower($2::text)
            OR lower(r.fields ->> 'initiativeEntryTokenId') = lower($3::text))
        AND NOT EXISTS (SELECT 1 FROM initiative_entry e
            WHERE e.id::text = lower(r.fields ->> 'initiativeEntryId'))
        AND EXISTS (SELECT 1 FROM combat c
            WHERE c.id::text = lower(r.fields ->> 'initiativeEntryCombatId'))
        AND EXISTS (SELECT 1 FROM token t
            WHERE t.id::text = lower(r.fields ->> 'initiativeEntryTokenId'))
    RETURNING combat_id::text";

/// The current entry the engine holds of the combat `$1` — of every combat
/// when it is null — written on its row once the table holds that entry, or
/// none when the engine names none.
const CURRENT: &str = "UPDATE combat c
    SET current_entry_id = e.id
    FROM dataguard_rows r
    LEFT JOIN initiative_entry e ON e.id::text = lower(r.fields ->> 'currentEntryId')
    WHERE NOT r.tombstoned AND r.partition LIKE 'BattlemapInstance/%'
        AND ($1::text IS NULL OR r.partition = 'BattlemapInstance/' || lower($1::text))
        AND r.partition = 'BattlemapInstance/' || lower(r.fields ->> 'combatId')
        AND c.id::text = lower(r.fields ->> 'combatId')
        AND (e.id IS NOT NULL OR r.fields ->> 'currentEntryId' IS NULL)
        AND c.current_entry_id IS DISTINCT FROM e.id";

/// The end the engine holds of the combat partition `$1` names — of every
/// combat when it is null — written on its row: the one column an end writes
/// beside its current entry (`table.endCombat`), and never back to none
/// (`one-active-combat`). After each apply of an end, and at a start,
/// so an end whose projection failed after its apply is written again. The
/// combat is found by its key, the id its partition names; one the table
/// does not hold yet is the start's to insert, end and all.
const END: &str = "UPDATE combat c
    SET ended_at = (r.fields ->> 'endedAt')::timestamptz
    FROM dataguard_rows r
    WHERE NOT r.tombstoned AND r.partition LIKE 'BattlemapInstance/%'
        AND ($1::text IS NULL OR r.partition = $1::text)
        AND r.partition = 'BattlemapInstance/' || lower(r.fields ->> 'combatId')
        AND c.id::text = lower(r.fields ->> 'combatId')
        AND r.fields ->> 'endedAt' IS NOT NULL
        AND c.ended_at IS DISTINCT FROM (r.fields ->> 'endedAt')::timestamptz";

/// The entry of the partition `$1` — of every entry when it is null — the
/// engine holds tombstoned, out of the table: the row is the read side's, the
/// tombstone the engine keeps (a soft delete, reversible through its journal)
/// is what was removed (`table.removeParticipant`). The entry is found by the
/// partition its id names, which a tombstone keeps whatever its fields. Its
/// token stays; a turn still on it is no one's by the table's foreign key
/// (`currentEntryId`, nullify), and the carousel's places close up
/// (`initiative_order_view`).
const LEAVE: &str = "DELETE FROM initiative_entry e
    USING dataguard_rows r
    WHERE r.tombstoned
        AND ($1::text IS NULL OR r.partition = $1::text)
        AND r.partition = 'BattlemapInstance/' || e.id::text";

/// The combat of the partition `$1` — of every combat when it is null — the
/// engine holds tombstoned, out of the table (`table.resetBattlemap`): its
/// entries go with it by the table's foreign key (`initiativeEntryCombatId`,
/// cascade); the tombstone the engine keeps is what was removed.
const WIPE: &str = "DELETE FROM combat c
    USING dataguard_rows r
    WHERE r.tombstoned
        AND ($1::text IS NULL OR r.partition = $1::text)
        AND r.partition = 'BattlemapInstance/' || c.id::text";

/// The combats the engine holds and the table does not: a projection that
/// failed after its apply, or was lost since.
const UNPROJECTED: &str = "SELECT r.partition FROM dataguard_rows r
    WHERE r.partition LIKE 'BattlemapInstance/%' AND NOT r.tombstoned
        AND r.partition = 'BattlemapInstance/' || lower(r.fields ->> 'combatId')
        AND NOT EXISTS (SELECT 1 FROM combat c WHERE c.id::text = lower(r.fields ->> 'combatId'))
    ORDER BY r.partition";

/// The entries the engine holds and the table does not.
const UNENTERED: &str = "SELECT r.partition FROM dataguard_rows r
    WHERE r.partition LIKE 'BattlemapInstance/%' AND NOT r.tombstoned
        AND r.partition = 'BattlemapInstance/' || lower(r.fields ->> 'initiativeEntryId')
        AND NOT EXISTS (SELECT 1 FROM initiative_entry e
            WHERE e.id::text = lower(r.fields ->> 'initiativeEntryId'))
    ORDER BY r.partition";

/// What the tables hold of the instance a start names — an id the tables
/// can read, lowercase — with the tokens `queued` on it, `(token,
/// character)`: the lineup, none open yet, and the combats of the instance
/// that may be.
///
/// # Errors
///
/// The database's, when it cannot be read.
pub async fn lineup(
    client: &Client,
    instance: &str,
    queued: &[(String, String)],
) -> Result<(Lineup, Vec<String>), tokio_postgres::Error> {
    let known: bool = client.query_one(INSTANCE, &[&instance]).await?.get(0);
    let (ids, characters): (Vec<String>, Vec<String>) = queued.iter().cloned().unzip();
    let tokens = client
        .query(TOKENS, &[&instance, &ids, &characters])
        .await?
        .iter()
        .map(|row| Participant {
            token: row.get(0),
            character: row.get(1),
            kind: row.get(2),
            dex: row.get(3),
        })
        .collect();
    let combats = open(client, instance).await?;
    let lineup = Lineup {
        instance: known,
        tokens,
        open: false,
    };
    Ok((lineup, combats))
}

/// The combats of the instance an end names — an id the tables can read,
/// lowercase — that the table, or the engine, holds with no end: the ones
/// that may be open.
///
/// # Errors
///
/// The database's, when it cannot be read.
pub async fn open(client: &Client, instance: &str) -> Result<Vec<String>, tokio_postgres::Error> {
    Ok(client
        .query(COMBATS, &[&instance])
        .await?
        .iter()
        .map(|row| row.get(0))
        .collect())
}

/// Projects the combat of `partition`, logging a failure — an instance reset
/// since, which the table's foreign keys refuse.
pub async fn project(client: &Client, partition: &str) {
    if let Err(e) = client.execute(PROJECT, &[&partition]).await {
        let db = e.as_db_error();
        eprintln!(
            "projection: {partition} not projected: {} {}",
            db.map_or("-", |db| db.code().code()),
            db.and_then(|db| db.constraint()).unwrap_or_default()
        );
    }
}

/// Projects the entry of `partition`, and every entry of the combat
/// `combat` or of the token `token`, whose combat and token the table holds
/// — then points each combat one was entered in at its current entry —,
/// logging a failure.
pub async fn enter(client: &Client, partition: &str, combat: Option<&str>, token: Option<&str>) {
    match client.query(ENTER, &[&partition, &combat, &token]).await {
        Ok(rows) => {
            let mut combats: Vec<&str> = rows.iter().map(|row| row.get(0)).collect();
            combats.sort_unstable();
            combats.dedup();
            for combat in combats {
                current(client, Some(combat)).await;
            }
        }
        Err(e) => {
            let db = e.as_db_error();
            eprintln!(
                "projection: {partition} not projected: {} {}",
                db.map_or("-", |db| db.code().code()),
                db.and_then(|db| db.constraint()).unwrap_or_default()
            );
        }
    }
}

/// Points the combat `combat` — every combat, `None` — at the current entry
/// the engine holds of it, logging a failure: after each apply of a combat
/// or an entry, and at a start. Tables not migrated yet hold none.
pub async fn current(client: &Client, combat: Option<&str>) {
    if let Err(e) = client.execute(CURRENT, &[&combat]).await {
        eprintln!(
            "projection: combat's current entry not written: {}",
            e.as_db_error().map_or("-", |db| db.code().code())
        );
    }
}

/// Writes the end the engine holds of the combat `partition` names — of
/// every combat, `None` — and points it at its current entry, none once it
/// ended, logging a failure: after each apply of an end, and at a start.
/// Tables not migrated yet hold none.
pub async fn end(client: &Client, partition: Option<&str>) {
    if let Err(e) = client.execute(END, &[&partition]).await {
        eprintln!(
            "projection: combat's end not written: {}",
            e.as_db_error().map_or("-", |db| db.code().code())
        );
    }
    let combat = partition.map(|partition| partition.trim_start_matches("BattlemapInstance/"));
    current(client, combat).await;
}

/// Takes the entry `partition` names out of the table once the engine holds
/// it tombstoned — every such entry, `None` — logging a failure: after each
/// tombstone of the aggregate, and at a start, so a removal whose projection
/// failed after its apply is made again. Tables not migrated yet hold none.
pub async fn leave(client: &Client, partition: Option<&str>) {
    if let Err(e) = client.execute(LEAVE, &[&partition]).await {
        eprintln!(
            "projection: initiative entry not removed: {}",
            e.as_db_error().map_or("-", |db| db.code().code())
        );
    }
}

/// Takes the combat `partition` names out of the table once the engine
/// holds it tombstoned — every such combat, `None` — logging a failure:
/// after each tombstone of the aggregate, and at a start, so a reset whose
/// projection failed after its apply is made again. Tables not migrated yet
/// hold none.
pub async fn wipe(client: &Client, partition: Option<&str>) {
    if let Err(e) = client.execute(WIPE, &[&partition]).await {
        eprintln!(
            "projection: combat not removed: {}",
            e.as_db_error().map_or("-", |db| db.code().code())
        );
    }
}

/// Projects every combat, then every entry, the engine holds and the table
/// does not, one by one, so one the table refuses keeps none of the others
/// out, then writes each combat's end and points it at its current entry:
/// at a start. Tables not migrated yet hold none.
pub async fn repair(client: &Client) {
    for sql in [UNPROJECTED, UNENTERED] {
        match client.query(sql, &[]).await {
            Ok(rows) => {
                for row in rows {
                    let partition = row.get::<_, &str>(0);
                    if sql == UNPROJECTED {
                        project(client, partition).await;
                    } else {
                        enter(client, partition, None, None).await;
                    }
                }
            }
            Err(e) => eprintln!(
                "projection: combat not read: {}",
                e.as_db_error().map_or("-", |db| db.code().code())
            ),
        }
    }
    end(client, None).await;
}
