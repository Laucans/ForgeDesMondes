//! The combats of a battlemap instance and their initiative in the tables
//! (`combat`, `initiative_entry`,
//! `crates/data/migrations/0010_battlemap_instance.sql`).
//!
//! A participant joining a running combat (`table.addParticipant`) is judged
//! against what the tables and the engine's rows hold of the instance's
//! combats, their entries and the token's character before it is queued
//! ([`seating`]): the engine judges a row alone, and an entry is one. The
//! queue's own is [`crate::Rows::seating`]'s to add. Once the engine applied
//! an entry on `BattlemapInstance/<its id>`, [`project`] inserts its row —
//! or, the table holding it already, writes its rank, the one column a
//! participant joining ahead of it moves; an entry whose token was taken off
//! the map (a tombstone on the token's partition) is left out, its row gone
//! with the token's (`initiativeEntryTokenId`, cascade).

use add_participant::Character;
use serde_json::Value;
use tokio_postgres::Client;

/// The combats of the instance `$1` with no end — in the table, or in the
/// engine's rows, which say whether it ended — that no tombstone took: each
/// id, and whether the table holds it running, by id.
const COMBATS: &str = "SELECT c.id::text, true FROM combat c
    WHERE c.instance_id = $1::text::uuid AND c.ended_at IS NULL
        AND NOT EXISTS (SELECT 1 FROM dataguard_rows r
            WHERE r.tombstoned AND r.partition = 'BattlemapInstance/' || c.id::text)
    UNION
    SELECT lower(r.fields ->> 'combatId'), false FROM dataguard_rows r
    WHERE r.partition LIKE 'BattlemapInstance/%' AND NOT r.tombstoned
        AND r.partition = 'BattlemapInstance/' || lower(r.fields ->> 'combatId')
        AND lower(r.fields ->> 'combatInstanceId') = $1::text
        AND NOT EXISTS (SELECT 1 FROM combat c
            WHERE c.id::text = lower(r.fields ->> 'combatId') AND c.ended_at IS NULL)
    ORDER BY 1";

/// The entries of the combats `$1` — in the table, with their fields as the
/// engine names them, or in the engine's rows, with none — whose token and
/// own partition no tombstone took: each one's combat, its id and its row.
const ENTRIES: &str = "SELECT e.combat_id::text, e.id::text,
        jsonb_build_object('initiativeEntryId', e.id::text,
            'initiativeEntryCombatId', e.combat_id::text,
            'initiativeEntryTokenId', e.token_id::text, 'roll', e.roll,
            'initiative', e.initiative, 'dexScore', e.dex_score,
            'tiebreak', e.tiebreak, 'rank', e.rank)::text
    FROM initiative_entry e
    WHERE e.combat_id::text = ANY($1::text[])
        AND NOT EXISTS (SELECT 1 FROM dataguard_rows r WHERE r.tombstoned
            AND r.partition IN ('BattlemapInstance/' || e.token_id::text,
                'BattlemapInstance/' || e.id::text))
    UNION ALL
    SELECT lower(r.fields ->> 'initiativeEntryCombatId'), lower(r.fields ->> 'initiativeEntryId'),
        '{}'
    FROM dataguard_rows r
    WHERE r.partition LIKE 'BattlemapInstance/%' AND NOT r.tombstoned
        AND r.partition = 'BattlemapInstance/' || lower(r.fields ->> 'initiativeEntryId')
        AND lower(r.fields ->> 'initiativeEntryCombatId') = ANY($1::text[])
        AND NOT EXISTS (SELECT 1 FROM dataguard_rows t WHERE t.tombstoned
            AND t.partition = 'BattlemapInstance/' || lower(r.fields ->> 'initiativeEntryTokenId'))";

/// The kind and the DEX score of the character `$1`, a PC or an NPC with
/// its sheet.
const CHARACTER: &str = "SELECT e.type, c.dexterity::bigint FROM entity e
    JOIN character c ON c.entity_id = e.id
    WHERE e.id = $1::text::uuid AND e.type IN ('pj', 'pnj')";

/// The entry the engine holds on partition `$1`, inserted when the table
/// holds it not, its rank written when the
/// table holds another: an entry's other fields never change once it is
/// seated. Its row is the partition's own: its id is the one the partition
/// names. One of a token taken off the map is left out.
const PROJECT: &str = "INSERT INTO initiative_entry
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
        AND NOT EXISTS (SELECT 1 FROM dataguard_rows t WHERE t.tombstoned
            AND t.partition = 'BattlemapInstance/' || lower(r.fields ->> 'initiativeEntryTokenId'))
    ON CONFLICT (id) DO UPDATE SET rank = EXCLUDED.rank
        WHERE initiative_entry.rank IS DISTINCT FROM EXCLUDED.rank";

/// The entries the engine holds and the table does not, or holds at another
/// rank: a projection that failed after its apply, or was lost since. One
/// of a token taken off the map is none.
const UNPROJECTED: &str = "SELECT r.partition FROM dataguard_rows r
    WHERE r.partition LIKE 'BattlemapInstance/%' AND NOT r.tombstoned
        AND r.partition = 'BattlemapInstance/' || lower(r.fields ->> 'initiativeEntryId')
        AND NOT EXISTS (SELECT 1 FROM dataguard_rows t WHERE t.tombstoned
            AND t.partition = 'BattlemapInstance/' || lower(r.fields ->> 'initiativeEntryTokenId'))
        AND NOT EXISTS (SELECT 1 FROM initiative_entry e
            WHERE e.id::text = lower(r.fields ->> 'initiativeEntryId')
                AND e.rank::numeric = (r.fields ->> 'rank')::numeric)
    ORDER BY r.partition";

/// What the tables and the engine's rows hold of an instance's combats and
/// of a character, as [`seating`] reads them.
#[derive(Debug, Default)]
pub struct Tabled {
    /// The combats with no end, by id, and whether the table holds it
    /// running — the engine's row, when there is one, says better.
    pub combats: Vec<(String, bool)>,
    /// Their entries: each one's combat, its id, and the table's row of it,
    /// empty when only the engine holds it.
    pub entries: Vec<(String, String, add_participant::Row)>,
    /// The character, when it is a PC or an NPC.
    pub character: Option<Character>,
}

/// What the tables and the engine's rows hold of the combats of the
/// instance `instance` and of the character `character` — ids the tables
/// can read, lowercase.
///
/// # Errors
///
/// The database's, when it cannot be read.
pub async fn seating(
    client: &Client,
    instance: &str,
    character: &str,
) -> Result<Tabled, tokio_postgres::Error> {
    let combats: Vec<(String, bool)> = client
        .query(COMBATS, &[&instance])
        .await?
        .iter()
        .map(|row| (row.get(0), row.get(1)))
        .collect();
    let ids: Vec<&str> = combats.iter().map(|(id, _)| id.as_str()).collect();
    let entries = client
        .query(ENTRIES, &[&ids])
        .await?
        .iter()
        .map(|row| {
            let fields: Value = serde_json::from_str(row.get(2)).unwrap_or_default();
            let fields = match fields {
                Value::Object(fields) => fields.into_iter().collect(),
                _ => add_participant::Row::new(),
            };
            (row.get(0), row.get(1), fields)
        })
        .collect();
    let character = client
        .query_opt(CHARACTER, &[&character])
        .await?
        .map(|row| Character {
            kind: row.get(0),
            dex: row.get(1),
        });
    Ok(Tabled {
        combats,
        entries,
        character,
    })
}

/// Projects the entry of `partition`, logging a failure — a combat the
/// table does not hold, which its foreign key refuses.
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

/// Projects every entry the engine holds and the table does not, or holds
/// at another rank, one by one, so one the table refuses keeps none of the
/// others out: at a start, after the tokens. Tables not migrated yet hold
/// none.
pub async fn repair(client: &Client) {
    match client.query(UNPROJECTED, &[]).await {
        Ok(rows) => {
            for row in rows {
                project(client, row.get::<_, &str>(0)).await;
            }
        }
        Err(e) => eprintln!(
            "projection: initiative entry not read: {}",
            e.as_db_error().map_or("-", |db| db.code().code())
        ),
    }
}
