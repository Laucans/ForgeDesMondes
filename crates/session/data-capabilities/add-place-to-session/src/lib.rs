//! DataCapability `session.addPlaceToSession` (`data-capability.json`): the
//! DM puts a place of the campaign in a scene of the session they are
//! preparing, from the Médias page or from the session's preparation. Every
//! medium of that place is then the session's too, through
//! `session_media_view`: the association is never stored, so it follows
//! every later change of place (`session-media-by-place`).
//!
//! A scene is a partition of its own, `Session/<its id>`, whose row lists
//! the elements it links, in order (`crates/session/aggregates/Session`).
//! Linking a place is writing that row again with the place after its last
//! element. What the screen sends is `{ "session", "scene"?, "place" }`,
//! and it becomes the scene's row in two steps:
//!
//! - [`asked`] reads the request: the session, the scene when it names one,
//!   and the place are uuids, lowercase as the tables write them; one that
//!   is no uuid is malformed ([`Refusal::InvalidSession`],
//!   [`Refusal::InvalidScene`], [`Refusal::InvalidPlace`]). Any other field
//!   is refused ([`Refusal::Unknown`]).
//! - [`prepare`] judges it against the session's row, the scene's — the one
//!   named, else the session's first ([`Refusal::NoScene`] when it has
//!   none) — and the place's, as the store holds them: the scene must be
//!   the session's ([`Refusal::OtherSession`]), and the place a « lieu » of
//!   its campaign ([`Refusal::OtherCampaign`], [`Refusal::NotAPlace`]) —
//!   `scene-links-same-campaign`. Nothing is queued.
//!
//! The row written is the scene's as it is, its title and its notes kept,
//! the place appended to its links — so the aggregate ranks it after the
//! scene's last element. A place the scene links already keeps its rank:
//! a single link remains.
//!
//! The rows are read under the aggregates' fields — the session's
//! `sessionCampaignId`, a scene's `sceneId`, `sceneSessionId`, `sceneTitle`,
//! `notes` and `entityId` (Session), a place's `type` and
//! `entityCampaignId` (Entity) — the tables' first, the engine's laid over
//! them.

use std::collections::BTreeMap;

use aggregate_session::{
    LINKS, NOTES, PARTITION, SAME_CAMPAIGN, SCENE_ID, SCENE_SESSION_ID, SCENE_TITLE, uuid,
};
use async_trait::async_trait;
use harness_dataguard::plugins::{self, Context, Plugin, Prepare};
use serde_json::Value;

/// This DataCapability's id, as `data-capability.json` names it.
pub const DATA_CAPABILITY: &str = "session.addPlaceToSession";

/// The payload's fields.
const SESSION: &str = "session";
const SCENE: &str = "scene";
const PLACE: &str = "place";
const FIELDS: [&str; 3] = [SESSION, SCENE, PLACE];

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

/// A scene as the tables hold it: its id, its session, its title, its notes
/// and the elements it links, by rank. A scene's tombstone takes it out of
/// `scene`: the tables hold no scene that is gone.
macro_rules! scene_row {
    () => {
        "SELECT c.id::text AS \"sceneId\", c.session_id::text AS \"sceneSessionId\",
            c.title AS \"sceneTitle\", c.notes AS \"notes\",
            coalesce((SELECT jsonb_agg(l.entity_id::text ORDER BY l.position, l.entity_id)
                FROM scene_entity l WHERE l.scene_id = c.id), '[]'::jsonb) AS \"entityId\"
        FROM scene c "
    };
}

/// The scene `$1` names.
const SCENE_ROW: &str = concat!(scene_row!(), "WHERE c.id = $1::uuid");

/// The first scene of the session `$1` names: the one ranked first.
const FIRST_SCENE: &str = concat!(
    scene_row!(),
    "WHERE c.session_id = $1::uuid ORDER BY c.position, c.id LIMIT 1"
);

/// The element `$1` names, as the tables hold it live: its type and its
/// campaign.
const ELEMENT_ROW: &str = "SELECT e.type AS \"type\", e.campaign_id::text AS \"entityCampaignId\"
    FROM entity e
    WHERE e.id = $1::uuid
        AND NOT EXISTS (SELECT 1 FROM dataguard_rows r
            WHERE r.partition = 'Entity/' || e.id::text AND r.tombstoned)";

/// One partition's row, as the engine holds it.
pub type Row = BTreeMap<String, Value>;

/// What a request asks: `place` in a scene of `session`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Asked {
    /// The session's id, lowercase.
    pub session: String,
    /// The scene's id, lowercase; `None` for the session's first.
    pub scene: Option<String>,
    /// The place's id, lowercase.
    pub place: String,
}

/// Why a place is refused before it is queued.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// The payload carries a field the command does not take.
    Unknown(String),
    /// The session is no uuid.
    InvalidSession,
    /// The scene is named, by no uuid.
    InvalidScene,
    /// The store holds no such session.
    UnknownSession,
    /// The store holds no such scene.
    UnknownScene,
    /// No scene is named, and the session has none.
    NoScene,
    /// The place is no uuid.
    InvalidPlace,
    /// The scene is another session's.
    OtherSession,
    /// The place is no element of the session's campaign.
    OtherCampaign,
    /// An element of the session's campaign is no « lieu ».
    NotAPlace,
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unknown(field) => write!(
                f,
                "{field} : un lieu rejoint une scène par session, scene et place"
            ),
            Self::InvalidSession => write!(f, "session : l'identifiant d'une séance"),
            Self::InvalidScene => write!(f, "scene : l'identifiant d'une scène"),
            Self::UnknownSession => write!(f, "séance introuvable"),
            Self::UnknownScene => write!(f, "scène introuvable"),
            Self::NoScene => write!(f, "la séance n'a pas encore de scène"),
            Self::InvalidPlace => write!(f, "place : l'identifiant d'un lieu"),
            Self::OtherSession => {
                write!(f, "{SAME_CAMPAIGN} : la scène est celle d'une autre séance")
            }
            Self::OtherCampaign => write!(
                f,
                "{SAME_CAMPAIGN} : une scène ne lie que des éléments de la campagne de sa séance"
            ),
            Self::NotAPlace => write!(f, "place : seul un lieu de la campagne"),
        }
    }
}

/// What the DataCapability plugs into the engine: its `Prepare`.
#[must_use]
pub fn plugin() -> Plugin {
    Plugin::new().prepare(DATA_CAPABILITY, Places)
}

/// A place made the row of the scene it joins, judged against its session,
/// that scene and the place ([`session`], [`scene`], [`element`]): one of a
/// session or a scene there is not is refused, 404; one malformed, or
/// linking anything but a place of the session's campaign to one of its
/// scenes (`scene-links-same-campaign`), 422 — neither is queued.
struct Places;

#[async_trait]
impl Prepare for Places {
    async fn prepare(
        &self,
        context: &Context<'_>,
        payload: &BTreeMap<String, Value>,
        _target_id: Option<&str>,
    ) -> Result<plugins::Prepared, plugins::Refusal> {
        let asked = asked(payload).map_err(refused)?;
        let session = session(context, &asked.session).await?;
        let scene = scene(context, &asked).await?;
        let place = element(context, &asked.place).await?;
        let payload = prepare(&asked, &session, &scene, &place).map_err(refused)?;
        // The row is the payload's `sceneId` (`target.id`).
        Ok(plugins::Prepared {
            payload,
            ..plugins::Prepared::default()
        })
    }
}

/// A refusal as the server answers it: a session or a scene there is not
/// is not found, anything else unprocessable.
fn refused(refusal: Refusal) -> plugins::Refusal {
    match refusal {
        Refusal::UnknownSession | Refusal::UnknownScene | Refusal::NoScene => {
            plugins::Refusal::not_found(refusal.to_string())
        }
        _ => plugins::Refusal::unprocessable(refusal.to_string()),
    }
}

/// The first row the tables' read answers, empty when none; a failed read
/// is unprocessable.
async fn read(context: &Context<'_>, sql: &str, parameter: &str) -> Result<Row, plugins::Refusal> {
    let Some(rows) = context.rows else {
        return Ok(Row::new());
    };
    let found = rows
        .rows(sql, &[Value::from(parameter)])
        .await
        .map_err(|e| plugins::Refusal::unprocessable(e.to_string()))?;
    Ok(found
        .as_array()
        .into_iter()
        .flatten()
        .find_map(|row| match row {
            Value::Object(row) => Some(row.clone().into_iter().collect()),
            _ => None,
        })
        .unwrap_or_default())
}

/// The session `id` names as a place is judged against it: over
/// PostgreSQL, as the tables hold it live — the fixtures' among them — and,
/// laid over it, the engine's row as the commands queued on it will leave
/// it. Empty when neither holds it.
async fn session(context: &Context<'_>, id: &str) -> Result<Row, plugins::Refusal> {
    let mut row = read(context, SESSION_ROW, id).await?;
    row.extend(context.row(&format!("{PARTITION}{id}")).await?);
    Ok(row)
}

/// The scene `asked` names — else the first the tables rank in its session
/// — the same way. Empty when none is found.
async fn scene(context: &Context<'_>, asked: &Asked) -> Result<Row, plugins::Refusal> {
    let mut row = match &asked.scene {
        Some(id) => read(context, SCENE_ROW, id).await?,
        None => read(context, FIRST_SCENE, &asked.session).await?,
    };
    let id = asked.scene.clone().or_else(|| text(&row, SCENE_ID));
    if let Some(id) = id {
        row.extend(context.row(&format!("{PARTITION}{id}")).await?);
    }
    Ok(row)
}

/// The element `id` names, the same way.
async fn element(context: &Context<'_>, id: &str) -> Result<Row, plugins::Refusal> {
    let mut row = read(context, ELEMENT_ROW, id).await?;
    row.extend(context.row(&format!("{ENTITY_PARTITION}{id}")).await?);
    Ok(row)
}

/// `field`'s text in `row`, lowercase.
fn text(row: &Row, field: &str) -> Option<String> {
    row.get(field)
        .and_then(Value::as_str)
        .map(str::to_ascii_lowercase)
}

/// The uuid `value` holds, trimmed and lowercase.
fn id(value: Option<&Value>) -> Option<String> {
    value
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|id| uuid(id))
        .map(str::to_ascii_lowercase)
}

/// What `wire` asks.
///
/// # Errors
///
/// [`Refusal`] when `wire` carries another field, or names its session, its
/// scene or its place by something other than a uuid
/// ([`Refusal::InvalidSession`], [`Refusal::InvalidScene`],
/// [`Refusal::InvalidPlace`]).
pub fn asked(wire: &Row) -> Result<Asked, Refusal> {
    if let Some(field) = wire.keys().find(|field| !FIELDS.contains(&field.as_str())) {
        return Err(Refusal::Unknown(field.clone()));
    }
    let session = id(wire.get(SESSION)).ok_or(Refusal::InvalidSession)?;
    let scene = match wire.get(SCENE) {
        None | Some(Value::Null) => None,
        named => Some(id(named).ok_or(Refusal::InvalidScene)?),
    };
    let place = id(wire.get(PLACE)).ok_or(Refusal::InvalidPlace)?;
    Ok(Asked {
        session,
        scene,
        place,
    })
}

/// `asked` judged against its session's row — empty when the store holds
/// no such session —, the scene's — empty when it holds none — and the
/// place's — empty when it holds no such element: the row the scene
/// writes, the place after its last element.
///
/// # Errors
///
/// [`Refusal::UnknownSession`] when there is no session,
/// [`Refusal::UnknownScene`] or [`Refusal::NoScene`] when there is no scene,
/// [`Refusal::OtherSession`] when it is another session's,
/// [`Refusal::OtherCampaign`] when the place is no element of the session's
/// campaign, and [`Refusal::NotAPlace`] when it is an element of it that is
/// no « lieu ».
pub fn prepare(asked: &Asked, session: &Row, scene: &Row, place: &Row) -> Result<Row, Refusal> {
    let ours = text(session, SESSION_CAMPAIGN_ID).ok_or(Refusal::UnknownSession)?;
    let Some(scene_id) = text(scene, SCENE_ID) else {
        return Err(if asked.scene.is_some() {
            Refusal::UnknownScene
        } else {
            Refusal::NoScene
        });
    };
    if text(scene, SCENE_SESSION_ID).as_ref() != Some(&asked.session) {
        return Err(Refusal::OtherSession);
    }
    if text(place, ENTITY_CAMPAIGN_ID).as_ref() != Some(&ours) {
        return Err(Refusal::OtherCampaign);
    }
    if text(place, TYPE).as_deref() != Some(LIEU) {
        return Err(Refusal::NotAPlace);
    }
    let mut links: Vec<String> = scene
        .get(LINKS)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_ascii_lowercase)
        .collect();
    if !links.contains(&asked.place) {
        links.push(asked.place.clone());
    }
    let kept = |field: &str| scene.get(field).cloned().unwrap_or(Value::Null);
    Ok(Row::from([
        (SCENE_ID.to_owned(), Value::from(scene_id)),
        (
            SCENE_SESSION_ID.to_owned(),
            Value::from(asked.session.clone()),
        ),
        (SCENE_TITLE.to_owned(), kept(SCENE_TITLE)),
        (NOTES.to_owned(), kept(NOTES)),
        (LINKS.to_owned(), Value::from(links)),
    ]))
}
