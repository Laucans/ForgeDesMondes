//! DataCapability `session.addScene` (`data-capability.json`): the DM adds
//! a scene to the session they are preparing, with its title, its notes and
//! the places it links.
//!
//! A scene is one command on a partition of its own, `Session/<a fresh
//! id>`. What the screen sends is `{ "session", "title", "notes"?,
//! "places"? }`, and it becomes the aggregate's fields in two steps:
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
//!   ([`Refusal::OtherCampaign`]), and nothing is queued.
//!
//! The scene's rank is not chosen here: the Session aggregate settles it
//! when the scene is applied, after the session's last, so scenes queued
//! side by side each take the next one (`scene-order-contiguous`), and it
//! judges the places again then. The media of a place it links are the
//! session's through `session_media_view`, never written
//! (`session-media-by-place`).
//!
//! The rows are read under the aggregates' fields: the session's
//! `sessionCampaignId` (Session), a place's `entityCampaignId` (Entity) —
//! the tables' first, the engine's laid over them.

use std::collections::BTreeMap;

use aggregate_session::{LINKS, NOTES, SAME_CAMPAIGN, SCENE_ID, SCENE_SESSION_ID, SCENE_TITLE};
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

/// The session's campaign, and an element's, under the aggregates' fields.
pub const SESSION_CAMPAIGN_ID: &str = "sessionCampaignId";
pub const ENTITY_CAMPAIGN_ID: &str = "entityCampaignId";

/// The partitions of the Session aggregate, and of the Entity aggregate,
/// whose elements a place is one of.
const SESSION_PARTITION: &str = "Session/";
const ENTITY_PARTITION: &str = "Entity/";

/// A session as the tables hold it live: its campaign.
const SESSION_ROW: &str = "SELECT s.campaign_id::text AS \"sessionCampaignId\"
    FROM session s
    WHERE s.id::text = $1
        AND NOT EXISTS (SELECT 1 FROM dataguard_rows r
            WHERE r.partition = 'Session/' || s.id::text AND r.tombstoned)";

/// An element as the tables hold it live: its campaign.
const ELEMENT_ROW: &str = "SELECT e.campaign_id::text AS \"entityCampaignId\"
    FROM entity e
    WHERE e.id::text = $1
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
        }
    }
}

/// What the DataCapability plugs into the engine: its `Prepare`.
#[must_use]
pub fn plugin() -> Plugin {
    Plugin::new().prepare(DATA_CAPABILITY, Scenes)
}

/// A scene made the row it writes on a fresh partition, judged against its
/// session and its places ([`rows`]): one of a session there is not is
/// refused, 404; one malformed, or linking a place of another campaign
/// (`scene-links-same-campaign`), 422 — neither is queued.
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
        let session = row(context, SESSION_PARTITION, SESSION_ROW, &asked.session).await?;
        let mut places = Vec::with_capacity(asked.places.len());
        for place in &asked.places {
            places.push(row(context, ENTITY_PARTITION, ELEMENT_ROW, place).await?);
        }
        let scene = context.ids.fresh().to_ascii_lowercase();
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

/// The row `id` names in `partition` as a scene is judged against it: over
/// PostgreSQL, as the tables hold it live (`sql`) — the fixtures' among
/// them — and, laid over it, the engine's row as the commands queued on it
/// will leave it. Empty when neither holds it.
async fn row(
    context: &Context<'_>,
    partition: &str,
    sql: &str,
    id: &str,
) -> Result<Row, plugins::Refusal> {
    let mut row = match context.rows {
        Some(rows) => {
            let found = rows
                .rows(sql, &[Value::from(id)])
                .await
                .map_err(|e| plugins::Refusal::unprocessable(e.to_string()))?;
            match found.as_array().and_then(|found| found.first()) {
                Some(Value::Object(row)) => row.clone().into_iter().collect(),
                _ => Row::new(),
            }
        }
        None => Row::new(),
    };
    row.extend(context.row(&format!("{partition}{id}")).await?);
    Ok(row)
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
/// [`Refusal::OtherCampaign`] when a place is no element of its campaign.
pub fn prepare(asked: &Asked, scene: &str, session: &Row, places: &[Row]) -> Result<Row, Refusal> {
    let campaign = |row: &Row, field: &str| {
        row.get(field)
            .and_then(Value::as_str)
            .map(str::to_ascii_lowercase)
    };
    let ours = campaign(session, SESSION_CAMPAIGN_ID).ok_or(Refusal::UnknownSession)?;
    if places.len() != asked.places.len()
        || places
            .iter()
            .any(|place| campaign(place, ENTITY_CAMPAIGN_ID).as_ref() != Some(&ours))
    {
        return Err(Refusal::OtherCampaign);
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

/// A uuid written 8-4-4-4-12 in hex digits, in either case: what
/// `session.id` and `entity.id` hold.
fn uuid(id: &str) -> bool {
    id.len() == 36
        && id.char_indices().all(|(at, c)| match at {
            8 | 13 | 18 | 23 => c == '-',
            _ => c.is_ascii_hexdigit(),
        })
}
