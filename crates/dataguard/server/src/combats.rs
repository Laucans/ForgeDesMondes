//! The combat of a battlemap instance in the tables (`combat` and
//! `initiative_entry`, `crates/data/migrations/0010_battlemap_instance.sql`).
//!
//! A participant joining a running combat (`table.addParticipant`) is judged
//! against what the tables and the engine's rows hold of the instance's
//! combats, their entries and the token's character before it is queued
//! ([`seating`]): the engine judges a row alone, and an entry is one. The
//! queue's own is [`crate::Rows::seating`]'s to add. An entry the table holds
//! already has its rank written when the engine's differs: the one column a
//! participant joining ahead of it moves.
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
//! comes last. A row the table holds already is left as it is, but for an
//! entry's rank. The combat's
//! current entry is an entry's key: [`current`] points it at the entry the
//! engine names once the table holds that entry — after each entry
//! inserted, after each apply of a combat, and at a start.

use add_participant::Character;
use serde_json::Value;
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

/// The combats of the instance `$1` with no end — in the table, or in the
/// engine's rows, which say whether it ended — that no tombstone took: each
/// id, and whether it is open when the engine's row names no `endedAt` (a
/// start writes none): true for one the table holds running and for one only
/// the engine holds, false for one the table holds ended; by id.
const OPEN_COMBATS: &str = "SELECT c.id::text, true FROM combat c
    WHERE c.instance_id = $1::text::uuid AND c.ended_at IS NULL
        AND NOT EXISTS (SELECT 1 FROM dataguard_rows r
            WHERE r.tombstoned AND r.partition = 'BattlemapInstance/' || c.id::text)
    UNION
    SELECT lower(r.fields ->> 'combatId'),
        NOT EXISTS (SELECT 1 FROM combat c WHERE c.id::text = lower(r.fields ->> 'combatId'))
    FROM dataguard_rows r
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
/// combat and their token, their rank written when the table holds another:
/// an entry's other fields never change once it is seated. One of a token
/// taken off the map is left out. The combats they are of.
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
        AND EXISTS (SELECT 1 FROM combat c
            WHERE c.id::text = lower(r.fields ->> 'initiativeEntryCombatId'))
        AND EXISTS (SELECT 1 FROM token t
            WHERE t.id::text = lower(r.fields ->> 'initiativeEntryTokenId'))
        AND NOT EXISTS (SELECT 1 FROM dataguard_rows t WHERE t.tombstoned
            AND t.partition = 'BattlemapInstance/' || lower(r.fields ->> 'initiativeEntryTokenId'))
    ON CONFLICT (id) DO UPDATE SET rank = EXCLUDED.rank
        WHERE initiative_entry.rank IS DISTINCT FROM EXCLUDED.rank
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

/// The combats the engine holds and the table does not: a projection that
/// failed after its apply, or was lost since.
const UNPROJECTED: &str = "SELECT r.partition FROM dataguard_rows r
    WHERE r.partition LIKE 'BattlemapInstance/%' AND NOT r.tombstoned
        AND r.partition = 'BattlemapInstance/' || lower(r.fields ->> 'combatId')
        AND NOT EXISTS (SELECT 1 FROM combat c WHERE c.id::text = lower(r.fields ->> 'combatId'))
    ORDER BY r.partition";

/// The entries the engine holds and the table does not, or holds at another
/// rank.
const UNENTERED: &str = "SELECT r.partition FROM dataguard_rows r
    WHERE r.partition LIKE 'BattlemapInstance/%' AND NOT r.tombstoned
        AND r.partition = 'BattlemapInstance/' || lower(r.fields ->> 'initiativeEntryId')
        AND NOT EXISTS (SELECT 1 FROM initiative_entry e
            WHERE e.id::text = lower(r.fields ->> 'initiativeEntryId')
                AND e.rank::numeric = (r.fields ->> 'rank')::numeric)
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

/// What the tables and the engine's rows hold of an instance's combats and
/// of a character, as [`seating`] reads them.
#[derive(Debug, Default)]
pub struct Tabled {
    /// The combats with no end, by id, and whether each is open when the
    /// engine's row names no end — the engine's row, when it does, says
    /// better.
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
        .query(OPEN_COMBATS, &[&instance])
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

/// Projects every combat, then every entry, the engine holds and the table
/// does not, one by one, so one the table refuses keeps none of the others
/// out, then points each combat at its current entry: at a start. Tables not
/// migrated yet hold none.
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
    current(client, None).await;
}
