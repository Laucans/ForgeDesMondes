//! DataCapability `session.createSession` (`data-capability.json`): the DM
//! creates the next session of a campaign, with its title, the day it is
//! planned for and the PCs who will attend.
//!
//! A session is one command on a partition of its own, `Session/<its id>`.
//! What the screen sends is `{ "campaign", "title", "plannedOn"?,
//! "present"? }`, and it becomes the aggregate's fields in two steps:
//!
//! - [`asked`] reads the request: the campaign is an id — any text, the
//!   tables say whether they hold it; the title a text with more than
//!   blanks; the day a `YYYY-MM-DD` of the calendar or nothing; the PCs
//!   present a list of ids, each kept once, in order, lowercase as the
//!   tables write them. Any other field is refused ([`Refusal::Unknown`]).
//! - [`prepare`] judges it against the campaign and the PCs the tables hold:
//!   a campaign they do not hold, or whose tombstone the engine keeps, is
//!   refused ([`Refusal::UnknownCampaign`]); a oneshot that has its session
//!   takes no other (`oneshot-single-session`, [`Refusal::OneshotHasItsSession`]);
//!   a character that is no PC of the campaign is never present
//!   (`present-only-pj`, [`Refusal::NotAPc`]). Nothing is queued.
//!
//! The session starts in preparation (`status-lifecycle`), its microphone
//! off. Its position is not chosen here: the Session aggregate settles it
//! when the session is applied, after the campaign's last, and judges the
//! oneshot and the PCs again then. A replay under an idempotency key already
//! submitted is the engine's to answer, with the first creation: nothing is
//! created twice.

use std::collections::BTreeMap;

use aggregate_session::{
    MICROPHONE_ON, ONESHOT, ONESHOT_SINGLE_SESSION, PLANNED_ON, PRESENT, PRESENT_ONLY_PJ,
    SESSION_CAMPAIGN_ID, SESSION_ID, SESSION_STATUS, SESSION_TITLE, uuid,
};
use async_trait::async_trait;
use harness_dataguard::plugins::{self, Context, Plugin, Prepare};
use serde_json::Value;

/// This DataCapability's id, as `data-capability.json` names it.
pub const DATA_CAPABILITY: &str = "session.createSession";

/// A new session's status: in preparation (`status-lifecycle`).
pub const STATUS: &str = "prep";

/// A new session's microphone: off.
pub const MICROPHONE: bool = false;

/// The payload's fields.
const CAMPAIGN: &str = "campaign";
const TITLE: &str = "title";
const PRESENT_PCS: &str = "present";
const FIELDS: [&str; 4] = [CAMPAIGN, TITLE, PLANNED_ON, PRESENT_PCS];

/// The campaign `$1` names, as the tables hold it live: its kind, and how
/// many sessions it holds.
const CAMPAIGN_ROW: &str = "SELECT c.kind AS kind,
        (SELECT count(*) FROM session s WHERE s.campaign_id = c.id)::integer AS sessions
    FROM campaign c
    WHERE c.id = $1::text::uuid
        AND NOT EXISTS (SELECT 1 FROM dataguard_rows r
            WHERE r.partition = 'Campaign/' || c.id::text AND r.tombstoned)";

/// The PCs of the campaign `$1` among the ids the list `$2` names, as the
/// tables hold them live: a « pj » with its sheet.
const PC_ROWS: &str = "SELECT e.id::text AS id
    FROM entity e JOIN character c ON c.entity_id = e.id
    WHERE e.type = 'pj' AND e.campaign_id = $1::text::uuid
        AND e.id IN (SELECT listed::uuid FROM jsonb_array_elements_text($2::jsonb) AS listed)
        AND NOT EXISTS (SELECT 1 FROM dataguard_rows r
            WHERE r.partition = 'Entity/' || e.id::text AND r.tombstoned)";

/// One partition's row, as the engine holds it.
pub type Row = BTreeMap<String, Value>;

/// What a request asks: the next session of `campaign`, with its title, its
/// day and the PCs present.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Asked {
    /// The campaign's id, trimmed, lowercase.
    pub campaign: String,
    /// The title, trimmed.
    pub title: String,
    /// The day it is planned for, `YYYY-MM-DD`; `None` for none.
    pub planned_on: Option<String>,
    /// The PCs' ids, trimmed, lowercase, each once, in order.
    pub present: Vec<String>,
}

/// The campaign a session is judged against, as the tables hold it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Campaign {
    /// `campagne` or `oneshot`.
    pub kind: String,
    /// How many sessions it holds.
    pub sessions: u64,
}

/// Why a session is refused before it is queued.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// The payload carries a field the command does not take.
    Unknown(String),
    /// The payload names no campaign.
    NoCampaign,
    /// The title is missing, blank, or no text.
    NoTitle,
    /// The day is neither a `YYYY-MM-DD` nor null.
    InvalidDate,
    /// The PCs present are no list of ids.
    InvalidPresent,
    /// The store holds no such campaign.
    UnknownCampaign,
    /// The campaign is a oneshot that has its session.
    OneshotHasItsSession,
    /// A character ticked present is no PC of the campaign.
    NotAPc,
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unknown(field) => write!(
                f,
                "{field} : une séance se décrit par campaign, title, plannedOn et present"
            ),
            Self::NoCampaign => write!(f, "campaign : une séance est d'une campagne"),
            Self::NoTitle => write!(f, "title : une séance a un titre"),
            Self::InvalidDate => write!(f, "plannedOn : une date AAAA-MM-JJ, ou rien"),
            Self::InvalidPresent => write!(f, "present : une liste d'identifiants de PJ"),
            Self::UnknownCampaign => write!(f, "campagne introuvable"),
            Self::OneshotHasItsSession => {
                write!(f, "{ONESHOT_SINGLE_SESSION} : un oneshot n'a qu'une séance")
            }
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
    Plugin::new().prepare(DATA_CAPABILITY, Creates)
}

/// A session made the row it writes on a partition of its own, judged
/// against its campaign and its PCs ([`campaign`], [`pcs`]): one of a
/// campaign there is not is refused, 404; one malformed, a oneshot's second,
/// or one ticking anything but a PC of the campaign present, 422 — none is
/// queued.
struct Creates;

#[async_trait]
impl Prepare for Creates {
    async fn prepare(
        &self,
        context: &Context<'_>,
        payload: &BTreeMap<String, Value>,
        _target_id: Option<&str>,
    ) -> Result<plugins::Prepared, plugins::Refusal> {
        let asked = asked(payload).map_err(refused)?;
        let campaign = campaign(context, &asked.campaign).await?;
        let pcs = pcs(context, &asked).await?;
        let session = context.ids.fresh().to_ascii_lowercase();
        let payload = prepare(&asked, &session, campaign.as_ref(), &pcs).map_err(refused)?;
        // The row is the payload's `sessionId` (`target.id`).
        Ok(plugins::Prepared {
            payload,
            ..plugins::Prepared::default()
        })
    }
}

/// A refusal as the server answers it: a campaign there is not is not
/// found, anything else unprocessable.
fn refused(refusal: Refusal) -> plugins::Refusal {
    if refusal == Refusal::UnknownCampaign {
        plugins::Refusal::not_found(refusal.to_string())
    } else {
        plugins::Refusal::unprocessable(refusal.to_string())
    }
}

/// What the tables' read answers, as rows; a failed read is unprocessable.
async fn read(
    context: &Context<'_>,
    sql: &str,
    parameters: &[Value],
) -> Result<Vec<Row>, plugins::Refusal> {
    let Some(rows) = context.rows else {
        return Ok(Vec::new());
    };
    let found = rows
        .rows(sql, parameters)
        .await
        .map_err(|e| plugins::Refusal::unprocessable(e.to_string()))?;
    Ok(found
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|row| match row {
            Value::Object(row) => Some(row.clone().into_iter().collect()),
            _ => None,
        })
        .collect())
}

/// The campaign `id` names, as the tables hold it live — the fixtures'
/// among them: `None` when they hold none, or the engine keeps its
/// tombstone. An id that is no uuid names none.
async fn campaign(context: &Context<'_>, id: &str) -> Result<Option<Campaign>, plugins::Refusal> {
    if !uuid(id) {
        return Ok(None);
    }
    let row = read(context, CAMPAIGN_ROW, &[Value::from(id)])
        .await?
        .into_iter()
        .next();
    Ok(row.map(|row| Campaign {
        kind: row
            .get("kind")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        sessions: row.get("sessions").and_then(Value::as_u64).unwrap_or(0),
    }))
}

/// The PCs of the campaign among those `asked` ticks present, as the tables
/// hold them live. An id that is no uuid names none.
async fn pcs(context: &Context<'_>, asked: &Asked) -> Result<Vec<String>, plugins::Refusal> {
    let listed: Vec<String> = asked
        .present
        .iter()
        .filter(|pc| uuid(pc))
        .cloned()
        .collect();
    if listed.is_empty() || !uuid(&asked.campaign) {
        return Ok(Vec::new());
    }
    let rows = read(
        context,
        PC_ROWS,
        &[Value::from(asked.campaign.clone()), Value::from(listed)],
    )
    .await?;
    Ok(rows
        .iter()
        .filter_map(|row| row.get("id").and_then(Value::as_str).map(str::to_owned))
        .collect())
}

/// An id as the tables write it: trimmed, lowercase, never blank.
fn id(value: &Value) -> Option<String> {
    value
        .as_str()
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .map(str::to_ascii_lowercase)
}

/// A day of the calendar written `YYYY-MM-DD`: four digits, a dash, two,
/// a dash, two — a year from 1, a month of it, a day of that month.
fn day(text: &str) -> bool {
    let shaped = text.len() == 10
        && text.char_indices().all(|(at, c)| match at {
            4 | 7 => c == '-',
            _ => c.is_ascii_digit(),
        });
    let number = |range: std::ops::Range<usize>| text[range].parse::<u32>().unwrap_or(0);
    shaped && number(0..4) > 0 && (1..=days_in(number(0..4), number(5..7))).contains(&number(8..10))
}

/// How many days `month` of `year` has: none for a month there is not.
fn days_in(year: u32, month: u32) -> u32 {
    let leap = year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400));
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => 0,
    }
}

/// What `wire` asks.
///
/// # Errors
///
/// [`Refusal`] when `wire` carries another field, names no campaign, has
/// no title, a day that is no `YYYY-MM-DD`, or PCs present that are no list
/// of ids.
pub fn asked(wire: &Row) -> Result<Asked, Refusal> {
    if let Some(field) = wire.keys().find(|field| !FIELDS.contains(&field.as_str())) {
        return Err(Refusal::Unknown(field.clone()));
    }
    let campaign = wire.get(CAMPAIGN).and_then(id).ok_or(Refusal::NoCampaign)?;
    let title = wire
        .get(TITLE)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|title| !title.is_empty())
        .ok_or(Refusal::NoTitle)?
        .to_owned();
    let planned_on = match wire.get(PLANNED_ON) {
        None | Some(Value::Null) => None,
        Some(Value::String(text)) if day(text.trim()) => Some(text.trim().to_owned()),
        Some(_) => return Err(Refusal::InvalidDate),
    };
    let mut present: Vec<String> = Vec::new();
    match wire.get(PRESENT_PCS) {
        None | Some(Value::Null) => {}
        Some(Value::Array(ids)) => {
            for pc in ids {
                let pc = id(pc).ok_or(Refusal::InvalidPresent)?;
                if !present.contains(&pc) {
                    present.push(pc);
                }
            }
        }
        Some(_) => return Err(Refusal::InvalidPresent),
    }
    Ok(Asked {
        campaign,
        title,
        planned_on,
        present,
    })
}

/// `asked` judged against its campaign — `None` when the store holds no
/// such campaign — and the PCs of it among those it ticks: the row the
/// session `session` writes.
///
/// # Errors
///
/// [`Refusal::UnknownCampaign`] when there is no campaign,
/// [`Refusal::OneshotHasItsSession`] when it is a oneshot that holds a
/// session, and [`Refusal::NotAPc`] when a character ticked present is not
/// among `pcs`.
pub fn prepare(
    asked: &Asked,
    session: &str,
    campaign: Option<&Campaign>,
    pcs: &[String],
) -> Result<Row, Refusal> {
    let campaign = campaign.ok_or(Refusal::UnknownCampaign)?;
    if campaign.kind == ONESHOT && campaign.sessions > 0 {
        return Err(Refusal::OneshotHasItsSession);
    }
    if asked.present.iter().any(|pc| !pcs.contains(pc)) {
        return Err(Refusal::NotAPc);
    }
    Ok(Row::from([
        (SESSION_ID.to_owned(), Value::from(session)),
        (
            SESSION_CAMPAIGN_ID.to_owned(),
            Value::from(asked.campaign.clone()),
        ),
        (SESSION_TITLE.to_owned(), Value::from(asked.title.clone())),
        (
            PLANNED_ON.to_owned(),
            asked.planned_on.clone().map_or(Value::Null, Value::from),
        ),
        (SESSION_STATUS.to_owned(), Value::from(STATUS)),
        (MICROPHONE_ON.to_owned(), Value::from(MICROPHONE)),
        (PRESENT.to_owned(), Value::from(asked.present.clone())),
    ]))
}
