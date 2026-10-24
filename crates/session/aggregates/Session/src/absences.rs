//! A PC unticked at a session, out of `session_present_character`
//! (`crates/data/migrations/0006_session.sql`), by [`Absences`] inside the
//! transaction that applies it.
//!
//! A presence is a partition of its own, `Session/<session>.<pc>`: the
//! session's id and the PC's, lowercase, joined by a dot — never a uuid, so
//! never a session's, a scene's or an order's row ([`presence_of`], the
//! inverse of [`crate::presence_id`]). Ticked
//! after the session's creation, the presence's row is written there; ticked
//! at its creation, the PC is one of the session's own row's list, and the
//! presence's partition holds nothing until it is unticked.
//!
//! Unticking a PC is that partition's deletion, which the engine keeps as a
//! tombstone (a soft delete, in the command journal): whatever the
//! partition held, the tombstone takes the one link row of that session and
//! that PC out of the table, and nothing else — the character, its sheet,
//! another session's PCs are never written. A PC who is not present leaves
//! no row to take: nothing changes. `session_roster_view` offers the PCs
//! that remain, and only them (`present-only-pj`).
//!
//! Any other write — a presence ticked, a session's, a scene's, an order's
//! row, or the tombstone of a row that is no presence — writes nothing here.

use async_trait::async_trait;
use harness_dataguard::plugins::{ProjectPg, ProjectionError};
use harness_dataguard::store::Write;
use harness_dataguard::tokio_postgres::Transaction;

use crate::scenes::{PARTITION, judged};
use crate::uuid;

/// The PC `$2` no longer present at the session `$1`: its one link row.
const REMOVE_PRESENT: &str = "DELETE FROM session_present_character
    WHERE session_id = $1::text::uuid AND character_id = $2::text::uuid";

/// The session and the PC the presence `id` is of, lowercase as the tables
/// write them: `None` when `id` is no two uuids joined by a dot.
#[must_use]
pub fn presence_of(id: &str) -> Option<(String, String)> {
    let (session, character) = id.split_once('.')?;
    (uuid(session) && uuid(character))
        .then(|| (session.to_ascii_lowercase(), character.to_ascii_lowercase()))
}

/// The Session aggregate's projection of a PC unticked out of
/// `session_present_character`.
pub struct Absences;

#[async_trait]
impl ProjectPg for Absences {
    /// The link row a presence's tombstone takes out. Any other write
    /// writes nothing.
    async fn project(&self, write: &Write, tx: &Transaction<'_>) -> Result<(), ProjectionError> {
        let Some((session, character)) = absence(write) else {
            return Ok(());
        };
        tx.execute(REMOVE_PRESENT, &[&session, &character])
            .await
            .map_err(judged)?;
        Ok(())
    }
}

/// The session and the PC a write unticks: `None` for any write but a
/// presence's tombstone.
fn absence(write: &Write) -> Option<(String, String)> {
    if !write.tombstone {
        return None;
    }
    presence_of(write.partition.strip_prefix(PARTITION)?)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use harness_dataguard::store::Write;

    use super::{absence, presence_of};
    use crate::{order_id, presence_id};

    const SEANCE: &str = "51000000-0000-4000-8000-00000000000A";
    const PJ: &str = "E1000000-0000-4000-8000-000000000004";

    fn on(partition: &str, tombstone: bool) -> Write {
        Write {
            partition: partition.to_owned(),
            values: BTreeMap::new(),
            tombstone,
        }
    }

    #[test]
    fn a_presence_is_its_session_and_its_pc_joined_by_a_dot_lowercase() {
        assert_eq!(
            presence_of(&format!("{SEANCE}.{PJ}")),
            Some((SEANCE.to_lowercase(), PJ.to_lowercase()))
        );
        // The row a tick writes is the one an untick reads back.
        assert_eq!(
            presence_of(&presence_id(SEANCE, PJ)),
            Some((SEANCE.to_lowercase(), PJ.to_lowercase()))
        );
        // The order of the two ids is the presence's: session first.
        assert_eq!(
            presence_of(&format!("{PJ}.{SEANCE}")),
            Some((PJ.to_lowercase(), SEANCE.to_lowercase()))
        );
        let order = order_id(&SEANCE.to_lowercase()).expect("a uuid");
        for id in [
            SEANCE,
            order.as_str(),
            "",
            ".",
            &format!("{SEANCE}."),
            &format!(".{PJ}"),
            &format!("{SEANCE}.elowen"),
            &format!("s1.{PJ}"),
            &format!("{SEANCE}.{PJ}.{PJ}"),
        ] {
            assert_eq!(presence_of(id), None, "{id}");
        }
    }

    #[test]
    fn only_a_presence_s_tombstone_unticks_a_pc() {
        let presence = format!("Session/{SEANCE}.{PJ}");
        assert_eq!(
            absence(&on(&presence, true)),
            Some((SEANCE.to_lowercase(), PJ.to_lowercase()))
        );
        // A presence ticked is no absence.
        assert_eq!(absence(&on(&presence, false)), None);
        // The tombstone of a session's, a scene's row, or of another
        // aggregate's partition.
        for partition in [
            format!("Session/{SEANCE}"),
            format!("Session{SEANCE}.{PJ}"),
            format!("Entity/{SEANCE}.{PJ}"),
        ] {
            assert_eq!(absence(&on(&partition, true)), None, "{partition}");
        }
    }
}
