//! DataCapability `live.showCover` (`data-capability.json`): the DM brings
//! the player view back to the campaign's cover, the neutral screen nothing
//! of the previous map or illustration lingers on.
//!
//! The engine writes one partition per command: a return to the cover is one
//! command on `Diffusion/<the session's id>`, the one row both player views
//! read (single-diffusion-source), which the aggregate projects onto the
//! `diffusion` table — made by it when the session had none, its base
//! replaced otherwise. The session is the live session of the campaign the
//! payload names ([`campaign`]), which its `Prepare` reads over PostgreSQL
//! ([`live_session`], `LIVE`); [`prepare`] turns it into the state written:
//!
//! - the base is the cover, its media and its instance emptied, so the base
//!   is the cover alone (base-coherent);
//! - the floating window and the framing are not written: they stay as the
//!   row holds them (`live.closeWindow` closes the window);
//! - a payload naming no campaign, or one with no live session, is refused
//!   before it is queued ([`Refusal`]): nothing changes. Over the engine's
//!   memory, which has no table, no live session is known: every return is
//!   refused.

use std::collections::BTreeMap;

use async_trait::async_trait;
use harness_dataguard::plugins::{self, Context, Plugin, Prepare};
use harness_dataguard::reads::Rows;
use serde_json::Value;

/// This DataCapability's id, as `data-capability.json` names it.
pub const DATA_CAPABILITY: &str = "live.showCover";

/// The base it shows.
const COVER: &str = "cover";

/// A row, as the engine holds it.
pub type Row = BTreeMap<String, Value>;

/// The live session of the campaign `$1` names, its id as text. Of two live
/// sessions, the last one planned. A session the engine deleted (its
/// tombstone) is none.
const LIVE: &str = "SELECT s.id::text AS session FROM session s
    WHERE s.campaign_id = ($1::text)::uuid AND s.status = 'live'
        AND NOT EXISTS (SELECT 1 FROM dataguard_rows r
            WHERE r.partition = 'Session/' || s.id::text AND r.tombstoned)
    ORDER BY s.position DESC, s.id
    LIMIT 1";

/// What the DataCapability plugs into the engine: its `Prepare`.
#[must_use]
pub fn plugin() -> Plugin {
    Plugin::new().prepare(DATA_CAPABILITY, Covers)
}

/// A return to the cover made the state it writes on its live session's
/// diffusion row ([`prepare`]); anything [`Refusal`] names is refused, 422,
/// and not queued.
struct Covers;

#[async_trait]
impl Prepare for Covers {
    async fn prepare(
        &self,
        context: &Context<'_>,
        payload: &BTreeMap<String, Value>,
        _target_id: Option<&str>,
    ) -> Result<plugins::Prepared, plugins::Refusal> {
        let refused = |refusal: Refusal| plugins::Refusal::unprocessable(refusal.to_string());
        let campaign = campaign(payload).map_err(refused)?;
        let session = match context.rows {
            Some(rows) => live_session(rows, &campaign)
                .await
                .map_err(|e| plugins::Refusal::unprocessable(e.to_string()))?,
            None => None,
        };
        let now = aggregate_diffusion::timestamp(context.now_millis);
        // The partition is the manifest's `$sessionId`, which the payload
        // carries: whatever target the caller names, the session's row.
        Ok(plugins::Prepared {
            payload: prepare(session, &now).map_err(refused)?,
            ..plugins::Prepared::default()
        })
    }
}

/// The live session of `campaign`, its id in lower case (`LIVE`).
///
/// # Errors
///
/// The read's: the tables cannot be read.
pub async fn live_session(
    rows: &dyn Rows,
    campaign: &str,
) -> harness_dataguard::Result<Option<String>> {
    let answer = rows.rows(LIVE, &[Value::from(campaign)]).await?;
    Ok(answer
        .get(0)
        .and_then(|row| row.get("session"))
        .and_then(Value::as_str)
        .map(str::to_owned))
}

/// Why a return to the cover is refused before it is queued: nothing
/// changes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// The payload names no campaign.
    NoCampaign,
    /// The campaign named is not a uuid.
    NotAnId,
    /// No session of the campaign is live: no player view is shown.
    NoLiveSession,
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::NoCampaign => "campaign manque",
            Self::NotAnId => "campaign n'est pas un identifiant",
            Self::NoLiveSession => "aucune séance n'est en cours",
        })
    }
}

/// The campaign the payload names, its uuid in lower case: the one whose
/// live session goes back to its cover.
///
/// # Errors
///
/// [`Refusal::NoCampaign`] when it names none, [`Refusal::NotAnId`] when
/// what it names is no uuid.
pub fn campaign(wire: &Row) -> Result<String, Refusal> {
    match wire.get("campaign") {
        None | Some(Value::Null) => Err(Refusal::NoCampaign),
        Some(Value::String(text)) if is_uuid(text) => Ok(text.to_ascii_lowercase()),
        Some(_) => Err(Refusal::NotAnId),
    }
}

/// The state a return to the cover writes on the diffusion row of
/// `session`, the campaign's live session, stamped `now`: the cover, with
/// neither a media nor an instance, and nothing of the window or the
/// framing.
///
/// # Errors
///
/// [`Refusal::NoLiveSession`] when the campaign has no live session.
pub fn prepare(session: Option<String>, now: &str) -> Result<Row, Refusal> {
    let session = session.ok_or(Refusal::NoLiveSession)?;
    Ok(Row::from([
        ("sessionId".to_string(), Value::from(session)),
        ("baseKind".to_string(), Value::from(COVER)),
        ("baseMediaId".to_string(), Value::Null),
        ("baseInstanceId".to_string(), Value::Null),
        ("updatedAt".to_string(), Value::from(now)),
    ]))
}

fn is_uuid(text: &str) -> bool {
    text.len() == 36
        && text.char_indices().all(|(i, c)| match i {
            8 | 13 | 18 | 23 => c == '-',
            _ => c.is_ascii_hexdigit(),
        })
}
