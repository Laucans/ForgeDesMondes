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
//! Once the engine applied a combat on `BattlemapInstance/<its id>`,
//! [`project`] inserts its row, and once it applied an entry on
//! `BattlemapInstance/<its id>`, [`enter`] inserts its: a row the table
//! holds already is left as it is. The combat's current entry is an entry's
//! key, which the combat's row is written before: [`current`] points it at
//! the entry the engine names once the table holds that entry — after each
//! apply of a combat or an entry, and at a start.

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

/// The combats of the instance `$1` the table holds with no end, and every
/// one the engine holds of it: which are still open is the engine's row's
/// to tell, as the queue will leave it.
const COMBATS: &str = "SELECT c.id::text FROM combat c
    WHERE c.instance_id = $1::text::uuid AND c.ended_at IS NULL
        AND NOT EXISTS (SELECT 1 FROM dataguard_rows r
            WHERE r.partition = 'BattlemapInstance/' || c.id::text AND r.tombstoned)
    UNION
    SELECT lower(r.fields ->> 'combatId') FROM dataguard_rows r
    WHERE r.partition LIKE 'BattlemapInstance/%' AND NOT r.tombstoned
        AND r.partition = 'BattlemapInstance/' || lower(r.fields ->> 'combatId')
        AND lower(r.fields ->> 'combatInstanceId') = $1::text";

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

/// The entry the engine holds on partition `$1`, inserted when the table
/// holds it not.
const ENTER: &str = "INSERT INTO initiative_entry
        (id, combat_id, token_id, roll, initiative, dex_score, tiebreak, rank)
    SELECT (r.fields ->> 'initiativeEntryId')::uuid,
        (r.fields ->> 'initiativeEntryCombatId')::uuid,
        (r.fields ->> 'initiativeEntryTokenId')::uuid, (r.fields ->> 'roll')::numeric::smallint,
        (r.fields ->> 'initiative')::numeric::smallint,
        (r.fields ->> 'dexScore')::numeric::smallint,
        (r.fields ->> 'tiebreak')::double precision, (r.fields ->> 'rank')::numeric::integer
    FROM dataguard_rows r
    WHERE r.partition = $1::text AND NOT r.tombstoned
        AND r.partition = 'BattlemapInstance/' || lower(r.fields ->> 'initiativeEntryId')
        AND NOT EXISTS (SELECT 1 FROM initiative_entry e
            WHERE e.id = (r.fields ->> 'initiativeEntryId')::uuid)";

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
    let combats = client
        .query(COMBATS, &[&instance])
        .await?
        .iter()
        .map(|row| row.get(0))
        .collect();
    let lineup = Lineup {
        instance: known,
        tokens,
        open: false,
    };
    Ok((lineup, combats))
}

/// Projects the combat of `partition`, logging a failure — an instance reset
/// since, which the table's foreign keys refuse.
pub async fn project(client: &Client, partition: &str) {
    run(client, PROJECT, partition).await;
}

/// Projects the entry of `partition`, logging a failure — a combat or a
/// token removed since.
pub async fn enter(client: &Client, partition: &str) {
    run(client, ENTER, partition).await;
}

async fn run(client: &Client, sql: &str, partition: &str) {
    if let Err(e) = client.execute(sql, &[&partition]).await {
        let db = e.as_db_error();
        eprintln!(
            "projection: {partition} not projected: {} {}",
            db.map_or("-", |db| db.code().code()),
            db.and_then(|db| db.constraint()).unwrap_or_default()
        );
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

/// Projects every combat, then every entry, the engine holds and the table
/// does not, one by one, so one the table refuses keeps none of the others
/// out, then points each combat at its current entry: at a start. Tables not
/// migrated yet hold none.
pub async fn repair(client: &Client) {
    for (sql, each) in [(UNPROJECTED, PROJECT), (UNENTERED, ENTER)] {
        match client.query(sql, &[]).await {
            Ok(rows) => {
                for row in rows {
                    run(client, each, row.get::<_, &str>(0)).await;
                }
            }
            Err(e) => eprintln!(
                "projection: combat not read: {}",
                e.as_db_error().map_or("-", |db| db.code().code())
            ),
        }
    }
    current(client, None).await;
}
