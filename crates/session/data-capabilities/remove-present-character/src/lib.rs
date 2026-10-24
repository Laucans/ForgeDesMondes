//! DataCapability `session.removePresentCharacter` (`data-capability.json`):
//! a player cannot come, and the DM unticks their PC. From then on the PC
//! is no longer offered, in the DM view or on the session's battlemaps,
//! through `session_roster_view` (« present »); the character and its sheet
//! stay as they are.
//!
//! An untick is one deletion on the presence's own partition,
//! `Session/<session>.<pc>` (`presence_id`, the row
//! `session.addPresentCharacter` ticks) — never the session's own row,
//! whose tombstone would remove the session: the same PC ticked and
//! unticked queues on one row. Its tombstone takes the one link row of the
//! session and the PC out of the table when it is applied
//! (`crates/session/aggregates/Session`, `Absences`), whether the PC was
//! ticked at the session's creation or later. What the screen sends is
//! `{ "character" }` with the session as `target.id`, or `{ "session",
//! "character" }`, and it is judged in two steps:
//!
//! - [`asked`] reads the request: the session and the character are uuids,
//!   lowercase as the tables write them; one that is no uuid is malformed
//!   ([`Refusal::InvalidSession`], [`Refusal::InvalidCharacter`]), and so
//!   is a payload's session that is not the target's
//!   ([`Refusal::OtherSession`]). Any other field is refused
//!   ([`Refusal::Unknown`]).
//! - [`prepare`] judges it against the tables: a session they do not hold,
//!   or whose tombstone the engine keeps, is not found
//!   ([`Refusal::UnknownSession`]). Nothing is queued.
//!
//! A PC who is not present — never ticked, unticked already, or no PC at
//! all — is not refused: there is no link row to take, and nothing changes,
//! at this session or any other.

use std::collections::BTreeMap;

use aggregate_session::{presence_id, uuid};
use async_trait::async_trait;
use harness_dataguard::plugins::{self, Context, Plugin, Prepare};
use serde_json::Value;

/// This DataCapability's id, as `data-capability.json` names it.
pub const DATA_CAPABILITY: &str = "session.removePresentCharacter";

/// The payload's fields.
const SESSION: &str = "session";
const CHARACTER: &str = "character";
const FIELDS: [&str; 2] = [SESSION, CHARACTER];

/// The session `$1` names, as the tables hold it live.
const SESSION_ROW: &str = "SELECT s.id::text AS id
    FROM session s
    WHERE s.id = $1::text::uuid
        AND NOT EXISTS (SELECT 1 FROM dataguard_rows r
            WHERE r.partition = 'Session/' || s.id::text AND r.tombstoned)";

/// One partition's row, or a payload, by field.
pub type Row = BTreeMap<String, Value>;

/// What a request asks: `character` no longer present at `session`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Asked {
    /// The session's id, lowercase.
    pub session: String,
    /// The character's id, lowercase.
    pub character: String,
}

/// Why an untick is refused before it is queued.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// The payload carries a field the command does not take.
    Unknown(String),
    /// The session is missing, or no uuid.
    InvalidSession,
    /// The payload's session is not the one `target.id` names.
    OtherSession,
    /// The character is missing, or no uuid.
    InvalidCharacter,
    /// The store holds no such session.
    UnknownSession,
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unknown(field) => write!(
                f,
                "{field} : un PJ quitte une séance par session et character"
            ),
            Self::InvalidSession => write!(f, "session : l'identifiant d'une séance"),
            Self::OtherSession => write!(f, "session : celle que target.id nomme"),
            Self::InvalidCharacter => write!(f, "character : l'identifiant d'un PJ"),
            Self::UnknownSession => write!(f, "séance introuvable"),
        }
    }
}

/// What the DataCapability plugs into the engine: its `Prepare`.
#[must_use]
pub fn plugin() -> Plugin {
    Plugin::new().prepare(DATA_CAPABILITY, Unticks)
}

/// An untick made the deletion of the presence's own partition, judged
/// against its session ([`live`]): one of a session there is not is
/// refused, 404; one malformed, 422 — neither is queued.
struct Unticks;

#[async_trait]
impl Prepare for Unticks {
    async fn prepare(
        &self,
        context: &Context<'_>,
        payload: &BTreeMap<String, Value>,
        target_id: Option<&str>,
    ) -> Result<plugins::Prepared, plugins::Refusal> {
        let asked = asked(payload, target_id).map_err(refused)?;
        let session = live(context, &asked.session).await?;
        let target = prepare(&asked, session).map_err(refused)?;
        // A deletion writes no field: the presence's partition is
        // tombstoned whole.
        Ok(plugins::Prepared {
            target_id: Some(target),
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

/// Whether the tables hold the session `id` live — the fixtures' among
/// them — and the engine keeps no tombstone of it; a failed read is
/// unprocessable.
async fn live(context: &Context<'_>, id: &str) -> Result<bool, plugins::Refusal> {
    let Some(rows) = context.rows else {
        return Ok(false);
    };
    let found = rows
        .rows(SESSION_ROW, &[Value::from(id)])
        .await
        .map_err(|e| plugins::Refusal::unprocessable(e.to_string()))?;
    Ok(found.as_array().is_some_and(|rows| !rows.is_empty()))
}

/// The uuid `value` holds, trimmed and lowercase.
fn id(value: Option<&Value>) -> Option<String> {
    value
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|id| uuid(id))
        .map(str::to_ascii_lowercase)
}

/// What `wire` asks, its session being `target`'s when the submission names
/// one.
///
/// # Errors
///
/// [`Refusal`] when `wire` carries another field, names its session or its
/// character by something other than a uuid ([`Refusal::InvalidSession`],
/// [`Refusal::InvalidCharacter`]), or a session other than `target`
/// ([`Refusal::OtherSession`]).
pub fn asked(wire: &Row, target: Option<&str>) -> Result<Asked, Refusal> {
    if let Some(field) = wire.keys().find(|field| !FIELDS.contains(&field.as_str())) {
        return Err(Refusal::Unknown(field.clone()));
    }
    let sent = match wire.get(SESSION) {
        None | Some(Value::Null) => None,
        named => Some(id(named).ok_or(Refusal::InvalidSession)?),
    };
    let session = match target {
        Some(target) => id(Some(&Value::from(target))).ok_or(Refusal::InvalidSession)?,
        None => sent.clone().ok_or(Refusal::InvalidSession)?,
    };
    if sent.is_some_and(|sent| sent != session) {
        return Err(Refusal::OtherSession);
    }
    let character = id(wire.get(CHARACTER)).ok_or(Refusal::InvalidCharacter)?;
    Ok(Asked { session, character })
}

/// `asked` judged against whether the store holds its session live: the
/// presence whose partition the untick deletes, the row a tick writes
/// (`presence_id`).
///
/// # Errors
///
/// [`Refusal::UnknownSession`] when there is no session.
pub fn prepare(asked: &Asked, session: bool) -> Result<String, Refusal> {
    if !session {
        return Err(Refusal::UnknownSession);
    }
    Ok(presence_id(&asked.session, &asked.character))
}
