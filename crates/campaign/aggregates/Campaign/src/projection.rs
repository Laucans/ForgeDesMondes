//! The Campaign aggregate in the product's tables (`campaign`,
//! `crates/data/migrations/0002_campaign.sql`), over PostgreSQL.
//!
//! An export (`campaign.exportCampaign`) writes its campaign's partition,
//! `Campaign/<its id>`, with the stamp alone: [`Stamps`] writes
//! `campaign.last_exported_at` from it inside the transaction that applies
//! it, and no other column (`export-stamps-date`) — never moved back, and a
//! stamp that is no time left aside. At a start it writes again every stamp
//! the engine holds and the table lost. A campaign the fixtures wrote into
//! the table, which no command wrote yet, is the engine's row as the table
//! holds it ([`Stamps`]' `hydrate`): what an export is judged against.
//!
//! A database whose `campaign` table is not migrated yet holds no campaign:
//! nothing is stamped, nothing hydrated.

use std::collections::BTreeMap;

use async_trait::async_trait;
use harness_dataguard::plugins::{ProjectPg, ProjectionError};
use harness_dataguard::store::Write;
use harness_dataguard::tokio_postgres::{Client, GenericClient, Transaction};
use serde_json::Value;

/// The partitions of the aggregate: `Campaign/<campaign id>`.
const PARTITION: &str = "Campaign/";

/// Whether the `campaign` table exists: a database not migrated yet holds
/// none.
const MIGRATED: &str = "SELECT to_regclass('campaign') IS NOT NULL";

/// A campaign as its table holds it, under the aggregate's fields, `$1` its
/// id in whatever case: what the engine judges an export against when it
/// never wrote the campaign itself — the fixtures' campaigns.
const CAMPAIGN_ROW: &str = "SELECT jsonb_strip_nulls(jsonb_build_object(
        'id', id, 'name', name, 'kind', kind, 'pitch', pitch, 'art', art, 'glyph', glyph,
        'coverMediaId', cover_media_id, 'createdAt', created_at,
        'lastExportedAt', last_exported_at, 'importedAt', imported_at,
        'originCampaignId', origin_campaign_id, 'soundMasterVolume', sound_master_volume
    ))
    FROM campaign c WHERE c.id::text = lower($1)";

/// The last export the engine stamped on the campaign partition `$1` names —
/// on every one when it is null — written on its row: the one column an
/// export changes (`export-stamps-date`), never moved back. A stamp that is
/// no time is left aside, so it keeps no other campaign's from its row.
const STAMP_EXPORTS: &str = "UPDATE campaign c
    SET last_exported_at = (r.fields ->> 'lastExportedAt')::timestamptz
    FROM dataguard_rows r
    WHERE r.partition = 'Campaign/' || c.id::text AND NOT r.tombstoned
        AND ($1::text IS NULL OR r.partition = $1::text)
        AND pg_input_is_valid(r.fields ->> 'lastExportedAt', 'timestamptz')
        AND (c.last_exported_at IS NULL
            OR c.last_exported_at < (r.fields ->> 'lastExportedAt')::timestamptz)";

/// The aggregate's projection: the exports' stamps, and the fixtures'
/// campaigns hydrated.
pub struct Stamps;

/// Whether `client`'s database has the `campaign` table.
async fn migrated(client: &impl GenericClient) -> Result<bool, ProjectionError> {
    Ok(client.query_one(MIGRATED, &[]).await?.get(0))
}

/// Writes the stamps of the partition `partition` names — of every one,
/// `None` — on the campaigns' rows.
async fn stamp(
    client: &impl GenericClient,
    partition: Option<&str>,
) -> Result<(), ProjectionError> {
    if migrated(client).await? {
        client.execute(STAMP_EXPORTS, &[&partition]).await?;
    }
    Ok(())
}

#[async_trait]
impl ProjectPg for Stamps {
    /// The stamp of the campaign written, on its row; a deletion stamps
    /// nothing (`STAMP_EXPORTS` skips a tombstone).
    async fn project(&self, write: &Write, tx: &Transaction<'_>) -> Result<(), ProjectionError> {
        stamp(tx, Some(&write.partition)).await
    }

    /// Every stamp the table lost, written again: one whose write failed
    /// before this projection ran in the apply's transaction.
    async fn warm(&self, client: &Client) -> Result<(), ProjectionError> {
        stamp(client, None).await
    }

    async fn hydrate(
        &self,
        partition: &str,
        client: &Client,
    ) -> Result<Option<BTreeMap<String, Value>>, ProjectionError> {
        let Some(id) = partition.strip_prefix(PARTITION) else {
            return Ok(None);
        };
        if !migrated(client).await? {
            return Ok(None);
        }
        let Some(row) = client.query_opt(CAMPAIGN_ROW, &[&id]).await? else {
            return Ok(None);
        };
        Ok(match row.get::<_, Value>(0) {
            Value::Object(fields) => Some(fields.into_iter().collect()),
            _ => None,
        })
    }
}
