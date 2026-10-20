//! DataCapability `session.addPlaceToSession` (`data-capability.json`): the
//! DM puts a place of the campaign in a scene of the session they prepare,
//! from the Médias page or from the session's preparation. Every medium of
//! that place is then the session's too, through `session_media_view`: the
//! association is never stored, so it follows every later change of place
//! (`session-media-by-place`). [`asked`] reads the wire's payload —
//! `{session, scene?, place}` — and [`prepare`] turns it into the command:
//!
//! - one link is one row of the Session aggregate, on a partition of its
//!   own, [`link_id`]: the session, the scene and the place, lowercase. The
//!   same place asked again in the same scene is the same row, so a single
//!   link remains;
//! - the scene is the one the payload names, else the session's first: the
//!   server finds it ([`Scene`]). A session with no scene takes no place
//!   ([`Refusal::NoScene`]);
//! - the place is ranked after the scene's last element, or keeps its rank
//!   when the scene links it already;
//! - that the scene is the session's, and the place an element of its
//!   campaign, is the DataGuard's to judge (`scene-links-same-campaign`):
//!   the command carries the link, never the verdict;
//! - a payload naming no session or no place is refused before anything is
//!   queued ([`Refusal`]).

use std::collections::BTreeMap;

use serde_json::Value;

/// This DataCapability's id, as `data-capability.json` names it.
pub const DATA_CAPABILITY: &str = "session.addPlaceToSession";

/// A link's scene (`scene_entity.scene_id`).
pub const SCENE: &str = "sceneEntitySceneId";

/// A link's element, here the place (`scene_entity.entity_id`).
pub const PLACE: &str = "entityId";

/// A link's rank in its scene (`scene_entity.position`).
pub const POSITION: &str = "sceneEntityPosition";

/// The type of the elements this command links: a place.
pub const LIEU: &str = "lieu";

/// What separates the session, the scene and the place in [`link_id`].
const SEPARATOR: char = ':';

/// One partition's row, as the engine holds it.
pub type Row = BTreeMap<String, Value>;

/// What the wire asks: lowercase ids, the scene when it names one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Asked {
    /// The session the place joins.
    pub session: String,
    /// The scene of that session, when the payload names one.
    pub scene: Option<String>,
    /// The place.
    pub place: String,
}

/// The scene the place joins, as the server finds it: its id, and the
/// elements it links already with their rank.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Scene {
    /// The scene's id.
    pub id: String,
    /// Each element it links, by id, and its rank.
    pub links: BTreeMap<String, u64>,
}

/// Why a link is refused before it is queued.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// The payload names no session.
    NoSession,
    /// The payload names no place.
    NoPlace,
    /// The payload names no scene, and the session has none.
    NoScene,
    /// The element named is not a place.
    NotAPlace,
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::NoSession => "un lieu rejoint une séance",
            Self::NoPlace => "la séance accueille un lieu",
            Self::NoScene => "la séance n’a pas encore de scène",
            Self::NotAPlace => "seul un lieu rejoint une scène ainsi",
        })
    }
}

/// What a link becomes before it is submitted: what the command writes on
/// `Session/<target_id>`.
#[derive(Clone, Debug, PartialEq)]
pub struct Prepared {
    /// The link's fields: its scene, its place, its rank.
    pub payload: Row,
    /// The link's id ([`link_id`]): the command's target.
    pub target_id: String,
}

/// Whether `id` names this DataCapability, with or without its `@version`.
#[must_use]
pub fn names(id: &str) -> bool {
    id.split('@').next() == Some(DATA_CAPABILITY)
}

/// A uuid's text, or none: trimmed, lowercase, never blank.
fn id(wire: &Row, field: &str) -> Option<String> {
    wire.get(field)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .map(str::to_ascii_lowercase)
}

/// What the wire's payload asks.
///
/// # Errors
///
/// [`Refusal::NoSession`] or [`Refusal::NoPlace`] when it names no session or
/// no place.
pub fn asked(wire: &Row) -> Result<Asked, Refusal> {
    Ok(Asked {
        session: id(wire, "session").ok_or(Refusal::NoSession)?,
        scene: id(wire, "scene"),
        place: id(wire, "place").ok_or(Refusal::NoPlace)?,
    })
}

/// The id of the link of `place` to `scene` asked for `session`: the
/// partition `Session/<link id>` its command writes.
#[must_use]
pub fn link_id(session: &str, scene: &str, place: &str) -> String {
    [session, scene, place]
        .map(str::to_ascii_lowercase)
        .join(&SEPARATOR.to_string())
}

/// The session, the scene and the place a [`link_id`] names.
#[must_use]
pub fn link_of(id: &str) -> Option<(&str, &str, &str)> {
    let mut parts = id.split(SEPARATOR);
    match (parts.next(), parts.next(), parts.next(), parts.next()) {
        (Some(session), Some(scene), Some(place), None)
            if [session, scene, place].iter().all(|part| !part.is_empty()) =>
        {
            Some((session, scene, place))
        }
        _ => None,
    }
}

/// The link `asked` makes in `scene`, the one the server found, `None` when
/// the session has none; `kind` is the element's type, when it is known.
///
/// # Errors
///
/// [`Refusal::NoScene`] when there is no scene to join, [`Refusal::NotAPlace`]
/// when the element is known and is no place.
pub fn prepare(
    asked: &Asked,
    scene: Option<&Scene>,
    kind: Option<&str>,
) -> Result<Prepared, Refusal> {
    let scene = scene.ok_or(Refusal::NoScene)?;
    if kind.is_some_and(|kind| kind != LIEU) {
        return Err(Refusal::NotAPlace);
    }
    let position = scene
        .links
        .get(&asked.place)
        .copied()
        .unwrap_or_else(|| scene.links.values().max().map_or(1, |last| last + 1));
    let scene_id = scene.id.to_ascii_lowercase();
    Ok(Prepared {
        target_id: link_id(&asked.session, &scene_id, &asked.place),
        payload: Row::from([
            (SCENE.into(), scene_id.into()),
            (PLACE.into(), asked.place.clone().into()),
            (POSITION.into(), position.into()),
        ]),
    })
}
