//! DataCapability `live.addSuggestion` (`data-capability.json`): records an
//! element or a sound the microphone recognised during a live session, as a
//! new `Suggestion` row — the pill's pile and the history are read from it.
//!
//! It keeps the invariants of Suggestion that its own row cannot say: the
//! session must exist, be `live` with the microphone on
//! (`live-only-local-audio`), and the element or the sound must belong to the
//! session's campaign (`suggestion-target`). The engine judges the row it
//! leaves; the instant of the detection is the engine's clock, and the row
//! holds nothing but the suggestion (`pile-and-history-derived`).
//!
//! [`request`] reads the wire payload, [`prepare`] turns the request and the
//! session's row into the row written, or into the [`Refusal`] that says why
//! not — before anything is queued.

use std::collections::BTreeMap;

use async_trait::async_trait;
use harness_dataguard::plugins::{self, Context, Plugin, Prepare};
use serde_json::{Value, json};
use uuid::Uuid;

/// This DataCapability's id, as `data-capability.json` names it.
pub const DATA_CAPABILITY: &str = "live.addSuggestion";

/// The session and whether the target belongs to its campaign, in one row:
/// no row when the session is unknown. `$1` is the session, `$2` the element
/// and `$3` the sound (an empty text when the request names none), each
/// bound and compared as text.
const SESSION: &str = "\
SELECT s.status, s.microphone_on, \
       EXISTS (SELECT 1 FROM entity e \
               WHERE e.id::text = $2 AND e.campaign_id = s.campaign_id) AS entity_in_campaign, \
       EXISTS (SELECT 1 FROM sound d \
               WHERE d.id::text = $3 AND d.campaign_id = s.campaign_id) AS sound_in_campaign \
FROM session s \
WHERE s.id::text = $1";

/// One partition's row, as the engine holds it.
pub type Row = BTreeMap<String, Value>;

/// What the suggestion aims at: exactly one of an element or a sound, by id.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    /// An element of the campaign.
    Entity(String),
    /// A sound of the campaign.
    Sound(String),
}

/// What the command asks for, read from the wire.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Request {
    /// The row it writes, lowercase and hyphenated: the engine's fresh id.
    pub id: String,
    /// The live session whose microphone heard it.
    pub session_id: String,
    /// The element or the sound recognised.
    pub target: Target,
    /// The name or alias heard, trimmed.
    pub matched_text: String,
}

/// Why the command is refused before it is queued.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// A field that must be a uuid is not one.
    NotAnId(&'static str),
    /// The command names the row it writes: a suggestion is always new.
    NamedRow,
    /// The request names both an element and a sound, or neither.
    OneTarget,
    /// The matched text is blank.
    BlankText,
    /// The session it names does not exist.
    UnknownSession,
    /// The session is in prep or closed.
    NotLive,
    /// The session is live but its microphone is off.
    MicrophoneOff,
    /// The element or the sound (`"element"` or `"sound"`) is not one of the
    /// session's campaign.
    OutsideCampaign(&'static str),
}

impl Refusal {
    /// Whether the refusal is of something the request names that does not
    /// exist (404) rather than of a request that cannot be done (422).
    #[must_use]
    pub const fn not_found(&self) -> bool {
        matches!(self, Self::UnknownSession)
    }
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotAnId(field) => write!(f, "{field}: not an id (a uuid)"),
            Self::NamedRow => f.write_str("a suggestion is always new: the engine names its row"),
            Self::OneTarget => {
                f.write_str("a suggestion aims at exactly one of an entityId and a soundId")
            }
            Self::BlankText => f.write_str("matchedText: the words heard are blank"),
            Self::UnknownSession => f.write_str("that session does not exist"),
            Self::NotLive => f.write_str("the session is not live: nothing is suggested"),
            Self::MicrophoneOff => f.write_str("the session's microphone is off: nothing is heard"),
            Self::OutsideCampaign(kind) => {
                write!(f, "that {kind} is not one of the session's campaign")
            }
        }
    }
}

/// What the command becomes before it is submitted: the partition it
/// writes and the fields it writes there.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Prepared {
    /// The row's id: the partition the command lands on.
    pub target_id: String,
    /// The fields written.
    pub payload: Row,
}

/// What the DataCapability plugs into the engine: its `Prepare`.
#[must_use]
pub fn plugin() -> Plugin {
    Plugin::new().prepare(DATA_CAPABILITY, Prepares)
}

/// The command `wire` asks for, writing the row `fresh`, on `target`, the
/// row the submission names (none is allowed to).
///
/// # Errors
///
/// [`Refusal::NamedRow`] when `target` names a row,
/// [`Refusal::NotAnId`] for a session, element or sound that is no uuid,
/// [`Refusal::OneTarget`] for both an element and a sound, or neither,
/// [`Refusal::BlankText`] for a blank `matchedText`.
pub fn request(wire: &Row, target: Option<&str>, fresh: &str) -> Result<Request, Refusal> {
    if target.is_some() {
        return Err(Refusal::NamedRow);
    }
    let session_id = named(wire, "sessionId")?.ok_or(Refusal::NotAnId("sessionId"))?;
    let target = match (named(wire, "entityId")?, named(wire, "soundId")?) {
        (Some(entity), None) => Target::Entity(entity),
        (None, Some(sound)) => Target::Sound(sound),
        _ => return Err(Refusal::OneTarget),
    };
    let matched_text = wire
        .get("matchedText")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .ok_or(Refusal::BlankText)?;
    Ok(Request {
        id: fresh.to_string(),
        session_id,
        target,
        matched_text: matched_text.to_string(),
    })
}

/// The row `request` writes at `now_millis`, `session` being the row
/// [`SESSION`] answered for it (empty when the session is unknown).
///
/// # Errors
///
/// [`Refusal::UnknownSession`] when the session is not there,
/// [`Refusal::NotLive`] when it is not `live`, [`Refusal::MicrophoneOff`]
/// when its microphone is off, [`Refusal::OutsideCampaign`] when the element
/// or the sound is not of its campaign.
pub fn prepare(request: &Request, session: &Row, now_millis: u64) -> Result<Prepared, Refusal> {
    if session.is_empty() {
        return Err(Refusal::UnknownSession);
    }
    if session.get("status").and_then(Value::as_str) != Some("live") {
        return Err(Refusal::NotLive);
    }
    if session.get("microphone_on") != Some(&Value::Bool(true)) {
        return Err(Refusal::MicrophoneOff);
    }
    let (field, kind, column, id) = match &request.target {
        Target::Entity(id) => ("entityId", "element", "entity_in_campaign", id),
        Target::Sound(id) => ("soundId", "sound", "sound_in_campaign", id),
    };
    if session.get(column) != Some(&Value::Bool(true)) {
        return Err(Refusal::OutsideCampaign(kind));
    }
    let payload = [
        ("id", json!(request.id)),
        ("sessionId", json!(request.session_id)),
        (field, json!(id)),
        ("matchedText", json!(request.matched_text)),
        ("detectedAt", json!(instant(now_millis))),
    ]
    .into_iter()
    .map(|(field, value)| (field.to_string(), value))
    .collect();
    Ok(Prepared {
        target_id: request.id.clone(),
        payload,
    })
}

/// The uuid `wire` holds in `field`, lowercase and hyphenated as the tables
/// write it; `None` when it holds none (absent or `null`).
fn named(wire: &Row, field: &'static str) -> Result<Option<String>, Refusal> {
    match wire.get(field) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => value
            .as_str()
            .and_then(uuid)
            .map(Some)
            .ok_or(Refusal::NotAnId(field)),
    }
}

/// `text` as a uuid, lowercase and hyphenated as the tables write it; `None`
/// when it is no uuid.
fn uuid(text: &str) -> Option<String> {
    Uuid::parse_str(text.trim()).ok().map(|id| id.to_string())
}

/// `millis` since the epoch as an ISO 8601 instant in UTC, to the
/// millisecond: what PostgreSQL reads into a `timestamptz`.
fn instant(millis: u64) -> String {
    const DAY: u64 = 86_400_000;
    let (days, of_day) = (millis / DAY, millis % DAY);
    // The civil date of a day count (Howard Hinnant's `civil_from_days`),
    // years running from March so that the leap day closes them.
    let shifted = days + 719_468;
    let era = shifted / 146_097;
    let day_of_era = shifted % 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_from_march = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_from_march + 2) / 5 + 1;
    let (month, year) = if month_from_march < 10 {
        (month_from_march + 3, year_of_era + era * 400)
    } else {
        (month_from_march - 9, year_of_era + era * 400 + 1)
    };
    let (hour, minute, second, milli) = (
        of_day / 3_600_000,
        of_day / 60_000 % 60,
        of_day / 1_000 % 60,
        of_day % 1_000,
    );
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{milli:03}Z")
}

/// The command made the row it writes ([`prepare`]), judged against the
/// session as the tables hold it: refused 404 for what is not there, 422
/// otherwise — nothing queued.
struct Prepares;

#[async_trait]
impl Prepare for Prepares {
    async fn prepare(
        &self,
        context: &Context<'_>,
        payload: &BTreeMap<String, Value>,
        target_id: Option<&str>,
    ) -> Result<plugins::Prepared, plugins::Refusal> {
        let request = request(payload, target_id, &context.ids.fresh()).map_err(refused)?;
        let rows = context.rows.ok_or_else(|| {
            plugins::Refusal::unprocessable("the session cannot be read: no database")
        })?;
        let (entity, sound) = match &request.target {
            Target::Entity(id) => (id.as_str(), ""),
            Target::Sound(id) => ("", id.as_str()),
        };
        let answer = rows
            .rows(
                SESSION,
                &[json!(request.session_id), json!(entity), json!(sound)],
            )
            .await
            .map_err(|e| {
                plugins::Refusal::unprocessable(format!("the session is unreadable: {e}"))
            })?;
        let session: Row = answer
            .as_array()
            .and_then(|all| all.first())
            .and_then(Value::as_object)
            .map(|row| row.clone().into_iter().collect())
            .unwrap_or_default();
        let prepared = prepare(&request, &session, context.now_millis).map_err(refused)?;
        Ok(plugins::Prepared {
            target_id: Some(prepared.target_id),
            payload: prepared.payload,
            ..plugins::Prepared::default()
        })
    }
}

/// The engine's refusal of `refusal`: 404 for what is not there, 422
/// otherwise.
fn refused(refusal: Refusal) -> plugins::Refusal {
    if refusal.not_found() {
        plugins::Refusal::not_found(refusal.to_string())
    } else {
        plugins::Refusal::unprocessable(refusal.to_string())
    }
}
