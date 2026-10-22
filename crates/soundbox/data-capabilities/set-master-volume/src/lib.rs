//! DataCapability `soundbox.setMasterVolume` (`data-capability.json`): the
//! DM moves the soundbox's master slider and every sound of the campaign
//! follows, keeping their ratio (EffectiveVolume@1). The slider speaks in
//! percent, from 0 to 100; the campaign stores a fraction from 0 to 1, the
//! percent divided by 100, as `soundMasterVolume` — one command on
//! `Campaign/<its id>`, which writes nothing else of it.
//!
//! [`prepare`] turns what the Micro-UI sends — `{campaign, volume}`, or the
//! volume alone on the command's target — into that command's payload and
//! target:
//!
//! - the target is the campaign, lowercase as the table writes it — one
//!   partition per campaign whatever case the request used;
//! - its payload is `soundMasterVolume`, the percent divided by 100, and
//!   nothing else — to the whole percent, the hundredths the campaign's
//!   column holds, so what the engine judges is what the table keeps;
//! - a volume out of range is never clamped nor rounded into range: its
//!   fraction is written as it is, and the campaign's own invariant refuses
//!   it (`master-volume-range`); a volume that is no number is written
//!   `null`, which the invariant refuses too;
//! - a request naming no campaign, or one the engine does not hold, is
//!   refused before anything is queued, 404 ([`Refusal`]).
//!
//! Over PostgreSQL, the plugin also projects the volume into the `campaign`
//! table ([`MasterVolumes`]): the Campaign aggregate's own projection writes
//! the exports' stamps alone, and the soundbox reads the volume there.

use std::collections::BTreeMap;

use async_trait::async_trait;
use harness_dataguard::plugins::{self, Context, Plugin, Prepare, ProjectPg, ProjectionError};
use harness_dataguard::store::Write;
use harness_dataguard::tokio_postgres::{Client, GenericClient, Transaction};
use serde_json::{Number, Value};

/// This DataCapability's id, as `data-capability.json` names it.
pub const DATA_CAPABILITY: &str = "soundbox.setMasterVolume";

/// The aggregate it writes.
const AGGREGATE: &str = "Campaign";

/// The one field it writes: the master volume, a fraction from 0 to 1.
pub const MASTER_VOLUME: &str = "soundMasterVolume";

/// What a percent is divided by to be stored.
const PERCENT: f64 = 100.0;

/// Whether the `campaign` table exists: a database not migrated yet holds
/// none.
const MIGRATED: &str = "SELECT to_regclass('campaign') IS NOT NULL";

/// The master volume the engine holds on the campaign partition `$1`
/// names — on every one when it is null — written on its row when it is not
/// that already: the one column a change of it writes. After each apply of a
/// campaign, and at a start, so a volume whose projection failed after its
/// apply is written again. A volume the column could not hold, or outside
/// `master-volume-range`, is left aside, so it keeps no other campaign's from
/// its row.
pub const MASTER_VOLUMES: &str = "UPDATE campaign c
    SET sound_master_volume = (r.fields ->> 'soundMasterVolume')::numeric(3, 2)
    FROM dataguard_rows r
    WHERE r.partition = 'Campaign/' || c.id::text AND NOT r.tombstoned
        AND ($1::text IS NULL OR r.partition = $1::text)
        AND pg_input_is_valid(r.fields ->> 'soundMasterVolume', 'numeric(3, 2)')
        AND (r.fields ->> 'soundMasterVolume')::numeric(3, 2) BETWEEN 0 AND 1
        AND c.sound_master_volume IS DISTINCT FROM
            (r.fields ->> 'soundMasterVolume')::numeric(3, 2)";

/// One partition's row, as the engine holds it.
pub type Row = BTreeMap<String, Value>;

/// Why a change of the master volume is refused before it is queued.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// The request names no campaign, or one the engine does not hold.
    NoCampaign,
}

impl From<Refusal> for plugins::Refusal {
    /// No campaign is a 404.
    fn from(refusal: Refusal) -> Self {
        Self::not_found(refusal.to_string())
    }
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoCampaign => f.write_str("cette campagne n'existe pas"),
        }
    }
}

/// What a change of the master volume becomes before it is submitted: what
/// the command writes on `Campaign/<target_id>`.
#[derive(Clone, Debug, PartialEq)]
pub struct Prepared {
    /// The fields written: the master volume alone.
    pub payload: Row,
    /// The campaign's id, lowercase: the command's target.
    pub target_id: String,
}

/// What the DataCapability plugs into the engine: its `Prepare`, and over
/// PostgreSQL the volume's projection into the `campaign` table.
#[must_use]
pub fn plugin() -> Plugin {
    Plugin::new()
        .prepare(DATA_CAPABILITY, Setting)
        .project_pg(AGGREGATE, MasterVolumes)
}

/// The change's `Prepare`: the percent made the campaign's fraction, on a
/// campaign the engine holds.
struct Setting;

#[async_trait]
impl Prepare for Setting {
    async fn prepare(
        &self,
        context: &Context<'_>,
        payload: &BTreeMap<String, Value>,
        target_id: Option<&str>,
    ) -> Result<plugins::Prepared, plugins::Refusal> {
        let prepared = prepare(payload, target_id)?;
        let held = context
            .row(&format!("{AGGREGATE}/{}", prepared.target_id))
            .await?;
        if held.is_empty() {
            return Err(Refusal::NoCampaign.into());
        }
        Ok(plugins::Prepared {
            payload: prepared.payload,
            target_id: Some(prepared.target_id),
            ..plugins::Prepared::default()
        })
    }
}

/// The change `wire` asks for — `{campaign, volume}`, the volume in percent
/// — on the campaign `target` names, else the one its `campaign` does.
///
/// # Errors
///
/// [`Refusal::NoCampaign`] when neither names a campaign.
pub fn prepare(wire: &Row, target: Option<&str>) -> Result<Prepared, Refusal> {
    let target_id = named(target)
        .or_else(|| named(wire.get("campaign").and_then(Value::as_str)))
        .ok_or(Refusal::NoCampaign)?
        .to_ascii_lowercase();
    let volume = wire
        .get("volume")
        .and_then(Value::as_f64)
        .map(|percent| {
            let percent = if (0.0..=PERCENT).contains(&percent) {
                percent.round()
            } else {
                percent
            };
            percent / PERCENT
        })
        .and_then(Number::from_f64)
        .map_or(Value::Null, Value::Number);
    Ok(Prepared {
        payload: Row::from([(MASTER_VOLUME.to_owned(), volume)]),
        target_id,
    })
}

/// `id`, trimmed, when it names something.
fn named(id: Option<&str>) -> Option<&str> {
    id.map(str::trim).filter(|id| !id.is_empty())
}

/// The master volume in the `campaign` table: written from the engine's row
/// after each apply of a campaign, and again at a start for every one the
/// table lost.
pub struct MasterVolumes;

/// Writes the volume of the partition `partition` names — of every one,
/// `None` — on the campaigns' rows, when the table exists.
async fn write_volumes(
    client: &impl GenericClient,
    partition: Option<&str>,
) -> Result<(), ProjectionError> {
    if client.query_one(MIGRATED, &[]).await?.get(0) {
        client.execute(MASTER_VOLUMES, &[&partition]).await?;
    }
    Ok(())
}

#[async_trait]
impl ProjectPg for MasterVolumes {
    /// The volume of the campaign written, on its row; a deletion writes
    /// nothing (`MASTER_VOLUMES` skips a tombstone).
    async fn project(&self, write: &Write, tx: &Transaction<'_>) -> Result<(), ProjectionError> {
        write_volumes(tx, Some(&write.partition)).await
    }

    /// Every volume the table lost, written again: one whose write failed
    /// before this projection ran in the apply's transaction.
    async fn warm(&self, client: &Client) -> Result<(), ProjectionError> {
        write_volumes(client, None).await
    }
}
