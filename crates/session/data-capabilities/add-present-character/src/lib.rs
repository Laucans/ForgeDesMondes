//! DataCapability `session.addPresentCharacter` (`data-capability.json`):
//! a player joins late, and the DM ticks their PC present. From then on the
//! PC is offered like the others, in the DM view and on the session's
//! battlemaps, through `session_roster_view` (« present »).
//!
//! A presence is one command on a partition of its own, `Session/<its id>`
//! — never the session's own row, which keeps the PCs of its creation
//! (`crates/session/aggregates/Session`, `Presences`). What the screen sends
//! is `{ "character" }` with the session as `target.id`, or `{ "session",
//! "character" }`, and it becomes the presence's row in two steps:
//!
//! - [`asked`] reads the request: the session and the character are uuids,
//!   lowercase as the tables write them; one that is no uuid is malformed
//!   ([`Refusal::InvalidSession`], [`Refusal::InvalidCharacter`]), and so
//!   is a payload's session that is not the target's
//!   ([`Refusal::OtherSession`]). Any other field is refused
//!   ([`Refusal::Unknown`]).
//! - [`prepare`] judges it against the tables: a session they do not hold,
//!   or whose tombstone the engine keeps, is not found
//!   ([`Refusal::UnknownSession`]); a character that is no PC — a « pj »
//!   with its sheet — of the session's campaign is never present
//!   (`present-only-pj`, [`Refusal::NotAPc`]). Nothing is queued.
//!
//! The Session aggregate judges the PC again when the presence is applied,
//! and writes it once: a PC present already keeps its one row. A replay
//! under an idempotency key already submitted — the caller keys it by
//! session and character — is the engine's to answer, with the first
//! command: nothing is queued twice.

use std::collections::BTreeMap;

use aggregate_session::{ADDED_AT, PRESENT, PRESENT_ONLY_PJ, PRESENT_SESSION_ID, uuid};
use async_trait::async_trait;
use harness_dataguard::plugins::{self, Context, Plugin, Prepare};
use serde_json::Value;

/// This DataCapability's id, as `data-capability.json` names it.
pub const DATA_CAPABILITY: &str = "session.addPresentCharacter";

/// The payload's fields.
const SESSION: &str = "session";
const CHARACTER: &str = "character";
const FIELDS: [&str; 2] = [SESSION, CHARACTER];

/// The session `$1` names, as the tables hold it live: its campaign.
const SESSION_ROW: &str = "SELECT s.campaign_id::text AS campaign
    FROM session s
    WHERE s.id = $1::text::uuid
        AND NOT EXISTS (SELECT 1 FROM dataguard_rows r
            WHERE r.partition = 'Session/' || s.id::text AND r.tombstoned)";

/// The character `$2` names when it is a live PC — a « pj » with its sheet
/// — of the campaign `$1`.
const PC_ROW: &str = "SELECT e.id::text AS id
    FROM entity e JOIN character c ON c.entity_id = e.id
    WHERE e.id = $2::text::uuid AND e.type = 'pj' AND e.campaign_id = $1::text::uuid
        AND NOT EXISTS (SELECT 1 FROM dataguard_rows r
            WHERE r.partition = 'Entity/' || e.id::text AND r.tombstoned)";

/// One partition's row, as the engine holds it.
pub type Row = BTreeMap<String, Value>;

/// What a request asks: `character` present at `session`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Asked {
    /// The session's id, lowercase.
    pub session: String,
    /// The character's id, lowercase.
    pub character: String,
}

/// Why a presence is refused before it is queued.
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
    /// The character is no PC of the session's campaign.
    NotAPc,
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unknown(field) => write!(
                f,
                "{field} : un PJ rejoint une séance par session et character"
            ),
            Self::InvalidSession => write!(f, "session : l'identifiant d'une séance"),
            Self::OtherSession => write!(f, "session : celle que target.id nomme"),
            Self::InvalidCharacter => write!(f, "character : l'identifiant d'un PJ"),
            Self::UnknownSession => write!(f, "séance introuvable"),
            Self::NotAPc => write!(
                f,
                "{PRESENT_ONLY_PJ} : seuls des PJ de la campagne sont présents"
            ),
        }
    }
}

/// What the DataCapability plugs into the engine: its `Prepare`.
#[must_use]
pub fn plugin() -> Plugin {
    Plugin::new().prepare(DATA_CAPABILITY, Presents)
}

/// A PC ticked present made the row it writes on a partition of its own,
/// judged against its session and the PC ([`campaign`], [`pc`]): one of a
/// session there is not is refused, 404; one malformed, or ticking anything
/// but a PC of the session's campaign (`present-only-pj`), 422 — neither is
/// queued.
struct Presents;

#[async_trait]
impl Prepare for Presents {
    async fn prepare(
        &self,
        context: &Context<'_>,
        payload: &BTreeMap<String, Value>,
        target_id: Option<&str>,
    ) -> Result<plugins::Prepared, plugins::Refusal> {
        let asked = asked(payload, target_id).map_err(refused)?;
        let campaign = campaign(context, &asked.session).await?;
        let pc = match &campaign {
            Some(campaign) => pc(context, campaign, &asked.character).await?,
            None => false,
        };
        let payload =
            prepare(&asked, campaign.as_deref(), pc, context.now_millis).map_err(refused)?;
        Ok(plugins::Prepared {
            payload,
            // A partition of its own, never the session's row.
            target_id: Some(context.ids.fresh().to_ascii_lowercase()),
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

/// Whether the tables' read answers a row; a failed read is unprocessable.
async fn found(
    context: &Context<'_>,
    sql: &str,
    parameters: &[Value],
) -> Result<Option<Row>, plugins::Refusal> {
    let Some(rows) = context.rows else {
        return Ok(None);
    };
    let found = rows
        .rows(sql, parameters)
        .await
        .map_err(|e| plugins::Refusal::unprocessable(e.to_string()))?;
    Ok(found
        .as_array()
        .into_iter()
        .flatten()
        .find_map(|row| match row {
            Value::Object(row) => Some(row.clone().into_iter().collect()),
            _ => None,
        }))
}

/// The campaign of the session `id` names, as the tables hold it live — the
/// fixtures' among them: `None` when they hold none, or the engine keeps
/// its tombstone.
async fn campaign(context: &Context<'_>, id: &str) -> Result<Option<String>, plugins::Refusal> {
    let row = found(context, SESSION_ROW, &[Value::from(id)]).await?;
    Ok(row.and_then(|row| {
        row.get("campaign")
            .and_then(Value::as_str)
            .map(str::to_owned)
    }))
}

/// Whether `character` is a live PC of `campaign`, as the tables hold it.
async fn pc(
    context: &Context<'_>,
    campaign: &str,
    character: &str,
) -> Result<bool, plugins::Refusal> {
    let parameters = [Value::from(campaign), Value::from(character)];
    Ok(found(context, PC_ROW, &parameters).await?.is_some())
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

/// `asked` judged against its session's campaign — `None` when the store
/// holds no such session — and whether its character is a PC of it: the
/// row the presence writes, ticked at `now_millis`.
///
/// # Errors
///
/// [`Refusal::UnknownSession`] when there is no session, and
/// [`Refusal::NotAPc`] when the character is no PC of its campaign.
pub fn prepare(
    asked: &Asked,
    campaign: Option<&str>,
    pc: bool,
    now_millis: u64,
) -> Result<Row, Refusal> {
    campaign.ok_or(Refusal::UnknownSession)?;
    if !pc {
        return Err(Refusal::NotAPc);
    }
    Ok(Row::from([
        (
            PRESENT_SESSION_ID.to_owned(),
            Value::from(asked.session.clone()),
        ),
        (PRESENT.to_owned(), Value::from(asked.character.clone())),
        (ADDED_AT.to_owned(), Value::from(now_millis)),
    ]))
}
