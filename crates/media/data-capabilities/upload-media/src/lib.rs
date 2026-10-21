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
//!
//! Its `Prepare` ([`plugin`]) reads what an upload is judged against: over
//! PostgreSQL its campaign and its place as the tables hold them — the
//! fixtures' among them — and over the engine's memory, which has no table,
//! every campaign is held and the place is the engine's row. The media's id
//! is a fresh one, its time the engine's to the millisecond. A replay under
//! an idempotency key already submitted is the engine's to answer, before
//! anything is read again.

use std::collections::BTreeMap;

use async_trait::async_trait;
use harness_dataguard::plugins::{self, Context, Plugin, Prepare};
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

/// What the DataCapability plugs into the engine: its `Prepare`.
#[must_use]
pub fn plugin() -> Plugin {
    Plugin::new().prepare(DATA_CAPABILITY, Uploads)
}

/// A live campaign the tables hold, `$1` its id in whatever case. A
/// campaign the engine deleted (its tombstone) is none.
const CAMPAIGN: &str = "SELECT c.id::text AS id FROM campaign c WHERE c.id::text = lower($1)
    AND NOT EXISTS (SELECT 1 FROM dataguard_rows r
        WHERE r.partition = 'Campaign/' || c.id::text AND r.tombstoned)";

/// The partitions of the Entity aggregate, whose elements a place is one of.
const ENTITY: &str = "Entity/";

/// An upload made the media it writes on a new media's partition: checked
/// first, then its campaign and its place read. What the tables could never
/// hold, a campaign they do not hold, or a place that is no lieu of the
/// campaign is refused, 422 — and so is a read that fails.
struct Uploads;

#[async_trait]
impl Prepare for Uploads {
    async fn prepare(
        &self,
        context: &Context<'_>,
        payload: &BTreeMap<String, Value>,
        _target_id: Option<&str>,
    ) -> Result<plugins::Prepared, plugins::Refusal> {
        let upload = checked(payload).map_err(refused)?;
        if !held(context, upload.campaign()).await? {
            return Err(refused(Refusal::UnknownCampaign));
        }
        let place = match upload.place() {
            Some(id) => placed(context, id).await?,
            None => None,
        };
        let media = context.ids.fresh();
        let now = stamp(context.now_millis);
        let prepared = upload
            .prepare(
                place.as_ref(),
                Fresh {
                    media: &media,
                    now: &now,
                },
            )
            .map_err(refused)?;
        // The payload's `id` is the new media's: the manifest's `target.id`
        // (`$id`) names its partition from it.
        Ok(plugins::Prepared {
            payload: prepared.payload,
            ..plugins::Prepared::default()
        })
    }
}

/// An upload refused: 422, saying why.
fn refused(refusal: Refusal) -> plugins::Refusal {
    plugins::Refusal::unprocessable(refusal.to_string())
}

/// A read that failed: nothing is queued.
fn unread(error: harness_dataguard::Error) -> plugins::Refusal {
    plugins::Refusal::unprocessable(error.to_string())
}

/// Whether the tables hold the campaign `id` names, live — over the
/// engine's memory, which has no table, every campaign is held.
async fn held(context: &Context<'_>, id: &str) -> Result<bool, plugins::Refusal> {
    let Some(rows) = context.rows else {
        return Ok(true);
    };
    let found = rows
        .rows(CAMPAIGN, &[Value::from(id)])
        .await
        .map_err(unread)?;
    Ok(found.as_array().is_some_and(|found| !found.is_empty()))
}

/// The element `id` names as a media's place reads it — its type and its
/// campaign — from the tables over PostgreSQL, which hold the fixtures'
/// elements and every one applied; from the engine's row over its memory.
/// `None` when there is no such live element.
async fn placed(context: &Context<'_>, id: &str) -> Result<Option<Place>, plugins::Refusal> {
    let row = match context.rows {
        Some(rows) => aggregate_media::place(rows, id).await.map_err(unread)?,
        // The ids the server makes are lower case.
        None => Some(
            context
                .row(&format!("{ENTITY}{}", id.to_ascii_lowercase()))
                .await?,
        ),
    };
    let text = |row: &Row, field: &str| row.get(field).and_then(Value::as_str).map(str::to_owned);
    Ok(row.and_then(|row| {
        text(&row, "type")
            .zip(text(&row, "entityCampaignId"))
            .map(|(kind, campaign)| Place { kind, campaign })
    }))
}

/// `millis` since the epoch as RFC 3339 in UTC, to the millisecond
/// (`2026-10-21T09:00:00.123Z`): what `media.uploaded_at` takes.
#[must_use]
pub fn stamp(millis: u64) -> String {
    let unix_seconds = millis / 1000;
    // Howard Hinnant's civil_from_days.
    // A day count since 1970 is far below `i64::MAX`.
    let days = (unix_seconds / 86_400).cast_signed() + 719_468;
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
        millis % 1000
    )
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
