//! A campaign's sessions in their tables, `session` and
//! `session_present_character` (`crates/data/migrations/0006_session.sql`),
//! written by [`Sessions`] inside the transaction that applies a write of a
//! session.
//!
//! A session is a partition of its own, `Session/<its id>`, whose row holds
//! `sessionId` and `sessionCampaignId`: its title, the day it is planned
//! for, its status, its microphone, and the PCs ticked present, as one list
//! of ids under `sessionPresentCharacterCharacterId`. Any other row of the
//! aggregate — a scene's, an order's — is not a session's, and writes
//! nothing here.
//!
//! - Its position is settled here, never at submit: the campaign's row is
//!   locked until the commit and the session takes the position after the
//!   campaign's last, so positions go from 1 to n whatever was queued side
//!   by side.
//! - A oneshot has exactly one session (`oneshot-single-session`): a new
//!   session of a oneshot that holds one rejects the command, and nothing of
//!   it is written.
//! - The PCs ticked present are PCs — a « pj » with its sheet — of the
//!   session's campaign (`present-only-pj`): one that is not rejects the
//!   command, and nothing of it is written. Each is present once.
//!
//! A session written again keeps its position and takes its row's title,
//! day, status and microphone; when its row holds the list of the PCs
//! present, those it no longer names are no longer present. A session's
//! tombstone writes nothing: the tables keep it.

use std::collections::BTreeMap;

use async_trait::async_trait;
use harness_dataguard::plugins::{ProjectPg, ProjectionError};
use harness_dataguard::store::Write;
use harness_dataguard::tokio_postgres::Transaction;
use serde_json::Value;

use crate::scenes::judged;

/// The fields of a session's row.
pub const SESSION_ID: &str = "sessionId";
pub const SESSION_CAMPAIGN_ID: &str = "sessionCampaignId";
pub const SESSION_TITLE: &str = "sessionTitle";
pub const PLANNED_ON: &str = "plannedOn";
pub const SESSION_STATUS: &str = "sessionStatus";
pub const MICROPHONE_ON: &str = "microphoneOn";
/// The PCs ticked present, in order: one list of ids.
pub const PRESENT: &str = "sessionPresentCharacterCharacterId";

/// The invariant a second session of a oneshot breaks.
pub const ONESHOT_SINGLE_SESSION: &str = "oneshot-single-session";
/// The invariant a character that is no PC of the campaign breaks.
pub const PRESENT_ONLY_PJ: &str = "present-only-pj";

/// The kind of campaign that has a single session.
pub const ONESHOT: &str = "oneshot";

/// The campaign `$1` names, its row locked until the commit: its kind.
const CAMPAIGN: &str = "SELECT kind FROM campaign WHERE id = $1::text::uuid FOR UPDATE";

/// Whether the tables hold the session `$1` already.
const KNOWN: &str = "SELECT EXISTS (SELECT 1 FROM session WHERE id = $1::text::uuid)";

/// How many sessions the campaign `$1` holds.
const SESSIONS: &str = "SELECT count(*) FROM session WHERE campaign_id = $1::text::uuid";

/// The session, after the campaign's last; written again, its title, its
/// day, its status and its microphone, its position kept.
const ADD_SESSION: &str = "INSERT INTO session
        (id, campaign_id, position, title, planned_on, status, microphone_on)
    SELECT $1::text::uuid, $2::text::uuid, coalesce(max(s.position), 0) + 1,
        $3, $4::text::date, $5, $6
    FROM session s WHERE s.campaign_id = $2::text::uuid
    ON CONFLICT (id) DO UPDATE SET title = excluded.title,
        planned_on = excluded.planned_on, status = excluded.status,
        microphone_on = excluded.microphone_on";

/// How many of the ids `$2` lists name no PC — a « pj » with its sheet — of
/// the campaign `$1`.
const STRANGERS: &str = "SELECT count(*) FROM unnest($2::text[]) AS listed(id)
    WHERE NOT EXISTS (SELECT 1 FROM character c JOIN entity e ON e.id = c.entity_id
        WHERE e.id::text = lower(listed.id) AND e.type = 'pj'
            AND e.campaign_id = $1::text::uuid)";

/// The PCs of the session `$1` its list `$2` no longer names.
const ABSENT: &str = "DELETE FROM session_present_character
    WHERE session_id = $1::text::uuid AND NOT character_id::text = ANY (
        SELECT lower(listed.id) FROM unnest($2::text[]) AS listed(id))";

/// The PCs it names, each once.
const PRESENT_PCS: &str = "INSERT INTO session_present_character (session_id, character_id)
    SELECT $1::text::uuid, lower(listed.id)::uuid FROM unnest($2::text[]) AS listed(id)
    ON CONFLICT (session_id, character_id) DO NOTHING";

/// The Session aggregate's projection of its sessions into their tables.
pub struct Sessions;

#[async_trait]
impl ProjectPg for Sessions {
    /// The session a write leaves, with its PCs present. A row that is no
    /// session's, or a tombstone, writes nothing.
    async fn project(&self, write: &Write, tx: &Transaction<'_>) -> Result<(), ProjectionError> {
        let text = |field: &str| write.values.get(field).and_then(Value::as_str);
        let (Some(session), Some(campaign)) = (text(SESSION_ID), text(SESSION_CAMPAIGN_ID)) else {
            return Ok(());
        };
        if write.tombstone {
            return Ok(());
        }
        let Some(kind) = tx.query_opt(CAMPAIGN, &[&campaign]).await.map_err(judged)? else {
            return Err(ProjectionError::Refused(format!(
                "{SESSION_CAMPAIGN_ID} : la campagne {campaign} n'existe pas"
            )));
        };
        let kind: String = kind.try_get(0)?;
        let known: bool = tx
            .query_one(KNOWN, &[&session])
            .await
            .map_err(judged)?
            .try_get(0)?;
        if !known && kind == ONESHOT {
            let sessions: i64 = tx
                .query_one(SESSIONS, &[&campaign])
                .await
                .map_err(judged)?
                .try_get(0)?;
            if sessions > 0 {
                return Err(ProjectionError::Refused(format!(
                    "{ONESHOT_SINGLE_SESSION} : un oneshot n'a qu'une séance"
                )));
            }
        }
        let present = present(&write.values);
        if let Some(present) = &present {
            let strangers: i64 = tx
                .query_one(STRANGERS, &[&campaign, present])
                .await
                .map_err(judged)?
                .try_get(0)?;
            if strangers > 0 {
                return Err(ProjectionError::Refused(format!(
                    "{PRESENT_ONLY_PJ} : seuls des PJ de la campagne sont présents"
                )));
            }
        }
        let microphone = write.values.get(MICROPHONE_ON).and_then(Value::as_bool);
        tx.execute(
            ADD_SESSION,
            &[
                &session,
                &campaign,
                &text(SESSION_TITLE),
                &text(PLANNED_ON),
                &text(SESSION_STATUS),
                &microphone,
            ],
        )
        .await
        .map_err(judged)?;
        if let Some(present) = &present {
            for sql in [ABSENT, PRESENT_PCS] {
                tx.execute(sql, &[&session, present])
                    .await
                    .map_err(judged)?;
            }
        }
        Ok(())
    }
}

/// The ids of the PCs a session's row ticks present, in order: `None` when
/// it holds no list.
fn present(row: &BTreeMap<String, Value>) -> Option<Vec<String>> {
    row.get(PRESENT).and_then(Value::as_array).map(|ids| {
        ids.iter()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect()
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{PRESENT, present};

    #[test]
    fn a_session_ticks_the_ids_of_its_list_and_holds_none_without_one() {
        let row = |value| [(PRESENT.to_owned(), value)].into_iter().collect();
        assert_eq!(
            present(&row(json!(["b", "a", 3]))),
            Some(vec!["b".into(), "a".into()])
        );
        assert_eq!(present(&row(json!([]))), Some(Vec::new()));
        assert_eq!(present(&row(json!("a"))), None);
        assert_eq!(present(&std::collections::BTreeMap::new()), None);
    }
}
