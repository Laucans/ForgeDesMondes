//! DataCapability `soundbox.stopAll` (`data-capability.json`): one click on
//! « Tout arrêter » silences everything playing. Every playback still
//! playing in the campaign — in the one session the DM names, or in any of
//! its sessions — gets its stop time; a playback stopped already keeps the
//! one it has (`stop-all`). Nothing starts, and no volume moves.
//!
//! The engine writes one partition per command, and a playback is one,
//! `SoundPlayback/<its id>`: a stop-all is one command on each playback
//! still playing, writing its `stoppedAt` and nothing else, which the
//! DataGuard's `stop-all` judges on the playback's own row. [`scope`] reads
//! which playbacks the DM's request names; [`prepare`] turns those the tables
//! hold — each with its row as the queue will leave it — into the commands:
//!
//! - a playback whose row says it stopped, or will once a command queued
//!   ahead of it is applied, is left out: a second click stops nothing twice;
//! - with no playback still playing, there is no command at all: the request
//!   succeeds and writes nothing;
//! - a request naming no campaign, or a campaign or a session that is no
//!   uuid, is refused before anything is queued ([`Refusal`]).
//!
//! It is relative (`mode: relative`): it composes with what is queued ahead
//! and never asks its author to confirm.

use std::collections::BTreeMap;

use serde_json::Value;

/// This DataCapability's id, as `data-capability.json` names it.
pub const DATA_CAPABILITY: &str = "soundbox.stopAll";

/// The one field a stop writes: when the playback stopped.
pub const STOPPED: &str = "stoppedAt";

/// One partition's row, as the engine holds it.
pub type Row = BTreeMap<String, Value>;

/// Which playbacks a stop-all names: the campaign's, of one of its sessions
/// when it names one — each id lowercase, as the tables write it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Scope {
    /// The campaign.
    pub campaign: String,
    /// The session, when only its playbacks stop.
    pub session: Option<String>,
}

/// Why a stop-all is refused before it is queued.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// The request names no campaign, or one that is no uuid.
    NoCampaign,
    /// The session it names is no uuid.
    NotSession,
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoCampaign => f.write_str("« Tout arrêter » nomme sa campagne"),
            Self::NotSession => f.write_str("cette séance n'existe pas"),
        }
    }
}

/// One command a stop-all queues: the playback it stops, and what it writes
/// there.
#[derive(Clone, Debug, PartialEq)]
pub struct Stop {
    /// The playback's id: the command's target, its partition
    /// `SoundPlayback/<id>`.
    pub target_id: String,
    /// The fields written: its stop time alone.
    pub payload: Row,
}

/// Whether `id` names this DataCapability, with or without its `@version`.
#[must_use]
pub fn names(id: &str) -> bool {
    id.split('@').next() == Some(DATA_CAPABILITY)
}

/// Whether `id` is a uuid, in whatever case: what the tables can read.
fn uuid(id: &str) -> bool {
    let groups: Vec<&str> = id.split('-').collect();
    groups.len() == 5
        && groups
            .iter()
            .zip([8, 4, 4, 4, 12])
            .all(|(group, len)| group.len() == len && group.chars().all(|c| c.is_ascii_hexdigit()))
}

/// The id `value` holds, trimmed and lowercase: `None` when it holds none,
/// `Some(Err)` when it is no uuid.
fn id_in(value: Option<&Value>) -> Option<Result<String, ()>> {
    let id = match value? {
        Value::Null => return None,
        Value::String(id) if id.trim().is_empty() => return None,
        Value::String(id) => id.trim(),
        _ => return Some(Err(())),
    };
    Some(if uuid(id) {
        Ok(id.to_lowercase())
    } else {
        Err(())
    })
}

/// The playbacks `wire` names: those of its `campaign` — or of the command's
/// `target`, as a Micro-UI names it — and of its `session` only when it
/// names one.
///
/// # Errors
///
/// [`Refusal::NoCampaign`] when neither names a campaign, or names one that
/// is no uuid; [`Refusal::NotSession`] when the session is no uuid.
pub fn scope(wire: &Row, target: Option<&str>) -> Result<Scope, Refusal> {
    let target = target.map(|id| Value::String(id.to_owned()));
    let campaign = id_in(wire.get("campaign"))
        .or_else(|| id_in(target.as_ref()))
        .ok_or(Refusal::NoCampaign)?
        .map_err(|()| Refusal::NoCampaign)?;
    let session = id_in(wire.get("session"))
        .transpose()
        .map_err(|()| Refusal::NotSession)?;
    Ok(Scope { campaign, session })
}

/// Whether the playback whose row is `row` still plays: it has no stop time.
#[must_use]
pub fn playing(row: &Row) -> bool {
    row.get(STOPPED).is_none_or(Value::is_null)
}

/// The commands stopping, at `now`, each playback of `running` — its id and
/// its row as the commands queued on it will leave it — that still plays
/// then. None when none does.
#[must_use]
pub fn prepare(running: &[(String, Row)], now: &str) -> Vec<Stop> {
    running
        .iter()
        .filter(|(_, row)| playing(row))
        .map(|(id, _)| Stop {
            target_id: id.to_lowercase(),
            payload: Row::from([(STOPPED.to_owned(), Value::from(now))]),
        })
        .collect()
}
