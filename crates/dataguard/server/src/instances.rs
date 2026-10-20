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

use create_battlemap_instance::Placement;
use tokio_postgres::Client;

/// The partitions of the BattlemapInstance aggregate.
pub const BATTLEMAP_INSTANCE: &str = "BattlemapInstance/";

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
