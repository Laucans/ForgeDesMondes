//! DataCapability `live.showWindowOnPlayerView` (`data-capability.json`):
//! the DM opens an illustration in a floating window over the player view's
//! base, so the players see it without losing the map.
//!
//! The engine writes one partition per command: opening a window is one
//! command on `Diffusion/<the session's id>`, the one row both player views
//! read (single-diffusion-source) — made by it when the session had none,
//! its window replaced otherwise. The session is the live session of the
//! campaign the payload names; the server looks it up, and what the tables
//! say of the media, from [`named`] ([`Found`]); [`prepare`] turns them,
//! with the row as the engine will leave it, into the state written:
//!
//! - the window shows the media at its default position and width
//!   ([`X`], [`Y`], [`WIDTH`]), the three set together
//!   (window-fields-together); its height is never written: it follows from
//!   the width and the illustration's proportions (window-proportional);
//! - the base is not written: it stays as the row holds it. A session whose
//!   row holds no base yet takes the one its table row shows, else the
//!   cover — what its player view shows before anything is sent
//!   (base-coherent);
//! - a media that is no illustration of the campaign, or one with no
//!   proportions, gets no width its height could follow: it is queued all
//!   the same, for the DataGuard to refuse under window-proportional, and a
//!   warning says why;
//! - a payload naming no campaign or no media, a campaign with no live
//!   session, or a media the tables do not hold, is refused before it is
//!   queued ([`Refusal`]): nothing changes.

use std::collections::BTreeMap;

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

/// Whether `id` — `live.showWindowOnPlayerView`, with or without its
/// `@version` — names this DataCapability.
pub fn names(id: &str) -> bool {
    id.split('@').next() == Some(DATA_CAPABILITY)
}

/// What a window names, the rows the server looks up: uuids in lower case.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Named {
    /// The campaign whose live session's player view opens the window.
    pub campaign: String,
    /// The media the window shows.
    pub media: String,
}

/// The base a session's diffusion row shows, as its table holds it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Base {
    /// `cover`, `illustration` or `battlemap`.
    pub kind: String,
    /// The illustration shown, for an illustration.
    pub media: Option<String>,
    /// The battlemap instance shown, for a battlemap.
    pub instance: Option<String>,
}

impl Base {
    /// Whether the base shows something — the cover, an illustration with
    /// its media, a battlemap with its instance — as base-coherent asks.
    fn shows(&self) -> bool {
        match self.kind.as_str() {
            COVER => self.media.is_none() && self.instance.is_none(),
            "illustration" => self.media.is_some() && self.instance.is_none(),
            "battlemap" => self.instance.is_some() && self.media.is_none(),
            _ => false,
        }
    }
}

/// What the tables say of what a window names.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Found {
    /// The campaign's live session, when it has one.
    pub session: Option<String>,
    /// Whether the tables hold the media at all.
    pub held: bool,
    /// Whether the media is an illustration of the campaign, with a width
    /// and a height its window can keep the proportions of.
    pub illustration: bool,
    /// The base the live session's diffusion row shows in its table, when
    /// the table holds one.
    pub base: Option<Base>,
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
    NotAnId,
    /// No session of the campaign is live: no player view is shown.
    NoLiveSession,
    /// The tables hold no such media.
    UnknownMedia,
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::NoCampaign => "campaign manque",
            Self::NoMedia => "media manque",
            Self::NotAnId => "campaign ou media n'est pas un identifiant",
            Self::NoLiveSession => "aucune séance n'est en cours",
            Self::UnknownMedia => "ce média n'existe pas",
        })
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
/// of the campaign's live session, `row` as the commands queued there will
/// leave it, stamped `now`.
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
    // A row the engine does not hold yet is made: the session's, showing the
    // base its table row shows — the cover when it has none, or one whose
    // media or instance is gone (onDelete nullify), which shows nothing.
    if !set(row, "sessionId") {
        payload.insert("sessionId".to_string(), Value::from(session.as_str()));
    }
    if !set(row, "baseKind") {
        let base = found
            .base
            .clone()
            .filter(Base::shows)
            .unwrap_or_else(|| Base {
                kind: COVER.to_string(),
                ..Base::default()
            });
        payload.insert("baseKind".to_string(), Value::from(base.kind));
        payload.insert("baseMediaId".to_string(), Value::from(base.media));
        payload.insert("baseInstanceId".to_string(), Value::from(base.instance));
    }
    Ok(Prepared {
        payload,
        target_id: session,
        warning,
    })
}

/// The uuid `wire` names under `field`, in lower case.
fn id(wire: &Row, field: &str, missing: Refusal) -> Result<String, Refusal> {
    match wire.get(field) {
        None | Some(Value::Null) => Err(missing),
        Some(Value::String(text)) if is_uuid(text) => Ok(text.to_ascii_lowercase()),
        Some(_) => Err(Refusal::NotAnId),
    }
}

/// Whether `row` holds a value for `field`: a `null` is none.
fn set(row: &Row, field: &str) -> bool {
    row.get(field).is_some_and(|value| !value.is_null())
}

fn is_uuid(text: &str) -> bool {
    text.len() == 36
        && text.char_indices().all(|(i, c)| match i {
            8 | 13 | 18 | 23 => c == '-',
            _ => c.is_ascii_hexdigit(),
        })
}
