//! DataCapability `media.setMediaPlace` (`data-capability.json`): the DM
//! ties a media of a campaign — a battlemap or an illustration — to one of
//! the campaign's places, changes it, or removes it.
//!
//! A change of place is one command on `Media/<the media's id>` writing its
//! `placeId` alone. What the screen sends is `{ "media", "place" }`, and it
//! becomes the aggregate's field in two steps:
//!
//! - [`asked`] reads the request: the media is the command's target or, with
//!   none, the payload's `media` — a uuid, or no media at all; the place is a uuid, or `null` to detach
//!   the media — a media without a place stays valid. Both are lowercase, as
//!   the tables write them, so one media is one partition whatever case the
//!   request used. Any other field is refused ([`Refusal::Unknown`]).
//! - [`prepare`] judges it against the media's row and the place's, as the
//!   store holds them: a media the store does not hold is refused
//!   ([`Refusal::UnknownMedia`]); a place that is no element, not a « lieu »,
//!   or a lieu of another campaign breaks `media-place-is-lieu`
//!   ([`Refusal::NotALieu`]), and nothing is queued. Detaching needs no
//!   place, only the media.
//!
//! The rows are read under the aggregates' fields: the media's `campaignId`
//! (Media), the place's `type` and `entityCampaignId` (Entity). The table's
//! constraint trigger judges `media-place-is-lieu` again when the place is
//! written.

use std::collections::BTreeMap;

use async_trait::async_trait;
use harness_dataguard::plugins::{self, Context, Plugin, Prepare};
use serde_json::Value;

/// This DataCapability's id, as `data-capability.json` names it.
pub const DATA_CAPABILITY: &str = "media.setMediaPlace";

/// The payload's field naming the media when the command names no target
/// (`target.id` of `data-capability.json`); never written.
const MEDIA: &str = "media";

/// The payload's field naming the place, or `null`.
const PLACE: &str = "place";

/// The aggregate's field the place is written under.
pub const PLACE_ID: &str = "placeId";

/// The type of element a place is.
const LIEU: &str = "lieu";

/// The invariant a place breaks when it is no lieu of the media's campaign.
pub const MEDIA_PLACE_IS_LIEU: &str = "media-place-is-lieu";

/// One partition's row, as the engine holds it.
pub type Row = BTreeMap<String, Value>;

/// What a request asks: the media, and the place it is attached to — `None`
/// to detach it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Asked {
    /// The media's id, lowercase.
    pub media: String,
    /// The place's id, lowercase; `None` detaches the media.
    pub place: Option<String>,
}

/// What a change of place becomes before it is submitted.
#[derive(Clone, Debug, PartialEq)]
pub struct Prepared {
    /// The media's id, lowercase as the table writes it.
    pub target_id: String,
    /// The one field written on its partition: `placeId`, a uuid or `null`.
    pub payload: Row,
}

/// Why a change of place is refused before it is queued.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// Neither the command nor its payload names the media.
    NoMedia,
    /// The payload carries a field that is neither `media` nor `place`.
    Unknown(String),
    /// The payload says nothing of the place: `null` detaches, so silence
    /// is no answer.
    NoPlace,
    /// The place is neither a uuid nor `null`.
    InvalidPlace,
    /// The store holds no such media.
    UnknownMedia,
    /// The place is no element of type « lieu » of the media's campaign.
    NotALieu,
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoMedia => write!(f, "aucun média désigné"),
            Self::Unknown(field) => write!(f, "{field} : seuls media et place se règlent"),
            Self::NoPlace => write!(f, "place : un lieu, ou null pour détacher le média"),
            Self::InvalidPlace => write!(f, "place : l'identifiant d'un lieu, ou null"),
            Self::UnknownMedia => write!(f, "média introuvable"),
            Self::NotALieu => write!(
                f,
                "{MEDIA_PLACE_IS_LIEU} : le lieu d'un média est un élément de type « lieu » de sa campagne"
            ),
        }
    }
}

/// Whether `id` names this DataCapability, with or without its `@version`.
#[must_use]
pub fn names(id: &str) -> bool {
    id.split('@').next() == Some(DATA_CAPABILITY)
}

/// What the DataCapability plugs into the engine: its `Prepare`.
#[must_use]
pub fn plugin() -> Plugin {
    Plugin::new().prepare(DATA_CAPABILITY, Changes)
}

/// The partitions of the Media aggregate.
const MEDIA_PARTITION: &str = "Media/";

/// The partitions of the Entity aggregate, whose elements a place is one of.
const ENTITY_PARTITION: &str = "Entity/";

/// A change of place made the `placeId` it writes on the media's partition,
/// judged against the media and the place ([`placed`]): one of a media
/// there is not — or named by anything but a uuid — is refused, 404; one
/// naming a place that is no lieu of the media's campaign
/// (`media-place-is-lieu`), or no media, or a malformed place, 422 — neither
/// is queued.
struct Changes;

#[async_trait]
impl Prepare for Changes {
    async fn prepare(
        &self,
        context: &Context<'_>,
        payload: &BTreeMap<String, Value>,
        target_id: Option<&str>,
    ) -> Result<plugins::Prepared, plugins::Refusal> {
        let asked = asked(payload, target_id).map_err(refused)?;
        let (media, place) = placed(context, &asked).await?;
        let prepared = prepare(&asked, Some(&media), place.as_ref()).map_err(refused)?;
        Ok(plugins::Prepared {
            payload: prepared.payload,
            target_id: Some(prepared.target_id),
            ..plugins::Prepared::default()
        })
    }
}

/// A refusal as the server answers it: a media there is not is not found,
/// anything else unprocessable.
fn refused(refusal: Refusal) -> plugins::Refusal {
    if refusal == Refusal::UnknownMedia {
        plugins::Refusal::not_found(refusal.to_string())
    } else {
        plugins::Refusal::unprocessable(refusal.to_string())
    }
}

/// The media `asked` names and the element it places it at, as a change of
/// place is judged against them: the engine's rows as the commands queued
/// on them will leave them — a media only the tables hold is hydrated by
/// its aggregate, and a lieu saved a moment ago is there — and, over
/// PostgreSQL, the element under them as the tables hold it, the fixtures'
/// among them. The media's row is empty when there is no such media, the
/// place `None` when there is no such element, or none is named.
async fn placed(
    context: &Context<'_>,
    asked: &Asked,
) -> Result<(Row, Option<Row>), plugins::Refusal> {
    let media = context
        .row(&format!("{MEDIA_PARTITION}{}", asked.media))
        .await?;
    let Some(id) = &asked.place else {
        return Ok((media, None));
    };
    let mut place = match context.rows {
        Some(rows) => aggregate_media::place(rows, id)
            .await
            .map_err(|e| plugins::Refusal::unprocessable(e.to_string()))?
            .unwrap_or_default(),
        None => Row::new(),
    };
    place.extend(context.row(&format!("{ENTITY_PARTITION}{id}")).await?);
    Ok((media, Some(place).filter(|place| !place.is_empty())))
}

/// What `wire` asks of the media `target` names — or, with no target or a
/// blank one, the payload's `media`.
///
/// # Errors
///
/// [`Refusal`] when no media is named, or one by something other than a uuid
/// ([`Refusal::UnknownMedia`]), `wire` carries another field, says nothing of
/// the place, or names it by something other than a uuid.
pub fn asked(wire: &Row, target: Option<&str>) -> Result<Asked, Refusal> {
    if let Some(field) = wire.keys().find(|field| *field != MEDIA && *field != PLACE) {
        return Err(Refusal::Unknown(field.clone()));
    }
    let media = named(target)
        .or_else(|| named(wire.get(MEDIA).and_then(Value::as_str)))
        .ok_or(Refusal::NoMedia)?;
    // No media has an id that is no uuid: `media.id` is one.
    if !uuid(media) {
        return Err(Refusal::UnknownMedia);
    }
    let media = media.to_ascii_lowercase();
    let place = match wire.get(PLACE).ok_or(Refusal::NoPlace)? {
        Value::Null => None,
        Value::String(id) if uuid(id.trim()) => Some(id.trim().to_ascii_lowercase()),
        _ => return Err(Refusal::InvalidPlace),
    };
    Ok(Asked { media, place })
}

/// `asked` judged against the media's row — `None` when the store holds no
/// such media — and the place's — `None` when it holds no such element — as
/// it is submitted.
///
/// # Errors
///
/// [`Refusal::UnknownMedia`] when there is no media, [`Refusal::NotALieu`]
/// when the place is no lieu of the media's campaign.
pub fn prepare(
    asked: &Asked,
    media: Option<&Row>,
    place: Option<&Row>,
) -> Result<Prepared, Refusal> {
    let media = media
        .filter(|row| !row.is_empty())
        .ok_or(Refusal::UnknownMedia)?;
    if asked.place.is_some() {
        let place = place.ok_or(Refusal::NotALieu)?;
        let campaign = |row: &Row, field: &str| {
            row.get(field)
                .and_then(Value::as_str)
                .map(str::to_ascii_lowercase)
        };
        let lieu = place.get("type").and_then(Value::as_str) == Some(LIEU);
        let same = campaign(media, "campaignId")
            .is_some_and(|ours| campaign(place, "entityCampaignId") == Some(ours));
        if !lieu || !same {
            return Err(Refusal::NotALieu);
        }
    }
    let payload = Row::from([(
        PLACE_ID.to_string(),
        asked.place.clone().map_or(Value::Null, Value::from),
    )]);
    Ok(Prepared {
        target_id: asked.media.clone(),
        payload,
    })
}

/// An id given, trimmed: `None` when it is blank.
fn named(id: Option<&str>) -> Option<&str> {
    id.map(str::trim).filter(|id| !id.is_empty())
}

/// A uuid written 8-4-4-4-12 in hex digits, in either case: what
/// `media.place_id` holds.
fn uuid(id: &str) -> bool {
    id.len() == 36
        && id.char_indices().all(|(at, c)| match at {
            8 | 13 | 18 | 23 => c == '-',
            _ => c.is_ascii_hexdigit(),
        })
}
