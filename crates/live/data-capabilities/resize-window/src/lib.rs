//! DataCapability `live.resizeWindow` (`data-capability.json`): the DM
//! drags the floating window's corner to enlarge or shrink the illustration,
//! and the projected screen follows — never distorted.
//!
//! The engine writes one partition per command: a resize is one command on
//! `Diffusion/<the session's id>`, the one row both player views read
//! (single-diffusion-source), which the aggregate projects onto the
//! `diffusion` table. The session is the live session of the campaign the
//! payload names, which its `Prepare` reads over PostgreSQL
//! ([`live_session`], `LIVE`); the window is the one that row shows as the
//! commands queued on it leave it, its proportions the tables' ([`window`],
//! `PROPORTIONS`). [`prepare`] turns them, with the box [`named`] reads,
//! into the state written:
//!
//! - the window's width, a fraction of the screen's width, and the stamp —
//!   nothing else: the position (`live.moveWindow`'s) and the media stay as
//!   the row holds them, and the height is never written: it is the width ×
//!   `height_px` / `width_px` of the illustration (window-proportional);
//! - the box the corner draws, `w` × `h`, is a hint whatever its shape: the
//!   width kept is the largest the box holds at the illustration's ratio
//!   ([`fit`]) — or `w` itself when no `h` is named — brought between
//!   [`MIN_WIDTH`] and the screen's width, to 4 decimals: what the table's
//!   `numeric(6,4)` column holds, so the engine's row and the table say the
//!   same;
//! - a window that is not open is resized all the same, for the DataGuard to
//!   refuse: a width with no media, no position breaks
//!   window-fields-together, and the row stays. One showing a media the
//!   tables give no proportions of gets no width its height could follow:
//!   it is queued for the DataGuard to refuse under window-proportional, and
//!   a warning says why;
//! - drags in a row are queued one after the other and each applied, the
//!   last one's width kept: the mode is `overwrite`, nothing is asked;
//! - a payload naming no campaign or no width, or a campaign with no live
//!   session, is refused before it is queued ([`Refusal`]): nothing changes.
//!   Over the engine's memory, which has no table, no live session is
//!   known: every resize is refused.

use std::collections::BTreeMap;

use async_trait::async_trait;
use harness_dataguard::plugins::{self, Context, Plugin, Prepare};
use harness_dataguard::reads::Rows;
use serde_json::Value;

/// This DataCapability's id, as `data-capability.json` names it.
pub const DATA_CAPABILITY: &str = "live.resizeWindow";

/// The narrowest a window is kept, a fraction of the screen's width: as the
/// player view lets the DM draw it.
pub const MIN_WIDTH: f64 = 0.1;

/// The widest: the screen's width.
const MAX_WIDTH: f64 = 1.0;

/// The width a media whose height cannot follow is given: none above 0, so
/// window-proportional refuses it.
const NO_WIDTH: f64 = 0.0;

/// The decimals the `diffusion` table keeps of a width (`numeric(6,4)`):
/// the engine's row holds the width the table does.
const DECIMALS: f64 = 10_000.0;

/// What the warning of a resize the DataGuard will refuse says.
pub const NO_PROPORTIONS: &str =
    "l'illustration de la fenêtre n'a pas de proportions : la fenêtre ne peut être redimensionnée";

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

/// The size in pixels of the illustration `$1` names — its id in whatever
/// case; a text that is no uuid names none. A media that is no illustration,
/// or one the engine deleted (its tombstone), has none.
const PROPORTIONS: &str = "SELECT m.width_px, m.height_px FROM media m
    WHERE m.id = CASE WHEN pg_input_is_valid($1::text, 'uuid') THEN $1::text::uuid END
        AND m.kind = 'illustration'
        AND NOT EXISTS (SELECT 1 FROM dataguard_rows r
            WHERE r.partition = 'Media/' || m.id::text AND r.tombstoned)";

/// The field of the diffusion row naming the media its window shows.
const WINDOW_MEDIA: &str = "windowMediaId";

/// What the DataCapability plugs into the engine: its `Prepare`.
#[must_use]
pub fn plugin() -> Plugin {
    Plugin::new().prepare(DATA_CAPABILITY, Resizes)
}

/// A drag of the corner made the state it writes on its live session's
/// diffusion row ([`prepare`]), against the window that row shows
/// ([`window`]): one with no proportions to keep is queued all the same, for
/// the DataGuard to refuse under window-proportional, and told among the
/// warnings; anything [`Refusal`] names is refused, 422, and not queued.
struct Resizes;

#[async_trait]
impl Prepare for Resizes {
    async fn prepare(
        &self,
        context: &Context<'_>,
        payload: &BTreeMap<String, Value>,
        _target_id: Option<&str>,
    ) -> Result<plugins::Prepared, plugins::Refusal> {
        let refused = |refusal: Refusal| plugins::Refusal::unprocessable(refusal.to_string());
        let unreadable =
            |e: harness_dataguard::Error| plugins::Refusal::unprocessable(e.to_string());
        let named = named(payload).map_err(refused)?;
        let (session, shown) = match context.rows {
            Some(rows) => {
                let session = live_session(rows, &named.campaign)
                    .await
                    .map_err(unreadable)?;
                let shown = match &session {
                    Some(session) => {
                        let partition = format!("{}{session}", aggregate_diffusion::PARTITION);
                        window(rows, &context.row(&partition).await?)
                            .await
                            .map_err(unreadable)?
                    }
                    None => Window::Closed,
                };
                (session, shown)
            }
            None => (None, Window::Closed),
        };
        let now = aggregate_diffusion::timestamp(context.now_millis);
        let prepared = prepare(&named, session, shown, &now).map_err(refused)?;
        // The partition is the session's; the payload carries no `sessionId`.
        Ok(plugins::Prepared {
            payload: prepared.payload,
            target_id: Some(prepared.target_id),
            warnings: prepared.warning.into_iter().map(String::from).collect(),
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

/// The window `diffusion`, a session's diffusion row, shows: closed when it
/// names no media in one, else open on that media, with its proportions
/// when the tables hold it as an illustration (`PROPORTIONS`).
///
/// # Errors
///
/// The read's: the tables cannot be read.
pub async fn window(rows: &dyn Rows, diffusion: &Row) -> harness_dataguard::Result<Window> {
    let Some(media) = diffusion.get(WINDOW_MEDIA).and_then(Value::as_str) else {
        return Ok(Window::Closed);
    };
    let answer = rows.rows(PROPORTIONS, &[Value::from(media)]).await?;
    let pixels = |column: &str| {
        answer
            .get(0)
            .and_then(|row| row.get(column))
            .and_then(Value::as_i64)
            .and_then(|n| i32::try_from(n).ok())
    };
    Ok(Window::Open(
        pixels("width_px")
            .zip(pixels("height_px"))
            .map(|(width_px, height_px)| Proportions {
                width_px,
                height_px,
            }),
    ))
}

/// What a resize names: the campaign whose live session is looked up, and
/// the box the corner draws.
#[derive(Clone, Debug, PartialEq)]
pub struct Named {
    /// The campaign whose live session's player view shows the window: a
    /// uuid in lower case.
    pub campaign: String,
    /// The box's width, a fraction of the screen's width, as asked.
    pub w: f64,
    /// The box's height, a fraction of the screen's width too, as asked:
    /// none, and the width leads.
    pub h: Option<f64>,
}

/// An illustration's size in pixels, as the tables hold it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Proportions {
    /// Its width.
    pub width_px: i32,
    /// Its height.
    pub height_px: i32,
}

impl Proportions {
    /// Its width over its height (2/3 for 800 × 1200), when both are above 0.
    #[must_use]
    pub fn ratio(self) -> Option<f64> {
        (self.width_px > 0 && self.height_px > 0)
            .then(|| f64::from(self.width_px) / f64::from(self.height_px))
    }
}

/// The window the session's diffusion row shows, as the commands queued on
/// it will leave it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Window {
    /// No window is open: the row shows no media in one.
    Closed,
    /// A window shows a media: its proportions, when the tables hold it as
    /// an illustration with a width and a height.
    Open(Option<Proportions>),
}

/// What a resize becomes before it is submitted.
#[derive(Clone, Debug, PartialEq)]
pub struct Prepared {
    /// The state it writes on the session's diffusion row.
    pub payload: Row,
    /// The partition it writes: the session's id.
    pub target_id: String,
    /// Why the DataGuard will refuse it, for the DM.
    pub warning: Option<&'static str>,
}

/// Why a resize is refused before it is queued: nothing changes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// The payload names no campaign.
    NoCampaign,
    /// The campaign named is not a uuid.
    NotAnId,
    /// The payload names no `w`.
    NoSize,
    /// The `w` or the `h` named is not a number.
    NotANumber,
    /// No session of the campaign is live: no player view is shown.
    NoLiveSession,
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::NoCampaign => "campaign manque",
            Self::NotAnId => "campaign n'est pas un identifiant",
            Self::NoSize => "w manque",
            Self::NotANumber => "w ou h n'est pas un nombre",
            Self::NoLiveSession => "aucune séance n'est en cours",
        })
    }
}

/// What the payload names: the campaign, `w` and, maybe, `h`.
///
/// # Errors
///
/// [`Refusal::NoCampaign`] or [`Refusal::NoSize`] when it names none,
/// [`Refusal::NotAnId`] when the campaign is no uuid,
/// [`Refusal::NotANumber`] when the box's sides are no numbers.
pub fn named(wire: &Row) -> Result<Named, Refusal> {
    let campaign = match wire.get("campaign") {
        None | Some(Value::Null) => return Err(Refusal::NoCampaign),
        Some(Value::String(text)) if is_uuid(text) => text.to_ascii_lowercase(),
        Some(_) => return Err(Refusal::NotAnId),
    };
    let w = number(wire, "w")?.ok_or(Refusal::NoSize)?;
    Ok(Named {
        campaign,
        w,
        h: number(wire, "h")?,
    })
}

/// The state resizing `window` to `named`'s box writes on the diffusion row
/// of `session`, the campaign's live session, stamped `now`.
///
/// # Errors
///
/// [`Refusal::NoLiveSession`] when the campaign has no live session.
pub fn prepare(
    named: &Named,
    session: Option<String>,
    window: Window,
    now: &str,
) -> Result<Prepared, Refusal> {
    let session = session.ok_or(Refusal::NoLiveSession)?;
    let (width, warning) = match window {
        // No media to keep the proportions of: the width asked, which
        // window-fields-together refuses.
        Window::Closed => (on_screen(named.w), None),
        Window::Open(proportions) => match proportions.and_then(Proportions::ratio) {
            Some(ratio) => (fit(named.w, named.h, ratio), None),
            None => (NO_WIDTH, Some(NO_PROPORTIONS)),
        },
    };
    Ok(Prepared {
        payload: Row::from([
            ("windowWidth".to_string(), Value::from(width)),
            ("updatedAt".to_string(), Value::from(now)),
        ]),
        target_id: session,
        warning,
    })
}

/// The width of the largest window of `ratio` — its width over its height —
/// the box `w` × `h` holds, both fractions of the screen's width: `w` alone
/// when there is no `h`. Brought between [`MIN_WIDTH`] and the screen's
/// width.
fn fit(w: f64, h: Option<f64>, ratio: f64) -> f64 {
    on_screen(h.map_or(w, |h| w.min(h * ratio)))
}

/// `width` brought between [`MIN_WIDTH`] and the screen's width, to the
/// decimals the table keeps.
fn on_screen(width: f64) -> f64 {
    (width.clamp(MIN_WIDTH, MAX_WIDTH) * DECIMALS).round() / DECIMALS
}

/// The number `wire` names under `field`, when it names one.
fn number(wire: &Row, field: &str) -> Result<Option<f64>, Refusal> {
    match wire.get(field) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => value.as_f64().map(Some).ok_or(Refusal::NotANumber),
    }
}

fn is_uuid(text: &str) -> bool {
    text.len() == 36
        && text.char_indices().all(|(i, c)| match i {
            8 | 13 | 18 | 23 => c == '-',
            _ => c.is_ascii_hexdigit(),
        })
}
