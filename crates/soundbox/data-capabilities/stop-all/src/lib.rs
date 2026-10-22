//! DataCapability `soundbox.stopAll` (`data-capability.json`): one click on
//! « Tout arrêter » silences everything playing. Every playback still
//! playing in the campaign — in the one session the DM names, or in any of
//! its sessions — gets its stop time; a playback stopped already keeps the
//! one it has (`stop-all`). Nothing starts, and no volume moves.
//!
//! The engine writes one partition per command, and a playback is one,
//! `SoundPlayback/<its id>`: a stop-all is one command on each playback
//! still playing, writing its `stoppedAt` and nothing else, which the
//! DataGuard's `stop-all` judges on the playback's own row. [`scope`] reads
//! which playbacks the DM's request names; [`stops`] turns those the tables
//! hold — each with its row as the queue will leave it — into the commands:
//!
//! - a playback whose row says it stopped, or will once a command queued
//!   ahead of it is applied, is left out: a second click stops nothing twice;
//! - with no playback still playing, the one command queued writes nothing,
//!   on `SoundPlayback/<the campaign>`, which no playback is: the request
//!   succeeds and changes no row;
//! - a request naming no campaign, or a campaign or a session that is no
//!   uuid, is refused before anything is queued, 422 ([`Refusal`]).
//!
//! The engine runs it through this crate's [`plugin`]: the first playback's
//! command is the one submitted — its idempotency key, its result — and the
//! `Prepare` asks the engine for the others after it (`Prepared.then`). It
//! is relative (`mode: relative`): it composes with what is queued ahead
//! and never asks its author to confirm.
//!
//! Over PostgreSQL, the plugin also projects each stop into the
//! `sound_playback` table ([`StoppedAt`]): the SoundPlayback aggregate
//! projects nothing, and the soundbox reads what plays there.

use std::collections::BTreeMap;
use std::sync::Mutex;

use async_trait::async_trait;
use harness_dataguard::api::Submission;
use harness_dataguard::plugins::{self, Context, Plugin, Prepare, ProjectPg, ProjectionError};
use harness_dataguard::store::Write;
use harness_dataguard::tokio_postgres::{Client, GenericClient, Transaction};
use serde_json::Value;

/// This DataCapability's id, as `data-capability.json` names it.
pub const DATA_CAPABILITY: &str = "soundbox.stopAll";

/// The aggregate it writes.
const AGGREGATE: &str = "SoundPlayback";

/// The one field a stop writes: when the playback stopped.
pub const STOPPED: &str = "stoppedAt";

/// The playbacks still playing in the campaign `$1` — in its session `$2`
/// only, when it is not null — by the order they started in.
pub const RUNNING: &str = "SELECT p.id::text AS id FROM sound_playback p
    JOIN session s ON s.id = p.session_id
    WHERE s.campaign_id = $1::text::uuid
        AND ($2::text IS NULL OR p.session_id = $2::text::uuid)
        AND p.stopped_at IS NULL
    ORDER BY p.started_at, p.id";

/// Whether the `sound_playback` table exists: a database not migrated yet
/// holds none.
const MIGRATED: &str = "SELECT to_regclass('sound_playback') IS NOT NULL";

/// The stop the engine holds on the playback `$1` — on every one when it is
/// null — written on its row when it has none: after each apply of a
/// playback, and at a start, so a stop whose projection failed after its
/// apply is written again. A playback stopped keeps the instant it stopped
/// at (`stop-all`); a stop that is no time is left aside.
pub const PROJECT: &str = "UPDATE sound_playback p
    SET stopped_at = (r.fields ->> 'stoppedAt')::timestamptz
    FROM dataguard_rows r
    WHERE r.partition = 'SoundPlayback/' || p.id::text AND NOT r.tombstoned
        AND ($1::text IS NULL OR p.id = $1::text::uuid)
        AND pg_input_is_valid(r.fields ->> 'stoppedAt', 'timestamptz')
        AND p.stopped_at IS NULL";

/// How long a playback whose stop was prepared counts as stopping, in
/// milliseconds: what separates a command's `Prepare` from its queueing,
/// and then some. Past it, the queue says whether it stops.
pub const STOPPING_MILLIS: u64 = 5_000;

/// One partition's row, as the engine holds it.
pub type Row = BTreeMap<String, Value>;

/// Which playbacks a stop-all names: the campaign's, of one of its sessions
/// when it names one — each id lowercase, as the tables write it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Scope {
    /// The campaign.
    pub campaign: String,
    /// The session, when only its playbacks stop.
    pub session: Option<String>,
}

/// Why a stop-all is refused before it is queued.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// The request names no campaign, or one that is no uuid.
    NoCampaign,
    /// The session it names is no uuid.
    NotSession,
}

impl From<Refusal> for plugins::Refusal {
    /// A request the tables could not read: 422.
    fn from(refusal: Refusal) -> Self {
        Self::unprocessable(refusal.to_string())
    }
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoCampaign => f.write_str("« Tout arrêter » nomme sa campagne"),
            Self::NotSession => f.write_str("« Tout arrêter » ne reconnaît pas cette séance"),
        }
    }
}

/// One command a stop-all queues: the playback it stops, and what it writes
/// there.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stop {
    /// The playback's id: the command's target, its partition
    /// `SoundPlayback/<id>`.
    pub target_id: String,
    /// The fields written: its stop time alone.
    pub payload: Row,
}

/// What the DataCapability plugs into the engine: its `Prepare`, and over
/// PostgreSQL the stops' projection into the `sound_playback` table.
#[must_use]
pub fn plugin() -> Plugin {
    Plugin::new()
        .prepare(DATA_CAPABILITY, Stopping::default())
        .project_pg(AGGREGATE, StoppedAt)
}

/// The stop-all's `Prepare`: the playbacks still playing made commands, the
/// first submitted, the others announced as its follow-ups.
///
/// Stop-alls are prepared one at a time (`turn`), and a playback whose stop
/// was prepared counts as stopping for [`STOPPING_MILLIS`] ([`Announced`]),
/// queued or not yet: two clicks at once never queue two stops on one
/// playback.
#[derive(Default)]
struct Stopping {
    announced: Announced,
    turn: tokio::sync::Mutex<()>,
}

#[async_trait]
impl Prepare for Stopping {
    async fn prepare(
        &self,
        context: &Context<'_>,
        payload: &Row,
        target_id: Option<&str>,
    ) -> Result<plugins::Prepared, plugins::Refusal> {
        if let Some(stop) = self.announced.take(target_id, payload) {
            return Ok(plugins::Prepared {
                payload: stop,
                target_id: target_id.map(str::to_owned),
                ..plugins::Prepared::default()
            });
        }
        let scope = scope(payload, target_id)?;
        let now = context.now_millis;
        let _turn = self.turn.lock().await;
        let mut running = Vec::new();
        for id in playing(context, &scope).await? {
            if self.announced.holds(&id, now) {
                continue;
            }
            let row = context.row(&format!("{AGGREGATE}/{id}")).await?;
            running.push((id, row));
        }
        let mut stops = stops(&running, &timestamp(now)).into_iter();
        let Some(first) = stops.next() else {
            return Ok(plugins::Prepared {
                target_id: Some(scope.campaign),
                ..plugins::Prepared::default()
            });
        };
        self.announced.claim(&first.target_id, now);
        Ok(plugins::Prepared {
            payload: first.payload,
            target_id: Some(first.target_id),
            then: stops
                .map(|stop| self.announced.follow_up(&stop.target_id, stop.payload, now))
                .collect(),
            ..plugins::Prepared::default()
        })
    }
}

/// The ids of the playbacks the tables say still play in `scope`, by the
/// order they started in — none over the engine's memory, which has no
/// table.
async fn playing(context: &Context<'_>, scope: &Scope) -> Result<Vec<String>, plugins::Refusal> {
    let Some(rows) = context.rows else {
        return Ok(Vec::new());
    };
    let found = rows
        .rows(
            RUNNING,
            &[
                Value::from(scope.campaign.as_str()),
                scope.session.as_deref().map_or(Value::Null, Value::from),
            ],
        )
        .await
        .map_err(|error| plugins::Refusal::unprocessable(error.to_string()))?;
    Ok(found
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|row| row["id"].as_str().map(str::to_owned))
        .collect())
}

/// The playbacks whose stop a stop-all prepared, by id, each for
/// [`STOPPING_MILLIS`] from then: the first one, submitted, and the others,
/// announced as its follow-ups until each is taken back. A follow-up is
/// submitted with an empty payload — what the author sent was the first
/// command's — and names the playback it stops; the `Prepare` finds there
/// what it announced, and writes it as it is. A request of the author with
/// no payload names no playback announced, so it is never mistaken for one.
#[derive(Default)]
struct Announced {
    stops: Mutex<BTreeMap<String, Claim>>,
}

/// A playback's stop, prepared at `at`; its payload until its follow-up
/// takes it back — none for the command submitted.
struct Claim {
    payload: Option<Row>,
    at: u64,
}

impl Announced {
    /// The playback `id` claimed at `now` with what its follow-up writes, if
    /// any; claims past [`STOPPING_MILLIS`] forgotten.
    fn record(&self, id: &str, payload: Option<Row>, now: u64) {
        let mut stops = self
            .stops
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        stops.retain(|_, claim| fresh(claim, now));
        stops.insert(id.to_owned(), Claim { payload, at: now });
    }

    /// The stop of the playback `id`, submitted at `now`, claimed.
    fn claim(&self, id: &str, now: u64) {
        self.record(id, None, now);
    }

    /// The follow-up writing `payload` on the playback `target_id`,
    /// announced at `now`: the submission the `Prepare` hands the engine.
    fn follow_up(&self, target_id: &str, payload: Row, now: u64) -> Submission {
        self.record(target_id, Some(payload), now);
        Submission {
            data_capability: DATA_CAPABILITY.to_owned(),
            target_id: Some(target_id.to_owned()),
            ..Submission::default()
        }
    }

    /// What the follow-up submitted with `payload` on `target_id` writes,
    /// taken back once — the playback still claimed: `None` for a
    /// submission that is no follow-up announced.
    fn take(&self, target_id: Option<&str>, payload: &Row) -> Option<Row> {
        let target_id = target_id.filter(|_| payload.is_empty())?;
        self.stops
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get_mut(target_id)?
            .payload
            .take()
    }

    /// Whether the stop of the playback `id` was prepared less than
    /// [`STOPPING_MILLIS`] before `now`.
    fn holds(&self, id: &str, now: u64) -> bool {
        self.stops
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(id)
            .is_some_and(|claim| fresh(claim, now))
    }
}

/// Whether `claim` was made less than [`STOPPING_MILLIS`] before `now`.
fn fresh(claim: &Claim, now: u64) -> bool {
    now < claim.at + STOPPING_MILLIS
}

/// Whether `id` is a uuid, in whatever case: what the tables can read.
fn uuid(id: &str) -> bool {
    let groups: Vec<&str> = id.split('-').collect();
    groups.len() == 5
        && groups
            .iter()
            .zip([8, 4, 4, 4, 12])
            .all(|(group, len)| group.len() == len && group.chars().all(|c| c.is_ascii_hexdigit()))
}

/// The id `value` holds, trimmed and lowercase: `None` when it holds none,
/// `Some(Err)` when it is no uuid.
fn id_in(value: Option<&Value>) -> Option<Result<String, ()>> {
    let id = match value? {
        Value::Null => return None,
        Value::String(id) if id.trim().is_empty() => return None,
        Value::String(id) => id.trim(),
        _ => return Some(Err(())),
    };
    Some(if uuid(id) {
        Ok(id.to_ascii_lowercase())
    } else {
        Err(())
    })
}

/// The playbacks `wire` names: those of its `campaign` — or of the command's
/// `target`, as a Micro-UI names it — and of its `session` only when it
/// names one.
///
/// # Errors
///
/// [`Refusal::NoCampaign`] when neither names a campaign, or names one that
/// is no uuid; [`Refusal::NotSession`] when the session is no uuid.
pub fn scope(wire: &Row, target: Option<&str>) -> Result<Scope, Refusal> {
    let target = target.map(|id| Value::String(id.to_owned()));
    let campaign = id_in(wire.get("campaign"))
        .or_else(|| id_in(target.as_ref()))
        .ok_or(Refusal::NoCampaign)?
        .map_err(|()| Refusal::NoCampaign)?;
    let session = id_in(wire.get("session"))
        .transpose()
        .map_err(|()| Refusal::NotSession)?;
    Ok(Scope { campaign, session })
}

/// Whether the playback whose row is `row` still plays: it has no stop time.
#[must_use]
pub fn plays(row: &Row) -> bool {
    row.get(STOPPED).is_none_or(Value::is_null)
}

/// The commands stopping, at `now`, each playback of `running` — its id and
/// its row as the commands queued on it will leave it — that still plays
/// then. None when none does.
#[must_use]
pub fn stops(running: &[(String, Row)], now: &str) -> Vec<Stop> {
    running
        .iter()
        .filter(|(_, row)| plays(row))
        .map(|(id, _)| Stop {
            target_id: id.to_ascii_lowercase(),
            payload: Row::from([(STOPPED.to_owned(), Value::from(now))]),
        })
        .collect()
}

/// `millis` since the epoch as RFC 3339 in UTC, to the millisecond
/// (`2026-10-21T09:00:00.123Z`): what `sound_playback.stopped_at` takes.
#[must_use]
pub fn timestamp(millis: u64) -> String {
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

/// The stops in the `sound_playback` table: written from the engine's row
/// after each apply of a playback, and again at a start for every one the
/// table lost.
pub struct StoppedAt;

/// Writes the stop of the playback `id` — of every one, `None` — on the
/// playbacks' rows, when the table exists.
async fn write_stops(client: &impl GenericClient, id: Option<&str>) -> Result<(), ProjectionError> {
    if client.query_one(MIGRATED, &[]).await?.get(0) {
        client.execute(PROJECT, &[&id]).await?;
    }
    Ok(())
}

#[async_trait]
impl ProjectPg for StoppedAt {
    /// The stop of the playback written, on its row; a deletion, or a
    /// write on a partition that names no playback, writes nothing.
    async fn project(&self, write: &Write, tx: &Transaction<'_>) -> Result<(), ProjectionError> {
        match write.partition.split_once('/') {
            Some((_, id)) if uuid(id) => write_stops(tx, Some(id)).await,
            _ => Ok(()),
        }
    }

    /// Every stop the table lost, written again: one whose write failed
    /// before this projection ran in the apply's transaction.
    async fn warm(&self, client: &Client) -> Result<(), ProjectionError> {
        write_stops(client, None).await
    }
}

#[cfg(test)]
mod tests {
    use harness_dataguard::guard::Caller;
    use harness_dataguard::manifest::DataCapability;
    use harness_dataguard::memory::Counter;
    use harness_dataguard::plugins::{Partitions, Shared};
    use harness_dataguard::reads::Rows;
    use serde_json::json;

    use super::*;

    fn row(value: Value) -> Row {
        serde_json::from_value(value).expect("an object")
    }

    /// The tables, where `p1` and `p2` play; the queue, which holds no
    /// command on either.
    struct Tables;

    #[async_trait]
    impl Rows for Tables {
        async fn rows(
            &self,
            _sql: &str,
            _parameters: &[Value],
        ) -> harness_dataguard::Result<Value> {
            Ok(json!([{ "id": "p1" }, { "id": "p2" }]))
        }
    }

    #[async_trait]
    impl Partitions for Tables {
        async fn row(&self, _partition: &str) -> harness_dataguard::Result<Row> {
            Ok(Row::new())
        }
    }

    #[tokio::test]
    async fn a_playback_whose_stop_is_announced_and_not_yet_queued_is_not_stopped_again() {
        let manifest =
            DataCapability::parse(include_str!("../data-capability.json")).expect("contract F");
        let context = Context {
            manifest: &manifest,
            caller: &Caller::default(),
            base: &Row::new(),
            now: 0,
            now_millis: 0,
            ids: &Counter::default(),
            shared: &Shared::default(),
            partitions: &Tables,
            rows: Some(&Tables),
        };
        let stopping = Stopping::default();
        // A stop-all prepared before this one announced `p2`'s stop, whose
        // follow-up the engine has not submitted yet.
        stopping
            .announced
            .follow_up("p2", row(json!({ STOPPED: "earlier" })), 0);

        let prepared = stopping
            .prepare(
                &context,
                &row(json!({ "campaign": "c1000000-0000-4000-8000-000000000001" })),
                None,
            )
            .await
            .expect("prepared");

        assert_eq!(prepared.target_id.as_deref(), Some("p1"));
        assert!(prepared.then.is_empty(), "{:?}", prepared.then);
    }

    #[test]
    fn a_follow_up_names_its_playback_carries_no_payload_and_is_taken_back_once() {
        let announced = Announced::default();
        let stop = row(json!({ STOPPED: "2026-10-22T10:00:00.000Z" }));
        let submission = announced.follow_up("p1", stop.clone(), 1_000);
        assert_eq!(submission.data_capability, DATA_CAPABILITY);
        assert_eq!(submission.target_id.as_deref(), Some("p1"));
        assert!(submission.payload.is_empty());
        assert!(announced.holds("p1", 1_000));
        assert!(!announced.holds("p2", 1_000));

        // The author's own request, with a payload or naming no playback
        // announced, takes nothing.
        assert_eq!(
            announced.take(Some("p1"), &row(json!({ "campaign": "c" }))),
            None
        );
        assert_eq!(announced.take(None, &Row::new()), None);
        assert_eq!(announced.take(Some("p2"), &Row::new()), None);
        assert_eq!(announced.take(Some("p1"), &Row::new()), Some(stop));
        assert_eq!(announced.take(Some("p1"), &Row::new()), None, "once");
        // Taken back, its playback is stopping still: its command is
        // being queued.
        assert!(announced.holds("p1", 1_000));
    }

    #[test]
    fn a_playback_claimed_stops_stopping_once_the_queue_can_say_so() {
        let announced = Announced::default();
        announced.claim("p1", 1_000);
        announced.claim("p2", 3_000);

        // The command submitted carries its own payload: nothing to take.
        assert_eq!(announced.take(Some("p1"), &Row::new()), None);
        assert!(announced.holds("p1", 1_000 + STOPPING_MILLIS - 1));
        assert!(!announced.holds("p1", 1_000 + STOPPING_MILLIS));
        assert!(announced.holds("p2", 1_000 + STOPPING_MILLIS));

        // A follow-up announced and never submitted holds nothing for ever.
        announced.follow_up("p3", Row::new(), 3_000);
        assert!(!announced.holds("p3", 3_000 + STOPPING_MILLIS));
    }
}
