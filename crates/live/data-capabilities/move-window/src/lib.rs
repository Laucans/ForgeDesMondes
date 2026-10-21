//! DataCapability `live.moveWindow` (`data-capability.json`): the DM drags
//! the floating window to where it hides the least of the map, and the
//! projected screen follows.
//!
//! The engine writes one partition per command: a move is one command on
//! `Diffusion/<the session's id>`, the one row both player views read
//! (single-diffusion-source), which the aggregate projects onto the
//! `diffusion` table. The session is the live session of the campaign the
//! payload names, which its `Prepare` reads over PostgreSQL
//! ([`live_session`], `LIVE`); [`prepare`] turns it, with the position
//! [`named`] reads, into the state written:
//!
//! - the window's `x` and `y`, fractions of the screen, and the stamp —
//!   nothing else: the width (`live.resizeWindow`'s) and the media stay as
//!   the row holds them. A position outside 0 to 1 is brought back to it,
//!   the screen's edge, to 4 decimals: what the table's `numeric(6,4)`
//!   columns hold, so the engine's row and the table say the same;
//! - a window that is not open is moved all the same, for the DataGuard to
//!   refuse: a position with no media and no width breaks
//!   window-fields-together, the command is rejected and the row stays;
//! - drags in a row are queued one after the other and each applied, the
//!   last one's position kept: the mode is `overwrite`, nothing is asked;
//! - a payload naming no campaign or no position, or a campaign with no live
//!   session, is refused before it is queued ([`Refusal`]): nothing changes.
//!   Over the engine's memory, which has no table, no live session is
//!   known: every move is refused.

use std::collections::BTreeMap;

use async_trait::async_trait;
use harness_dataguard::plugins::{self, Context, Plugin, Prepare};
use harness_dataguard::reads::Rows;
use serde_json::Value;

/// This DataCapability's id, as `data-capability.json` names it.
pub const DATA_CAPABILITY: &str = "live.moveWindow";

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
    Plugin::new().prepare(DATA_CAPABILITY, Moves)
}

/// A drag made the state it writes on its live session's diffusion row
/// ([`prepare`]); anything [`Refusal`] names is refused, 422, and not
/// queued.
struct Moves;

#[async_trait]
impl Prepare for Moves {
    async fn prepare(
        &self,
        context: &Context<'_>,
        payload: &BTreeMap<String, Value>,
        _target_id: Option<&str>,
    ) -> Result<plugins::Prepared, plugins::Refusal> {
        let refused = |refusal: Refusal| plugins::Refusal::unprocessable(refusal.to_string());
        let named = named(payload).map_err(refused)?;
        let session = match context.rows {
            Some(rows) => live_session(rows, &named.campaign)
                .await
                .map_err(|e| plugins::Refusal::unprocessable(e.to_string()))?,
            None => None,
        };
        let now = aggregate_diffusion::timestamp(context.now_millis);
        prepare(&named, session, &now).map_err(refused)
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

/// What a move names: the campaign whose live session is looked up, and
/// where the window goes.
#[derive(Clone, Debug, PartialEq)]
pub struct Named {
    /// The campaign whose live session's player view shows the window: a
    /// uuid in lower case.
    pub campaign: String,
    /// The window's left edge, a fraction of the screen's width, as asked.
    pub x: f64,
    /// The window's top edge, a fraction of the screen's height, as asked.
    pub y: f64,
}

/// Why a move is refused before it is queued: nothing changes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// The payload names no campaign.
    NoCampaign,
    /// The campaign named is not a uuid.
    NotAnId,
    /// The payload names no `x` or no `y`.
    NoPosition,
    /// The `x` or the `y` named is not a number.
    NotANumber,
    /// No session of the campaign is live: no player view is shown.
    NoLiveSession,
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::NoCampaign => "campaign manque",
            Self::NotAnId => "campaign n'est pas un identifiant",
            Self::NoPosition => "x ou y manque",
            Self::NotANumber => "x ou y n'est pas un nombre",
            Self::NoLiveSession => "aucune séance n'est en cours",
        })
    }
}

/// What the payload names: the campaign, `x` and `y`.
///
/// # Errors
///
/// [`Refusal::NoCampaign`] or [`Refusal::NoPosition`] when it names none,
/// [`Refusal::NotAnId`] when the campaign is no uuid,
/// [`Refusal::NotANumber`] when the position is no number.
pub fn named(wire: &Row) -> Result<Named, Refusal> {
    let campaign = match wire.get("campaign") {
        None | Some(Value::Null) => return Err(Refusal::NoCampaign),
        Some(Value::String(text)) if is_uuid(text) => text.to_ascii_lowercase(),
        Some(_) => return Err(Refusal::NotAnId),
    };
    Ok(Named {
        campaign,
        x: number(wire, "x")?,
        y: number(wire, "y")?,
    })
}

/// What moving the window to `named`'s position writes: on the diffusion
/// row of `session`, the campaign's live session — the partition, whatever
/// target the caller names —, its `x` and `y` brought onto the screen and
/// the stamp `now`.
///
/// # Errors
///
/// [`Refusal::NoLiveSession`] when the campaign has no live session.
pub fn prepare(
    named: &Named,
    session: Option<String>,
    now: &str,
) -> Result<plugins::Prepared, Refusal> {
    let session = session.ok_or(Refusal::NoLiveSession)?;
    Ok(plugins::Prepared {
        payload: Row::from([
            ("windowX".to_string(), Value::from(on_screen(named.x))),
            ("windowY".to_string(), Value::from(on_screen(named.y))),
            ("updatedAt".to_string(), Value::from(now)),
        ]),
        target_id: Some(session),
        ..plugins::Prepared::default()
    })
}

/// `fraction` brought back onto the screen, from 0 to 1, to the 4 decimals
/// `numeric(6,4)` keeps.
fn on_screen(fraction: f64) -> f64 {
    (fraction.clamp(0.0, 1.0) * 10_000.0).round() / 10_000.0
}

/// The number `wire` names under `field`.
fn number(wire: &Row, field: &str) -> Result<f64, Refusal> {
    match wire.get(field) {
        None | Some(Value::Null) => Err(Refusal::NoPosition),
        Some(value) => value.as_f64().ok_or(Refusal::NotANumber),
    }
}

fn is_uuid(text: &str) -> bool {
    text.len() == 36
        && text.char_indices().all(|(i, c)| match i {
            8 | 13 | 18 | 23 => c == '-',
            _ => c.is_ascii_hexdigit(),
        })
}
