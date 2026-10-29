//! The BattlemapInstance aggregate's tables, `battlemap_instance`, `token`,
//! `combat` and `initiative_entry`
//! (`crates/data/migrations/0010_battlemap_instance.sql`), written by
//! [`Tables`] inside the transaction that applies a write of the aggregate.
//!
//! The engine writes one partition per command, `BattlemapInstance/<id>`,
//! and an instance, a token, a combat and an entry are each a row of their
//! own: the row is the one whose id field names its partition. The engine's
//! row is written first, in the same transaction, so [`Tables`] reads it
//! back from `dataguard_rows` whole, whatever the command changed of it.
//!
//! - A live row is upserted into its table. A token or a combat waits until
//!   the table holds its instance, an entry until it holds its combat and
//!   its token: the engine applies the partitions of a run in the order of
//!   their names, not of their commands, so a child may land before its
//!   parent. When a parent first enters its table, the children waiting on
//!   it follow ([`follow`]). A row that waits is judged when it arrives
//!   ([`alone`]): the tables' own checks on it but for its parents — a value
//!   no column takes, a value they require, a character they do not hold —
//!   reject its command then. One the tables still refuse when its parent
//!   arrives rejects the parent's command whole.
//! - A token carries its character's type, `pj` or `pnj`, which the tables
//!   give it (a trigger) when the engine's row names none: the tables' checks
//!   judge, whatever rows interleave, that a PC tracks no HP, an NPC both,
//!   and that a PC has one token per instance
//!   (`crates/data/migrations/0015_battlemap_token_character_type.sql`).
//! - A combat's current entry is an entry's key: it is pointed at the entry
//!   the engine names once the table holds that entry, and none until then.
//! - A tombstone (a token removed, a participant taken out, a reset) takes
//!   its row out of its table, and what cascades from it: a token's entries,
//!   a combat's entries, a current entry nullified. The engine keeps the
//!   tombstone, the soft delete.
//!
//! The tables' foreign keys and types judge the write a second time, at
//! once (`SET CONSTRAINTS ALL IMMEDIATE`): what they refuse — a value no
//! column takes (SQLSTATE class 22), a session, scene, battlemap or
//! character the tables do not hold (class 23) — rejects the command, whose
//! engine row rolls back with its projection.
//!
//! At a start, [`Tables`] writes again what the engine holds and the tables
//! lost or hold otherwise — a live row no table holds or holds with other
//! values, a tombstoned one a table still holds — each partition alone,
//! instances first, so one the tables refuse keeps no other from its row.
//! And a row only the tables hold (the fixtures', written by SQL) is the
//! engine's row of its partition until a command writes it.

use std::collections::BTreeMap;

use async_trait::async_trait;
use harness_dataguard::plugins::{ProjectPg, ProjectionError};
use harness_dataguard::store::Write;
use harness_dataguard::tokio_postgres::{Client, Error, GenericClient, Transaction};
use serde_json::Value;

/// The partitions of the aggregate: `BattlemapInstance/<row id>`.
const PARTITION: &str = "BattlemapInstance/";

/// The instance the engine holds on partition `$1`, upserted by its id; and
/// whether it entered the table. Its creation is never written again
/// (`state-persisted-reset`), nor its last reset, which the engine's row does
/// not carry.
const UPSERT_INSTANCE: &str = "INSERT INTO battlemap_instance
        (id, battlemap_media_id, session_id, scene_id, created_at, reset_at)
    SELECT (v ->> 'battlemapInstanceId')::uuid, (v ->> 'battlemapMediaId')::uuid,
        (v ->> 'sessionId')::uuid, (v ->> 'sceneId')::uuid,
        coalesce((v ->> 'createdAt')::timestamptz, now()), (v ->> 'resetAt')::timestamptz
    FROM dataguard_rows r
    CROSS JOIN LATERAL (SELECT r.fields::jsonb AS v) held
    WHERE r.partition = $1::text AND NOT r.tombstoned
        AND r.partition = 'BattlemapInstance/' || lower(v ->> 'battlemapInstanceId')
    ON CONFLICT (id) DO UPDATE SET
        battlemap_media_id = EXCLUDED.battlemap_media_id,
        session_id = EXCLUDED.session_id,
        scene_id = EXCLUDED.scene_id
    WHERE (battlemap_instance.battlemap_media_id, battlemap_instance.session_id,
            battlemap_instance.scene_id)
        IS DISTINCT FROM (EXCLUDED.battlemap_media_id, EXCLUDED.session_id,
            EXCLUDED.scene_id)
    RETURNING xmax = 0";

/// A token's columns as the engine's row gives them, its instance as
/// `$instance` reads it.
macro_rules! token_values {
    ($instance:literal) => {
        concat!(
            "(v ->> 'tokenId')::uuid AS id, ",
            $instance,
            " AS instance_id, (v ->> 'characterId')::uuid AS character_id,
            (v ->> 'number')::numeric::smallint AS number, v ->> 'name' AS name,
            (v ->> 'q')::numeric::integer AS q, (v ->> 'r')::numeric::integer AS r,
            (v ->> 'visible')::boolean AS visible,
            (v ->> 'hpCurrent')::numeric::smallint AS hp_current,
            (v ->> 'hpMax')::numeric::smallint AS hp_max, v ->> 'characterType' AS character_type,
            ARRAY(SELECT jsonb_array_elements_text(
                coalesce(nullif(v -> 'conditions', 'null'), '[]'))) AS conditions"
        )
    };
}

/// The token the engine holds on partition `$1`, upserted by its id once the
/// table holds its instance; and whether it entered the table.
const UPSERT_TOKEN: &str = concat!(
    "INSERT INTO token
        (id, instance_id, character_id, number, name, q, r, visible, hp_current, hp_max,
         character_type, conditions)
    SELECT ",
    token_values!("i.id"),
    "
    FROM dataguard_rows r
    CROSS JOIN LATERAL (SELECT r.fields::jsonb AS v) held
    JOIN battlemap_instance i ON i.id = (v ->> 'tokenInstanceId')::uuid
    WHERE r.partition = $1::text AND NOT r.tombstoned
        AND r.partition = 'BattlemapInstance/' || lower(v ->> 'tokenId')
    ON CONFLICT (id) DO UPDATE SET
        instance_id = EXCLUDED.instance_id,
        character_id = EXCLUDED.character_id,
        number = EXCLUDED.number,
        name = EXCLUDED.name,
        q = EXCLUDED.q,
        r = EXCLUDED.r,
        visible = EXCLUDED.visible,
        hp_current = EXCLUDED.hp_current,
        hp_max = EXCLUDED.hp_max,
        character_type = EXCLUDED.character_type,
        conditions = EXCLUDED.conditions
    WHERE (token.instance_id, token.character_id, token.number, token.name, token.q, token.r,
            token.visible, token.hp_current, token.hp_max, token.character_type,
            token.conditions)
        IS DISTINCT FROM (EXCLUDED.instance_id, EXCLUDED.character_id, EXCLUDED.number,
            EXCLUDED.name, EXCLUDED.q, EXCLUDED.r, EXCLUDED.visible, EXCLUDED.hp_current,
            EXCLUDED.hp_max, EXCLUDED.character_type, EXCLUDED.conditions)
    RETURNING xmax = 0"
);

/// A combat's columns as the engine's row gives them, its instance as
/// `$instance` reads it. Its current entry is [`POINT_CURRENT`]'s.
macro_rules! combat_values {
    ($instance:literal) => {
        concat!(
            "(v ->> 'combatId')::uuid AS id, ",
            $instance,
            " AS instance_id, (v ->> 'round')::numeric::integer AS round,
            (v ->> 'startedAt')::timestamptz AS started_at,
            (v ->> 'endedAt')::timestamptz AS ended_at"
        )
    };
}

/// The combat the engine holds on partition `$1`, upserted by its id once
/// the table holds its instance; and whether it entered the table.
const UPSERT_COMBAT: &str = concat!(
    "INSERT INTO combat (id, instance_id, round, started_at, ended_at)
    SELECT ",
    combat_values!("i.id"),
    "
    FROM dataguard_rows r
    CROSS JOIN LATERAL (SELECT r.fields::jsonb AS v) held
    JOIN battlemap_instance i ON i.id = (v ->> 'combatInstanceId')::uuid
    WHERE r.partition = $1::text AND NOT r.tombstoned
        AND r.partition = 'BattlemapInstance/' || lower(v ->> 'combatId')
    ON CONFLICT (id) DO UPDATE SET
        instance_id = EXCLUDED.instance_id,
        round = EXCLUDED.round,
        started_at = EXCLUDED.started_at,
        ended_at = EXCLUDED.ended_at
    WHERE (combat.instance_id, combat.round, combat.started_at, combat.ended_at)
        IS DISTINCT FROM (EXCLUDED.instance_id, EXCLUDED.round, EXCLUDED.started_at,
            EXCLUDED.ended_at)
    RETURNING xmax = 0"
);

/// An entry's columns as the engine's row gives them, its combat and its
/// token as `$combat` and `$token` read them.
macro_rules! entry_values {
    ($combat:literal, $token:literal) => {
        concat!(
            "(v ->> 'initiativeEntryId')::uuid AS id, ",
            $combat,
            " AS combat_id, ",
            $token,
            " AS token_id, (v ->> 'roll')::numeric::smallint AS roll,
            (v ->> 'initiative')::numeric::smallint AS initiative,
            (v ->> 'dexScore')::numeric::smallint AS dex_score,
            (v ->> 'tiebreak')::double precision AS tiebreak,
            (v ->> 'rank')::numeric::integer AS rank"
        )
    };
}

/// The entry the engine holds on partition `$1`, upserted by its id once the
/// table holds its combat and its token — a token taken off the map is none
/// it holds; and whether it entered the table, and its combat.
const UPSERT_ENTRY: &str = concat!(
    "INSERT INTO initiative_entry
        (id, combat_id, token_id, roll, initiative, dex_score, tiebreak, rank)
    SELECT ",
    entry_values!("c.id", "t.id"),
    "
    FROM dataguard_rows r
    CROSS JOIN LATERAL (SELECT r.fields::jsonb AS v) held
    JOIN combat c ON c.id = (v ->> 'initiativeEntryCombatId')::uuid
    JOIN token t ON t.id = (v ->> 'initiativeEntryTokenId')::uuid
    WHERE r.partition = $1::text AND NOT r.tombstoned
        AND r.partition = 'BattlemapInstance/' || lower(v ->> 'initiativeEntryId')
    ON CONFLICT (id) DO UPDATE SET
        combat_id = EXCLUDED.combat_id,
        token_id = EXCLUDED.token_id,
        roll = EXCLUDED.roll,
        initiative = EXCLUDED.initiative,
        dex_score = EXCLUDED.dex_score,
        tiebreak = EXCLUDED.tiebreak,
        rank = EXCLUDED.rank
    WHERE (initiative_entry.combat_id, initiative_entry.token_id, initiative_entry.roll,
            initiative_entry.initiative, initiative_entry.dex_score, initiative_entry.tiebreak,
            initiative_entry.rank)
        IS DISTINCT FROM (EXCLUDED.combat_id, EXCLUDED.token_id, EXCLUDED.roll,
            EXCLUDED.initiative, EXCLUDED.dex_score, EXCLUDED.tiebreak, EXCLUDED.rank)
    RETURNING xmax = 0, combat_id::text"
);

/// The row on partition `$1` when it is live, of the kind `$key` names, and
/// `$parents` finds a parent the tables do not hold: whether it could enter
/// its table but for that parent. Its columns are cast as the upsert casts
/// them, `$required` must all be set, and `$references` must hold; the foreign
/// keys to its parents are left to the day they arrive.
macro_rules! judge {
    ($values:expr, $key:literal, $parents:literal, $required:literal, $references:literal) => {
        concat!(
            "SELECT jsonb_strip_nulls(to_jsonb(p)) ?& ARRAY[",
            $required,
            "] AND ",
            $references,
            " FROM (SELECT ",
            $values,
            " FROM dataguard_rows r
                CROSS JOIN LATERAL (SELECT r.fields::jsonb AS v) held
                WHERE r.partition = $1::text AND NOT r.tombstoned
                    AND r.partition = 'BattlemapInstance/' || lower(v ->> '",
            $key,
            "')
                    AND (",
            $parents,
            ")) p"
        )
    };
}

/// A token whose instance the tables do not hold: judged on its columns and
/// its character.
const JUDGE_TOKEN: &str = judge!(
    token_values!("(v ->> 'tokenInstanceId')::uuid"),
    "tokenId",
    "NOT EXISTS (SELECT 1 FROM battlemap_instance i
                        WHERE i.id = (v ->> 'tokenInstanceId')::uuid)",
    "'id', 'instance_id', 'character_id', 'name', 'q', 'r', 'visible', 'conditions'",
    "EXISTS (SELECT 1 FROM character c WHERE c.entity_id = p.character_id)
        AND token_hp_tracked(
            coalesce(p.character_type, (SELECT e.type FROM entity e WHERE e.id = p.character_id)),
            p.hp_current, p.hp_max)"
);

/// A combat whose instance the tables do not hold: judged on its columns.
const JUDGE_COMBAT: &str = judge!(
    combat_values!("(v ->> 'combatInstanceId')::uuid"),
    "combatId",
    "NOT EXISTS (SELECT 1 FROM battlemap_instance i
                        WHERE i.id = (v ->> 'combatInstanceId')::uuid)",
    "'id', 'instance_id', 'round', 'started_at'",
    "true"
);

/// An entry whose combat or token the tables do not hold: judged on its
/// columns.
const JUDGE_ENTRY: &str = judge!(
    entry_values!(
        "(v ->> 'initiativeEntryCombatId')::uuid",
        "(v ->> 'initiativeEntryTokenId')::uuid"
    ),
    "initiativeEntryId",
    "NOT EXISTS (SELECT 1 FROM combat c WHERE c.id = (v ->> 'initiativeEntryCombatId')::uuid)
                    OR NOT EXISTS (SELECT 1 FROM token t
                        WHERE t.id = (v ->> 'initiativeEntryTokenId')::uuid)",
    "'id', 'combat_id', 'token_id', 'initiative', 'tiebreak', 'rank'",
    "true"
);

/// The current entry the engine holds of the combat `$1`, pointed at that
/// entry once the table holds it; none until then, and when the engine
/// names none.
const POINT_CURRENT: &str = "UPDATE combat c SET current_entry_id = e.id
    FROM dataguard_rows r
    LEFT JOIN initiative_entry e ON e.id = CASE
        WHEN pg_input_is_valid(r.fields ->> 'currentEntryId', 'uuid')
        THEN (r.fields ->> 'currentEntryId')::uuid END
    WHERE c.id = $1::text::uuid
        AND r.partition = 'BattlemapInstance/' || $1::text AND NOT r.tombstoned
        AND c.current_entry_id IS DISTINCT FROM e.id";

/// The live rows the engine holds that wait on the row `$1` to enter their
/// table: an instance's tokens and combats, a combat's or a token's
/// entries.
const WAITING: &str = "SELECT r.partition FROM dataguard_rows r
    WHERE r.partition LIKE 'BattlemapInstance/%' AND NOT r.tombstoned
        AND $1::text IN (lower(r.fields ->> 'tokenInstanceId'),
            lower(r.fields ->> 'combatInstanceId'),
            lower(r.fields ->> 'initiativeEntryCombatId'),
            lower(r.fields ->> 'initiativeEntryTokenId'))
    ORDER BY r.partition";

/// A tombstoned row `$1`, out of whichever table holds it — children first:
/// what cascades from it goes with it, by the tables' foreign keys.
const REMOVALS: [&str; 4] = [
    "DELETE FROM initiative_entry WHERE id = $1::text::uuid",
    "DELETE FROM combat WHERE id = $1::text::uuid",
    "DELETE FROM token WHERE id = $1::text::uuid",
    "DELETE FROM battlemap_instance WHERE id = $1::text::uuid",
];

/// The deferred foreign keys judged now, inside the projection, rather than
/// at the commit: their refusal is the projection's.
const JUDGED_NOW: &str = "SET CONSTRAINTS ALL IMMEDIATE";

/// Every row the four tables hold, by its key, under the aggregate's fields
/// — the derived ones are the tables' and stay there.
macro_rules! tabled {
    () => {
        "SELECT i.id, jsonb_build_object('battlemapInstanceId', i.id,
                'battlemapMediaId', i.battlemap_media_id, 'sessionId', i.session_id,
                'sceneId', i.scene_id, 'createdAt', i.created_at, 'resetAt', i.reset_at) AS v
            FROM battlemap_instance i
        UNION ALL
        SELECT t.id, jsonb_build_object('tokenId', t.id, 'tokenInstanceId', t.instance_id,
                'characterId', t.character_id, 'number', t.number, 'name', t.name, 'q', t.q,
                'r', t.r, 'visible', t.visible, 'hpCurrent', t.hp_current, 'hpMax', t.hp_max,
                'characterType', t.character_type, 'conditions', to_jsonb(t.conditions))
            FROM token t
        UNION ALL
        SELECT c.id, jsonb_build_object('combatId', c.id, 'combatInstanceId', c.instance_id,
                'round', c.round, 'currentEntryId', c.current_entry_id,
                'startedAt', c.started_at, 'endedAt', c.ended_at)
            FROM combat c
        UNION ALL
        SELECT e.id, jsonb_build_object('initiativeEntryId', e.id,
                'initiativeEntryCombatId', e.combat_id, 'initiativeEntryTokenId', e.token_id,
                'roll', e.roll, 'initiative', e.initiative, 'dexScore', e.dex_score,
                'tiebreak', e.tiebreak, 'rank', e.rank)
            FROM initiative_entry e"
    };
}

/// The row `$1` as the tables hold it.
const ROW: &str = concat!(
    "SELECT jsonb_strip_nulls(x.v)::text FROM (",
    tabled!(),
    ") x WHERE x.id = $1::text::uuid LIMIT 1"
);

/// The partitions whose rows the tables lost or hold otherwise: a live row
/// of the aggregate no table holds, or holds with other values — its fields
/// read as the table's, a time as a time, but for an instance's creation and
/// last reset, never written again —, a tombstoned one a table still holds.
/// Tombstones first, then instances, tokens, combats and entries: a parent
/// before its children.
const UNPROJECTED: &str = concat!(
    "WITH held AS (
        SELECT r.partition, r.tombstoned, r.fields::jsonb AS v,
            CASE WHEN pg_input_is_valid(substring(r.partition FROM 19), 'uuid')
                THEN substring(r.partition FROM 19)::uuid END AS id
        FROM dataguard_rows r WHERE r.partition LIKE 'BattlemapInstance/%'
    ),
    tabled AS (",
    tabled!(),
    ")
    SELECT h.partition, h.tombstoned FROM held h
    LEFT JOIN tabled t ON t.id = h.id
    CROSS JOIN LATERAL (SELECT CASE h.id::text
            WHEN lower(h.v ->> 'battlemapInstanceId') THEN 0
            WHEN lower(h.v ->> 'tokenId') THEN 1
            WHEN lower(h.v ->> 'combatId') THEN 2
            WHEN lower(h.v ->> 'initiativeEntryId') THEN 3
        END AS kind) of
    CROSS JOIN LATERAL (SELECT t.v - 'createdAt' - 'resetAt'
            - CASE WHEN h.v ->> 'characterType' IS NULL
                THEN ARRAY['characterType'] ELSE ARRAY[]::text[] END AS v) read
    WHERE CASE WHEN h.tombstoned THEN t.id IS NOT NULL
        ELSE of.kind IS NOT NULL AND (t.id IS NULL
            OR jsonb_strip_nulls(read.v) IS DISTINCT FROM jsonb_strip_nulls((
                SELECT jsonb_object_agg(k.key, CASE
                    WHEN k.key NOT IN ('startedAt', 'endedAt')
                        THEN coalesce(h.v -> k.key, 'null')
                    WHEN pg_input_is_valid(h.v ->> k.key, 'timestamptz')
                        THEN to_jsonb((h.v ->> k.key)::timestamptz)
                    ELSE coalesce(h.v -> k.key, 'null') END)
                FROM jsonb_object_keys(read.v) k (key))))
        END
    ORDER BY h.tombstoned DESC, of.kind, h.partition"
);

/// The BattlemapInstance aggregate's projection into its tables.
pub struct Tables;

/// What a row's write did to its table: whether it entered it — what waits
/// on it may follow —, and the combats whose current entry may now point.
#[derive(Default)]
struct Landed {
    entered: bool,
    combats: Vec<String>,
}

/// The id a partition names, lowercase, when it is a uuid: the key its row
/// has in its table. A table holds no row of another partition.
fn key(partition: &str) -> Option<String> {
    let id = partition.strip_prefix(PARTITION)?.to_ascii_lowercase();
    let hex =
        |part: &str, len: usize| part.len() == len && part.bytes().all(|b| b.is_ascii_hexdigit());
    let parts: Vec<&str> = id.split('-').collect();
    let canonical = parts.len() == 5
        && parts
            .iter()
            .zip([8, 4, 4, 4, 12])
            .all(|(part, len)| hex(part, len));
    canonical.then_some(id)
}

/// Logs what was not projected by its SQLSTATE and its constraint alone: a
/// value may be the DM's.
macro_rules! logged {
    ($what:expr, $error:expr) => {{
        let db = $error.as_db_error();
        eprintln!(
            "projection: {}: {} {}",
            $what,
            db.map_or("-", |db| db.code().code()),
            db.and_then(|db| db.constraint()).unwrap_or_default()
        );
    }};
}

/// Writes the live row of `partition` into its table — none when it is no
/// row of the aggregate, or waits on its parents — and points the combats
/// it bears on at their current entry.
async fn written(db: &impl GenericClient, partition: &str) -> Result<Landed, Error> {
    let mut landed = Landed::default();
    for sql in [UPSERT_INSTANCE, UPSERT_TOKEN, UPSERT_COMBAT, UPSERT_ENTRY] {
        for row in db.query(sql, &[&partition]).await? {
            landed.entered |= row.get::<_, bool>(0);
            if row.len() > 1 {
                landed.combats.push(row.get(1));
            }
        }
    }
    if let Some(id) = key(partition) {
        landed.combats.push(id);
    }
    for combat in &landed.combats {
        db.execute(POINT_CURRENT, &[combat]).await?;
    }
    Ok(landed)
}

/// Whether the tables could take the row of `partition` but for a parent
/// they do not hold yet. A row of a table, or one whose parents are there,
/// is judged by its write alone: this one is judged now, so a command is
/// never applied for a row its parent's arrival would then refuse.
async fn alone(db: &impl GenericClient, partition: &str) -> Result<bool, Error> {
    for sql in [JUDGE_TOKEN, JUDGE_COMBAT, JUDGE_ENTRY] {
        if let Some(row) = db.query_opt(sql, &[&partition]).await?
            && !row.get::<_, bool>(0)
        {
            return Ok(false);
        }
    }
    Ok(true)
}

/// Writes the row of `partition` as the engine holds it — out of its table
/// when it is `tombstoned` — and has the tables judge it at once; then what
/// waited on it.
async fn sync(db: &impl GenericClient, partition: &str, tombstoned: bool) -> Result<(), Error> {
    if tombstoned {
        if let Some(id) = key(partition) {
            for sql in REMOVALS {
                db.execute(sql, &[&id]).await?;
            }
        }
        return db.batch_execute(JUDGED_NOW).await;
    }
    let landed = written(db, partition).await?;
    db.batch_execute(JUDGED_NOW).await?;
    follow(db, partition, landed).await
}

/// The rows waiting on the row of `partition`, once it `landed` in its
/// table, written each in turn — and the rows waiting on them: one the
/// tables refuse fails the write of `partition`, whose command is then
/// rejected whole.
async fn follow(db: &impl GenericClient, partition: &str, landed: Landed) -> Result<(), Error> {
    let mut parents: Vec<(String, Landed)> = vec![(partition.to_owned(), landed)];
    while let Some((parent, landed)) = parents.pop() {
        let Some(id) = key(&parent).filter(|_| landed.entered) else {
            continue;
        };
        for child in db.query(WAITING, &[&id]).await? {
            let child: String = child.get(0);
            let landed = written(db, &child).await?;
            parents.push((child, landed));
        }
    }
    Ok(())
}

/// Whether the tables refuse what was written — a value no column holds
/// (SQLSTATE class 22), a foreign key or a NOT NULL it breaks (class 23) —
/// rather than the store failing.
fn refused(error: &Error) -> bool {
    error.as_db_error().is_some_and(|db| {
        ["22", "23"]
            .iter()
            .any(|class| db.code().code().starts_with(class))
    })
}

/// A write the tables refuse rejects the command; any other error is the
/// store failing.
fn judged(error: Error) -> ProjectionError {
    match error.as_db_error() {
        Some(db) if refused(&error) => ProjectionError::Refused(format!(
            "the tables refuse it: {} ({})",
            db.message(),
            db.code().code()
        )),
        _ => ProjectionError::from(error),
    }
}

#[async_trait]
impl ProjectPg for Tables {
    async fn project(&self, write: &Write, tx: &Transaction<'_>) -> Result<(), ProjectionError> {
        if !write.tombstone && !alone(tx, &write.partition).await.map_err(judged)? {
            return Err(ProjectionError::Refused(
                "the tables refuse it: a value they require is missing, or a character they do not hold"
                    .to_owned(),
            ));
        }
        sync(tx, &write.partition, write.tombstone)
            .await
            .map_err(judged)
    }

    /// Writes again, each partition in a transaction of its own, what the
    /// tables lost or hold otherwise: a failure is logged and never keeps
    /// the server from starting — tables not migrated yet hold none.
    async fn warm(&self, client: &Client) -> Result<(), ProjectionError> {
        let lost = match client.query(UNPROJECTED, &[]).await {
            Ok(lost) => lost,
            Err(e) => {
                logged!("rows not read", e);
                return Ok(());
            }
        };
        for row in lost {
            let partition: &str = row.get(0);
            let projected = async {
                client.batch_execute("BEGIN").await?;
                sync(client, partition, row.get(1)).await?;
                client.batch_execute("COMMIT").await
            }
            .await;
            if let Err(e) = projected {
                // A failed COMMIT has rolled back already; anything else has
                // not.
                if let Err(rollback) = client.batch_execute("ROLLBACK").await {
                    logged!(format!("{partition} not rolled back"), rollback);
                }
                logged!(format!("{partition} not projected"), e);
            }
        }
        Ok(())
    }

    /// A row only the tables hold — a fixture's — as they hold it.
    async fn hydrate(
        &self,
        partition: &str,
        client: &Client,
    ) -> Result<Option<BTreeMap<String, Value>>, ProjectionError> {
        let Some(id) = key(partition) else {
            return Ok(None);
        };
        let Some(found) = client.query_opt(ROW, &[&id]).await? else {
            return Ok(None);
        };
        match serde_json::from_str(found.try_get::<_, &str>(0)?) {
            Ok(Value::Object(row)) => Ok(Some(row.into_iter().collect())),
            _ => Ok(None),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::key;

    #[test]
    fn a_partition_s_key_is_its_uuid_lowercase_and_no_other_text_is_one() {
        assert_eq!(
            key("BattlemapInstance/B1000000-0000-4000-8000-00000000000A").as_deref(),
            Some("b1000000-0000-4000-8000-00000000000a")
        );
        for partition in [
            "BattlemapInstance/a1",
            "BattlemapInstance/b1000000-0000-4000-8000-00000000000",
            "BattlemapInstance/b1000000-0000-4000-8000-00000000000a0",
            "BattlemapInstance/b10000000000-4000-8000-00000000000a-",
            "BattlemapInstance/b1000000-0000-4000-8000-00000000000g",
            "BattlemapInstance/b1000000-0000-4000-8000-0000-0000000a",
            "BattlemapInstance/b1000000-0000-4000-8000-00000000000a-ff",
            "Media/b1000000-0000-4000-8000-00000000000a",
        ] {
            assert_eq!(key(partition), None, "{partition}");
        }
    }
}
