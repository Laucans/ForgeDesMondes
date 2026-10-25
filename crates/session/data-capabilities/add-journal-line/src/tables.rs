//! A session's journal lines in `session_journal_line`
//! (`crates/data/migrations/0006_session.sql`), written by [`JournalLines`]
//! inside the transaction that applies the write that carries one.
//!
//! A write of a session's row carries the session's last line — its id, when
//! it was spoken and its text — whatever the command that wrote it: the line
//! a `session.addJournalLine` adds, and the same line again on every later
//! write of the session. The line is inserted under its id once; the same id
//! again adds nothing. A tombstone, a row with no line, or a session the
//! tables do not hold writes nothing; the lines already there are never
//! changed.

use aggregate_session::uuid;
use async_trait::async_trait;
use harness_dataguard::plugins::{ProjectPg, ProjectionError};
use harness_dataguard::store::Write;
use harness_dataguard::tokio_postgres::Transaction;
use serde_json::Value;

use crate::{LINE_ID, SPOKEN_AT, TEXT};

/// The line `$1` of the session `$2`, spoken at `$3` milliseconds since the
/// epoch or at the timestamp text `$4`, saying `$5`: added once, and only to
/// a session the tables hold.
const ADD_LINE: &str = "INSERT INTO session_journal_line (id, session_id, spoken_at, text)
    SELECT $1::text::uuid, s.id,
        coalesce(to_timestamp($3::float8 / 1000.0), $4::text::timestamptz), $5
    FROM session s WHERE s.id = $2::text::uuid
    ON CONFLICT (id) DO NOTHING";

/// The projection of a session's journal lines into the tables.
pub struct JournalLines;

#[async_trait]
impl ProjectPg for JournalLines {
    /// The line a write of a session carries, added once; any other write,
    /// nothing.
    async fn project(&self, write: &Write, tx: &Transaction<'_>) -> Result<(), ProjectionError> {
        let Some(session) = write
            .partition
            .strip_prefix("Session/")
            .filter(|id| uuid(id))
        else {
            return Ok(());
        };
        let text = |field: &str| write.values.get(field).and_then(Value::as_str);
        let (Some(line), Some(said)) = (text(LINE_ID).filter(|id| uuid(id)), text(TEXT)) else {
            return Ok(());
        };
        if write.tombstone {
            return Ok(());
        }
        let spoken = write.values.get(SPOKEN_AT);
        let (millis, stamp) = (
            spoken.and_then(Value::as_f64),
            spoken.and_then(Value::as_str),
        );
        if millis.is_none() && stamp.is_none() {
            return Ok(());
        }
        tx.execute(ADD_LINE, &[&line, &session, &millis, &stamp, &said])
            .await?;
        Ok(())
    }
}
