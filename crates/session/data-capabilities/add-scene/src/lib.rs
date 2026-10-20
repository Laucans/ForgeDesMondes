//! DataCapability `session.addScene` (`data-capability.json`): the DM adds a
//! scene to the session they are preparing, with its title, its notes and
//! the places it links.
//!
//! The engine writes one partition per command: a new scene is one command
//! on `Session/<its id>`, carrying the scene and the places it links, which
//! the DataGuard's invariants judge and its store projects into `scene` and
//! `scene_entity`. What the DM sends is the wire payload `{session, title,
//! notes?, places?}`; [`prepare`] turns it into that state:
//!
//! - the scene takes the position it is given — the one after the session's
//!   last scene, which the server reads from what it holds, and settles
//!   again when the scene is applied, so that the ranks stay 1 to n
//!   (`scene-order-contiguous`);
//! - the places are kept in the order given, a duplicate once, as one list
//!   of ids: their order is their `scene_entity` position. That each is an
//!   element of the session's campaign (`scene-links-same-campaign`) is the
//!   DataGuard's to judge; the media of those places follow the scene
//!   through `session_media_view`, never stored (`session-media-by-place`);
//! - a scene the tables could never hold is refused before it is queued
//!   ([`Refusal`]): it names its session by a uuid, it has a title, its notes
//!   are a text, its places a list of ids, and no value holds the character
//!   U+0000. That the session is one the product holds is the server's to
//!   say ([`Refusal::UnknownSession`]), from what it holds.

use std::collections::BTreeMap;

use serde_json::Value;

/// This DataCapability's id, as `data-capability.json` names it.
pub const DATA_CAPABILITY: &str = "session.addScene";

/// The row's field naming the session a scene is of.
pub const SESSION: &str = "sceneSessionId";

/// The row's field holding a scene's rank in its session.
pub const POSITION: &str = "scenePosition";

/// The row's field holding the elements a scene links, an array of their
/// ids in their order.
pub const LINKED: &str = "entityId";

/// One partition's row, as the engine holds it.
pub type Row = BTreeMap<String, Value>;

/// What a new scene is given besides its wire payload: its id and its
/// position in the session.
#[derive(Clone, Copy, Debug)]
pub struct Fresh<'a> {
    /// The scene's id, which the server made.
    pub scene: &'a str,
    /// Its position: the one after the session's last scene.
    pub position: u64,
}

/// Why a new scene is refused before it is queued: what the tables could
/// never hold, which no invariant of the aggregate names.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// No session is named, or not by a uuid.
    NoSession,
    /// The session named is none the product holds.
    UnknownSession,
    /// No title, or one of blanks alone.
    NoTitle,
    /// The notes are not a text.
    NotNotes,
    /// The places are not a list of ids.
    NotPlaces,
    /// A value holds the character U+0000, which `text` cannot hold.
    Nul,
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::NoSession => "une scène nomme sa séance par son identifiant",
            Self::UnknownSession => "la séance de cette scène n'existe pas",
            Self::NoTitle => "une scène a un titre",
            Self::NotNotes => "les notes d'une scène sont un texte",
            Self::NotPlaces => "les lieux d'une scène sont une liste d'identifiants",
            Self::Nul => "une scène ne contient pas de caractère nul",
        })
    }
}

/// What a new scene becomes before it is submitted.
#[derive(Clone, Debug, PartialEq)]
pub struct Prepared {
    /// The scene's id: the partition `Session/<target_id>`.
    pub target_id: String,
    /// What the command writes on it.
    pub payload: Row,
}

/// Whether `id` names this DataCapability, with or without its `@version`.
#[must_use]
pub fn names(id: &str) -> bool {
    id.split('@').next() == Some(DATA_CAPABILITY)
}

/// The session a new scene names, when it names one by a uuid.
#[must_use]
pub fn session_of(wire: &Row) -> Option<String> {
    wire.get("session")
        .and_then(Value::as_str)
        .filter(|id| is_uuid(id))
        .map(str::to_lowercase)
}

/// The places a new scene names, in the order given, each once: an id given
/// again in another case is the same one. An id that is no uuid is kept,
/// for the DataGuard to judge: it is no element of the campaign.
///
/// # Errors
///
/// [`Refusal::NotPlaces`] when they are not a list of strings.
pub fn places_of(wire: &Row) -> Result<Vec<String>, Refusal> {
    let Some(given) = wire.get("places").filter(|v| !v.is_null()) else {
        return Ok(Vec::new());
    };
    let ids = given.as_array().ok_or(Refusal::NotPlaces)?;
    let mut kept: Vec<String> = Vec::new();
    for id in ids {
        let id = id.as_str().ok_or(Refusal::NotPlaces)?;
        let id = if is_uuid(id) {
            id.to_lowercase()
        } else {
            id.to_owned()
        };
        if !kept.contains(&id) {
            kept.push(id);
        }
    }
    Ok(kept)
}

/// Whether `text` is a uuid as PostgreSQL writes one: 8-4-4-4-12 hex digits.
fn is_uuid(text: &str) -> bool {
    text.len() == 36
        && text.char_indices().all(|(i, c)| match i {
            8 | 13 | 18 | 23 => c == '-',
            _ => c.is_ascii_hexdigit(),
        })
}

/// The command a new scene of `wire` submits on the partition
/// `Session/<fresh.scene>`.
///
/// # Errors
///
/// [`Refusal`] for a scene the tables could never hold.
pub fn prepare(wire: &Row, fresh: Fresh<'_>) -> Result<Prepared, Refusal> {
    let session = session_of(wire).ok_or(Refusal::NoSession)?;
    let title = wire
        .get("title")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|title| !title.is_empty())
        .ok_or(Refusal::NoTitle)?;
    let notes = match wire.get("notes") {
        None | Some(Value::Null) => None,
        Some(Value::String(notes)) if notes.trim().is_empty() => None,
        Some(Value::String(notes)) => Some(notes.as_str()),
        Some(_) => return Err(Refusal::NotNotes),
    };
    let places = places_of(wire)?;
    let nul = |text: &str| text.contains('\0');
    if nul(title) || notes.is_some_and(nul) || places.iter().any(|id| nul(id)) {
        return Err(Refusal::Nul);
    }
    let id = fresh.scene.to_owned();
    let mut payload = Row::new();
    for (field, value) in [
        ("sceneId", Value::from(id.clone())),
        (SESSION, session.into()),
        (POSITION, fresh.position.into()),
        ("sceneTitle", title.into()),
        ("notes", notes.map_or(Value::Null, Value::from)),
    ] {
        payload.insert(field.into(), value);
    }
    if !places.is_empty() {
        payload.insert("sceneEntitySceneId".into(), id.clone().into());
        payload.insert(
            LINKED.into(),
            Value::Array(places.into_iter().map(Value::from).collect()),
        );
    }
    Ok(Prepared {
        target_id: id,
        payload,
    })
}
