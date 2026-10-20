//! DataCapability `soundbox.playSound` (`data-capability.json`): the DM
//! triggers a sound — from the soundbox, or from a suggestion's pastille —
//! and a playback of it starts on the DM view, layered over whatever plays
//! already.
//!
//! The engine writes one partition per command and judges a row alone: a
//! playback is its own row, `SoundPlayback/<its id>`, which the DataGuard's
//! invariants judge and its store projects into `sound_playback`. The sound
//! is the command's target; what the engine cannot see from one row — the
//! sound's campaign, the session it plays in — is read before the playback
//! is queued ([`Stage`]); [`prepare`] turns the request and that stage into
//! the playback's row:
//!
//! - a new row, with no `stoppedAt`: it plays, and stops none of the others
//!   (`simultaneous-playback`);
//! - its origin is the DM view's surface it was triggered from: `soundbox`
//!   by default, `pastille` for a suggestion's; any other source, the player
//!   view's, is carried as it is, for the DataGuard to refuse
//!   (`dm-side-only`);
//! - its session is the one the payload names, of the sound's campaign —
//!   else the campaign's live session, else its latest one not closed,
//!   else its latest ([`Stage`]);
//! - what the tables could never hold, or a sound with no session to play
//!   in, is refused before anything is queued ([`Refusal`]).

use std::collections::BTreeMap;

use serde_json::{Value, json};

/// This DataCapability's id, as `data-capability.json` names it.
pub const DATA_CAPABILITY: &str = "soundbox.playSound";

/// The origin of a sound triggered from the soundbox: the default.
const SOUNDBOX: &str = "soundbox";

/// The source a suggestion's pastille sends.
const SUGGESTION: &str = "suggestion";

/// The origin the table and the DataGuard know the pastille's by.
const PASTILLE: &str = "pastille";

/// One partition's row, as the engine holds it.
pub type Row = BTreeMap<String, Value>;

/// Whether `id` names this DataCapability, at any version.
pub fn names(id: &str) -> bool {
    id.split('@').next() == Some(DATA_CAPABILITY)
}

/// What a trigger asks for: the sound and the session by their ids,
/// lowercase, and the origin the playback carries.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Request {
    /// The sound played.
    pub sound: String,
    /// The session the payload names; `None` for the campaign's own.
    pub session: Option<String>,
    /// Its origin: `soundbox`, `pastille`, or the source as it was sent.
    pub origin: Value,
}

/// What the tables hold of the sound a trigger names.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Stage {
    /// The sound's campaign; `None` when there is no such sound.
    pub campaign: Option<String>,
    /// The session the campaign plays in: its live one, else its latest
    /// not closed, else its latest; `None` when it has none.
    pub current: Option<String>,
    /// The campaign of the session the payload names; `None` when it names
    /// none, or one there is not.
    pub named: Option<String>,
}

/// The ids and the instant a new playback is given.
#[derive(Clone, Copy, Debug)]
pub struct Fresh<'a> {
    /// The playback's id, which its partition is named after.
    pub id: &'a str,
    /// When it starts, as an RFC 3339 instant.
    pub now: &'a str,
}

/// Why a trigger is refused before it is queued.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// A field that must be a uuid is not one.
    NotAnId(&'static str),
    /// The tables hold no such sound.
    UnknownSound,
    /// The tables hold no such session.
    UnknownSession,
    /// The session named is of another campaign than the sound's.
    SessionOfAnotherCampaign,
    /// The sound's campaign has no session to play it in.
    NoSession,
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotAnId(field) => write!(f, "{field} n'est pas un identifiant"),
            Self::UnknownSound => f.write_str("ce son n'existe pas"),
            Self::UnknownSession => f.write_str("cette séance n'existe pas"),
            Self::SessionOfAnotherCampaign => {
                f.write_str("cette séance n'est pas de la campagne du son")
            }
            Self::NoSession => f.write_str("la campagne de ce son n'a aucune séance"),
        }
    }
}

/// The trigger `wire` asks for on `target`, the sound the command names.
///
/// # Errors
///
/// [`Refusal::NotAnId`] for a sound, or a session named, that is no uuid.
pub fn request(wire: &Row, target: Option<&str>) -> Result<Request, Refusal> {
    let sound = target.and_then(uuid).ok_or(Refusal::NotAnId("sound"))?;
    let session = match wire.get("session") {
        None | Some(Value::Null) => None,
        Some(session) => Some(
            session
                .as_str()
                .and_then(uuid)
                .ok_or(Refusal::NotAnId("session"))?,
        ),
    };
    let origin = match wire.get("source") {
        None | Some(Value::Null) => json!(SOUNDBOX),
        Some(Value::String(source)) if source == SUGGESTION => json!(PASTILLE),
        Some(source) => source.clone(),
    };
    Ok(Request {
        sound,
        session,
        origin,
    })
}

/// The playback row `request` starts, against what `stage` holds, under the
/// id and at the instant `fresh` gives.
///
/// # Errors
///
/// [`Refusal::UnknownSound`]; for a session named, [`Refusal::UnknownSession`]
/// and [`Refusal::SessionOfAnotherCampaign`]; with none named,
/// [`Refusal::NoSession`].
pub fn prepare(request: &Request, stage: &Stage, fresh: Fresh<'_>) -> Result<Row, Refusal> {
    let campaign = stage.campaign.as_deref().ok_or(Refusal::UnknownSound)?;
    let session = match &request.session {
        Some(session) => {
            let of = stage.named.as_deref().ok_or(Refusal::UnknownSession)?;
            if !of.eq_ignore_ascii_case(campaign) {
                return Err(Refusal::SessionOfAnotherCampaign);
            }
            session.clone()
        }
        None => stage.current.clone().ok_or(Refusal::NoSession)?,
    };
    Ok(Row::from([
        ("id".to_string(), json!(fresh.id)),
        ("sessionId".to_string(), json!(session.to_ascii_lowercase())),
        ("soundId".to_string(), json!(request.sound)),
        ("origin".to_string(), request.origin.clone()),
        ("startedAt".to_string(), json!(fresh.now)),
    ]))
}

/// `text` as a uuid, lowercase as the tables write it; `None` when it is no
/// uuid.
fn uuid(text: &str) -> Option<String> {
    let hyphens = [8, 13, 18, 23];
    let shaped = text.len() == 36
        && text.char_indices().all(|(i, c)| {
            if hyphens.contains(&i) {
                c == '-'
            } else {
                c.is_ascii_hexdigit()
            }
        });
    shaped.then(|| text.to_ascii_lowercase())
}
