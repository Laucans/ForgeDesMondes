//! The stops of « Tout arrêter » (`soundbox.stopAll`) in the tables
//! (`sound_playback`, `crates/data/migrations/0009_sound_playback.sql`).
//!
//! A stop-all is judged against what the tables hold before it is queued
//! ([`RUNNING`]): the playbacks still playing in the campaign, or in one of
//! its sessions — the engine judges a row alone, and cannot see which
//! playbacks a campaign has — and the playbacks still queued in one of those
//! sessions ([`SESSIONS`]), which the DM started before « Tout arrêter ». It is queued as one command per playback, on
//! `SoundPlayback/<its id>`; once the engine applied one, [`PROJECT`] writes
//! its stop time on the playback's row, never over one it has: a playback
//! stopped keeps the instant it stopped at (`stop-all`).
//!
//! The DM follows one command, the first; [`Stops`] keeps the others under
//! its id, so that its result says the stop-all applied once every one of
//! them is, and rejected when one is — or when one could not be queued. Stop-alls are prepared and queued one
//! at a time ([`Stops::turn`]): two clicks at once in two tabs never queue
//! two stops on one playback, the second seeing the first's queued.

use std::collections::VecDeque;
use std::sync::Mutex;

/// The partitions of the SoundPlayback aggregate: `SoundPlayback/<its id>`.
pub const PLAYBACK: &str = "SoundPlayback/";

/// The playbacks still playing in the campaign `$1` — in its session `$2`
/// only, when it is not null — by the order they started in.
pub const RUNNING: &str = "SELECT p.id::text FROM sound_playback p
    JOIN session s ON s.id = p.session_id
    WHERE s.campaign_id = $1::text::uuid
        AND ($2::text IS NULL OR p.session_id = $2::text::uuid)
        AND p.stopped_at IS NULL
    ORDER BY p.started_at, p.id";

/// The sessions of the campaign `$1` — the one `$2` names only, when it is
/// not null — whose playbacks a stop-all stops, the ones still queued among
/// them.
pub const SESSIONS: &str = "SELECT id::text FROM session
    WHERE campaign_id = $1::text::uuid AND ($2::text IS NULL OR id = $2::text::uuid)";

/// The stop the engine applied on the playback partition `$1` names — on
/// every one when it is null — written on its row when it has none: after
/// each apply of a playback, and at a start, so a stop whose projection
/// failed after its apply is written again. A stop that is no time is left
/// aside.
pub const PROJECT: &str = "UPDATE sound_playback p
    SET stopped_at = (r.fields ->> 'stoppedAt')::timestamptz
    FROM dataguard_rows r
    WHERE r.partition = 'SoundPlayback/' || p.id::text AND NOT r.tombstoned
        AND ($1::text IS NULL OR r.partition = $1::text)
        AND pg_input_is_valid(r.fields ->> 'stoppedAt', 'timestamptz')
        AND p.stopped_at IS NULL";

/// How many stop-alls are followed: one per click a DM may follow at once,
/// and then some.
pub const KEPT: usize = 16;

/// A command of a stop-all beside the one answered.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Linked {
    /// Queued, under its id.
    Queued(String),
    /// Its submit failed: the server holds no id to follow it by.
    Unqueued,
}

impl Linked {
    /// What a stop that could not be queued stands as: not applied, final.
    #[must_use]
    pub fn unqueued() -> serde_json::Value {
        serde_json::json!({ "status": "rejected", "violations": [] })
    }
}

/// The commands of the last stop-alls, under the id of the one each answers,
/// the newest first.
#[derive(Default)]
pub struct Stops {
    kept: Mutex<VecDeque<(String, Vec<Linked>)>>,
    /// Held while a stop-all reads what plays and queues its stops.
    pub turn: tokio::sync::Mutex<()>,
}

impl Stops {
    /// Keeps `others` under `answered`, forgetting the oldest beyond
    /// [`KEPT`]. A stop-all of one command keeps nothing.
    pub fn keep(&self, answered: &str, others: Vec<Linked>) {
        if others.is_empty() {
            return;
        }
        let mut kept = self.kept.lock().unwrap_or_else(|e| e.into_inner());
        if kept.iter().any(|(id, _)| id == answered) {
            return;
        }
        kept.push_front((answered.to_owned(), others));
        kept.truncate(KEPT);
    }

    /// The other commands of the stop-all `answered` answers, when it is one.
    pub fn of(&self, answered: &str) -> Vec<Linked> {
        let kept = self.kept.lock().unwrap_or_else(|e| e.into_inner());
        kept.iter()
            .find(|(id, _)| id == answered)
            .map(|(_, others)| others.clone())
            .unwrap_or_default()
    }
}

/// The statuses a command ends in.
const FINAL: [&str; 4] = ["applied", "rejected", "cancelled", "expired"];

/// `answered`'s result — `{status, violations, …}` — as the whole stop-all
/// stands, given the results of its `others`: applied once every command is,
/// not yet while one is not settled, and rejected, cancelled or expired as
/// the first that was, with every command's violations.
pub fn settled(mut answered: serde_json::Value, others: &[serde_json::Value]) -> serde_json::Value {
    let status = |result: &serde_json::Value| result["status"].as_str().unwrap_or("").to_owned();
    let all = std::iter::once(&answered).chain(others);
    let pending = all
        .clone()
        .map(status)
        .find(|s| !FINAL.contains(&s.as_str()));
    let failed = all.clone().map(status).find(|s| s != "applied");
    let violations: Vec<serde_json::Value> = all
        .flat_map(|result| result["violations"].as_array().cloned().unwrap_or_default())
        .fold(Vec::new(), |mut seen, violation| {
            if !seen.contains(&violation) {
                seen.push(violation);
            }
            seen
        });
    if let Some(status) = pending.or(failed) {
        answered["status"] = status.into();
    }
    if !violations.is_empty() {
        answered["violations"] = violations.into();
    }
    answered
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn a_stop_all_is_applied_once_every_one_of_its_commands_is() {
        let applied = json!({ "status": "applied", "violations": [] });
        assert_eq!(
            settled(applied.clone(), std::slice::from_ref(&applied))["status"],
            "applied"
        );
        let queued = json!({ "status": "queued", "violations": [] });
        assert_eq!(
            settled(applied.clone(), &[applied.clone(), queued])["status"],
            "queued"
        );
    }

    #[test]
    fn a_stop_all_one_command_of_which_is_rejected_is_rejected_with_its_violations() {
        let applied = json!({ "status": "applied", "violations": [] });
        let rejected = json!({ "status": "rejected", "violations": ["stop-all"] });
        let result = settled(applied, &[rejected.clone(), rejected]);
        assert_eq!(result["status"], "rejected");
        assert_eq!(result["violations"], json!(["stop-all"]));
    }

    fn queued(id: &str) -> Linked {
        Linked::Queued(id.to_owned())
    }

    #[test]
    fn the_commands_of_the_last_stop_alls_are_kept_under_the_one_answered() {
        let stops = Stops::default();
        stops.keep("a", Vec::new());
        assert!(stops.of("a").is_empty());
        stops.keep("a", vec![queued("b"), Linked::Unqueued]);
        stops.keep("a", vec![queued("d")]);
        assert_eq!(stops.of("a"), [queued("b"), Linked::Unqueued]);
        for i in 0..KEPT {
            stops.keep(&format!("x{i}"), vec![queued("y")]);
        }
        assert!(stops.of("a").is_empty(), "the oldest is forgotten");
        assert_eq!(stops.of(&format!("x{}", KEPT - 1)), [queued("y")]);
    }

    #[test]
    fn exactly_as_many_stop_alls_as_kept_are_all_followed_and_one_more_forgets_the_oldest() {
        let stops = Stops::default();
        for i in 0..KEPT {
            stops.keep(&format!("x{i}"), vec![queued("y")]);
        }
        for i in 0..KEPT {
            assert_eq!(stops.of(&format!("x{i}")), [queued("y")], "x{i} kept");
        }
        stops.keep("last", vec![queued("y")]);
        assert!(stops.of("x0").is_empty(), "the oldest is forgotten");
        assert_eq!(stops.of("x1"), [queued("y")], "the next one is kept");
        assert_eq!(stops.of("last"), [queued("y")]);
    }

    #[test]
    fn a_stop_that_could_not_be_queued_leaves_the_stop_all_rejected() {
        let applied = json!({ "status": "applied", "violations": [] });
        assert_eq!(Linked::unqueued()["status"], "rejected");
        assert_eq!(
            settled(applied, &[Linked::unqueued()])["status"],
            "rejected"
        );
    }
}
