//! A session's close in the product's tables (`crates/data/migrations/
//! 0006_session.sql`, `0009_sound_playback.sql`), written by [`Closes`]
//! inside the transaction that applies it.
//!
//! - A session's row the engine never wrote — one the fixtures or a server
//!   before the engine wrote — is read from `session` ([`ProjectPg::hydrate`]):
//!   the close is judged on the status the tables hold, and refused on a
//!   session in prep or closed already.
//! - A write that leaves a live session closed lands on its `session` row:
//!   its status, its close, its microphone off, its summary and the model
//!   that wrote it. Every playback of the session still running stops at
//!   the close (`stop-all`); one that stopped keeps the instant it stopped
//!   at. The journal lines are never written: they stay as they were.
//!
//! A session the tables hold closed already is never closed again: a later
//! write of it keeps its close, its summary and its stops as they are. Any
//! other write of the aggregate — a session still in prep or live, a scene,
//! an entry « à créer », a partition whose id is no uuid — writes nothing
//! here.

use std::collections::BTreeMap;

use async_trait::async_trait;
use harness_dataguard::plugins::{ProjectPg, ProjectionError};
use harness_dataguard::store::Write;
use harness_dataguard::tokio_postgres::{Client, Transaction};
use serde_json::Value;

use crate::{CLOSED, CLOSED_AT, STATUS, SUMMARY, SUMMARY_MODEL, uuid};

/// The session `$1` names, as the aggregate names its fields; the fields
/// the row holds no value for left out.
const SESSION: &str = "SELECT jsonb_strip_nulls(jsonb_build_object(
        'sessionId', id::text,
        'sessionCampaignId', campaign_id::text,
        'sessionPosition', position,
        'sessionTitle', title,
        'plannedOn', planned_on,
        'sessionStatus', status,
        'startedAt', started_at,
        'closedAt', closed_at,
        'microphoneOn', microphone_on,
        'summary', summary,
        'summaryModel', summary_model,
        'summaryEditedAt', summary_edited_at))
    FROM session WHERE id = $1::text::uuid";

/// The close of the live session `$1`: closed at `$2`, its microphone off,
/// its summary `$3` written by the model `$4`; then every playback of it
/// still running, stopped at its close.
const CLOSE: &str = "WITH closed AS (
        UPDATE session
        SET status = 'closed', closed_at = $2::text::timestamptz, microphone_on = false,
            summary = $3, summary_model = $4
        WHERE id = $1::text::uuid AND status = 'live'
        RETURNING id, closed_at)
    UPDATE sound_playback p SET stopped_at = closed.closed_at
    FROM closed WHERE p.session_id = closed.id AND p.stopped_at IS NULL";

/// The Session aggregate's projection of a session's close into its tables.
pub struct Closes;

#[async_trait]
impl ProjectPg for Closes {
    /// A session left closed, on its row and its running playbacks; any
    /// other write, nothing.
    async fn project(&self, write: &Write, tx: &Transaction<'_>) -> Result<(), ProjectionError> {
        let text = |field: &str| write.values.get(field).and_then(Value::as_str);
        let id = session_id(&write.partition);
        if write.tombstone || text(STATUS) != Some(CLOSED) || !uuid(id) {
            return Ok(());
        }
        let summary = (text(SUMMARY), text(SUMMARY_MODEL));
        tx.execute(CLOSE, &[&id, &text(CLOSED_AT), &summary.0, &summary.1])
            .await?;
        Ok(())
    }

    /// The session's row as `session` holds it; `None` for a partition no
    /// session's.
    async fn hydrate(
        &self,
        partition: &str,
        client: &Client,
    ) -> Result<Option<BTreeMap<String, Value>>, ProjectionError> {
        let id = session_id(partition);
        if !uuid(id) {
            return Ok(None);
        }
        let Some(row) = client.query_opt(SESSION, &[&id]).await? else {
            return Ok(None);
        };
        serde_json::from_value(row.try_get(0)?)
            .map(Some)
            .map_err(|e| ProjectionError::Failed(e.to_string()))
    }
}

/// The id a partition of the aggregate names: `Session/<id>`.
fn session_id(partition: &str) -> &str {
    partition.split_once('/').map_or(partition, |(_, id)| id)
}
