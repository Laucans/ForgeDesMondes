//! The playbacks of the soundbox in the tables (`sound_playback`,
//! `crates/data/migrations/0009_sound_playback.sql`).
//!
//! A trigger (`soundbox.playSound`) is judged against what the tables hold
//! of the sound it names before it is queued ([`stage`]): the engine judges
//! a row alone, and a playback is one — it cannot see the sound's campaign,
//! nor the session the playback is of. Once the engine applied a playback
//! on `SoundPlayback/<its id>`, [`project`] inserts its row; a playback the
//! table holds already is left as it is: a later write of its partition (a
//! stop) is not the trigger's to project.

use play_sound::Stage;
use tokio_postgres::Client;

/// The partitions of the SoundPlayback aggregate.
const PARTITIONS: &str = "SoundPlayback/";

/// What the tables hold of the sound `$1` and the session `$2` (null when
/// the trigger names none): the sound's campaign, the session that campaign
/// plays in — its live one, else its latest not closed, the highest
/// position, else its latest closed — and the campaign of the session
/// named.
const STAGE: &str = "SELECT s.campaign_id::text,
        (SELECT x.id::text FROM session x WHERE x.campaign_id = s.campaign_id
            ORDER BY x.status = 'live' DESC, x.status <> 'closed' DESC, x.position DESC, x.id
            LIMIT 1),
        (SELECT x.campaign_id::text FROM session x WHERE x.id = $2::text::uuid)
    FROM (SELECT 1) AS one
    LEFT JOIN sound s ON s.id = $1::text::uuid";

/// The playback the engine holds on partition `$1`, inserted when the table
/// holds it not. Its row is the partition's own: its id is the one the
/// partition names.
const PROJECT: &str = "INSERT INTO sound_playback
        (id, session_id, sound_id, origin, started_at, stopped_at)
    SELECT (r.fields ->> 'id')::uuid, (r.fields ->> 'sessionId')::uuid,
        (r.fields ->> 'soundId')::uuid, r.fields ->> 'origin',
        (r.fields ->> 'startedAt')::timestamptz, (r.fields ->> 'stoppedAt')::timestamptz
    FROM dataguard_rows r
    WHERE r.partition = $1::text AND NOT r.tombstoned
        AND r.partition = 'SoundPlayback/' || lower(r.fields ->> 'id')
        AND NOT EXISTS (SELECT 1 FROM sound_playback p WHERE p.id::text = lower(r.fields ->> 'id'))";

/// The playbacks the engine holds and the table does not: a projection that
/// failed after its apply, or was lost since.
const UNPROJECTED: &str = "SELECT r.partition FROM dataguard_rows r
    WHERE r.partition LIKE 'SoundPlayback/%' AND NOT r.tombstoned
        AND r.partition = 'SoundPlayback/' || lower(r.fields ->> 'id')
        AND NOT EXISTS (SELECT 1 FROM sound_playback p WHERE p.id::text = lower(r.fields ->> 'id'))
    ORDER BY r.partition";

/// What the tables hold of the sound and the session a trigger names — ids
/// the tables can read, lowercase.
///
/// # Errors
///
/// The database's, when it cannot be read.
pub async fn stage(
    client: &Client,
    sound: &str,
    session: Option<&str>,
) -> Result<Stage, tokio_postgres::Error> {
    let row = client.query_one(STAGE, &[&sound, &session]).await?;
    Ok(Stage {
        campaign: row.get(0),
        current: row.get(1),
        named: row.get(2),
    })
}

/// Whether `partition` is one of the aggregate's, where a playback lands.
pub fn of_aggregate(partition: &str) -> bool {
    partition.starts_with(PARTITIONS)
}

/// Projects the playback of `partition`, logging a failure — a sound or a
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

/// Projects every playback the engine holds and the table does not, one by
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
            "projection: sound_playback not read: {}",
            e.as_db_error().map_or("-", |db| db.code().code())
        ),
    }
}
