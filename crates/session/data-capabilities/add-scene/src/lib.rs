//! DataCapability `session.addScene` (`data-capability.json`): the DM adds
//! a scene to the session they are preparing, with its title, its notes and
//! the places it links.
//!
//! A scene is one command on a partition of its own, `Session/<its id>`.
//! What the screen sends is `{ "session", "title", "notes"?, "places"? }`,
//! and it becomes the aggregate's fields in two steps:
//!
//! - [`asked`] reads the request: the session is a uuid — no session has
//!   any other id ([`Refusal::UnknownSession`]); the title is a text with
//!   more than blanks; the notes a text or nothing; the places a list of
//!   uuids, each kept once, in order, lowercase as the tables write them.
//!   Any other field is refused ([`Refusal::Unknown`]).
//! - [`prepare`] judges it against the session's row and the places', as
//!   the store holds them: a session the store does not hold is refused
//!   ([`Refusal::UnknownSession`]); a place that is no element of the
//!   session's campaign breaks `scene-links-same-campaign`
//!   ([`Refusal::OtherCampaign`]); an element of it that is no « lieu » is
//!   no place ([`Refusal::NotAPlace`]). Nothing is queued.
//!
//! The scene's rank is not chosen here: the Session aggregate settles it
//! when the scene is applied, after the session's last, so scenes queued
//! side by side each take the next one (`scene-order-contiguous`), and it
//! judges the places again then. The engine applies them in the order of
//! their ids, so a scene's id begins with the time it was asked at
//! ([`scene_id`]): they are ranked in the order the DM added them. The
//! media of a place it links are the session's through
//! `session_media_view`, never written (`session-media-by-place`).
//!
//! The rows are read under the aggregates' fields: the session's
//! `sessionCampaignId` (Session), a place's `type` and `entityCampaignId`
//! (Entity) — the tables' first, the engine's laid over them.

use std::collections::BTreeMap;

use aggregate_session::{
    LINKS, NOTES, PARTITION, SAME_CAMPAIGN, SCENE_ID, SCENE_SESSION_ID, SCENE_TITLE, uuid,
};
use async_trait::async_trait;
use harness_dataguard::plugins::{self, Context, Plugin, Prepare};
use serde_json::Value;

/// This DataCapability's id, as `data-capability.json` names it.
pub const DATA_CAPABILITY: &str = "session.addScene";

/// The payload's fields.
const SESSION: &str = "session";
const TITLE: &str = "title";
const PLACES: &str = "places";
const FIELDS: [&str; 4] = [SESSION, TITLE, NOTES, PLACES];

/// The session's campaign, and an element's type and campaign, under the
/// aggregates' fields.
pub const SESSION_CAMPAIGN_ID: &str = "sessionCampaignId";
pub const ENTITY_CAMPAIGN_ID: &str = "entityCampaignId";
pub const TYPE: &str = "type";

/// The type of element a place is.
const LIEU: &str = "lieu";

/// The partitions of the Entity aggregate, whose elements a place is one of.
const ENTITY_PARTITION: &str = "Entity/";

/// The session `$1` names, as the tables hold it live: its campaign.
const SESSION_ROW: &str = "SELECT s.campaign_id::text AS \"sessionCampaignId\"
    FROM session s
    WHERE s.id = $1::uuid
        AND NOT EXISTS (SELECT 1 FROM dataguard_rows r
            WHERE r.partition = 'Session/' || s.id::text AND r.tombstoned)";

/// The elements the list `$1` names, as the tables hold them live: each
/// one's id, type and campaign.
const ELEMENT_ROWS: &str = "SELECT e.id::text AS id, e.type AS \"type\",
        e.campaign_id::text AS \"entityCampaignId\"
    FROM entity e
    WHERE e.id IN (SELECT listed::uuid FROM jsonb_array_elements_text($1::jsonb) AS listed)
        AND NOT EXISTS (SELECT 1 FROM dataguard_rows r
            WHERE r.partition = 'Entity/' || e.id::text AND r.tombstoned)";

/// One partition's row, as the engine holds it.
pub type Row = BTreeMap<String, Value>;

/// What a request asks: a scene of `session`, with its title, its notes
/// and the places it links.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Asked {
    /// The session's id, lowercase.
    pub session: String,
    /// The title, trimmed.
    pub title: String,
    /// The notes, as written; `None` for none.
    pub notes: Option<String>,
    /// The places' ids, lowercase, each once, in order.
    pub places: Vec<String>,
}

/// Why a scene is refused before it is queued.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// The payload carries a field the command does not take.
    Unknown(String),
    /// The title is missing, blank, or no text.
    NoTitle,
    /// The notes are neither a text nor null.
    InvalidNotes,
    /// The places are no list of uuids.
    InvalidPlaces,
    /// The store holds no such session.
    UnknownSession,
    /// A place is no element of the session's campaign.
    OtherCampaign,
    /// An element of the session's campaign is no « lieu ».
    NotAPlace,
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unknown(field) => write!(
                f,
                "{field} : une scène se décrit par session, title, notes et places"
            ),
            Self::NoTitle => write!(f, "title : une scène a un titre"),
            Self::InvalidNotes => write!(f, "notes : un texte, ou rien"),
            Self::InvalidPlaces => write!(f, "places : une liste d'identifiants de lieux"),
            Self::UnknownSession => write!(f, "séance introuvable"),
            Self::OtherCampaign => write!(
                f,
                "{SAME_CAMPAIGN} : une scène ne lie que des éléments de la campagne de sa séance"
            ),
            Self::NotAPlace => write!(f, "places : seuls des lieux de la campagne"),
        }
    }
}

/// What the DataCapability plugs into the engine: its `Prepare`.
#[must_use]
pub fn plugin() -> Plugin {
    Plugin::new().prepare(DATA_CAPABILITY, Scenes)
}

/// A scene made the row it writes on a partition of its own, judged against
/// its session and its places ([`session`], [`elements`]): one of a session
/// there is not is refused, 404; one malformed, or linking anything but a
/// place of the session's campaign (`scene-links-same-campaign`), 422 —
/// neither is queued.
struct Scenes;

#[async_trait]
impl Prepare for Scenes {
    async fn prepare(
        &self,
        context: &Context<'_>,
        payload: &BTreeMap<String, Value>,
        _target_id: Option<&str>,
    ) -> Result<plugins::Prepared, plugins::Refusal> {
        let asked = asked(payload).map_err(refused)?;
        let session = session(context, &asked.session).await?;
        let places = elements(context, &asked.places).await?;
        let scene = scene_id(context.now_millis, &context.ids.fresh());
        let payload = prepare(&asked, &scene, &session, &places).map_err(refused)?;
        Ok(plugins::Prepared {
            payload,
            target_id: Some(scene),
            ..plugins::Prepared::default()
        })
    }
}

/// A refusal as the server answers it: a session there is not is not
/// found, anything else unprocessable.
fn refused(refusal: Refusal) -> plugins::Refusal {
    if refusal == Refusal::UnknownSession {
        plugins::Refusal::not_found(refusal.to_string())
    } else {
        plugins::Refusal::unprocessable(refusal.to_string())
    }
}

/// What the tables' read answers, as rows; a failed read is unprocessable.
async fn read(
    context: &Context<'_>,
    sql: &str,
    parameter: Value,
) -> Result<Vec<Row>, plugins::Refusal> {
    let Some(rows) = context.rows else {
        return Ok(Vec::new());
    };
    let found = rows
        .rows(sql, &[parameter])
        .await
        .map_err(|e| plugins::Refusal::unprocessable(e.to_string()))?;
    Ok(found
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|row| match row {
            Value::Object(row) => Some(row.clone().into_iter().collect()),
            _ => None,
        })
        .collect())
}

/// The session `id` names as a scene is judged against it: over
/// PostgreSQL, as the tables hold it live — the fixtures' among them — and,
/// laid over it, the engine's row as the commands queued on it will leave
/// it. Empty when neither holds it.
async fn session(context: &Context<'_>, id: &str) -> Result<Row, plugins::Refusal> {
    let mut row = read(context, SESSION_ROW, Value::from(id))
        .await?
        .into_iter()
        .next()
        .unwrap_or_default();
    row.extend(context.row(&format!("{PARTITION}{id}")).await?);
    Ok(row)
}

/// The elements `ids` name, in order, the same way: one read of the tables
/// for them all, each one's engine row laid over its own.
async fn elements(context: &Context<'_>, ids: &[String]) -> Result<Vec<Row>, plugins::Refusal> {
    let mut tables = if ids.is_empty() {
        Vec::new()
    } else {
        read(context, ELEMENT_ROWS, Value::from(ids.to_vec())).await?
    };
    let mut elements = Vec::with_capacity(ids.len());
    for id in ids {
        let at = tables
            .iter()
            .position(|row| row.get("id").and_then(Value::as_str) == Some(id));
        let mut row = at.map(|at| tables.swap_remove(at)).unwrap_or_default();
        row.remove("id");
        row.extend(context.row(&format!("{ENTITY_PARTITION}{id}")).await?);
        elements.push(row);
    }
    Ok(elements)
}

/// A scene's id: a uuid whose first 48 bits are `millis`, the time it was
/// asked at, and whose rest is `fresh`'s hex digits — a uuid of version 7.
/// The ids of scenes asked one after the other sort in that order.
#[must_use]
pub fn scene_id(millis: u64, fresh: &str) -> String {
    let random: Vec<char> = fresh
        .chars()
        .filter(char::is_ascii_hexdigit)
        .map(|c| c.to_ascii_lowercase())
        .chain(std::iter::repeat('0'))
        .take(18)
        .collect();
    let random: String = random.into_iter().collect();
    let time = format!("{:012x}", millis & 0xffff_ffff_ffff);
    format!(
        "{}-{}-7{}-8{}-{}",
        &time[..8],
        &time[8..],
        &random[..3],
        &random[3..6],
        &random[6..]
    )
}

/// What `wire` asks.
///
/// # Errors
///
/// [`Refusal`] when `wire` carries another field, names its session by
/// something other than a uuid ([`Refusal::UnknownSession`]), has no title,
/// notes that are no text, or places that are no list of uuids.
pub fn asked(wire: &Row) -> Result<Asked, Refusal> {
    if let Some(field) = wire.keys().find(|field| !FIELDS.contains(&field.as_str())) {
        return Err(Refusal::Unknown(field.clone()));
    }
    let session = wire
        .get(SESSION)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|id| uuid(id))
        .ok_or(Refusal::UnknownSession)?
        .to_ascii_lowercase();
    let title = wire
        .get(TITLE)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|title| !title.is_empty())
        .ok_or(Refusal::NoTitle)?
        .to_owned();
    let notes = match wire.get(NOTES) {
        None | Some(Value::Null) => None,
        Some(Value::String(notes)) => Some(notes.clone()),
        Some(_) => return Err(Refusal::InvalidNotes),
    };
    let mut places: Vec<String> = Vec::new();
    match wire.get(PLACES) {
        None | Some(Value::Null) => {}
        Some(Value::Array(ids)) => {
            for id in ids {
                let id = id
                    .as_str()
                    .map(str::trim)
                    .filter(|id| uuid(id))
                    .ok_or(Refusal::InvalidPlaces)?
                    .to_ascii_lowercase();
                if !places.contains(&id) {
                    places.push(id);
                }
            }
        }
        Some(_) => return Err(Refusal::InvalidPlaces),
    }
    Ok(Asked {
        session,
        title,
        notes,
        places,
    })
}

/// `asked` judged against its session's row — empty when the store holds
/// no such session — and each of its places' rows, in order — empty when it
/// holds no such element: the row the scene `scene` writes.
///
/// # Errors
///
/// [`Refusal::UnknownSession`] when there is no session,
/// [`Refusal::OtherCampaign`] when a place is no element of its campaign —
/// or `places` does not answer each of them — and [`Refusal::NotAPlace`]
/// when one is an element of it that is no « lieu ».
pub fn prepare(asked: &Asked, scene: &str, session: &Row, places: &[Row]) -> Result<Row, Refusal> {
    let text = |row: &Row, field: &str| {
        row.get(field)
            .and_then(Value::as_str)
            .map(str::to_ascii_lowercase)
    };
    let ours = text(session, SESSION_CAMPAIGN_ID).ok_or(Refusal::UnknownSession)?;
    if places.len() != asked.places.len()
        || places
            .iter()
            .any(|place| text(place, ENTITY_CAMPAIGN_ID).as_ref() != Some(&ours))
    {
        return Err(Refusal::OtherCampaign);
    }
    if places
        .iter()
        .any(|place| text(place, TYPE).as_deref() != Some(LIEU))
    {
        return Err(Refusal::NotAPlace);
    }
    let mut row = Row::from([
        (SCENE_ID.to_owned(), Value::from(scene)),
        (
            SCENE_SESSION_ID.to_owned(),
            Value::from(asked.session.clone()),
        ),
        (SCENE_TITLE.to_owned(), Value::from(asked.title.clone())),
        (
            NOTES.to_owned(),
            asked.notes.clone().map_or(Value::Null, Value::from),
        ),
    ]);
    if !asked.places.is_empty() {
        row.insert(LINKS.to_owned(), Value::from(asked.places.clone()));
    }
    Ok(row)
}
