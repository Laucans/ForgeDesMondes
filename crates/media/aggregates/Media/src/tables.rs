//! The Media aggregate's tables, `media` and `battlemap_grid`
//! (`crates/data/migrations/0005_media.sql`), written by [`Tables`] inside
//! the transaction that applies a write of a media.
//!
//! - An upload (`media.uploadMedia`) writes its media's partition,
//!   `Media/<its id>`, whole: its `media` row and, for a battlemap, its
//!   `battlemap_grid` row.
//! - A save of a battlemap's grid (`media.saveGridSettings`) writes its six
//!   settings on its `battlemap_grid` row, never `feet_per_hex`, which the
//!   table generates.
//! - A change of a media's place (`media.setMediaPlace`) writes
//!   `media.place_id` as the engine's row holds it, and no other column.
//!
//! The tables' CHECKs and their deferred triggers judge the write a second
//! time, before the commit: what they refuse rejects the command, whose
//! write rolls back with its projection. A place the tables would refuse is
//! left aside (`PLACE_MEDIAS`), as one deleted since: another write of the
//! media never writes it again.
//!
//! At a start, [`Tables`] projects what the engine holds and the tables do
//! not — a media, a grid or a place an earlier server applied and lost —
//! each one alone, so one the tables refuse keeps no other from its row.
//! And a media only the tables hold (the fixtures', written by SQL) is the
//! engine's row of its partition until a command writes it: its kind is
//! judged as the table holds it.

use std::collections::BTreeMap;

use async_trait::async_trait;
use harness_dataguard::plugins::{ProjectPg, ProjectionError};
use harness_dataguard::reads::Rows;
use harness_dataguard::store::Write;
use harness_dataguard::tokio_postgres::{Client, Error, Transaction};
use serde_json::Value;

/// The partitions of the Media aggregate: `Media/<media id>`.
pub const PARTITION: &str = "Media/";

/// One partition's row, as the engine holds it.
pub type Row = BTreeMap<String, Value>;

/// Every live media the engine holds and `media` does not, with its row as
/// JSON text — of the partition `$1` names, of every one when it is null:
/// an upload not projected yet. A row naming no uuid is none the table
/// could hold.
const UNPROJECTED_MEDIA: &str = "SELECT r.partition, r.fields::text FROM dataguard_rows r
    WHERE r.partition LIKE 'Media/%' AND NOT r.tombstoned
        AND ($1::text IS NULL OR r.partition = $1::text)
        AND pg_input_is_valid(r.fields ->> 'id', 'uuid')
        AND NOT EXISTS (SELECT 1 FROM media m WHERE m.id = CASE
            WHEN pg_input_is_valid(r.fields ->> 'id', 'uuid') THEN (r.fields ->> 'id')::uuid END)
    ORDER BY r.partition";

/// An uploaded media, `$1` the engine's row as JSON text: its `media` row.
const ADD_MEDIA: &str = "INSERT INTO media
        (id, campaign_id, kind, title, file_path, width_px, height_px, place_id, uploaded_at)
    SELECT (v ->> 'id')::uuid, (v ->> 'campaignId')::uuid, v ->> 'kind', v ->> 'title',
        v ->> 'filePath', (v ->> 'widthPx')::integer, (v ->> 'heightPx')::integer,
        (v ->> 'placeId')::uuid, coalesce((v ->> 'uploadedAt')::timestamptz, now())
    FROM (SELECT $1::text::jsonb AS v) uploaded";

/// Its grid, when the row holds one — a battlemap's. `feet_per_hex` is the
/// table's to generate (`hex-is-five-feet`).
const ADD_GRID: &str = "INSERT INTO battlemap_grid
        (media_id, hex_size_px, offset_x, offset_y, orientation, color, opacity)
    SELECT (v ->> 'mediaId')::uuid, (v ->> 'hexSizePx')::numeric, (v ->> 'offsetX')::numeric,
        (v ->> 'offsetY')::numeric, v ->> 'orientation', v ->> 'color', (v ->> 'opacity')::numeric
    FROM (SELECT $1::text::jsonb AS v) uploaded
    WHERE v ->> 'mediaId' IS NOT NULL";

/// The grid the engine holds on the media partition `$1` names — on every
/// one when it is null — written on its battlemap's `battlemap_grid` row:
/// the six settings, never `feet_per_hex`. A row that is no battlemap's,
/// misses a setting, or holds one the table's CHECKs or precision would
/// refuse once rounded to its hundredth, is left aside: one such row would
/// fail this one statement and keep every other battlemap from its grid.
const SAVE_GRIDS: &str = "INSERT INTO battlemap_grid
        (media_id, hex_size_px, offset_x, offset_y, orientation, color, opacity)
    SELECT m.id, (v ->> 'hexSizePx')::numeric, (v ->> 'offsetX')::numeric,
        (v ->> 'offsetY')::numeric, v ->> 'orientation', v ->> 'color',
        (v ->> 'opacity')::numeric
    FROM dataguard_rows r
    CROSS JOIN LATERAL (SELECT r.fields::jsonb AS v) saved
    JOIN media m ON r.partition = 'Media/' || m.id::text
    WHERE NOT r.tombstoned
        AND ($1::text IS NULL OR r.partition = $1::text)
        AND m.kind = 'battlemap'
        AND jsonb_typeof(v -> 'hexSizePx') = 'number'
        AND jsonb_typeof(v -> 'offsetX') = 'number'
        AND jsonb_typeof(v -> 'offsetY') = 'number'
        AND jsonb_typeof(v -> 'orientation') = 'string'
        AND jsonb_typeof(v -> 'color') = 'string'
        AND jsonb_typeof(v -> 'opacity') = 'number'
        AND round((v ->> 'hexSizePx')::numeric, 2) BETWEEN 0.01 AND 99999.99
        AND abs(round((v ->> 'offsetX')::numeric, 2)) <= 99999.99
        AND abs(round((v ->> 'offsetY')::numeric, 2)) <= 99999.99
        AND v ->> 'orientation' IN ('pointe', 'plat')
        AND round((v ->> 'opacity')::numeric, 2) BETWEEN 0 AND 1
    ON CONFLICT (media_id) DO UPDATE SET
        hex_size_px = EXCLUDED.hex_size_px,
        offset_x = EXCLUDED.offset_x,
        offset_y = EXCLUDED.offset_y,
        orientation = EXCLUDED.orientation,
        color = EXCLUDED.color,
        opacity = EXCLUDED.opacity
    WHERE (battlemap_grid.hex_size_px, battlemap_grid.offset_x, battlemap_grid.offset_y,
            battlemap_grid.orientation, battlemap_grid.color, battlemap_grid.opacity)
        IS DISTINCT FROM (EXCLUDED.hex_size_px, EXCLUDED.offset_x, EXCLUDED.offset_y,
            EXCLUDED.orientation, EXCLUDED.color, EXCLUDED.opacity)";

/// The place the engine holds on the media partition `$1` names — on every
/// one when it is null; none when its row has no `placeId` — written on its
/// `media` row, and no other column, where it differs. A place no uuid
/// names, or one `media-place-is-lieu` would refuse — an element the tables
/// do not hold, no lieu, or one of another campaign — is left aside: one
/// such row would fail this one statement and keep every other media from
/// its place.
const PLACE_MEDIAS: &str = "UPDATE media m SET place_id = placed.place_id
    FROM (SELECT r.partition, CASE WHEN pg_input_is_valid(r.fields ->> 'placeId', 'uuid')
            THEN (r.fields ->> 'placeId')::uuid END AS place_id
        FROM dataguard_rows r
        WHERE r.partition LIKE 'Media/%' AND NOT r.tombstoned
            AND ($1::text IS NULL OR r.partition = $1::text)
            AND (r.fields ->> 'placeId' IS NULL
                OR pg_input_is_valid(r.fields ->> 'placeId', 'uuid'))) placed
    WHERE placed.partition = 'Media/' || m.id::text
        AND m.place_id IS DISTINCT FROM placed.place_id
        AND (placed.place_id IS NULL OR EXISTS (SELECT 1 FROM entity e
            WHERE e.id = placed.place_id AND e.type = 'lieu'
                AND e.campaign_id = m.campaign_id))";

/// A media as its tables hold it — its row and its battlemap's grid — under
/// the aggregate's fields, `$1` its id in whatever case. A media the engine
/// deleted (its tombstone) is none.
const MEDIA_ROW: &str = "SELECT jsonb_strip_nulls(jsonb_build_object(
        'id', m.id, 'campaignId', m.campaign_id, 'kind', m.kind, 'title', m.title,
        'filePath', m.file_path, 'widthPx', m.width_px, 'heightPx', m.height_px,
        'placeId', m.place_id, 'uploadedAt', m.uploaded_at,
        'mediaId', g.media_id, 'hexSizePx', g.hex_size_px, 'offsetX', g.offset_x,
        'offsetY', g.offset_y, 'orientation', g.orientation, 'color', g.color,
        'opacity', g.opacity, 'feetPerHex', g.feet_per_hex
    ))::text
    FROM media m LEFT JOIN battlemap_grid g ON g.media_id = m.id
    WHERE m.id::text = lower($1)
        AND NOT EXISTS (SELECT 1 FROM dataguard_rows r
            WHERE r.partition = 'Media/' || m.id::text AND r.tombstoned)";

/// The element `$1` names, in whatever case, as a media's place reads it —
/// its type and its campaign, under the Entity aggregate's fields — when
/// the tables hold it live: one the engine deleted (its tombstone) is none.
const PLACE: &str = "SELECT e.type AS \"type\", e.campaign_id::text AS \"entityCampaignId\"
    FROM entity e
    WHERE e.id::text = lower($1)
        AND NOT EXISTS (SELECT 1 FROM dataguard_rows r
            WHERE r.partition = 'Entity/' || e.id::text AND r.tombstoned)";

/// The deferred triggers judged now, inside the projection, rather than at
/// the commit: their refusal is the projection's.
const JUDGED_NOW: &str = "SET CONSTRAINTS ALL IMMEDIATE";

/// The Media aggregate's projection into its tables.
pub struct Tables;

#[async_trait]
impl ProjectPg for Tables {
    /// Its media first, an upload's — SAVE_GRIDS writes only the grid of a
    /// media the table holds — then its grid, then its place, which
    /// PLACE_MEDIAS writes only on a media the table holds; then the
    /// tables' deferred triggers. A tombstone writes nothing.
    async fn project(&self, write: &Write, tx: &Transaction<'_>) -> Result<(), ProjectionError> {
        if write.tombstone {
            return Ok(());
        }
        let partition = Some(write.partition.as_str());
        let unprojected = tx
            .query(UNPROJECTED_MEDIA, &[&partition])
            .await
            .map_err(judged)?;
        if !unprojected.is_empty() {
            let row = Value::Object(write.values.clone().into_iter().collect()).to_string();
            for sql in [ADD_MEDIA, ADD_GRID] {
                tx.execute(sql, &[&row]).await.map_err(judged)?;
            }
        }
        for sql in [SAVE_GRIDS, PLACE_MEDIAS] {
            tx.execute(sql, &[&partition]).await.map_err(judged)?;
        }
        tx.batch_execute(JUDGED_NOW).await.map_err(judged)
    }

    /// Projects what the engine holds and the tables do not, each media
    /// alone, then every grid and every place: a failure is logged by its
    /// SQLSTATE and never keeps the server from starting. Tables not
    /// migrated yet hold none.
    async fn warm(&self, client: &Client) -> Result<(), ProjectionError> {
        // Its SQLSTATE and its constraint alone: never a value.
        let logged = |what: &str, error: &Error| {
            let db = error.as_db_error();
            eprintln!(
                "projection: {what}: {} {}",
                db.map_or("-", |db| db.code().code()),
                db.and_then(|db| db.constraint()).unwrap_or_default()
            );
        };
        match client.query(UNPROJECTED_MEDIA, &[&None::<&str>]).await {
            Ok(unprojected) => {
                for media in unprojected {
                    let added = match (media.try_get::<_, &str>(0), media.try_get::<_, &str>(1)) {
                        (Ok(partition), Ok(row)) => added(client, row)
                            .await
                            .map_err(|e| (partition.to_owned(), e)),
                        (Err(e), _) | (_, Err(e)) => Err((String::from(PARTITION), e)),
                    };
                    if let Err((partition, e)) = added {
                        logged(&format!("{partition} not projected"), &e);
                    }
                }
            }
            Err(e) => logged("media not read", &e),
        }
        for (sql, what) in [
            (SAVE_GRIDS, "grids not saved"),
            (PLACE_MEDIAS, "media not placed"),
        ] {
            if let Err(e) = client.execute(sql, &[&None::<&str>]).await {
                logged(what, &e);
            }
        }
        Ok(())
    }

    /// A media only the tables hold — a fixture's — as its tables hold it.
    async fn hydrate(
        &self,
        partition: &str,
        client: &Client,
    ) -> Result<Option<Row>, ProjectionError> {
        let Some(id) = partition.strip_prefix(PARTITION) else {
            return Ok(None);
        };
        let Some(found) = client.query_opt(MEDIA_ROW, &[&id]).await? else {
            return Ok(None);
        };
        Ok(object(
            serde_json::from_str(found.try_get::<_, &str>(0)?).ok(),
        ))
    }
}

/// The element `id` names as the tables hold it, under the Entity
/// aggregate's fields (`type`, `entityCampaignId`): what a media's place is
/// judged against (`media-place-is-lieu`) — the fixtures' elements among
/// them. `None` when they hold no such live element.
///
/// # Errors
///
/// The read's: the tables cannot be read.
pub async fn place(rows: &dyn Rows, id: &str) -> harness_dataguard::Result<Option<Row>> {
    let found = rows.rows(PLACE, &[Value::from(id)]).await?;
    Ok(object(
        found.as_array().and_then(|found| found.first()).cloned(),
    ))
}

/// An uploaded media's rows, `row` its engine's row as JSON text, in one
/// transaction: the deferred triggers judge the media and its grid together.
async fn added(client: &Client, row: &str) -> Result<(), Error> {
    client.batch_execute("BEGIN").await?;
    let written = async {
        for sql in [ADD_MEDIA, ADD_GRID] {
            client.execute(sql, &[&row]).await?;
        }
        client.batch_execute("COMMIT").await
    }
    .await;
    if written.is_err() {
        // A failed COMMIT has rolled back already; anything else has not.
        client.batch_execute("ROLLBACK").await?;
    }
    written
}

/// What the tables refuse — a CHECK, a foreign key, a trigger's
/// `check_violation`, all of class 23 — rejects the command; any other
/// error fails the step.
fn judged(error: Error) -> ProjectionError {
    match error.as_db_error() {
        Some(db) if db.code().code().starts_with("23") => {
            ProjectionError::Refused(db.message().to_owned())
        }
        _ => error.into(),
    }
}

/// A JSON object as a row; anything else is none.
fn object(value: Option<Value>) -> Option<Row> {
    match value {
        Some(Value::Object(row)) => Some(row.into_iter().collect()),
        _ => None,
    }
}
