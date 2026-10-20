//! DataCapability `live.showOnPlayerView` (`data-capability.json`): the DM
//! sends a battlemap instance of the session, or an illustration, as what the
//! player view shows behind everything — its base — with the zoom and the pan
//! they framed it with.
//!
//! The engine writes one partition per command: a send is one command on
//! `Diffusion/<the session's id>`, the one row both player views read
//! (single-diffusion-source). The first send of a session makes the row,
//! later ones replace its base; the floating window is not touched. What the
//! DM sends is the wire payload of the Battlemap or of the media library;
//! [`prepare`] turns it into that state, against what the tables say of the
//! rows it names ([`Found`]), which the server looks up from [`named`]:
//!
//! - the session is the live session of the campaign the payload names, or
//!   — the Battlemap names none — of the instance's or the media's campaign;
//! - a battlemap base is the instance alone, an illustration base the media
//!   alone: the other is emptied, so the base is one thing (base-coherent);
//! - an instance of another session, or a media that is no illustration of
//!   the session's campaign, is not written: the base the state then has is
//!   none, which base-coherent refuses — the DataGuard rejects the send, and
//!   the row stays as it was;
//! - the framing is the payload's, or 1 and 0, 0 when it gives none; a send
//!   the table could never hold is refused before it is queued
//!   ([`Refusal`]): an id is a uuid, the zoom a number above 0 that a
//!   `numeric(6,3)` holds, a pan one a `numeric(9,2)` holds.

use std::collections::BTreeMap;

use serde_json::Value;

/// This DataCapability's id, as `data-capability.json` names it.
pub const DATA_CAPABILITY: &str = "live.showOnPlayerView";

/// The two bases a send shows; the cover is `live.showCover`'s.
const BATTLEMAP: &str = "battlemap";
const ILLUSTRATION: &str = "illustration";

/// The framing when the payload gives none.
const ZOOM: f64 = 1.0;
const PAN: f64 = 0.0;

/// The widest zoom and pan the table holds: `numeric(6,3)` and
/// `numeric(9,2)`, each bound excluded.
const ZOOM_BELOW: f64 = 1000.0;
const PAN_BELOW: f64 = 10_000_000.0;

/// What the warnings of a send the DataGuard will refuse say.
pub const NOT_OF_THE_SESSION: &str = "cette instance n'est pas une carte de la séance";
pub const NOT_AN_ILLUSTRATION: &str = "ce média n'est pas une illustration de la campagne";

/// A row, as the engine holds it.
pub type Row = BTreeMap<String, Value>;

/// Whether `id` — `live.showOnPlayerView`, with or without its `@version` —
/// names this DataCapability.
pub fn names(id: &str) -> bool {
    id.split('@').next() == Some(DATA_CAPABILITY)
}

/// The rows a send names, ids in lower case: what the server looks up before
/// it is prepared.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Named {
    /// The campaign whose live session shows the base, when the payload
    /// names one.
    pub campaign: Option<String>,
    /// The battlemap instance sent.
    pub instance: Option<String>,
    /// The illustration sent.
    pub media: Option<String>,
}

/// What the tables say of the rows a send names.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Found {
    /// The live session the base is shown in, its id in lower case.
    pub session: Option<String>,
    /// The session the instance sent belongs to.
    pub instance_session: Option<String>,
    /// Whether the media sent is an illustration of the session's campaign.
    pub illustration: bool,
    /// Whether the tables hold the instance or the media sent.
    pub held: bool,
}

/// What a send becomes before it is submitted.
#[derive(Clone, Debug, PartialEq)]
pub struct Prepared {
    /// The state it writes on the session's diffusion row.
    pub payload: Row,
    /// The partition it writes: the session's id.
    pub target_id: String,
    /// Why the DataGuard will refuse it, when it will.
    pub warning: Option<&'static str>,
}

/// Why a send is refused before it is queued: nothing changes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// The kind is neither `battlemap` nor `illustration`.
    Kind,
    /// The instance of a battlemap, or the media of an illustration, is not
    /// given.
    Missing(&'static str),
    /// A field naming a row is not a uuid.
    NotAnId(&'static str),
    /// The zoom is not a number above 0 and below 1000.
    Zoom,
    /// A pan is not a number the table holds.
    Pan(&'static str),
    /// No live session shows the base.
    NoLiveSession,
    /// The instance or the media sent, naming no campaign, is none the
    /// tables hold: no campaign, so no session, is known.
    Unknown(&'static str),
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Kind => f.write_str("on envoie aux joueurs une battlemap ou une illustration"),
            Self::Missing(field) => write!(f, "{field} manque"),
            Self::NotAnId(field) => write!(f, "{field} n'est pas un identifiant"),
            Self::Zoom => f.write_str("le zoom est un nombre au-dessus de 0 et sous 1000"),
            Self::Pan(field) => write!(f, "{field} est un nombre de moins de 10 000 000"),
            Self::NoLiveSession => f.write_str("aucune séance n'est en cours"),
            Self::Unknown(field) => write!(f, "{field} n'existe pas"),
        }
    }
}

/// The kind a send shows, and the rows it names.
///
/// # Errors
///
/// [`Refusal::Kind`], [`Refusal::Missing`] or [`Refusal::NotAnId`].
pub fn named(wire: &Row) -> Result<Named, Refusal> {
    let campaign = id(wire, "campaign")?;
    let (instance, media) = match wire.get("kind").and_then(Value::as_str) {
        Some(BATTLEMAP) => (Some(required(wire, "instance")?), None),
        Some(ILLUSTRATION) => (None, Some(required(wire, "media")?)),
        _ => return Err(Refusal::Kind),
    };
    Ok(Named {
        campaign,
        instance,
        media,
    })
}

/// The state a send writes on the session's diffusion row, stamped `now`.
///
/// # Errors
///
/// What [`named`] refuses, [`Refusal::Zoom`], [`Refusal::Pan`], and
/// [`Refusal::NoLiveSession`] when `found` holds no session —
/// [`Refusal::Unknown`] when the send names no campaign and the tables hold
/// neither its instance nor its media.
pub fn prepare(wire: &Row, found: &Found, now: &str) -> Result<Prepared, Refusal> {
    let Named {
        campaign,
        instance,
        media,
    } = named(wire)?;
    let zoom = number(wire, "zoom", ZOOM, 3)
        .filter(|zoom| *zoom > 0.0 && *zoom < ZOOM_BELOW)
        .ok_or(Refusal::Zoom)?;
    let pan = |field| {
        number(wire, field, PAN, 2)
            .filter(|pan| pan.abs() < PAN_BELOW)
            .ok_or(Refusal::Pan(field))
    };
    let (pan_x, pan_y) = (pan("panX")?, pan("panY")?);
    let session = found.session.clone().ok_or_else(|| {
        let sent = if instance.is_some() {
            "instance"
        } else {
            "media"
        };
        if campaign.is_none() && !found.held {
            Refusal::Unknown(sent)
        } else {
            Refusal::NoLiveSession
        }
    })?;
    let (kind, warning, media, instance) = match (instance, media) {
        (Some(instance), _) => {
            let ours = found.instance_session.as_deref() == Some(session.as_str());
            (
                BATTLEMAP,
                (!ours).then_some(NOT_OF_THE_SESSION),
                None,
                ours.then_some(instance),
            )
        }
        (_, media) => {
            let ours = found.illustration;
            (
                ILLUSTRATION,
                (!ours).then_some(NOT_AN_ILLUSTRATION),
                media.filter(|_| ours),
                None,
            )
        }
    };
    let payload = Row::from([
        ("sessionId".to_string(), Value::from(session.as_str())),
        ("baseKind".to_string(), Value::from(kind)),
        (
            "baseMediaId".to_string(),
            media.map_or(Value::Null, Value::from),
        ),
        (
            "baseInstanceId".to_string(),
            instance.map_or(Value::Null, Value::from),
        ),
        ("viewZoom".to_string(), Value::from(zoom)),
        ("viewPanX".to_string(), Value::from(pan_x)),
        ("viewPanY".to_string(), Value::from(pan_y)),
        ("updatedAt".to_string(), Value::from(now)),
    ]);
    Ok(Prepared {
        payload,
        target_id: session,
        warning,
    })
}

/// The uuid `field` names in lower case, `None` when absent or `null`.
fn id(wire: &Row, field: &'static str) -> Result<Option<String>, Refusal> {
    match wire.get(field) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(text)) if is_uuid(text) => Ok(Some(text.to_ascii_lowercase())),
        Some(_) => Err(Refusal::NotAnId(field)),
    }
}

/// The uuid `field` names, which the send cannot go without.
fn required(wire: &Row, field: &'static str) -> Result<String, Refusal> {
    id(wire, field)?.ok_or(Refusal::Missing(field))
}

/// `field` as a finite number rounded to `places` decimals, `default` when
/// absent or `null`; `None` when it is no number.
fn number(wire: &Row, field: &str, default: f64, places: i32) -> Option<f64> {
    let value = match wire.get(field) {
        None | Some(Value::Null) => Some(default),
        Some(value) => value.as_f64(),
    }?;
    let scale = 10_f64.powi(places);
    Some((value * scale).round() / scale).filter(|n| n.is_finite())
}

fn is_uuid(text: &str) -> bool {
    text.len() == 36
        && text.char_indices().all(|(i, c)| match i {
            8 | 13 | 18 | 23 => c == '-',
            _ => c.is_ascii_hexdigit(),
        })
}
