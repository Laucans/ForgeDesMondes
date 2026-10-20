//! A scene's battlemap instance in the tables (`battlemap_instance`,
//! `crates/data/migrations/0010_battlemap_instance.sql`).
//!
//! A creation (`table.createBattlemapInstance`) is judged against what the
//! tables hold of the session, the scene and the battlemap it names before
//! it is queued ([`placement`]): the engine judges a row alone. Once the
//! engine applied it on `BattlemapInstance/<its id>`, [`project`] inserts its
//! row — the instance alone, never the battlemap's nor its grid's
//! (`instance-isolated`) — and never a second instance of a scene
//! (`one-instance-per-scene`), which the tables then judge a second time. An
//! instance the table holds already is left as it is: a later write of its
//! partition (a reset, its tokens) is not the creation's to project.
//!
//! A reset (`table.resetBattlemap`) is judged against what the instance
//! holds before it is queued ([`map`]): its tokens, its combats and their
//! entries — in the tables, in the engine's rows, or queued, which
//! [`crate::Rows::reset_map`] adds —, none the engine tombstoned. It never
//! writes the instance's partition: each of its deletions carries the
//! instance and the reset's time, which the engine keeps with the command,
//! and once the engine applied all of them, [`stamp`] writes the
//! instance's `reset_at` — a later reset replaces it, none clears it.

use create_battlemap_instance::Placement;
use reset_battlemap::Map;
use tokio_postgres::Client;

/// What the tables hold of the session `$1`, the scene `$2` and the
/// battlemap `$3`: the session's campaign, the scene's session, the
/// battlemap's campaign — a media of another kind is no battlemap — and the
/// scene's instance, each null when there is none.
const PLACEMENT: &str = "SELECT
        (SELECT campaign_id::text FROM session WHERE id = $1::text::uuid),
        (SELECT session_id::text FROM scene WHERE id = $2::text::uuid),
        (SELECT campaign_id::text FROM media WHERE id = $3::text::uuid AND kind = 'battlemap'),
        (SELECT id::text FROM battlemap_instance WHERE scene_id = $2::text::uuid
            ORDER BY created_at, id LIMIT 1)";

/// The instance the engine holds on partition `$1`, inserted when the table holds it not, nor another instance of its
/// scene. Its row is the partition's own: its id is the one the partition
/// names.
const PROJECT: &str = "INSERT INTO battlemap_instance
        (id, battlemap_media_id, session_id, scene_id, created_at)
    SELECT (r.fields ->> 'battlemapInstanceId')::uuid, (r.fields ->> 'battlemapMediaId')::uuid,
        (r.fields ->> 'sessionId')::uuid, (r.fields ->> 'sceneId')::uuid,
        coalesce((r.fields ->> 'createdAt')::timestamptz, now())
    FROM dataguard_rows r
    WHERE r.partition = $1::text AND NOT r.tombstoned
        AND r.partition = 'BattlemapInstance/' || lower(r.fields ->> 'battlemapInstanceId')
        AND r.fields ->> 'sceneId' IS NOT NULL
        AND NOT EXISTS (SELECT 1 FROM battlemap_instance b
            WHERE b.id = (r.fields ->> 'battlemapInstanceId')::uuid
                OR b.scene_id = (r.fields ->> 'sceneId')::uuid)";

/// The instances the engine holds and the table does not: a projection that
/// failed after its apply, or was lost since.
const UNPROJECTED: &str = "SELECT r.partition FROM dataguard_rows r
    WHERE r.partition LIKE 'BattlemapInstance/%' AND NOT r.tombstoned
        AND r.partition = 'BattlemapInstance/' || lower(r.fields ->> 'battlemapInstanceId')
        AND NOT EXISTS (SELECT 1 FROM battlemap_instance b
            WHERE b.id::text = lower(r.fields ->> 'battlemapInstanceId'))
    ORDER BY r.partition";

/// Whether the tables, or the engine, hold the instance `$1`, and its
/// tokens, its combats — open or ended — and the entries of either, in the
/// tables or in the engine's rows, with the tokens `$2` and the combats `$3`
/// queued on it: each by id, none whose partition the engine tombstoned.
const MAP: &str = "WITH tokens AS (
        SELECT t.id::text AS id FROM token t WHERE t.instance_id = $1::text::uuid
        UNION
        SELECT lower(r.fields ->> 'tokenId') FROM dataguard_rows r
        WHERE r.partition LIKE 'BattlemapInstance/%' AND NOT r.tombstoned
            AND r.partition = 'BattlemapInstance/' || lower(r.fields ->> 'tokenId')
            AND lower(r.fields ->> 'tokenInstanceId') = $1::text
        UNION
        SELECT lower(q) FROM unnest($2::text[]) AS q
    ), combats AS (
        SELECT c.id::text AS id FROM combat c WHERE c.instance_id = $1::text::uuid
        UNION
        SELECT lower(r.fields ->> 'combatId') FROM dataguard_rows r
        WHERE r.partition LIKE 'BattlemapInstance/%' AND NOT r.tombstoned
            AND r.partition = 'BattlemapInstance/' || lower(r.fields ->> 'combatId')
            AND lower(r.fields ->> 'combatInstanceId') = $1::text
        UNION
        SELECT lower(q) FROM unnest($3::text[]) AS q
    ), entries AS (
        SELECT e.id::text AS id FROM initiative_entry e
        WHERE e.combat_id::text IN (SELECT id FROM combats)
            OR e.token_id::text IN (SELECT id FROM tokens)
        UNION
        SELECT lower(r.fields ->> 'initiativeEntryId') FROM dataguard_rows r
        WHERE r.partition LIKE 'BattlemapInstance/%' AND NOT r.tombstoned
            AND r.partition = 'BattlemapInstance/' || lower(r.fields ->> 'initiativeEntryId')
            AND (lower(r.fields ->> 'initiativeEntryCombatId') IN (SELECT id FROM combats)
                OR lower(r.fields ->> 'initiativeEntryTokenId') IN (SELECT id FROM tokens))
    ), live AS (
        SELECT 'token' AS kind, id FROM tokens
        UNION ALL SELECT 'combat', id FROM combats
        UNION ALL SELECT 'entry', id FROM entries
    ), standing AS (
        SELECT kind, id FROM live l
        WHERE NOT EXISTS (SELECT 1 FROM dataguard_rows r
            WHERE r.partition = 'BattlemapInstance/' || l.id AND r.tombstoned)
    )
    SELECT EXISTS (SELECT 1 FROM battlemap_instance WHERE id = $1::text::uuid)
            OR EXISTS (SELECT 1 FROM dataguard_rows r
                WHERE r.partition = 'BattlemapInstance/' || $1::text AND NOT r.tombstoned),
        ARRAY(SELECT id FROM standing WHERE kind = 'token' ORDER BY id),
        ARRAY(SELECT id FROM standing WHERE kind = 'combat' ORDER BY id),
        ARRAY(SELECT id FROM standing WHERE kind = 'entry' ORDER BY id)";

/// The last reset the engine applied of each instance — of the command
/// `$1`'s reset alone, or of every instance's when it is null —, as its
/// deletions carry it, written on the instance's row when it is later than
/// the one it holds: a reset replaces the last, none clears it. A reset is
/// the deletions sharing an instance and a time, and is applied once all
/// `resetSize` of them are — the command `$1`, being applied right now,
/// counts as one, the queue marking it a moment after its row is written;
/// at a start, `$1` null, none does (a null would pass `bool_and`).
/// While one is held for review, parked, refused or withdrawn, the stamp
/// waits (`state-persisted-reset`); a token dropped after the reset does not
/// hold it back.
const STAMP: &str = "UPDATE battlemap_instance b SET reset_at = s.at
    FROM (SELECT instance, max(at) AS at
        FROM (SELECT lower(q.entry -> 'payload' ->> 'battlemapInstanceId') AS instance,
                (q.entry -> 'payload' ->> 'resetAt')::timestamptz AS at
            FROM dataguard_queue q
            WHERE split_part(q.data_capability, '@', 1) = 'table.resetBattlemap'
                AND pg_input_is_valid(q.entry -> 'payload' ->> 'resetAt', 'timestamptz')
                AND pg_input_is_valid(q.entry -> 'payload' ->> 'resetSize', 'integer')
            GROUP BY 1, 2
            HAVING bool_and(q.state = 'applied' OR coalesce(q.command = $1::text, false))
                AND count(*) >= max((q.entry -> 'payload' ->> 'resetSize')::integer)
                AND ($1::text IS NULL OR bool_or(q.command = $1::text))) done
        GROUP BY instance) s
    WHERE b.id::text = s.instance AND (b.reset_at IS NULL OR b.reset_at < s.at)";

/// What the tables hold of the session, the scene and the battlemap named —
/// ids the tables can read.
///
/// # Errors
///
/// The database's, when it cannot be read.
pub async fn placement(
    client: &Client,
    session: &str,
    scene: &str,
    battlemap: &str,
) -> Result<Placement, tokio_postgres::Error> {
    let row = client
        .query_one(PLACEMENT, &[&session, &scene, &battlemap])
        .await?;
    Ok(Placement {
        session_campaign: row.get(0),
        scene_session: row.get(1),
        battlemap_campaign: row.get(2),
        instance: row.get(3),
    })
}

/// What the instance named holds — an id the tables can read, lowercase —
/// with the tokens and the combats `queued` on it: what a reset takes off.
///
/// # Errors
///
/// The database's, when it cannot be read.
pub async fn map(
    client: &Client,
    instance: &str,
    tokens: &[String],
    combats: &[String],
) -> Result<Map, tokio_postgres::Error> {
    let row = client
        .query_one(MAP, &[&instance, &tokens, &combats])
        .await?;
    Ok(Map {
        instance: row.get(0),
        tokens: row.get(1),
        combats: row.get(2),
        entries: row.get(3),
    })
}

/// Stamps the instance a reset's deletion `command` carries with its time
/// once every deletion of that reset is applied — every instance with its
/// last reset applied, `None` — logging a failure: after each tombstone of
/// the aggregate, and at a start. Tables not migrated yet hold none.
pub async fn stamp(client: &Client, command: Option<&str>) {
    if let Err(e) = client.execute(STAMP, &[&command]).await {
        eprintln!(
            "projection: instance's reset not stamped: {}",
            e.as_db_error().map_or("-", |db| db.code().code())
        );
    }
}

/// Projects the instance of `partition`, logging a failure — a scene or a
/// session deleted since, which the table's foreign keys refuse.
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

/// Projects every instance the engine holds and the table does not, one by
/// one, so one the table refuses keeps none of the others out: at a start.
/// Tables not migrated yet hold none.
pub async fn repair(client: &Client) {
    match client.query(UNPROJECTED, &[]).await {
        Ok(rows) => {
            for row in rows {
                project(client, row.get::<_, &str>(0)).await;
            }
        }
        Err(e) => eprintln!(
            "projection: battlemap_instance not read: {}",
            e.as_db_error().map_or("-", |db| db.code().code())
        ),
    }
}
