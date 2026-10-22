//! DataCapability `soundbox.playSound` (`data-capability.json`): the DM
//! triggers a sound — from the soundbox, or from a suggestion's pastille —
//! and a playback of it starts on the DM view, layered over whatever plays
//! already.
//!
//! The engine writes one partition per command and judges a row alone: a
//! playback is its own row, `SoundPlayback/<its id>`, a fresh id each time,
//! which the SoundPlayback aggregate's judge holds to its invariants. The
//! sound is the command's target; what one row cannot say — the sound's
//! campaign, the session it plays in — is read before the playback is
//! queued ([`Stage`]); [`prepare`] turns the request and that stage into the
//! playback's row:
//!
//! - a new row, with no `stoppedAt`: it plays, and stops none of the others
//!   (`simultaneous-playback`);
//! - its origin is the DM view's surface it was triggered from: `soundbox`
//!   by default, `pastille` for a suggestion's; any other source, the player
//!   view's, is carried as it is, for the judge to reject (`dm-side-only`);
//!   and a caller that is not the DM is the player side, whatever source it
//!   declares;
//! - its session is the one the payload names, of the sound's campaign —
//!   else the campaign's live session, else its latest one not closed,
//!   else its latest;
//! - what the tables could never hold, or a sound with no session to play
//!   in, is refused before anything is queued ([`Refusal`]).
//!
//! Once applied, the playback lands in `sound_playback`
//! (`crates/data/migrations/0009_sound_playback.sql`) in the transaction
//! that applies it ([`Playbacks`]): the table's foreign keys and its CHECK
//! judge it there again.

use std::collections::BTreeMap;

use async_trait::async_trait;
use harness_dataguard::plugins::{self, Context, Plugin, Prepare, ProjectPg, ProjectionError};
use harness_dataguard::reads::Rows;
use harness_dataguard::store::Write;
use harness_dataguard::tokio_postgres::{self, Transaction};
use serde_json::{Value, json};

/// This DataCapability's id, as `data-capability.json` names it.
pub const DATA_CAPABILITY: &str = "soundbox.playSound";

/// The aggregate a playback is a row of: its target.
const AGGREGATE: &str = "SoundPlayback";

/// The origin of a sound triggered from the soundbox: the default.
const SOUNDBOX: &str = "soundbox";

/// The source a suggestion's pastille sends.
const SUGGESTION: &str = "suggestion";

/// The origin the table and the aggregate know the pastille's by.
const PASTILLE: &str = "pastille";

/// The author of a command from the DM view.
const DM: &str = "dm";

/// The origin of a trigger whose caller is not the DM: the player side's.
const PLAYER: &str = "player";

/// Why a trigger is refused when the engine's fresh ids are no uuids: the
/// table could never hold its playback.
const NO_UUID: &str = "les identifiants du moteur ne sont pas des uuid";

/// One partition's row, as the engine holds it.
pub type Row = BTreeMap<String, Value>;

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

/// What the engine and the tables hold of the sound a trigger names.
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

/// The id and the instant a new playback is given.
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
    /// No such sound.
    UnknownSound,
    /// No such session.
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

/// What the DataCapability plugs into the engine: its `Prepare`, and the
/// projection of a new playback into `sound_playback`.
#[must_use]
pub fn plugin() -> Plugin {
    Plugin::new()
        .prepare(DATA_CAPABILITY, Plays)
        .project_pg(AGGREGATE, Playbacks)
}

/// A trigger made the playback it starts, on a partition of its own: one
/// naming a sound or a session there is not is refused, 404; one the tables
/// could not hold, or of a sound with no session to play in, 422 — neither
/// is queued.
struct Plays;

#[async_trait]
impl Prepare for Plays {
    async fn prepare(
        &self,
        context: &Context<'_>,
        payload: &BTreeMap<String, Value>,
        target_id: Option<&str>,
    ) -> Result<plugins::Prepared, plugins::Refusal> {
        let request = request(payload, target_id, &context.caller.by).map_err(refused)?;
        let stage = staged(context, &request).await?;
        let id =
            uuid(&context.ids.fresh()).ok_or_else(|| plugins::Refusal::unprocessable(NO_UUID))?;
        let now = stamp(context.now_millis);
        let row = prepare(&request, &stage, Fresh { id: &id, now: &now }).map_err(refused)?;
        Ok(plugins::Prepared {
            payload: row,
            target_id: Some(id),
            ..plugins::Prepared::default()
        })
    }
}

/// A refusal as the server answers it: a sound or a session there is not
/// is not found, anything else unprocessable.
fn refused(refusal: Refusal) -> plugins::Refusal {
    match refusal {
        Refusal::UnknownSound | Refusal::UnknownSession => {
            plugins::Refusal::not_found(refusal.to_string())
        }
        _ => plugins::Refusal::unprocessable(refusal.to_string()),
    }
}

/// What the tables hold of the sound `$1` — its campaign, unless the engine
/// says it (`$2`) — the session that campaign plays in — its live one, else
/// its latest not closed, the highest position, else its latest closed —
/// and the campaign of the session `$3` names, if any.
const STAGE: &str = "SELECT c.campaign,
        (SELECT x.id::text FROM session x WHERE x.campaign_id = c.campaign::uuid
            ORDER BY x.status = 'live' DESC, x.status <> 'closed' DESC, x.position DESC, x.id
            LIMIT 1) AS current,
        (SELECT x.campaign_id::text FROM session x WHERE x.id = $3::text::uuid) AS named
    FROM (SELECT coalesce($2::text,
        (SELECT s.campaign_id::text FROM sound s WHERE s.id = $1::text::uuid)) AS campaign) AS c";

/// What `request` is judged against: the sound's and the named session's
/// rows as the queue leaves them, and — over PostgreSQL — what the tables
/// hold under them, the fixtures' among them, and the session the
/// campaign plays in, which only the tables can tell.
async fn staged(context: &Context<'_>, request: &Request) -> Result<Stage, plugins::Refusal> {
    let sound = context.row(&format!("Sound/{}", request.sound)).await?;
    // A campaign the tables could not hold is none: they are asked instead.
    let campaign = text(&sound, "campaignId").and_then(|campaign| uuid(&campaign));
    let named = match &request.session {
        Some(id) => text(
            &context.row(&format!("Session/{id}")).await?,
            "sessionCampaignId",
        ),
        None => None,
    };
    let tables = match context.rows {
        Some(rows) => tables(rows, request, campaign.as_deref())
            .await
            .map_err(|e| plugins::Refusal::unprocessable(e.to_string()))?,
        None => Stage::default(),
    };
    Ok(Stage {
        campaign: campaign.or(tables.campaign),
        current: tables.current,
        named: named.or(tables.named),
    })
}

/// [`STAGE`] over `rows`, the sound's campaign `campaign` when the engine
/// knows it.
async fn tables(
    rows: &dyn Rows,
    request: &Request,
    campaign: Option<&str>,
) -> harness_dataguard::Result<Stage> {
    let found = rows
        .rows(
            STAGE,
            &[
                json!(request.sound),
                json!(campaign),
                json!(request.session),
            ],
        )
        .await?;
    let row: Row = found
        .as_array()
        .and_then(|found| found.first())
        .and_then(Value::as_object)
        .map(|row| row.clone().into_iter().collect())
        .unwrap_or_default();
    Ok(Stage {
        campaign: text(&row, "campaign"),
        current: text(&row, "current"),
        named: text(&row, "named"),
    })
}

/// `field` of `row` as lowercase text; `None` when it holds none.
fn text(row: &Row, field: &str) -> Option<String> {
    row.get(field)
        .and_then(Value::as_str)
        .map(str::to_ascii_lowercase)
}

/// The trigger `wire` asks for on `target`, the sound the command names,
/// from the caller `by`: one that is not the DM gets the player side's
/// origin, whatever its source.
///
/// # Errors
///
/// [`Refusal::NotAnId`] for a sound, or a session named, that is no uuid.
pub fn request(wire: &Row, target: Option<&str>, by: &str) -> Result<Request, Refusal> {
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
        _ if by != DM => json!(PLAYER),
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

/// A playback the engine applied on `$1`'s partition, inserted unless the
/// table holds it already: a later write of it (a stop) is not this
/// projection's — nor is it read, so a stop of a playback only the tables
/// hold, whose write says nothing but its stop, writes nothing here.
const PLAYBACK: &str = "INSERT INTO sound_playback
        (id, session_id, sound_id, origin, started_at, stopped_at)
    SELECT $1::text::uuid, $2::text::uuid, $3::text::uuid, $4::text,
        $5::text::timestamptz, $6::text::timestamptz
    WHERE NOT EXISTS (SELECT FROM sound_playback p WHERE p.id = $1::text::uuid)";

/// The SoundPlayback aggregate's projection of a new playback into
/// `sound_playback`, in the transaction that applies it.
struct Playbacks;

#[async_trait]
impl ProjectPg for Playbacks {
    /// A playback the table does not hold yet, inserted; a tombstone, or a
    /// partition whose id is no uuid, writes nothing. A row the table
    /// refuses — a sound or a session gone, an origin it does not know —
    /// rejects the command.
    async fn project(&self, write: &Write, tx: &Transaction<'_>) -> Result<(), ProjectionError> {
        let id = write
            .partition
            .split_once('/')
            .map_or(write.partition.as_str(), |(_, id)| id);
        if write.tombstone || uuid(id).is_none() {
            return Ok(());
        }
        let text = |field: &str| write.values.get(field).and_then(Value::as_str);
        tx.execute(
            PLAYBACK,
            &[
                &id,
                &text("sessionId"),
                &text("soundId"),
                &text("origin"),
                &text("startedAt"),
                &text("stoppedAt"),
            ],
        )
        .await
        .map_err(projection_error)?;
        Ok(())
    }
}

/// A write the table refuses — its integrity (class 23) or a value it cannot
/// read (class 22) — is the command's fault: refused. Anything else is the
/// store's.
fn projection_error(error: tokio_postgres::Error) -> ProjectionError {
    match error.as_db_error() {
        Some(db) if db.code().code().starts_with("23") || db.code().code().starts_with("22") => {
            ProjectionError::Refused(format!("sound_playback: {}", db.message()))
        }
        _ => ProjectionError::from(error),
    }
}

/// `text` as a uuid, lowercase as the tables write it; `None` when it is no
/// uuid.
fn uuid(text: &str) -> Option<String> {
    let shaped = text.len() == 36
        && text.char_indices().all(|(at, c)| match at {
            8 | 13 | 18 | 23 => c == '-',
            _ => c.is_ascii_hexdigit(),
        });
    shaped.then(|| text.to_ascii_lowercase())
}

/// `millis` since the epoch as RFC 3339 in UTC, to the millisecond
/// (`2026-10-21T09:00:00.123Z`): what `sound_playback.started_at` takes.
#[must_use]
pub fn stamp(millis: u64) -> String {
    let unix_seconds = millis / 1000;
    // Howard Hinnant's civil_from_days.
    // A day count since 1970 is far below `i64::MAX`.
    let days = (unix_seconds / 86_400).cast_signed() + 719_468;
    let seconds = unix_seconds % 86_400;
    let era = days.div_euclid(146_097);
    let doe = days.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{:03}Z",
        seconds / 3600,
        seconds % 3600 / 60,
        seconds % 60,
        millis % 1000
    )
}

#[cfg(test)]
mod tests {
    use super::{Refusal, Row, request, stamp};

    #[test]
    fn an_instant_is_written_as_rfc_3339_to_the_millisecond() {
        for (millis, written) in [
            (0, "1970-01-01T00:00:00.000Z"),
            (951_827_696_007, "2000-02-29T12:34:56.007Z"),
            (4_107_542_399_999, "2100-02-28T23:59:59.999Z"),
            (1_735_689_599_123, "2024-12-31T23:59:59.123Z"),
        ] {
            assert_eq!(stamp(millis), written);
        }
    }

    #[test]
    fn every_day_from_1970_to_2400_is_the_calendar_s() {
        let leap = |year: u64| {
            year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400))
        };
        let (mut year, mut month, mut day) = (1970_u64, 1_u64, 1_u64);
        for days in 0..157_000_u64 {
            let at = (days * 86_400 + 3_723) * 1000;
            assert_eq!(
                stamp(at),
                format!("{year:04}-{month:02}-{day:02}T01:02:03.000Z"),
                "{days} days after 1970"
            );
            let length = match month {
                2 if leap(year) => 29,
                2 => 28,
                4 | 6 | 9 | 11 => 30,
                _ => 31,
            };
            day += 1;
            if day > length {
                (day, month) = (1, month + 1);
            }
            if month > 12 {
                (month, year) = (1, year + 1);
            }
        }
    }

    #[test]
    fn a_sound_named_by_anything_but_a_uuid_is_refused() {
        for sound in [
            "abc",
            "50000000-0000-4000-8000-00000000000g",
            "50000000-0000-4000-8000-0000000000000",
            "500000000000-4000-8000-0000-00000000",
        ] {
            assert_eq!(
                request(&Row::new(), Some(sound), "dm"),
                Err(Refusal::NotAnId("sound")),
                "{sound}"
            );
        }
        assert_eq!(
            request(&Row::new(), None, "dm"),
            Err(Refusal::NotAnId("sound"))
        );
    }
}
