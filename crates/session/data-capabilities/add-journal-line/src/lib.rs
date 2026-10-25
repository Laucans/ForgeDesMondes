//! DataCapability `session.addJournalLine` (`data-capability.json`): a phrase
//! the microphone of a live session heard becomes the next line of its
//! journal, dated, text only (`text-only-no-audio`), kept on the machine for
//! the close's summary and the next preparation to read.
//!
//! The microphone's stream (`live.setMicrophone`) submits one per phrase it
//! hears, the session as the command's target. [`request`] reads the wire
//! payload, [`prepare`] turns the request and the session's row into the
//! fields written, or into the [`Refusal`] that says why not — before
//! anything is queued:
//!
//! - the session must be one the engine or the tables hold
//!   ([`Refusal::UnknownSession`], 404), live ([`Refusal::NotLive`]) with
//!   its microphone on ([`Refusal::MicrophoneOff`]);
//! - the line is written on the session's own row and judged with it: a
//!   closed session takes no journal line (`status-lifecycle`).
//!
//! The line lands in `session_journal_line` through [`JournalLines`], in the
//! transaction that applies it, under the id [`prepare`] gives it: a later
//! write of the session, which carries its last line again, adds none.

use std::collections::BTreeMap;

use aggregate_session::{MICROPHONE_ON, SESSION_STATUS, uuid};
use async_trait::async_trait;
use harness_dataguard::plugins::{self, Context, Plugin, Prepare};
use serde_json::{Value, json};

mod tables;

pub use tables::JournalLines;

/// This DataCapability's id, as `data-capability.json` names it.
pub const DATA_CAPABILITY: &str = "session.addJournalLine";

/// The aggregate it writes, and the prefix of its partitions.
const AGGREGATE: &str = "Session";

/// The fields of a journal line, as the Session aggregate declares them.
pub const LINE_ID: &str = "sessionJournalLineId";
/// When the line was spoken: milliseconds since the epoch, the engine's.
pub const SPOKEN_AT: &str = "spokenAt";
/// What was said, as transcribed.
pub const TEXT: &str = "text";

/// The one status a session takes lines in.
const LIVE: &str = "live";

/// One partition's row, as the engine holds it.
pub type Row = BTreeMap<String, Value>;

/// What the command asks for, read from the wire.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Request {
    /// The session whose journal takes the line, lowercase and hyphenated.
    pub session: String,
    /// The phrase heard, trimmed.
    pub text: String,
}

/// Why the command is refused before it is queued.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// The payload carries a field the command does not take.
    UnknownField(String),
    /// The command names no session, or one that is no uuid.
    NoSession,
    /// The phrase is no text, or a blank one.
    BlankText,
    /// Neither the engine nor the tables hold the session.
    UnknownSession,
    /// The session is in prep or closed.
    NotLive,
    /// The session is live but its microphone is off.
    MicrophoneOff,
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
            Self::UnknownField(field) => {
                write!(f, "{field} : une ligne de journal se décrit par son texte")
            }
            Self::NoSession => f.write_str("la séance visée : un identifiant"),
            Self::BlankText => f.write_str("text : la phrase entendue est vide"),
            Self::UnknownSession => f.write_str("séance introuvable"),
            Self::NotLive => {
                f.write_str("status-lifecycle : la séance n'est pas en cours, son journal est clos")
            }
            Self::MicrophoneOff => {
                f.write_str("le micro de la séance est coupé : rien n'est entendu")
            }
        }
    }
}

/// What the DataCapability plugs into the engine: its `Prepare`, and the
/// line's projection into the tables.
#[must_use]
pub fn plugin() -> Plugin {
    Plugin::new()
        .prepare(DATA_CAPABILITY, Prepares)
        .project_pg(AGGREGATE, JournalLines)
}

/// The command `wire` asks for on `target`, the session it names.
///
/// # Errors
///
/// [`Refusal::UnknownField`] for a field the command does not take;
/// [`Refusal::NoSession`] for a target that is missing or no uuid;
/// [`Refusal::BlankText`] for a text that is missing, no text, or blank.
pub fn request(wire: &Row, target: Option<&str>) -> Result<Request, Refusal> {
    if let Some(field) = wire.keys().find(|field| field.as_str() != TEXT) {
        return Err(Refusal::UnknownField(field.clone()));
    }
    let session = target
        .map(str::trim)
        .filter(|id| uuid(id))
        .ok_or(Refusal::NoSession)?
        .to_ascii_lowercase();
    let text = wire
        .get(TEXT)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .ok_or(Refusal::BlankText)?;
    Ok(Request {
        session,
        text: text.to_owned(),
    })
}

/// The fields `request` writes on the session's row, `session` being that
/// row as the engine or the tables hold it, the line named `line` and
/// spoken at `now_millis`.
///
/// # Errors
///
/// [`Refusal::UnknownSession`] when the row is empty; [`Refusal::NotLive`]
/// when the session is not live; [`Refusal::MicrophoneOff`] when its
/// microphone is off.
pub fn prepare(
    request: &Request,
    session: &Row,
    line: &str,
    now_millis: u64,
) -> Result<Row, Refusal> {
    if session.is_empty() {
        return Err(Refusal::UnknownSession);
    }
    if session.get(SESSION_STATUS).and_then(Value::as_str) != Some(LIVE) {
        return Err(Refusal::NotLive);
    }
    if session.get(MICROPHONE_ON) != Some(&Value::Bool(true)) {
        return Err(Refusal::MicrophoneOff);
    }
    Ok(Row::from([
        (LINE_ID.to_owned(), json!(line)),
        (SPOKEN_AT.to_owned(), json!(now_millis)),
        (TEXT.to_owned(), json!(request.text)),
    ]))
}

/// The command made the fields it writes ([`prepare`]), judged against the
/// session's row as the queue leaves it: refused 404 for a session that is
/// not there, 422 otherwise — nothing queued.
struct Prepares;

#[async_trait]
impl Prepare for Prepares {
    async fn prepare(
        &self,
        context: &Context<'_>,
        payload: &BTreeMap<String, Value>,
        target_id: Option<&str>,
    ) -> Result<plugins::Prepared, plugins::Refusal> {
        let request = request(payload, target_id).map_err(refused)?;
        let session = context
            .row(&format!("{AGGREGATE}/{}", request.session))
            .await?;
        let fields = prepare(&request, &session, &context.ids.fresh(), context.now_millis)
            .map_err(refused)?;
        Ok(plugins::Prepared {
            target_id: Some(request.session),
            payload: fields,
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
