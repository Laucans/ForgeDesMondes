//! DataCapability `live.setMicrophone` (`data-capability.json`): the DM's
//! microphone turned on or off in the campaign's session. A session still in
//! `prep` goes `live` and is dated once (`started_at` is stamped the first
//! time and never again), a `closed` one refuses (`status-lifecycle`: prep,
//! live, closed, never backwards). Once the command is applied, the microphone
//! on opens the local transcription with the campaign's vocabulary
//! (DetectionVocabulary@1, the terms `live.getTranscriptionVocabulary` reads)
//! and every element or sound a heard phrase names becomes a suggestion, only
//! while the session is live with the microphone on; the microphone off ends
//! the stream and its task. Only text leaves the transcription
//! (`text-only-no-audio`): no audio goes further than the speech-to-text port.
//!
//! The engine writes one partition per command, `Session/<id>`, and judges
//! the row it leaves. [`request`] reads the wire payload, [`prepare`] turns
//! the request and the session's row into the row written, or into the
//! [`Refusal`] that says why not — before anything is queued. [`listening`]
//! holds the streams, [`detect`] the vocabulary and how a phrase names it.

use std::collections::BTreeMap;
use std::sync::Arc;

use aggregate_session::{MICROPHONE_ON, SESSION_STATUS};
use async_trait::async_trait;
use harness_dataguard::plugins::{self, Context, Plugin, Prepare};
use ports_speech_to_text::SharedSpeechToText;
use serde_json::{Value, json};
use uuid::Uuid;

pub mod detect;
pub mod listening;

pub use listening::Listening;

use detect::Vocabulary;
use listening::{Observer, Plan, Plans};

/// This DataCapability's id, as `data-capability.json` names it.
pub const DATA_CAPABILITY: &str = "live.setMicrophone";

/// The aggregate it writes.
const AGGREGATE: &str = "Session";

/// When a session went live, in milliseconds since the epoch.
const STARTED_AT: &str = "startedAt";

/// The session the command is for, in one row: the one `$1` names (the empty
/// text names none), whatever its status; else, of the sessions not closed,
/// the live one — of two, the one whose microphone is on, then the last one
/// started, then the last one planned, as `live.listSuggestions` finds it
/// without a campaign. No row when there is none. A
/// session the engine deleted (its tombstone) is none; `$1` is compared as
/// text, so that an id which is no uuid finds nothing instead of failing.
const SESSION: &str = "\
SELECT s.id::text AS id, s.campaign_id::text AS campaign_id, s.status, \
       s.microphone_on, s.started_at IS NOT NULL AS started \
FROM session s \
WHERE (($1 = '' AND s.status <> 'closed') OR s.id::text = $1) \
  AND NOT EXISTS (SELECT 1 FROM dataguard_rows r \
      WHERE r.partition = 'Session/' || s.id::text AND r.tombstoned) \
ORDER BY (s.status = 'live') DESC, s.microphone_on DESC, \
         s.started_at DESC NULLS LAST, s.position DESC, s.id \
LIMIT 1";

/// What the microphone of the campaign `$1` listens for, in the order
/// DetectionVocabulary@1 gives: the name of each element, then its aliases,
/// the elements one after the other, then the name of each sound — a term
/// that names nothing (AliasMatch@1: blank once normalised) left out, an
/// element or a sound the engine deleted (its tombstone) too. `$1` is
/// compared as text.
const VOCABULARY: &str = "\
SELECT v.term, v.kind, v.id, v.alias \
FROM ( \
    SELECT e.name AS term, 'entity' AS kind, e.id::text AS id, false AS alias, \
           0 AS source, e.name AS owner, '' AS spelling \
    FROM entity e \
    WHERE e.campaign_id::text = $1 \
      AND NOT EXISTS (SELECT 1 FROM dataguard_rows r \
          WHERE r.partition = 'Entity/' || e.id::text AND r.tombstoned) \
    UNION ALL \
    SELECT a.alias, 'entity', e.id::text, true, 0, e.name, a.alias \
    FROM entity_alias a JOIN entity e ON e.id = a.entity_id \
    WHERE e.campaign_id::text = $1 \
      AND NOT EXISTS (SELECT 1 FROM dataguard_rows r \
          WHERE r.partition = 'Entity/' || e.id::text AND r.tombstoned) \
    UNION ALL \
    SELECT d.name, 'sound', d.id::text, false, 1, d.name, '' \
    FROM sound d \
    WHERE d.campaign_id::text = $1 \
      AND NOT EXISTS (SELECT 1 FROM dataguard_rows r \
          WHERE r.partition = 'Sound/' || d.id::text AND r.tombstoned) \
) v \
WHERE alias_match_normalized(v.term) <> '' \
ORDER BY v.source, v.owner, v.id, v.alias, v.spelling";

/// One partition's row, as the engine holds it.
pub type Row = BTreeMap<String, Value>;

/// What the command asks for, read from the wire.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Request {
    /// Whether the microphone is turned on (`true`) or off.
    pub on: bool,
    /// The session it names, lowercase and hyphenated; none asks for the
    /// campaign's session ([`SESSION`]).
    pub session: Option<String>,
}

/// Why the command is refused before it is queued.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// The session it names is no uuid.
    NotAnId(&'static str),
    /// `on` is not a boolean.
    NotABoolean,
    /// There is no session to turn the microphone on or off in.
    NoSession,
    /// The session is closed: it never goes back to live
    /// (`status-lifecycle`).
    Closed,
}

impl Refusal {
    /// Whether the refusal is of something the request names that does not
    /// exist (404) rather than of a request that cannot be done (422).
    #[must_use]
    pub const fn not_found(&self) -> bool {
        matches!(self, Self::NoSession)
    }
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotAnId(field) => write!(f, "{field}: not an id (a uuid)"),
            Self::NotABoolean => f.write_str("on: the microphone is on (true) or off (false)"),
            Self::NoSession => f.write_str("there is no session to listen in"),
            Self::Closed => {
                f.write_str("status-lifecycle: the session is closed, it does not go back to live")
            }
        }
    }
}

/// What the command becomes before it is submitted: the partition it writes
/// and the fields it writes there — the manifest's `touches`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Prepared {
    /// The session's id: the partition the command lands on.
    pub target_id: String,
    /// The session's campaign, whose vocabulary the microphone listens for.
    pub campaign: String,
    /// The fields written.
    pub payload: Row,
}

/// What the DataCapability plugs into the engine: its `Prepare`, and what
/// sees a session written — the microphone on opens the stream, off ends it.
#[must_use]
pub fn plugin() -> Plugin {
    let plans = Arc::new(Plans::default());
    Plugin::new()
        .prepare(
            DATA_CAPABILITY,
            Prepares {
                plans: Arc::clone(&plans),
            },
        )
        .project(AGGREGATE, Observer(plans))
        .needs::<SharedSpeechToText>()
        .needs::<Listening>()
}

/// The command `wire` asks for on `target`, the session the command names.
///
/// # Errors
///
/// [`Refusal::NotAnId`] for a target that is no uuid,
/// [`Refusal::NotABoolean`] when `on` is not a boolean.
pub fn request(wire: &Row, target: Option<&str>) -> Result<Request, Refusal> {
    let on = wire
        .get("on")
        .and_then(Value::as_bool)
        .ok_or(Refusal::NotABoolean)?;
    let session = target
        .map(|id| uuid(id).ok_or(Refusal::NotAnId("target")))
        .transpose()?;
    Ok(Request { on, session })
}

/// The row `request` writes at `now_millis`, `session` being the row
/// [`SESSION`] answered for it (empty when there is none).
///
/// The microphone on sets `microphoneOn`; a session in `prep` goes `live`
/// and a session never started is dated now. The microphone off sets
/// `microphoneOn` alone.
///
/// # Errors
///
/// [`Refusal::NoSession`] when the session is not there,
/// [`Refusal::Closed`] when the microphone is turned on in a closed session.
pub fn prepare(request: &Request, session: &Row, now_millis: u64) -> Result<Prepared, Refusal> {
    let text = |column: &str| session.get(column).and_then(Value::as_str);
    let (Some(id), Some(campaign), Some(status)) =
        (text("id"), text("campaign_id"), text("status"))
    else {
        return Err(Refusal::NoSession);
    };
    let mut payload = Row::from([(MICROPHONE_ON.to_string(), json!(request.on))]);
    if request.on {
        if status == "closed" {
            return Err(Refusal::Closed);
        }
        if status == "prep" {
            payload.insert(SESSION_STATUS.to_string(), json!("live"));
        }
        if session.get("started") != Some(&Value::Bool(true)) {
            payload.insert(STARTED_AT.to_string(), json!(now_millis));
        }
    }
    Ok(Prepared {
        target_id: id.to_string(),
        campaign: campaign.to_string(),
        payload,
    })
}

/// `text` as a uuid, lowercase and hyphenated as the tables write it; `None`
/// when it is no uuid.
fn uuid(text: &str) -> Option<String> {
    Uuid::parse_str(text.trim()).ok().map(|id| id.to_string())
}

/// The first row `answer` holds, as a row of the engine's kind; empty when
/// it holds none.
fn first(answer: &Value) -> Row {
    answer
        .as_array()
        .and_then(|all| all.first())
        .and_then(Value::as_object)
        .map(|row| row.clone().into_iter().collect())
        .unwrap_or_default()
}

/// The command made the row it writes ([`prepare`]), judged against the
/// session as the tables hold it: refused 404 for what is not there, 422
/// otherwise — nothing queued. It stages the stream the microphone on will
/// open once the command is applied.
struct Prepares {
    plans: Arc<Plans>,
}

#[async_trait]
impl Prepare for Prepares {
    async fn prepare(
        &self,
        context: &Context<'_>,
        payload: &BTreeMap<String, Value>,
        target_id: Option<&str>,
    ) -> Result<plugins::Prepared, plugins::Refusal> {
        let request = request(payload, target_id).map_err(refused)?;
        let rows = context.rows.ok_or_else(|| {
            plugins::Refusal::unprocessable("the session cannot be read: no database")
        })?;
        let unreadable = |what: &'static str| {
            move |e: harness_dataguard::Error| {
                plugins::Refusal::unprocessable(format!("{what} is unreadable: {e}"))
            }
        };
        let session = first(
            &rows
                .rows(
                    SESSION,
                    &[json!(request.session.as_deref().unwrap_or_default())],
                )
                .await
                .map_err(unreadable("the session"))?,
        );
        let prepared = prepare(&request, &session, context.now_millis).map_err(refused)?;
        let (Some(speech), Some(listening)) = (
            context.shared.get::<SharedSpeechToText>(),
            context.shared.get::<Listening>(),
        ) else {
            return Err(plugins::Refusal::unprocessable(
                "the microphone is not shared with the units: nothing listens",
            ));
        };
        let plan = if request.on {
            let vocabulary = rows
                .rows(VOCABULARY, &[json!(prepared.campaign)])
                .await
                .map_err(unreadable("the campaign's vocabulary"))?;
            Some(Plan {
                vocabulary: Vocabulary::from_rows(&vocabulary),
                speech: (*speech).clone(),
                caller: context.caller.clone(),
            })
        } else {
            None
        };
        self.plans.stage(&prepared.target_id, &listening, plan);
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
