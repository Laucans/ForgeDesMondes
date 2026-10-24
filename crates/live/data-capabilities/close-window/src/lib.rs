//! DataCapability `live.closeWindow` (`data-capability.json`): the DM takes
//! the illustration's floating window off the players' screen in one action,
//! leaving the map or the cover underneath.
//!
//! The engine writes one partition per command: a close is one command on
//! `Diffusion/<the session's id>`, the one row both player views read
//! (single-diffusion-source), which the aggregate projects onto the
//! `diffusion` table. The session is the live session of the campaign the
//! payload names ([`campaign`]), which its `Prepare` reads over PostgreSQL
//! ([`live_session`], `LIVE`); [`prepare`] turns it, with the row as the
//! queue leaves it, into the state written:
//!
//! - the window's media, `x`, `y` and width all emptied together, and the
//!   stamp — so no half-open window remains (window-fields-together);
//! - the base and the framing are not written: they stay as the row holds
//!   them (`live.showCover` brings the base back to the cover);
//! - a window already closed is closed again: the row is unchanged but its
//!   stamp, and the mode is `overwrite`, so nothing is asked;
//! - a payload naming no campaign, a campaign with no live session, or a
//!   session nothing was ever shown on — its row, which a close may not
//!   make, holds no window to close — is refused before it is queued
//!   ([`Refusal`]): nothing changes. Over the engine's memory, which has no
//!   table, no live session is known: every close is refused.

use std::collections::BTreeMap;

use async_trait::async_trait;
use harness_dataguard::plugins::{self, Context, Plugin, Prepare};
use harness_dataguard::reads::Rows;
use serde_json::Value;

/// This DataCapability's id, as `data-capability.json` names it.
pub const DATA_CAPABILITY: &str = "live.closeWindow";

/// The window's fields, all emptied by a close: its media, its position and
/// its width (its height is never stored).
const WINDOW: [&str; 4] = ["windowMediaId", "windowX", "windowY", "windowWidth"];

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
    Plugin::new().prepare(DATA_CAPABILITY, Closes)
}

/// A close made the state it writes on its live session's diffusion row
/// ([`prepare`]); anything [`Refusal`] names is refused, 422, and not
/// queued.
struct Closes;

#[async_trait]
impl Prepare for Closes {
    async fn prepare(
        &self,
        context: &Context<'_>,
        payload: &BTreeMap<String, Value>,
        _target_id: Option<&str>,
    ) -> Result<plugins::Prepared, plugins::Refusal> {
        let refused = |refusal: Refusal| plugins::Refusal::unprocessable(refusal.to_string());
        let campaign = campaign(payload).map_err(refused)?;
        let (session, row) = match context.rows {
            Some(rows) => {
                let session = live_session(rows, &campaign)
                    .await
                    .map_err(|e| plugins::Refusal::unprocessable(e.to_string()))?;
                let row = match &session {
                    Some(session) => {
                        let partition = format!("{}{session}", aggregate_diffusion::PARTITION);
                        context.row(&partition).await?
                    }
                    None => Row::new(),
                };
                (session, row)
            }
            None => (None, Row::new()),
        };
        let now = aggregate_diffusion::timestamp(context.now_millis);
        prepare(session, &row, &now).map_err(refused)
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

/// Why a close is refused before it is queued: nothing changes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// The payload names no campaign.
    NoCampaign,
    /// The campaign named is not a uuid.
    NotAnId,
    /// No session of the campaign is live: no player view is shown.
    NoLiveSession,
    /// Nothing was ever shown on the live session's player view: it has no
    /// diffusion row, so no window to close.
    NothingShown,
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::NoCampaign => "campaign manque",
            Self::NotAnId => "campaign n'est pas un identifiant",
            Self::NoLiveSession => "aucune séance n'est en cours",
            Self::NothingShown => "la vue joueurs n'a encore rien reçu",
        })
    }
}

/// The campaign the payload names, its uuid in lower case: the one whose
/// live session closes its window.
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

/// What closing the window writes: on the diffusion row of `session`, the
/// campaign's live session — the partition, whatever target the caller
/// names —, which the queue leaves as `row`, the window's four fields
/// emptied and the stamp `now`.
///
/// # Errors
///
/// [`Refusal::NoLiveSession`] when the campaign has no live session,
/// [`Refusal::NothingShown`] when `row` is no session's.
pub fn prepare(
    session: Option<String>,
    row: &Row,
    now: &str,
) -> Result<plugins::Prepared, Refusal> {
    let session = session.ok_or(Refusal::NoLiveSession)?;
    if row.get("sessionId").is_none_or(Value::is_null) {
        return Err(Refusal::NothingShown);
    }
    let mut payload: Row = WINDOW
        .iter()
        .map(|field| ((*field).to_string(), Value::Null))
        .collect();
    payload.insert("updatedAt".to_string(), Value::from(now));
    Ok(plugins::Prepared {
        payload,
        target_id: Some(session),
        ..plugins::Prepared::default()
    })
}

fn is_uuid(text: &str) -> bool {
    text.len() == 36
        && text.char_indices().all(|(i, c)| match i {
            8 | 13 | 18 | 23 => c == '-',
            _ => c.is_ascii_hexdigit(),
        })
}
