//! DataCapability `campaign.createCampaign` (`data-capability.json`): the DM
//! creates a campaign or a oneshot from the home page. One command on
//! `Campaign/<its new id>` writes the campaign whole; a oneshot's carries its
//! single session too, so the DM never adds it by hand
//! (`oneshot-single-session`). [`prepare`] turns the wire's payload —
//! `{name, type, pitch?, art?, glyph?}` — into that command:
//!
//! - the campaign is given its id, and its creation's time, by the engine
//!   ([`Fresh`]); it is the command's target, lowercase as the table writes
//!   it;
//! - its name is the DM's, trimmed, and never blank; its kind, `campagne` or
//!   `oneshot`, the wire's `type`; its pitch, art and glyph the DM's, or
//!   `null`;
//! - its master volume starts at full, [`MASTER_VOLUME`]: inside 0–1
//!   (`master-volume-range`), as the table's default holds it;
//! - a oneshot's session is given its id by the engine too: it is the
//!   campaign's first and only one — at position 1, in `prep`, the microphone
//!   off — titled after the campaign. A campagne carries none: its sessions
//!   are added one by one, later;
//! - a payload with no name, or of a kind there is not, is refused before
//!   anything is queued, 422 ([`Refusal`]).
//!
//! The Campaign aggregate judges what the command writes and projects it,
//! in the transaction that applies it, into `campaign` and — a oneshot's —
//! `session`. A replay under an idempotency key already submitted is the
//! engine's to answer, with the first creation: nothing is created twice.

use std::collections::BTreeMap;

use async_trait::async_trait;
use harness_dataguard::plugins::{self, Context, Plugin, Prepare};
use serde_json::Value;

/// This DataCapability's id, as `data-capability.json` names it.
pub const DATA_CAPABILITY: &str = "campaign.createCampaign";

/// A campaign's kind with several sessions.
pub const CAMPAGNE: &str = "campagne";

/// A campaign's kind with a single session.
pub const ONESHOT: &str = "oneshot";

/// A new campaign's master volume: full, until the DM moves it.
pub const MASTER_VOLUME: f64 = 1.0;

/// A oneshot's session's position: the campaign's first.
pub const SESSION_POSITION: u64 = 1;

/// A oneshot's session's status: in preparation (`status-lifecycle`).
pub const SESSION_STATUS: &str = "prep";

/// A oneshot's session's microphone: off.
pub const MICROPHONE_ON: bool = false;

/// One partition's row, as the engine holds it.
pub type Row = BTreeMap<String, Value>;

/// The ids and the time the engine gives a new campaign.
#[derive(Clone, Copy, Debug)]
pub struct Fresh<'a> {
    /// The campaign's id.
    pub campaign: &'a str,
    /// Its session's id, should it be a oneshot.
    pub session: &'a str,
    /// Now, RFC 3339.
    pub now: &'a str,
}

/// Why a creation is refused before it is queued.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// The payload names no campaign, or only blanks.
    NoName,
    /// The payload's `type` is neither `campagne` nor `oneshot`.
    UnknownType,
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::NoName => "une campagne a un nom",
            Self::UnknownType => "une campagne est une « campagne » ou un « oneshot »",
        })
    }
}

/// What a creation becomes before it is submitted: what the command writes
/// on `Campaign/<target_id>`.
#[derive(Clone, Debug, PartialEq)]
pub struct Prepared {
    /// The campaign's fields — and a oneshot's session's.
    pub payload: Row,
    /// The campaign's id, lowercase: the command's target.
    pub target_id: String,
}

/// What the DataCapability plugs into the engine: its `Prepare`.
#[must_use]
pub fn plugin() -> Plugin {
    Plugin::new().prepare(DATA_CAPABILITY, Creates)
}

/// A creation made the campaign it writes on a new campaign's partition,
/// under the engine's fresh ids and its time to the millisecond; one with no
/// name, or of a kind there is not, is refused, 422.
struct Creates;

#[async_trait]
impl Prepare for Creates {
    async fn prepare(
        &self,
        context: &Context<'_>,
        payload: &BTreeMap<String, Value>,
        _target_id: Option<&str>,
    ) -> Result<plugins::Prepared, plugins::Refusal> {
        let campaign = context.ids.fresh();
        let session = context.ids.fresh();
        let now = timestamp(context.now_millis);
        let fresh = Fresh {
            campaign: &campaign,
            session: &session,
            now: &now,
        };
        let prepared = prepare(payload, fresh)
            .map_err(|refusal| plugins::Refusal::unprocessable(refusal.to_string()))?;
        // The payload's `id` is the new campaign's: the manifest's
        // `target.id` (`$id`) names its partition from it.
        Ok(plugins::Prepared {
            payload: prepared.payload,
            ..plugins::Prepared::default()
        })
    }
}

/// The creation the wire's payload asks for, under the ids and at the time
/// `fresh` gives.
///
/// # Errors
///
/// [`Refusal`] when the payload has no name, or a `type` that is neither
/// `campagne` nor `oneshot`.
pub fn prepare(wire: &Row, fresh: Fresh<'_>) -> Result<Prepared, Refusal> {
    let name = wire
        .get("name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .ok_or(Refusal::NoName)?;
    let kind = wire
        .get("type")
        .and_then(Value::as_str)
        .filter(|kind| [CAMPAGNE, ONESHOT].contains(kind))
        .ok_or(Refusal::UnknownType)?;
    let id = fresh.campaign.to_ascii_lowercase();
    let optional = |field: &str| {
        wire.get(field)
            .filter(|value| value.is_string())
            .cloned()
            .unwrap_or(Value::Null)
    };
    let mut payload = Row::from([
        ("id".into(), id.clone().into()),
        ("name".into(), name.into()),
        ("kind".into(), kind.into()),
        ("pitch".into(), optional("pitch")),
        ("art".into(), optional("art")),
        ("glyph".into(), optional("glyph")),
        ("createdAt".into(), fresh.now.into()),
        ("soundMasterVolume".into(), MASTER_VOLUME.into()),
    ]);
    if kind == ONESHOT {
        payload.extend([
            (
                "sessionId".into(),
                fresh.session.to_ascii_lowercase().into(),
            ),
            ("sessionCampaignId".into(), id.clone().into()),
            ("sessionPosition".into(), SESSION_POSITION.into()),
            ("sessionTitle".into(), name.into()),
            ("sessionStatus".into(), SESSION_STATUS.into()),
            ("microphoneOn".into(), MICROPHONE_ON.into()),
        ]);
    }
    Ok(Prepared {
        payload,
        target_id: id,
    })
}

/// `unix_millis` as RFC 3339 in UTC, to the millisecond
/// (`2026-10-21T09:00:00.123Z`): what `campaign.created_at` takes.
#[must_use]
pub fn timestamp(unix_millis: u64) -> String {
    let unix_seconds = unix_millis / 1000;
    // Howard Hinnant's civil_from_days.
    let days = i64::try_from(unix_seconds / 86_400).unwrap_or_default() + 719_468;
    let seconds = unix_seconds % 86_400;
    let era = days.div_euclid(146_097);
    let doe = days.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{:03}Z",
        seconds / 3600,
        seconds % 3600 / 60,
        seconds % 60,
        unix_millis % 1000
    )
}
