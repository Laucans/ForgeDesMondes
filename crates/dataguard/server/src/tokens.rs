//! The tokens of a battlemap instance in the tables (`token`,
//! `crates/data/migrations/0010_battlemap_instance.sql`).
//!
//! A drop (`table.placeTokens`) is judged against what the tables hold of
//! the instance and the character it names before it is queued
//! ([`roster`]): the engine judges a row alone, and a token is one. The
//! numbers taken and the tokens placed are the table's and the engine's
//! rows' together — a token applied whose projection failed is one too —
//! and the queue's, which [`crate::Rows::roster`] adds. Once the engine
//! applied a token on `BattlemapInstance/<its id>`, [`project`] inserts its
//! row; a token the table holds already is left as it is: a later write of
//! its partition (a move, a rename) is not the drop's to project. A token
//! hidden or revealed (`table.setTokenVisibility`) is: [`show`] writes its
//! `visible` column, and no other. So is an NPC's change of HP
//! (`table.changeHitPoints`): [`hit_points`] writes its `hp_current` column,
//! and no other — its death and its band are the table's to derive. A token
//! taken off the map (`table.removeToken`) is a tombstone on its partition:
//! [`remove`] takes its row out of the table, and the table's foreign keys
//! follow the aggregate's relations — its initiative entry goes with it
//! (cascade), a turn still on that entry is no one's (nullify).

use place_tokens::{Character, Roster};
use tokio_postgres::Client;

/// The partitions of the BattlemapInstance aggregate.
const PARTITIONS: &str = "BattlemapInstance/";

/// What the tables hold of the instance `$1` and the character `$2`: its
/// session's campaign, the character's campaign, kind, name and HP max —
/// a PC or an NPC with its sheet alone, which a token's foreign key names —,
/// whether it is present at the session,
/// and the tokens it has on the instance, in the table or in the engine's
/// rows, with the numbers they hold.
const ROSTER: &str = "WITH placed AS (
        SELECT t.id::text AS id, t.number::bigint AS number FROM token t
        WHERE t.instance_id = $1::text::uuid AND t.character_id = $2::text::uuid
        UNION
        SELECT lower(r.fields ->> 'tokenId'), (r.fields ->> 'number')::numeric::bigint
        FROM dataguard_rows r
        WHERE r.partition LIKE 'BattlemapInstance/%' AND NOT r.tombstoned
            AND r.partition = 'BattlemapInstance/' || lower(r.fields ->> 'tokenId')
            AND lower(r.fields ->> 'tokenInstanceId') = $1::text
            AND lower(r.fields ->> 'characterId') = $2::text
    )
    SELECT
        (SELECT s.campaign_id::text FROM battlemap_instance i
            JOIN session s ON s.id = i.session_id WHERE i.id = $1::text::uuid),
        e.campaign_id::text, e.type, e.name, c.hp_max::bigint,
        EXISTS (SELECT 1 FROM battlemap_instance i
            JOIN session_present_character p ON p.session_id = i.session_id
            WHERE i.id = $1::text::uuid AND p.character_id = $2::text::uuid),
        (SELECT count(*) FROM placed),
        (SELECT coalesce(array_agg(number ORDER BY number) FILTER (WHERE number IS NOT NULL), '{}')
            FROM placed)
    FROM (SELECT 1) AS one
    LEFT JOIN (entity e JOIN character c ON c.entity_id = e.id)
        ON e.id = $2::text::uuid AND e.type IN ('pj', 'pnj')";

/// The token the engine holds on partition `$1`, inserted when the table
/// holds it not. Its row is the partition's own: its id is the one the
/// partition names.
const PROJECT: &str = "INSERT INTO token
        (id, instance_id, character_id, number, name, q, r, visible, hp_current, hp_max, conditions)
    SELECT (r.fields ->> 'tokenId')::uuid, (r.fields ->> 'tokenInstanceId')::uuid,
        (r.fields ->> 'characterId')::uuid, (r.fields ->> 'number')::numeric::smallint,
        r.fields ->> 'name', (r.fields ->> 'q')::numeric::integer,
        (r.fields ->> 'r')::numeric::integer, (r.fields ->> 'visible')::boolean,
        (r.fields ->> 'hpCurrent')::numeric::smallint, (r.fields ->> 'hpMax')::numeric::smallint,
        ARRAY(SELECT jsonb_array_elements_text(coalesce(r.fields -> 'conditions', '[]')))
    FROM dataguard_rows r
    WHERE r.partition = $1::text AND NOT r.tombstoned
        AND r.partition = 'BattlemapInstance/' || lower(r.fields ->> 'tokenId')
        AND NOT EXISTS (SELECT 1 FROM token t WHERE t.id = (r.fields ->> 'tokenId')::uuid)";

/// The visibility the engine holds of the token partition `$1` names — of
/// every token when it is null — written on its row: the one column a token
/// hidden or revealed changes (`table.setTokenVisibility`), and the token's
/// own alone, never an initiative rank (`hidden-keeps-rank`). After each
/// apply of a token, and at a start, so a visibility whose projection failed
/// after its apply is written again. The token is found by its key, the id
/// its partition names; one the table does not hold yet is the drop's to
/// insert, and a visibility that is no boolean is left aside, as the
/// DataGuard refuses it.
const SHOW: &str = "UPDATE token t
    SET visible = (r.fields ->> 'visible')::boolean
    FROM dataguard_rows r
    WHERE NOT r.tombstoned
        AND ($1::text IS NULL OR r.partition = $1::text)
        AND r.partition = 'BattlemapInstance/' || lower(r.fields ->> 'tokenId')
        AND t.id = CASE WHEN pg_input_is_valid(r.fields ->> 'tokenId', 'uuid')
            THEN (r.fields ->> 'tokenId')::uuid END
        AND jsonb_typeof(r.fields -> 'visible') = 'boolean'
        AND t.visible IS DISTINCT FROM (r.fields ->> 'visible')::boolean";

/// The current HP the engine holds of the token partition `$1` names — of
/// every token when it is null — written on its row: the one column a change
/// of HP writes (`table.changeHitPoints`), the token's own alone, never
/// another of the same character (`hp-change-one-token`); `dead` and
/// `health_band` follow, generated (`dead-derived`). After each apply of a
/// token, and at a start, so HP whose projection failed after their apply are
/// written again. The token is found by its key, the id its partition names;
/// one the table does not hold yet is the drop's to insert, and HP that are
/// no number — a PC's, which are never tracked — are left aside.
const HIT_POINTS: &str = "UPDATE token t
    SET hp_current = (r.fields ->> 'hpCurrent')::numeric::smallint
    FROM dataguard_rows r
    WHERE NOT r.tombstoned
        AND ($1::text IS NULL OR r.partition = $1::text)
        AND r.partition = 'BattlemapInstance/' || lower(r.fields ->> 'tokenId')
        AND t.id = CASE WHEN pg_input_is_valid(r.fields ->> 'tokenId', 'uuid')
            THEN (r.fields ->> 'tokenId')::uuid END
        AND jsonb_typeof(r.fields -> 'hpCurrent') = 'number'
        AND t.hp_current IS DISTINCT FROM (r.fields ->> 'hpCurrent')::numeric::smallint";

/// The token of the partition `$1` — of every token when it is null — the
/// engine holds tombstoned, out of the table: the row is the read side's, the
/// tombstone the engine keeps (a soft delete, reversible through its
/// journal) is what was removed. The token is found by the partition its id
/// names, which a tombstone keeps whatever its fields; the instance's own
/// partition names no token. Its entry and the turn on it follow by the
/// table's foreign keys (`initiativeEntryTokenId` cascade, `currentEntryId`
/// nullify), and the carousel's places close up (`initiative_order_view`).
const REMOVE: &str = "DELETE FROM token t
    USING dataguard_rows r
    WHERE r.tombstoned
        AND ($1::text IS NULL OR r.partition = $1::text)
        AND r.partition = 'BattlemapInstance/' || t.id::text";

/// The tokens the engine holds and the table does not: a projection that
/// failed after its apply, or was lost since.
const UNPROJECTED: &str = "SELECT r.partition FROM dataguard_rows r
    WHERE r.partition LIKE 'BattlemapInstance/%' AND NOT r.tombstoned
        AND r.partition = 'BattlemapInstance/' || lower(r.fields ->> 'tokenId')
        AND NOT EXISTS (SELECT 1 FROM token t WHERE t.id::text = lower(r.fields ->> 'tokenId'))
    ORDER BY r.partition";

/// What the tables hold of the instance and the character a drop names —
/// ids the tables can read, lowercase.
///
/// # Errors
///
/// The database's, when it cannot be read.
pub async fn roster(
    client: &Client,
    instance: &str,
    character: &str,
) -> Result<Roster, tokio_postgres::Error> {
    let row = client.query_one(ROSTER, &[&instance, &character]).await?;
    let kind: Option<String> = row.get(2);
    let placed: i64 = row.get(6);
    Ok(Roster {
        campaign: row.get(0),
        character: kind.map(|kind| Character {
            campaign: row.get::<_, Option<String>>(1).unwrap_or_default(),
            kind,
            name: row.get::<_, Option<String>>(3).unwrap_or_default(),
            hp_max: row.get(4),
        }),
        present: row.get(5),
        placed: usize::try_from(placed).unwrap_or_default(),
        taken: row.get(7),
    })
}

/// Whether `partition` is one of the aggregate's, where a token lands.
pub fn of_aggregate(partition: &str) -> bool {
    partition.starts_with(PARTITIONS)
}

/// Projects the token of `partition`, logging a failure — an instance reset
/// or a character deleted since, which the table's foreign keys refuse.
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

/// Writes the visibility the engine holds of the token `partition` names —
/// of every token, `None` — on its row, logging a failure: after each apply
/// of a token, and at a start. Tables not migrated yet hold none.
pub async fn show(client: &Client, partition: Option<&str>) {
    if let Err(e) = client.execute(SHOW, &[&partition]).await {
        eprintln!(
            "projection: token visibility not written: {}",
            e.as_db_error().map_or("-", |db| db.code().code())
        );
    }
}

/// Writes the current HP the engine holds of the token `partition` names —
/// of every token, `None` — on its row, logging a failure: after each apply
/// of a token, and at a start. Tables not migrated yet hold none.
pub async fn hit_points(client: &Client, partition: Option<&str>) {
    if let Err(e) = client.execute(HIT_POINTS, &[&partition]).await {
        eprintln!(
            "projection: token HP not written: {}",
            e.as_db_error().map_or("-", |db| db.code().code())
        );
    }
}

/// Takes the token `partition` names out of the table once the engine holds
/// it tombstoned — every such token, `None` — logging a failure: after each
/// tombstone of the aggregate, and at a start, so a removal whose projection
/// failed after its apply is made again. Tables not migrated yet hold none.
pub async fn remove(client: &Client, partition: Option<&str>) {
    if let Err(e) = client.execute(REMOVE, &[&partition]).await {
        eprintln!(
            "projection: token not removed: {}",
            e.as_db_error().map_or("-", |db| db.code().code())
        );
    }
}

/// Projects every token the engine holds and the table does not, one by
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
            "projection: token not read: {}",
            e.as_db_error().map_or("-", |db| db.code().code())
        ),
    }
}
