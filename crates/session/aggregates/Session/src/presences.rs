//! A PC ticked present at a session after its creation, in
//! `session_present_character` (`crates/data/migrations/0006_session.sql`),
//! written by [`Presences`] inside the transaction that applies it.
//!
//! A presence is a partition of its own, `Session/<its id>` ([`presence_id`]:
//! the session's and the PC's, so that ticking a PC again writes the same
//! row, and unticking it names that row), whose row holds
//! `sessionPresentCharacterSessionId`, the one PC under
//! `sessionPresentCharacterCharacterId` — a text, where a session's own row
//! holds a list — and when it was ticked, under `addedAt`, in milliseconds
//! since the epoch. Any other row of the aggregate — a session's, a scene's,
//! an order's — is not a presence's, and writes nothing here.
//!
//! - The PC is a PC — a « pj » with its sheet — of the session's campaign
//!   (`present-only-pj`), judged again here against the tables as they are
//!   when it is applied: one that is not, or a session the tables no longer
//!   hold, rejects the command, and nothing of it is written.
//! - A PC is present once: ticked again, by this command or by the
//!   session's creation, the first row stays, its `added_at` with it.
//!
//! A presence's tombstone writes nothing here: unticking a PC is another
//! command's to write.

use async_trait::async_trait;
use harness_dataguard::plugins::{ProjectPg, ProjectionError};
use harness_dataguard::store::Write;
use harness_dataguard::tokio_postgres::Transaction;
use serde_json::Value;

use crate::scenes::judged;
use crate::sessions::{PRESENT, PRESENT_ONLY_PJ, SESSION_ID};

/// The session a presence is of.
pub const PRESENT_SESSION_ID: &str = "sessionPresentCharacterSessionId";
/// When a PC was ticked present, in milliseconds since the epoch.
pub const ADDED_AT: &str = "addedAt";

/// The session `$1` names, as the tables hold it live: its campaign.
const SESSION: &str = "SELECT s.campaign_id::text FROM session s
    WHERE s.id = $1::text::uuid
        AND NOT EXISTS (SELECT 1 FROM dataguard_rows r
            WHERE r.partition = 'Session/' || s.id::text AND r.tombstoned)";

/// Whether `$2` names a live PC — a « pj » with its sheet — of the
/// campaign `$1`.
const PC: &str = "SELECT EXISTS (SELECT 1 FROM character c JOIN entity e ON e.id = c.entity_id
    WHERE e.id = $2::text::uuid AND e.type = 'pj' AND e.campaign_id = $1::text::uuid
        AND NOT EXISTS (SELECT 1 FROM dataguard_rows r
            WHERE r.partition = 'Entity/' || e.id::text AND r.tombstoned))";

/// The PC present at the session, once: ticked already, the first row
/// stays.
const ADD_PRESENT: &str = "INSERT INTO session_present_character
        (session_id, character_id, added_at)
    VALUES ($1::text::uuid, $2::text::uuid, coalesce(to_timestamp($3::float8 / 1000), now()))
    ON CONFLICT (session_id, character_id) DO NOTHING";

/// The id of the row the presence of `character` at `session` is written
/// on: their ids, lowercase, joined by a dot — never a uuid, so never a
/// session's, a scene's or an order's.
#[must_use]
pub fn presence_id(session: &str, character: &str) -> String {
    format!(
        "{}.{}",
        session.to_ascii_lowercase(),
        character.to_ascii_lowercase()
    )
}

/// The Session aggregate's projection of a PC ticked present into
/// `session_present_character`.
pub struct Presences;

#[async_trait]
impl ProjectPg for Presences {
    /// The PC a presence ticks, at its session. A row that is no presence's,
    /// or a tombstone, writes nothing.
    async fn project(&self, write: &Write, tx: &Transaction<'_>) -> Result<(), ProjectionError> {
        let Some((session, character)) = presence(write) else {
            return Ok(());
        };
        let Some(campaign) = tx.query_opt(SESSION, &[&session]).await.map_err(judged)? else {
            return Err(ProjectionError::Refused(format!(
                "{PRESENT_SESSION_ID} : la séance {session} n'existe pas"
            )));
        };
        let campaign: String = campaign.try_get(0)?;
        let pc: bool = tx
            .query_one(PC, &[&campaign, &character])
            .await
            .map_err(judged)?
            .try_get(0)?;
        if !pc {
            return Err(ProjectionError::Refused(format!(
                "{PRESENT_ONLY_PJ} : seuls des PJ de la campagne sont présents"
            )));
        }
        let added_at = write.values.get(ADDED_AT).and_then(Value::as_f64);
        tx.execute(ADD_PRESENT, &[&session, &character, &added_at])
            .await
            .map_err(judged)?;
        Ok(())
    }
}

/// The session and the PC a write ticks present: `None` for a tombstone, a
/// session's own row — which holds the PCs of its creation as a list — or
/// any row that names no session and no one PC.
fn presence(write: &Write) -> Option<(&str, &str)> {
    let text = |field: &str| write.values.get(field).and_then(Value::as_str);
    if write.tombstone || write.values.contains_key(SESSION_ID) {
        return None;
    }
    Some((text(PRESENT_SESSION_ID)?, text(PRESENT)?))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use harness_dataguard::store::Write;
    use serde_json::{Value, json};

    use super::{PRESENT, PRESENT_SESSION_ID, SESSION_ID, presence, presence_id};
    use crate::{order_id, uuid};

    #[test]
    fn a_presence_s_row_is_its_session_s_and_its_pc_s_and_no_other_row_s() {
        let session = "51000000-0000-4000-8000-00000000000A";
        let pc = "E1000000-0000-4000-8000-000000000004";
        let id = presence_id(session, pc);
        assert_eq!(
            id,
            "51000000-0000-4000-8000-00000000000a.e1000000-0000-4000-8000-000000000004"
        );
        assert_eq!(id, presence_id(&session.to_lowercase(), &pc.to_lowercase()));
        assert_ne!(id, presence_id(pc, session));
        assert!(!uuid(&id));
        assert_ne!(Some(id), order_id(session));
    }

    fn write(values: Value, tombstone: bool) -> Write {
        let values: BTreeMap<String, Value> = serde_json::from_value(values).expect("an object");
        Write {
            partition: "Session/p1".to_owned(),
            values,
            tombstone,
        }
    }

    #[test]
    fn a_presence_is_a_row_naming_its_session_and_one_pc_and_nothing_else_is() {
        let ticked = json!({ PRESENT_SESSION_ID: "s1", PRESENT: "pc" });
        assert_eq!(presence(&write(ticked.clone(), false)), Some(("s1", "pc")));
        // Its tombstone is unticking, another command's to write.
        assert_eq!(presence(&write(ticked, true)), None);
        // A session's own row, its PCs a list.
        assert_eq!(
            presence(&write(
                json!({ SESSION_ID: "s1", PRESENT_SESSION_ID: "s1", PRESENT: "pc" }),
                false
            )),
            None
        );
        for values in [
            json!({ PRESENT_SESSION_ID: "s1", PRESENT: ["pc"] }),
            json!({ PRESENT_SESSION_ID: "s1" }),
            json!({ PRESENT: "pc" }),
            json!({ "sceneId": "c1" }),
        ] {
            assert_eq!(presence(&write(values.clone(), false)), None, "{values}");
        }
    }
}
