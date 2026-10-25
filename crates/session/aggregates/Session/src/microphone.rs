//! A session's status, start and microphone in `session`
//! (`crates/data/migrations/0006_session.sql`), written by [`Microphone`]
//! inside the transaction that applies a write of a session the tables hold.
//!
//! A session is the partition `Session/<its id>`. A write of it that carries
//! `sessionStatus`, `startedAt` or `microphoneOn` moves the same columns of
//! its row, `status`, `started_at` and `microphone_on`; a field the write
//! does not carry is left as it is.
//!
//! - `started_at` is stamped once: a session that has its start keeps it,
//!   whatever a later write says.
//! - Any other row of the aggregate — a scene's, an order's, a presence's, an
//!   entry « à créer » — carries none of the three, and a tombstone, or a
//!   session the tables do not hold, writes nothing: the command is not
//!   rejected for it.
//! - The status's lifecycle (`status-lifecycle`) is the judge's, which
//!   answers before this runs. For a session only the tables hold — a
//!   fixture — [`Microphone::hydrate`] gives the engine the row they hold, so
//!   that the judge sees the status it moves from.
//! - The engine forgets the row of a deleted session, and a write that
//!   follows it starts from nothing: the judge has no status to compare. The
//!   table does not forget: a status taken back from the one the row holds
//!   is rejected here as `status-lifecycle`, so a delete is no way round it.

use std::collections::BTreeMap;

use async_trait::async_trait;
use harness_dataguard::plugins::{ProjectPg, ProjectionError};
use harness_dataguard::store::Write;
use harness_dataguard::tokio_postgres::{Client, Transaction};
use serde_json::Value;

use crate::rank;
use crate::scenes::judged;
use crate::sessions::{MICROPHONE_ON, SESSION_STATUS};
use crate::uuid;

/// When a session went live: in milliseconds since the epoch, or as the
/// timestamp text a DataCapability stamps.
const STARTED_AT: &str = "startedAt";

/// The partition of a session, and of the rows beside it.
const PARTITION: &str = "Session/";

/// The session `$1`, the fields the write carries moved: `$2` its status,
/// `$3` its microphone, `$4` its start in milliseconds, `$5` its start as a
/// text — a field left out (a `NULL`) leaves the column as it is, and a
/// start the session has is kept.
const MOVE: &str = "UPDATE session SET
        status = coalesce($2::text, status),
        microphone_on = coalesce($3::boolean, microphone_on),
        started_at = coalesce(started_at, to_timestamp($4::float8 / 1000.0), $5::text::timestamptz)
    WHERE id = $1::text::uuid";

/// The status the session `$1` holds, its row locked until the commit: what
/// a status written is compared with, whatever the engine remembers.
const HELD: &str = "SELECT status FROM session WHERE id = $1::text::uuid FOR UPDATE";

/// Whether the `session` table exists: a database not migrated yet holds
/// none.
const MIGRATED: &str = "SELECT to_regclass('session') IS NOT NULL";

/// The session `$1` as its row holds it, under the aggregate's fields, unless
/// the engine keeps a tombstone for it: what the judges ask of a session the
/// engine never wrote.
const SESSION_ROW: &str = "SELECT jsonb_strip_nulls(jsonb_build_object(
        'sessionId', s.id, 'sessionCampaignId', s.campaign_id,
        'sessionStatus', s.status, 'microphoneOn', s.microphone_on,
        'startedAt', floor(extract(epoch FROM s.started_at) * 1000)::int8,
        'closedAt', floor(extract(epoch FROM s.closed_at) * 1000)::int8,
        'summary', s.summary, 'summaryModel', s.summary_model
    ))::text
    FROM session s
    WHERE s.id = $1::text::uuid
        AND NOT EXISTS (SELECT 1 FROM dataguard_rows r
            WHERE r.partition = 'Session/' || s.id::text AND r.tombstoned)";

/// The Session aggregate's projection of a session's status, start and
/// microphone into `session`.
pub struct Microphone;

#[async_trait]
impl ProjectPg for Microphone {
    /// The status, the start and the microphone a write of a session
    /// carries. A tombstone, a row that is no session's, or a session the
    /// tables do not hold writes nothing.
    async fn project(&self, write: &Write, tx: &Transaction<'_>) -> Result<(), ProjectionError> {
        let Some(session) = session_id(&write.partition).filter(|_| !write.tombstone) else {
            return Ok(());
        };
        let status = write.values.get(SESSION_STATUS).and_then(Value::as_str);
        let microphone = write.values.get(MICROPHONE_ON).and_then(Value::as_bool);
        let started = write.values.get(STARTED_AT);
        let (millis, text) = (
            started.and_then(Value::as_f64),
            started.and_then(Value::as_str),
        );
        if status.is_none() && microphone.is_none() && millis.is_none() && text.is_none() {
            return Ok(());
        }
        if let Some(status) = status {
            let held = tx.query_opt(HELD, &[&session]).await.map_err(judged)?;
            if let Some(held) = held {
                let before = held.try_get::<_, &str>(0)?;
                if rank(&Value::from(status)) < rank(&Value::from(before)) {
                    return Err(ProjectionError::Refused(format!(
                        "status-lifecycle: session {session} is {before}, it does not go back to {status}"
                    )));
                }
            }
        }
        tx.execute(MOVE, &[&session, &status, &microphone, &millis, &text])
            .await
            .map_err(judged)?;
        Ok(())
    }

    /// A session only the tables hold — a fixture's — as they hold it.
    async fn hydrate(
        &self,
        partition: &str,
        client: &Client,
    ) -> Result<Option<BTreeMap<String, Value>>, ProjectionError> {
        let Some(session) = session_id(partition) else {
            return Ok(None);
        };
        if !client
            .query_one(MIGRATED, &[])
            .await?
            .try_get::<_, bool>(0)?
        {
            return Ok(None);
        }
        let Some(found) = client.query_opt(SESSION_ROW, &[&session]).await? else {
            return Ok(None);
        };
        Ok(match serde_json::from_str(found.try_get::<_, &str>(0)?) {
            Ok(Value::Object(row)) => Some(row.into_iter().collect()),
            _ => None,
        })
    }
}

/// The id a partition of the aggregate names, when it is a uuid: `None` for
/// a presence's, whose id is two joined by a dot. A scene's or an order's
/// is a uuid too, which no session's row answers to.
fn session_id(partition: &str) -> Option<&str> {
    partition.strip_prefix(PARTITION).filter(|id| uuid(id))
}

#[cfg(test)]
mod tests {
    use super::{MICROPHONE_ON, SESSION_ROW, SESSION_STATUS, STARTED_AT, session_id};
    use crate::{SESSION_CAMPAIGN_ID, SESSION_ID, presence_id};

    const SESSION: &str = "51000000-0000-4000-8000-00000000000a";

    #[test]
    fn a_partition_names_a_session_when_its_id_is_a_uuid() {
        assert_eq!(session_id(&format!("Session/{SESSION}")), Some(SESSION));
        for partition in [
            format!("Session/{}", presence_id(SESSION, SESSION)),
            "Session/".to_owned(),
            "Session/nothing".to_owned(),
            format!("Campaign/{SESSION}"),
            SESSION.to_owned(),
        ] {
            assert_eq!(session_id(&partition), None, "{partition}");
        }
    }

    #[test]
    fn the_hydrated_row_names_the_fields_the_aggregate_declares() {
        let aggregate: serde_json::Value =
            serde_json::from_str(include_str!("../aggregate.json")).expect("contract I");
        for field in [
            SESSION_ID,
            SESSION_CAMPAIGN_ID,
            SESSION_STATUS,
            MICROPHONE_ON,
            STARTED_AT,
            "closedAt",
            "summary",
            "summaryModel",
        ] {
            assert!(aggregate["fields"][field].is_object(), "{field}");
            assert!(SESSION_ROW.contains(&format!("'{field}'")), "{field}");
        }
    }
}
