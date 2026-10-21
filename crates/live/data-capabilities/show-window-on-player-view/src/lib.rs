//! DataCapability `live.showWindowOnPlayerView` (`data-capability.json`):
//! the DM opens an illustration in a floating window over the player view's
//! base, so the players see it without losing the map.
//!
//! The engine writes one partition per command: opening a window is one
//! command on `Diffusion/<the session's id>`, the one row both player views
//! read (single-diffusion-source), which the aggregate projects onto the
//! `diffusion` table — made by it when the session had none, its window
//! replaced otherwise. The session is the live session of the campaign the
//! payload names; its `Prepare` reads it, and what the tables say of the
//! media, over PostgreSQL from [`named`] ([`windowing`], `WINDOWING`), then
//! the session's row as the queue leaves it; [`prepare`] turns them into the
//! state written:
//!
//! - the window shows the media at its default position and width ([`X`],
//!   [`Y`], [`WIDTH`]), the three set together (window-fields-together); its
//!   height is never written: it follows from the width and the
//!   illustration's proportions (window-proportional);
//! - the base is not written: it stays as the row holds it. A session with
//!   no row yet gets one, its own (`sessionId`) showing the cover — what its
//!   player view shows before anything is sent (base-coherent): fields made,
//!   not existing ones changed, which the manifest's `touches` leave out;
//! - a media that is no illustration of the campaign, or one with no
//!   proportions, gets no width its height could follow: it is queued all
//!   the same, for the DataGuard to refuse under window-proportional, and a
//!   warning says why;
//! - a payload naming no campaign or no media, a campaign with no live
//!   session, or a media the tables do not hold, is refused before it is
//!   queued ([`Refusal`]): nothing changes. Over the engine's memory, which
//!   has no table, no live session is known: every window is refused.

use std::collections::BTreeMap;

use async_trait::async_trait;
use harness_dataguard::plugins::{self, Context, Plugin, Prepare};
use harness_dataguard::reads::Rows;
use serde_json::Value;

/// This DataCapability's id, as `data-capability.json` names it.
pub const DATA_CAPABILITY: &str = "live.showWindowOnPlayerView";

/// Where the window opens, in fractions of the screen: `x` and the width of
/// its width, `y` of its height — towards the top right, as the mock-up
/// opens it.
pub const X: f64 = 0.6;
/// See [`X`].
pub const Y: f64 = 0.12;
/// See [`X`].
pub const WIDTH: f64 = 0.3;

/// The width a media whose height cannot follow is given: none above 0, so
/// window-proportional refuses it.
const NO_WIDTH: f64 = 0.0;

/// The base a session shows before anything is sent.
const COVER: &str = "cover";

/// What the warning of a window the DataGuard will refuse says.
pub const NOT_AN_ILLUSTRATION: &str = "ce média n'est pas une illustration de la campagne : la fenêtre ne peut garder ses proportions";

/// A row, as the engine holds it.
pub type Row = BTreeMap<String, Value>;

/// What the tables say of what a window names, `$1` the campaign, `$2` the
/// media, each a uuid: the campaign's live session — of two, the last one
/// planned —, whether the tables hold the media, and whether it is an
/// illustration of the campaign with a width and a height its window can
/// keep the proportions of. A row the engine deleted (its tombstone) — a
/// session, a media — is none.
const WINDOWING: &str = "WITH live AS (
        SELECT s.id FROM session s
        WHERE s.campaign_id = ($1::text)::uuid AND s.status = 'live'
            AND NOT EXISTS (SELECT 1 FROM dataguard_rows r
                WHERE r.partition = 'Session/' || s.id::text AND r.tombstoned)
        ORDER BY s.position DESC, s.id
        LIMIT 1
    ), shown AS (
        SELECT m.campaign_id, m.kind, m.width_px, m.height_px FROM media m
        WHERE m.id = ($2::text)::uuid
            AND NOT EXISTS (SELECT 1 FROM dataguard_rows r
                WHERE r.partition = 'Media/' || m.id::text AND r.tombstoned)
    )
    SELECT (SELECT id::text FROM live) AS session,
        EXISTS (SELECT 1 FROM shown) AS held,
        EXISTS (SELECT 1 FROM shown
            WHERE campaign_id = ($1::text)::uuid AND kind = 'illustration'
                AND width_px > 0 AND height_px > 0) AS illustration";

/// What the DataCapability plugs into the engine: its `Prepare`.
#[must_use]
pub fn plugin() -> Plugin {
    Plugin::new().prepare(DATA_CAPABILITY, Windows)
}

/// A window made the state it writes on its live session's diffusion row
/// ([`prepare`]), against what the tables say of what it names
/// ([`windowing`]) and the row as the queue leaves it: one with no
/// proportions to keep is queued all the same, for the DataGuard to refuse
/// under window-proportional, and told among the warnings; anything
/// [`Refusal`] names is refused, 422, and not queued.
struct Windows;

#[async_trait]
impl Prepare for Windows {
    async fn prepare(
        &self,
        context: &Context<'_>,
        payload: &BTreeMap<String, Value>,
        _target_id: Option<&str>,
    ) -> Result<plugins::Prepared, plugins::Refusal> {
        let refused = |refusal: Refusal| plugins::Refusal::unprocessable(refusal.to_string());
        let named = named(payload).map_err(refused)?;
        let found = match context.rows {
            Some(rows) => windowing(rows, &named)
                .await
                .map_err(|e| plugins::Refusal::unprocessable(e.to_string()))?,
            None => Found::default(),
        };
        let row = match &found.session {
            Some(session) => {
                context
                    .row(&format!("{}{session}", aggregate_diffusion::PARTITION))
                    .await?
            }
            None => Row::new(),
        };
        let now = aggregate_diffusion::timestamp(context.now_millis);
        let prepared = prepare(&named, &found, &row, &now).map_err(refused)?;
        // The partition is the session's: the payload carries its
        // `sessionId` only when it makes the row.
        Ok(plugins::Prepared {
            payload: prepared.payload,
            target_id: Some(prepared.target_id),
            warnings: prepared.warning.into_iter().map(String::from).collect(),
            ..plugins::Prepared::default()
        })
    }
}

/// What the tables say of what `named` names (`WINDOWING`).
///
/// # Errors
///
/// The read's: the tables cannot be read.
pub async fn windowing(rows: &dyn Rows, named: &Named) -> harness_dataguard::Result<Found> {
    let answer = rows
        .rows(
            WINDOWING,
            &[
                Value::from(named.campaign.as_str()),
                Value::from(named.media.as_str()),
            ],
        )
        .await?;
    let row = answer.get(0);
    let yes = |column: &str| {
        row.and_then(|row| row.get(column))
            .and_then(Value::as_bool)
            .unwrap_or(false)
    };
    Ok(Found {
        session: row
            .and_then(|row| row.get("session"))
            .and_then(Value::as_str)
            .map(str::to_owned),
        held: yes("held"),
        illustration: yes("illustration"),
    })
}

/// What a window names: uuids in lower case.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Named {
    /// The campaign whose live session's player view opens the window.
    pub campaign: String,
    /// The media the window shows.
    pub media: String,
}

/// What the tables say of what a window names.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Found {
    /// The campaign's live session, its id in lower case, when it has one.
    pub session: Option<String>,
    /// Whether the tables hold the media at all.
    pub held: bool,
    /// Whether the media is an illustration of the campaign, with a width
    /// and a height its window can keep the proportions of.
    pub illustration: bool,
}

/// What opening a window becomes before it is submitted.
#[derive(Clone, Debug, PartialEq)]
pub struct Prepared {
    /// The state it writes on the session's diffusion row.
    pub payload: Row,
    /// The partition it writes: the session's id.
    pub target_id: String,
    /// Why the DataGuard will refuse it, for the DM.
    pub warning: Option<&'static str>,
}

/// Why opening a window is refused before it is queued: nothing changes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// The payload names no campaign.
    NoCampaign,
    /// The payload names no media.
    NoMedia,
    /// The campaign or the media named is not a uuid.
    NotAnId(&'static str),
    /// No session of the campaign is live: no player view is shown.
    NoLiveSession,
    /// The tables hold no such media.
    UnknownMedia,
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoCampaign => f.write_str("campaign manque"),
            Self::NoMedia => f.write_str("media manque"),
            Self::NotAnId(field) => write!(f, "{field} n'est pas un identifiant"),
            Self::NoLiveSession => f.write_str("aucune séance n'est en cours"),
            Self::UnknownMedia => f.write_str("ce média n'existe pas"),
        }
    }
}

/// What the payload names: the campaign and the media.
///
/// # Errors
///
/// [`Refusal::NoCampaign`] or [`Refusal::NoMedia`] when it names none,
/// [`Refusal::NotAnId`] when what it names is no uuid.
pub fn named(wire: &Row) -> Result<Named, Refusal> {
    Ok(Named {
        campaign: id(wire, "campaign", Refusal::NoCampaign)?,
        media: id(wire, "media", Refusal::NoMedia)?,
    })
}

/// The state opening `named`'s media in a window writes on the diffusion row
/// of the campaign's live session, `row` as the commands queued there leave
/// it, stamped `now`.
///
/// # Errors
///
/// [`Refusal::NoLiveSession`] when the campaign has no live session,
/// [`Refusal::UnknownMedia`] when the tables hold no such media.
pub fn prepare(named: &Named, found: &Found, row: &Row, now: &str) -> Result<Prepared, Refusal> {
    let session = found.session.clone().ok_or(Refusal::NoLiveSession)?;
    if !found.held {
        return Err(Refusal::UnknownMedia);
    }
    let (width, warning) = if found.illustration {
        (WIDTH, None)
    } else {
        (NO_WIDTH, Some(NOT_AN_ILLUSTRATION))
    };
    let mut payload = Row::from([
        (
            "windowMediaId".to_string(),
            Value::from(named.media.as_str()),
        ),
        ("windowX".to_string(), Value::from(X)),
        ("windowY".to_string(), Value::from(Y)),
        ("windowWidth".to_string(), Value::from(width)),
        ("updatedAt".to_string(), Value::from(now)),
    ]);
    // A session with no row yet — neither the engine's nor its table's — is
    // given one: its own, showing the cover.
    if row.is_empty() {
        payload.extend([
            ("sessionId".to_string(), Value::from(session.as_str())),
            ("baseKind".to_string(), Value::from(COVER)),
            ("baseMediaId".to_string(), Value::Null),
            ("baseInstanceId".to_string(), Value::Null),
        ]);
    }
    Ok(Prepared {
        payload,
        target_id: session,
        warning,
    })
}

/// The uuid `wire` names under `field`, in lower case.
fn id(wire: &Row, field: &'static str, missing: Refusal) -> Result<String, Refusal> {
    match wire.get(field) {
        None | Some(Value::Null) => Err(missing),
        Some(Value::String(text)) if is_uuid(text) => Ok(text.to_ascii_lowercase()),
        Some(_) => Err(Refusal::NotAnId(field)),
    }
}

fn is_uuid(text: &str) -> bool {
    text.len() == 36
        && text.char_indices().all(|(i, c)| match i {
            8 | 13 | 18 | 23 => c == '-',
            _ => c.is_ascii_hexdigit(),
        })
}
