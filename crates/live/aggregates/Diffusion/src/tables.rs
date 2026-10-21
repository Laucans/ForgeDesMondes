//! The Diffusion aggregate's table, `diffusion`
//! (`crates/data/migrations/0012_diffusion.sql`), written by [`Tables`]
//! inside the transaction that applies a write of a session's diffusion.
//!
//! - Every write of `Diffusion/<session id>` — a base sent
//!   (`live.showOnPlayerView`), and whatever the window's commands write —
//!   upserts its session's `diffusion` row with the engine's row whole: the
//!   one row both player views read (single-diffusion-source).
//! - A diffusion only the table holds (a fixture's, written by SQL) is the
//!   engine's row of its partition until a command writes it: its window is
//!   judged as the table holds it, and a command writing its base keeps it.
//!
//! The table's CHECKs and foreign keys judge the write a second time, before
//! the commit: a base media or instance it does not hold rejects the
//! command, whose write rolls back with its projection.

use std::collections::BTreeMap;

use async_trait::async_trait;
use harness_dataguard::plugins::{ProjectPg, ProjectionError};
use harness_dataguard::store::Write;
use harness_dataguard::tokio_postgres::{Client, Error, Transaction};
use serde_json::Value;

/// The partitions of the Diffusion aggregate: `Diffusion/<session id>`.
pub const PARTITION: &str = "Diffusion/";

/// One partition's row, as the engine holds it.
pub type Row = BTreeMap<String, Value>;

/// The engine's row of the partition `$1` names, written on its session's
/// `diffusion` row: every column, a window field the row does not hold
/// empty, the framing 1 and 0, 0 when it holds none, the stamp now when it
/// holds none.
const PROJECT: &str = "INSERT INTO diffusion (session_id, base_kind, base_media_id,
        base_instance_id, window_media_id, window_x, window_y, window_width, view_zoom,
        view_pan_x, view_pan_y, updated_at)
    SELECT (v ->> 'sessionId')::uuid, v ->> 'baseKind', (v ->> 'baseMediaId')::uuid,
        (v ->> 'baseInstanceId')::uuid, (v ->> 'windowMediaId')::uuid,
        (v ->> 'windowX')::numeric, (v ->> 'windowY')::numeric, (v ->> 'windowWidth')::numeric,
        coalesce((v ->> 'viewZoom')::numeric, 1), coalesce((v ->> 'viewPanX')::numeric, 0),
        coalesce((v ->> 'viewPanY')::numeric, 0),
        coalesce((v ->> 'updatedAt')::timestamptz, now())
    FROM dataguard_rows r
    CROSS JOIN LATERAL (SELECT r.fields::jsonb AS v) written
    WHERE r.partition = $1::text AND NOT r.tombstoned
    ON CONFLICT (session_id) DO UPDATE SET
        base_kind = EXCLUDED.base_kind,
        base_media_id = EXCLUDED.base_media_id,
        base_instance_id = EXCLUDED.base_instance_id,
        window_media_id = EXCLUDED.window_media_id,
        window_x = EXCLUDED.window_x,
        window_y = EXCLUDED.window_y,
        window_width = EXCLUDED.window_width,
        view_zoom = EXCLUDED.view_zoom,
        view_pan_x = EXCLUDED.view_pan_x,
        view_pan_y = EXCLUDED.view_pan_y,
        updated_at = EXCLUDED.updated_at";

/// A diffusion as its table holds it, under the aggregate's fields, `$1`
/// its session's id in whatever case — looked up by its key; a text that is
/// no uuid names none. One the engine deleted (its tombstone) is none.
const DIFFUSION_ROW: &str = "SELECT jsonb_strip_nulls(jsonb_build_object(
        'sessionId', d.session_id, 'baseKind', d.base_kind, 'baseMediaId', d.base_media_id,
        'baseInstanceId', d.base_instance_id, 'windowMediaId', d.window_media_id,
        'windowX', d.window_x, 'windowY', d.window_y, 'windowWidth', d.window_width,
        'viewZoom', d.view_zoom, 'viewPanX', d.view_pan_x, 'viewPanY', d.view_pan_y,
        'updatedAt', d.updated_at
    ))::text
    FROM diffusion d
    WHERE d.session_id = CASE WHEN pg_input_is_valid($1::text, 'uuid') THEN $1::text::uuid END
        AND NOT EXISTS (SELECT 1 FROM dataguard_rows r
            WHERE r.partition = 'Diffusion/' || d.session_id::text AND r.tombstoned)";

/// The Diffusion aggregate's projection into its table.
pub struct Tables;

#[async_trait]
impl ProjectPg for Tables {
    /// The session's `diffusion` row, as the engine's row now is. A
    /// tombstone writes nothing: `PROJECT` reads live rows only.
    async fn project(&self, write: &Write, tx: &Transaction<'_>) -> Result<(), ProjectionError> {
        tx.execute(PROJECT, &[&write.partition])
            .await
            .map_err(judged)?;
        Ok(())
    }

    /// A diffusion only the table holds — a fixture's — as it holds it.
    async fn hydrate(
        &self,
        partition: &str,
        client: &Client,
    ) -> Result<Option<Row>, ProjectionError> {
        let Some(session) = partition.strip_prefix(PARTITION) else {
            return Ok(None);
        };
        let Some(found) = client.query_opt(DIFFUSION_ROW, &[&session]).await? else {
            return Ok(None);
        };
        match serde_json::from_str(found.try_get::<_, &str>(0)?) {
            Ok(Value::Object(row)) => Ok(Some(row.into_iter().collect())),
            _ => Ok(None),
        }
    }
}

/// What the table refuses — a CHECK, a foreign key (class 23), a value it
/// cannot hold (class 22) — rejects the command; any other error fails the
/// step.
fn judged(error: Error) -> ProjectionError {
    match error.as_db_error() {
        Some(db)
            if ["22", "23"]
                .iter()
                .any(|class| db.code().code().starts_with(class)) =>
        {
            ProjectionError::Refused(db.message().to_owned())
        }
        _ => error.into(),
    }
}
