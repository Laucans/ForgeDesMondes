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
//!   parent. When a parent lands, the children waiting on it are written
//!   with it — but a token whose character the tables do not hold, which
//!   would have the tables refuse its instance's write.
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
//! lost — a live row no table holds, a tombstoned one a table still holds —
//! each partition alone, instances first, so one the tables refuse keeps no
//! other from its row. And a row only the tables hold (the fixtures',
//! written by SQL) is the engine's row of its partition until a command
//! writes it.

use std::collections::BTreeMap;

use async_trait::async_trait;
use harness_dataguard::plugins::{ProjectPg, ProjectionError};
use harness_dataguard::store::Write;
use harness_dataguard::tokio_postgres::types::ToSql;
use harness_dataguard::tokio_postgres::{Client, Error, Transaction};
use serde_json::Value;

/// The partitions of the aggregate: `BattlemapInstance/<row id>`.
const PARTITION: &str = "BattlemapInstance/";

/// The instance the engine holds on partition `$1`, upserted by its id. Its
/// creation is never written again (`state-persisted-reset`).
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
        scene_id = EXCLUDED.scene_id,
        reset_at = EXCLUDED.reset_at
    WHERE (battlemap_instance.battlemap_media_id, battlemap_instance.session_id,
            battlemap_instance.scene_id, battlemap_instance.reset_at)
        IS DISTINCT FROM (EXCLUDED.battlemap_media_id, EXCLUDED.session_id,
            EXCLUDED.scene_id, EXCLUDED.reset_at)";

/// The token the engine holds on partition `$1`, and every token of the
/// instance `$2` whose character the tables hold, upserted by its id once
/// the table holds its instance: a token waiting on its instance never makes
/// the instance's own write refused.
const UPSERT_TOKENS: &str = "INSERT INTO token
        (id, instance_id, character_id, number, name, q, r, visible, hp_current, hp_max,
         conditions)
    SELECT (v ->> 'tokenId')::uuid, i.id, (v ->> 'characterId')::uuid,
        (v ->> 'number')::numeric::smallint, v ->> 'name',
        (v ->> 'q')::numeric::integer, (v ->> 'r')::numeric::integer,
        (v ->> 'visible')::boolean,
        (v ->> 'hpCurrent')::numeric::smallint, (v ->> 'hpMax')::numeric::smallint,
        ARRAY(SELECT jsonb_array_elements_text(coalesce(nullif(v -> 'conditions', 'null'), '[]')))
    FROM dataguard_rows r
    CROSS JOIN LATERAL (SELECT r.fields::jsonb AS v) held
    JOIN battlemap_instance i ON i.id::text = lower(v ->> 'tokenInstanceId')
    WHERE r.partition LIKE 'BattlemapInstance/%' AND NOT r.tombstoned
        AND r.partition = 'BattlemapInstance/' || lower(v ->> 'tokenId')
        AND (r.partition = $1::text
            OR (i.id::text = $2::text AND EXISTS (SELECT 1 FROM character c
                WHERE c.entity_id::text = lower(v ->> 'characterId'))))
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
        conditions = EXCLUDED.conditions
    WHERE (token.instance_id, token.character_id, token.number, token.name, token.q, token.r,
            token.visible, token.hp_current, token.hp_max, token.conditions)
        IS DISTINCT FROM (EXCLUDED.instance_id, EXCLUDED.character_id, EXCLUDED.number,
            EXCLUDED.name, EXCLUDED.q, EXCLUDED.r, EXCLUDED.visible, EXCLUDED.hp_current,
            EXCLUDED.hp_max, EXCLUDED.conditions)";

/// The combat the engine holds on partition `$1`, and every combat of the
/// instance `$2`, upserted by its id once the table holds its instance. Its
/// current entry is [`POINT_CURRENT`]'s.
const UPSERT_COMBATS: &str = "INSERT INTO combat (id, instance_id, round, started_at, ended_at)
    SELECT (v ->> 'combatId')::uuid, i.id, (v ->> 'round')::numeric::integer,
        (v ->> 'startedAt')::timestamptz, (v ->> 'endedAt')::timestamptz
    FROM dataguard_rows r
    CROSS JOIN LATERAL (SELECT r.fields::jsonb AS v) held
    JOIN battlemap_instance i ON i.id::text = lower(v ->> 'combatInstanceId')
    WHERE r.partition LIKE 'BattlemapInstance/%' AND NOT r.tombstoned
        AND r.partition = 'BattlemapInstance/' || lower(v ->> 'combatId')
        AND (r.partition = $1::text OR i.id::text = $2::text)
    ON CONFLICT (id) DO UPDATE SET
        instance_id = EXCLUDED.instance_id,
        round = EXCLUDED.round,
        started_at = EXCLUDED.started_at,
        ended_at = EXCLUDED.ended_at
    WHERE (combat.instance_id, combat.round, combat.started_at, combat.ended_at)
        IS DISTINCT FROM (EXCLUDED.instance_id, EXCLUDED.round, EXCLUDED.started_at,
            EXCLUDED.ended_at)";

/// The entry the engine holds on partition `$1`, and every entry of the
/// combat, the token or the instance `$2`, upserted by its id once the table
/// holds its combat and its token. A token taken off the map is none the
/// table holds: its entries wait for nothing.
const UPSERT_ENTRIES: &str = "INSERT INTO initiative_entry
        (id, combat_id, token_id, roll, initiative, dex_score, tiebreak, rank)
    SELECT (v ->> 'initiativeEntryId')::uuid, c.id, t.id,
        (v ->> 'roll')::numeric::smallint, (v ->> 'initiative')::numeric::smallint,
        (v ->> 'dexScore')::numeric::smallint, (v ->> 'tiebreak')::double precision,
        (v ->> 'rank')::numeric::integer
    FROM dataguard_rows r
    CROSS JOIN LATERAL (SELECT r.fields::jsonb AS v) held
    JOIN combat c ON c.id::text = lower(v ->> 'initiativeEntryCombatId')
    JOIN token t ON t.id::text = lower(v ->> 'initiativeEntryTokenId')
    WHERE r.partition LIKE 'BattlemapInstance/%' AND NOT r.tombstoned
        AND r.partition = 'BattlemapInstance/' || lower(v ->> 'initiativeEntryId')
        AND (r.partition = $1::text OR c.id::text = $2::text OR t.id::text = $2::text
            OR c.instance_id::text = $2::text)
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
            EXCLUDED.initiative, EXCLUDED.dex_score, EXCLUDED.tiebreak, EXCLUDED.rank)";

/// The current entry the engine holds of each combat the row `$1` bears on
/// — the combat itself, the instance's combats, the combats of an entry or
/// of a token's entries — pointed at that entry once the table holds it,
/// none until then and when the engine names none.
const POINT_CURRENT: &str = "UPDATE combat c SET current_entry_id = e.id
    FROM dataguard_rows r
    LEFT JOIN initiative_entry e ON e.id::text = lower(r.fields ->> 'currentEntryId')
    WHERE r.partition = 'BattlemapInstance/' || c.id::text AND NOT r.tombstoned
        AND (c.id::text = $1::text OR c.instance_id::text = $1::text
            OR c.id IN (SELECT x.combat_id FROM initiative_entry x
                WHERE x.id::text = $1::text OR x.token_id::text = $1::text))
        AND c.current_entry_id IS DISTINCT FROM e.id";

/// A tombstoned row `$1`, out of whichever table holds it — children first:
/// what cascades from it goes with it, by the tables' foreign keys.
const REMOVALS: [&str; 4] = [
    "DELETE FROM initiative_entry WHERE id::text = $1::text",
    "DELETE FROM combat WHERE id::text = $1::text",
    "DELETE FROM token WHERE id::text = $1::text",
    "DELETE FROM battlemap_instance WHERE id::text = $1::text",
];

/// The deferred foreign keys judged now, inside the projection, rather than
/// at the commit: their refusal is the projection's.
const JUDGED_NOW: &str = "SET CONSTRAINTS ALL IMMEDIATE";

/// Whether the tables are migrated: the last of them exists.
const MIGRATED: &str = "SELECT to_regclass('initiative_entry') IS NOT NULL";

/// The partitions whose rows the tables lost: a live row of the aggregate
/// no table holds, a tombstoned one a table still holds. Tombstones first,
/// then instances, tokens, combats and entries: a parent before its
/// children.
const UNPROJECTED: &str = "WITH tabled AS (
        SELECT id::text FROM battlemap_instance
        UNION ALL SELECT id::text FROM token
        UNION ALL SELECT id::text FROM combat
        UNION ALL SELECT id::text FROM initiative_entry
    )
    SELECT r.partition, r.tombstoned FROM dataguard_rows r
    CROSS JOIN LATERAL (SELECT r.fields::jsonb AS v,
        lower(substring(r.partition FROM 19)) AS id) held
    CROSS JOIN LATERAL (SELECT CASE held.id
            WHEN lower(v ->> 'battlemapInstanceId') THEN 0
            WHEN lower(v ->> 'tokenId') THEN 1
            WHEN lower(v ->> 'combatId') THEN 2
            WHEN lower(v ->> 'initiativeEntryId') THEN 3
        END AS kind) of
    WHERE r.partition LIKE 'BattlemapInstance/%'
        AND r.tombstoned = EXISTS (SELECT 1 FROM tabled t WHERE t.id = held.id)
        AND (r.tombstoned OR of.kind IS NOT NULL)
    ORDER BY r.tombstoned DESC, of.kind, r.partition";

/// The row `$1` as the tables hold it, under the aggregate's fields — an
/// instance, a token, a combat or an entry, its id in whatever case; the
/// derived ones are the tables' and stay there.
const ROW: &str = "SELECT jsonb_strip_nulls(found.v)::text FROM (
        SELECT jsonb_build_object('battlemapInstanceId', i.id,
            'battlemapMediaId', i.battlemap_media_id, 'sessionId', i.session_id,
            'sceneId', i.scene_id, 'createdAt', i.created_at, 'resetAt', i.reset_at) AS v
        FROM battlemap_instance i WHERE i.id::text = lower($1::text)
        UNION ALL
        SELECT jsonb_build_object('tokenId', t.id, 'tokenInstanceId', t.instance_id,
            'characterId', t.character_id, 'number', t.number, 'name', t.name, 'q', t.q,
            'r', t.r, 'visible', t.visible, 'hpCurrent', t.hp_current, 'hpMax', t.hp_max,
            'conditions', to_jsonb(t.conditions))
        FROM token t WHERE t.id::text = lower($1::text)
        UNION ALL
        SELECT jsonb_build_object('combatId', c.id, 'combatInstanceId', c.instance_id,
            'round', c.round, 'currentEntryId', c.current_entry_id,
            'startedAt', c.started_at, 'endedAt', c.ended_at)
        FROM combat c WHERE c.id::text = lower($1::text)
        UNION ALL
        SELECT jsonb_build_object('initiativeEntryId', e.id,
            'initiativeEntryCombatId', e.combat_id, 'initiativeEntryTokenId', e.token_id,
            'roll', e.roll, 'initiative', e.initiative, 'dexScore', e.dex_score,
            'tiebreak', e.tiebreak, 'rank', e.rank)
        FROM initiative_entry e WHERE e.id::text = lower($1::text)
    ) found
    LIMIT 1";

/// The BattlemapInstance aggregate's projection into its tables.
pub struct Tables;

/// What a write is projected through: the apply's transaction, or — at a
/// start — the client, inside a transaction it opened.
enum Link<'a> {
    Transaction(&'a Transaction<'a>),
    Client(&'a Client),
}

impl Link<'_> {
    async fn execute(&self, sql: &str, parameters: &[&(dyn ToSql + Sync)]) -> Result<u64, Error> {
        match self {
            Self::Transaction(tx) => tx.execute(sql, parameters).await,
            Self::Client(client) => client.execute(sql, parameters).await,
        }
    }

    async fn batch_execute(&self, sql: &str) -> Result<(), Error> {
        match self {
            Self::Transaction(tx) => tx.batch_execute(sql).await,
            Self::Client(client) => client.batch_execute(sql).await,
        }
    }
}

/// Writes the row of `partition` as the engine holds it — out of its table
/// when it is `tombstoned` — with what waits on it, then has the tables
/// judge it at once.
async fn sync(link: &Link<'_>, partition: &str, tombstoned: bool) -> Result<(), Error> {
    let id = partition
        .strip_prefix(PARTITION)
        .unwrap_or(partition)
        .to_lowercase();
    if tombstoned {
        for sql in REMOVALS {
            link.execute(sql, &[&id]).await?;
        }
    } else {
        link.execute(UPSERT_INSTANCE, &[&partition]).await?;
        for sql in [UPSERT_TOKENS, UPSERT_COMBATS, UPSERT_ENTRIES] {
            link.execute(sql, &[&partition, &id]).await?;
        }
        link.execute(POINT_CURRENT, &[&id]).await?;
    }
    link.batch_execute(JUDGED_NOW).await
}

/// A database error the write is to blame for — a value no column holds
/// (SQLSTATE class 22), a foreign key or a NOT NULL it breaks (class 23) —
/// is the tables refusing it; any other is the store failing.
fn judged(error: Error) -> ProjectionError {
    match error.as_db_error() {
        Some(db)
            if ["22", "23"]
                .iter()
                .any(|class| db.code().code().starts_with(class)) =>
        {
            ProjectionError::Refused(format!(
                "the tables refuse it: {} ({})",
                db.message(),
                db.code().code()
            ))
        }
        _ => ProjectionError::from(error),
    }
}

#[async_trait]
impl ProjectPg for Tables {
    async fn project(&self, write: &Write, tx: &Transaction<'_>) -> Result<(), ProjectionError> {
        sync(&Link::Transaction(tx), &write.partition, write.tombstone)
            .await
            .map_err(judged)
    }

    /// Writes again, each partition in a transaction of its own, what the
    /// tables lost: a failure is logged by its SQLSTATE alone — a value may
    /// be the DM's — and never keeps the server from starting. Tables not
    /// migrated yet hold none.
    async fn warm(&self, client: &Client) -> Result<(), ProjectionError> {
        if !client.query_one(MIGRATED, &[]).await?.get::<_, bool>(0) {
            return Ok(());
        }
        let link = Link::Client(client);
        for lost in client.query(UNPROJECTED, &[]).await? {
            let partition: &str = lost.get(0);
            client.batch_execute("BEGIN").await?;
            match sync(&link, partition, lost.get(1)).await {
                Ok(()) => client.batch_execute("COMMIT").await?,
                Err(e) => {
                    client.batch_execute("ROLLBACK").await?;
                    eprintln!(
                        "projection: {partition} not projected: {}",
                        e.as_db_error().map_or("-", |db| db.code().code())
                    );
                }
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
        let Some(id) = partition.strip_prefix(PARTITION) else {
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
