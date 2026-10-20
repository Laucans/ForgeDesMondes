//! DataCapability `media.uploadMedia` (`data-capability.json`): the DM adds
//! a battlemap or an illustration to a campaign, optionally tied to one of
//! its places.
//!
//! The engine writes one partition per command: an upload is one command on
//! `Media/<a new id>`, carrying the media and — for a battlemap — its grid,
//! which the DataGuard's invariants judge and its store projects into
//! `media` and `battlemap_grid`. What the DM sends is the wire payload of the
//! media library; [`checked`] reads it and [`Upload::prepare`] turns it
//! into that state:
//!
//! - the wire's names are the aggregate's (`campaign` is `campaignId`,
//!   `file` is `filePath`, `width` is `widthPx`…); a field it does not name
//!   is dropped. The path is the one the DM chose: no file is copied;
//! - a battlemap has its grid from its upload (`grid-only-for-battlemap`):
//!   the mock-up's defaults ([`default_grid`]), its opacity a fraction of 1
//!   (`grid-values-range`). A hex's 5 feet is not written: the table
//!   generates it (`hex-is-five-feet`). An illustration has no grid;
//! - an upload the tables could never hold is refused before it is queued
//!   ([`Refusal`]): it names its campaign by a uuid — one the server holds —
//!   its kind, its title, pixel counts an `integer` holds, its place by a
//!   uuid, and no text holding U+0000;
//! - its place, when it names one, is a « lieu » of the same campaign
//!   (`media-place-is-lieu`): the element is read by the server, which the
//!   engine does not hold beside the media, and an upload naming another is
//!   refused under the invariant's id. The table's trigger judges it again
//!   when the upload is applied.

use std::collections::BTreeMap;

use serde_json::{Value, json};

/// This DataCapability's id, as `data-capability.json` names it.
pub const DATA_CAPABILITY: &str = "media.uploadMedia";

/// The kind of media that has a grid.
pub const BATTLEMAP: &str = "battlemap";

/// The kind of media that has none.
pub const ILLUSTRATION: &str = "illustration";

/// The type of element a media's place is.
pub const LIEU: &str = "lieu";

/// The grid a battlemap arrives with, under the aggregate's names: the
/// mock-up's new battlemap (`mock/components/MediaLibrary.html`) — a 32 px
/// hex, no offset, pointy, the text colour, 45 % opaque — its orientation as
/// `battlemap_grid` stores it and its opacity a fraction of 1.
#[must_use]
pub fn default_grid() -> Row {
    [
        ("hexSizePx", json!(32)),
        ("offsetX", json!(0)),
        ("offsetY", json!(0)),
        ("orientation", json!("pointe")),
        ("color", json!("text")),
        ("opacity", json!(0.45)),
    ]
    .into_iter()
    .map(|(field, value)| (field.to_owned(), value))
    .collect()
}

/// One partition's row, as the engine holds it.
pub type Row = BTreeMap<String, Value>;

/// The element an upload names as its place, as the server read it: its
/// type and its campaign.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Place {
    /// Its type: `pj`, `pnj`, `lieu` or `evenement`.
    pub kind: String,
    /// Its campaign's id.
    pub campaign: String,
}

/// What an upload is given besides its wire payload: the media's new id,
/// which the server made, and the time.
#[derive(Clone, Copy, Debug)]
pub struct Fresh<'a> {
    /// The media's id: a uuid.
    pub media: &'a str,
    /// The time of the upload, RFC 3339.
    pub now: &'a str,
}

/// Why an upload is refused before it is queued: what the tables could never
/// hold, or a place that is no lieu of the campaign.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// The upload names no campaign.
    NoCampaign,
    /// The campaign it names is none the tables hold.
    UnknownCampaign,
    /// Its kind is none of `battlemap`, `illustration`.
    NoKind,
    /// It has no title, or one of blanks alone.
    NoTitle,
    /// A field naming another row (its campaign, its place) is not a uuid.
    NotAnId(&'static str),
    /// The path is not a text.
    NotAPath,
    /// A width or a height is not an integer from 0 to 2147483647.
    NotAPixelCount(&'static str),
    /// A text holds the character U+0000, which `text` cannot hold.
    Nul(&'static str),
    /// Its place is no « lieu » of its campaign (`media-place-is-lieu`).
    PlaceNotLieu,
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoCampaign => f.write_str("un média nomme sa campagne"),
            Self::UnknownCampaign => f.write_str("la campagne du média n'existe pas"),
            Self::NoKind => f.write_str("un média est une battlemap ou une illustration"),
            Self::NoTitle => f.write_str("un média a un titre"),
            Self::NotAnId(field) => write!(f, "{field} n'est pas un identifiant"),
            Self::NotAPath => f.write_str("file n'est pas un chemin"),
            Self::NotAPixelCount(field) => {
                write!(f, "{field} n'est pas un nombre de pixels de 0 à 2147483647")
            }
            Self::Nul(field) => write!(f, "{field} contient un caractère nul"),
            Self::PlaceNotLieu => f.write_str(
                "media-place-is-lieu: le lieu d'un média est un élément de type « lieu » de sa campagne",
            ),
        }
    }
}

/// What an upload becomes before it is submitted.
#[derive(Clone, Debug, PartialEq)]
pub struct Prepared {
    /// The media's id: its partition is `Media/<it>`.
    pub target_id: String,
    /// The state written there.
    pub payload: Row,
}

/// Whether `id` names this DataCapability, with or without its `@version`.
#[must_use]
pub fn names(id: &str) -> bool {
    id.split('@').next() == Some(DATA_CAPABILITY)
}

/// An upload whose wire payload the tables could hold, its place not read
/// yet: what [`checked`] answers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Upload<'a> {
    campaign: &'a str,
    kind: &'a str,
    title: &'a str,
    file: &'a str,
    width: i64,
    height: i64,
    place: Option<&'a str>,
}

/// `wire` checked: everything the tables could never hold refused before
/// its place is read.
///
/// # Errors
///
/// [`Refusal`] when the tables could never hold the upload.
pub fn checked(wire: &Row) -> Result<Upload<'_>, Refusal> {
    let campaign = match wire.get("campaign") {
        None | Some(Value::Null) => return Err(Refusal::NoCampaign),
        Some(campaign) => campaign
            .as_str()
            .filter(|id| is_uuid(id))
            .ok_or(Refusal::NotAnId("campaign"))?,
    };
    let kind = wire
        .get("kind")
        .and_then(Value::as_str)
        .filter(|kind| [BATTLEMAP, ILLUSTRATION].contains(kind))
        .ok_or(Refusal::NoKind)?;
    let title = wire
        .get("title")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|title| !title.is_empty())
        .ok_or(Refusal::NoTitle)?;
    let file = match wire.get("file") {
        None | Some(Value::Null) => "",
        Some(file) => file.as_str().ok_or(Refusal::NotAPath)?,
    };
    for (field, text) in [("title", title), ("file", file)] {
        if text.contains('\0') {
            return Err(Refusal::Nul(field));
        }
    }
    // A place is named by a uuid; absent, `null` or empty, it is none.
    let place = match wire.get("place") {
        None | Some(Value::Null) => None,
        Some(Value::String(id)) if id.is_empty() => None,
        Some(id) => Some(
            id.as_str()
                .filter(|id| is_uuid(id))
                .ok_or(Refusal::NotAnId("place"))?,
        ),
    };
    Ok(Upload {
        campaign,
        kind,
        title,
        file,
        width: pixels(wire, "width")?,
        height: pixels(wire, "height")?,
        place,
    })
}

impl Upload<'_> {
    /// The campaign it is added to.
    #[must_use]
    pub fn campaign(&self) -> &str {
        self.campaign
    }

    /// The place it names, a uuid: the element the server reads.
    #[must_use]
    pub fn place(&self) -> Option<&str> {
        self.place
    }

    /// The state it writes on `Media/<fresh.media>`, `place` being the
    /// element its place names as the server read it — `None` when it names
    /// none, or one the server does not hold.
    ///
    /// # Errors
    ///
    /// [`Refusal::PlaceNotLieu`] when its place is no lieu of its campaign.
    pub fn prepare(&self, place: Option<&Place>, fresh: Fresh<'_>) -> Result<Prepared, Refusal> {
        let place_id = match self.place {
            None => Value::Null,
            Some(id) => {
                let lieu = place.is_some_and(|place| {
                    place.kind == LIEU && place.campaign.eq_ignore_ascii_case(self.campaign)
                });
                if !lieu {
                    return Err(Refusal::PlaceNotLieu);
                }
                id.into()
            }
        };
        let mut payload = Row::new();
        payload.insert("id".into(), fresh.media.into());
        payload.insert("campaignId".into(), self.campaign.into());
        payload.insert("kind".into(), self.kind.into());
        payload.insert("title".into(), self.title.into());
        payload.insert("filePath".into(), self.file.into());
        payload.insert("widthPx".into(), self.width.into());
        payload.insert("heightPx".into(), self.height.into());
        payload.insert("placeId".into(), place_id);
        payload.insert("uploadedAt".into(), fresh.now.into());
        if self.kind == BATTLEMAP {
            payload.insert("mediaId".into(), fresh.media.into());
            payload.extend(default_grid());
        }
        Ok(Prepared {
            target_id: fresh.media.to_owned(),
            payload,
        })
    }
}

/// A width or a height: 0 when not given, else an integer `integer` holds.
fn pixels(wire: &Row, field: &'static str) -> Result<i64, Refusal> {
    match wire.get(field) {
        None | Some(Value::Null) => Ok(0),
        Some(value) => value
            .as_i64()
            .filter(|n| (0..=i64::from(i32::MAX)).contains(n))
            .ok_or(Refusal::NotAPixelCount(field)),
    }
}

/// A uuid written 8-4-4-4-12 in hex digits, in either case.
fn is_uuid(text: &str) -> bool {
    text.len() == 36
        && text.char_indices().all(|(i, c)| match i {
            8 | 13 | 18 | 23 => c == '-',
            _ => c.is_ascii_hexdigit(),
        })
}
