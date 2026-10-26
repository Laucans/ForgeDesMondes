//! The Campaign aggregate in the product's tables (`campaign`,
//! `crates/data/migrations/0002_campaign.sql`, and a oneshot's `session`,
//! `0006_session.sql`), over PostgreSQL.
//!
//! A creation (`campaign.createCampaign`) writes its new partition,
//! `Campaign/<its id>`, whole: [`Stamps`] inserts its `campaign` row and a
//! oneshot's single `session`, in one statement, inside the transaction
//! that applies it — once: a campaign the table holds already is never
//! written over, and what the tables refuse rejects the command. At a start
//! it writes again every creation the engine applied and the tables lost,
//! each alone. Only a creation's partition is created so: a campaign the
//! fixtures wrote, that the engine holds since an export, is the table's.
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
//! An import (`campaign.importCampaign`) writes the partition of its copy,
//! `Campaign/<its new id>`, with the campaign's columns and the copy's
//! sections; [`Stamps`] writes them into the tables inside the transaction
//! that applies it, the campaign, its players, elements, media, sounds and
//! sessions with what hangs from them, once ([`import`]): a campaign the
//! table holds already is never written over, a row the tables refuse
//! rejects the command, and its `last_exported_at` is left to an export. At
//! a start it writes again every import the engine applied and the tables
//! lost, each alone.
//!
//! A database whose `campaign` table is not migrated yet holds no campaign:
//! nothing is stamped, nothing hydrated.

use std::collections::BTreeMap;

use async_trait::async_trait;
use harness_dataguard::plugins::{ProjectPg, ProjectionError};
use harness_dataguard::store::Write;
use harness_dataguard::tokio_postgres::{Client, Error, GenericClient, Transaction};
use serde_json::Value;

use crate::import;

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

/// The partitions a creation (`campaign.createCampaign`) wrote, live, that
/// the `campaign` table does not hold — the one `$1` names, every one when it
/// is null: a row under a uuid naming its own partition, which the engine's
/// queue says a creation was submitted on. Only then is `session` reached: a
/// database may hold campaigns and no session table.
const UNCREATED: &str = "SELECT r.partition FROM dataguard_rows r
    WHERE r.partition LIKE 'Campaign/%' AND NOT r.tombstoned
        AND ($1::text IS NULL OR r.partition = $1::text)
        AND pg_input_is_valid(r.fields ->> 'id', 'uuid')
        AND r.partition = 'Campaign/' || lower(r.fields ->> 'id')
        AND EXISTS (SELECT 1 FROM dataguard_queue q
            WHERE q.partition = r.partition AND q.data_capability = 'campaign.createCampaign')
        AND NOT EXISTS (SELECT 1 FROM campaign c WHERE 'Campaign/' || c.id::text = r.partition)
    ORDER BY r.partition";

/// The campaign of the partition `$1` names — one [`UNCREATED`] answers —
/// written on its row, with the session a oneshot was created with, in one
/// statement: the session only with the campaign this statement inserts, so
/// each is written once, and a session deleted since never comes back
/// (`oneshot-single-session`). A row the tables cannot hold — a session's id
/// that is no uuid, a title that is none — fails the statement.
const CREATE_CAMPAIGN: &str = "WITH created AS (
        INSERT INTO campaign (id, name, kind, pitch, art, glyph, created_at, sound_master_volume)
        SELECT (r.fields ->> 'id')::uuid, r.fields ->> 'name', r.fields ->> 'kind',
            r.fields ->> 'pitch', r.fields ->> 'art', r.fields ->> 'glyph',
            CASE WHEN pg_input_is_valid(r.fields ->> 'createdAt', 'timestamptz')
                THEN (r.fields ->> 'createdAt')::timestamptz ELSE now() END,
            coalesce((r.fields ->> 'soundMasterVolume')::numeric, 1)
        FROM dataguard_rows r WHERE r.partition = $1::text AND NOT r.tombstoned
        RETURNING id
    )
    INSERT INTO session (id, campaign_id, position, title, status, microphone_on)
    SELECT (r.fields ->> 'sessionId')::uuid, c.id,
        (r.fields ->> 'sessionPosition')::integer, r.fields ->> 'sessionTitle',
        r.fields ->> 'sessionStatus', (r.fields ->> 'microphoneOn')::boolean
    FROM dataguard_rows r JOIN created c ON r.partition = 'Campaign/' || c.id::text
    WHERE r.fields ? 'sessionId'";

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

/// The aggregate's projection: the creations' campaigns and sessions, the
/// imports' copies, the exports' stamps, and the fixtures' campaigns
/// hydrated.
pub struct Stamps;

/// Whether `client`'s database has the `campaign` table.
async fn migrated(client: &impl GenericClient) -> Result<bool, ProjectionError> {
    Ok(client.query_one(MIGRATED, &[]).await?.get(0))
}

#[async_trait]
impl ProjectPg for Stamps {
    /// The campaign a creation wrote, with its oneshot's session, or the
    /// copy an import wrote, then the stamp of the campaign written, on its
    /// row; a deletion creates, copies and stamps nothing (`UNCREATED`,
    /// `UNIMPORTED` and `STAMP_EXPORTS` skip a tombstone).
    async fn project(&self, write: &Write, tx: &Transaction<'_>) -> Result<(), ProjectionError> {
        if !migrated(tx).await? {
            return Ok(());
        }
        let partition = Some(write.partition.as_str());
        if !tx.query(UNCREATED, &[&partition]).await?.is_empty() {
            tx.execute(CREATE_CAMPAIGN, &[&write.partition])
                .await
                .map_err(judged)?;
        }
        for row in tx.query(import::UNIMPORTED, &[&partition]).await? {
            import::copy(tx, &row.get::<_, Value>(1))
                .await
                .map_err(judged)?;
        }
        tx.execute(STAMP_EXPORTS, &[&partition]).await?;
        Ok(())
    }

    /// Every creation and every import the tables lost, each alone, and
    /// every stamp the table lost, written again: one whose write failed
    /// before this projection ran in the apply's transaction. A creation
    /// the tables refuse is logged by its SQLSTATE and its constraint, and
    /// keeps no other from its row; an import is written in a transaction
    /// of its own, whole or not at all.
    async fn warm(&self, client: &Client) -> Result<(), ProjectionError> {
        if !migrated(client).await? {
            return Ok(());
        }
        for row in client.query(UNCREATED, &[&None::<&str>]).await? {
            let partition: String = row.get(0);
            if let Err(e) = client.execute(CREATE_CAMPAIGN, &[&partition]).await {
                let db = e.as_db_error();
                eprintln!(
                    "projection: {partition} not created: {} {}",
                    db.map_or("-", |db| db.code().code()),
                    db.and_then(|db| db.constraint()).unwrap_or_default()
                );
            }
        }
        for row in client.query(import::UNIMPORTED, &[&None::<&str>]).await? {
            let partition: String = row.get(0);
            if let Err(e) = copied(client, &row.get::<_, Value>(1)).await {
                let db = e.as_db_error();
                eprintln!(
                    "projection: {partition} not imported: {} {}",
                    db.map_or("-", |db| db.code().code()),
                    db.and_then(|db| db.constraint()).unwrap_or_default()
                );
            }
        }
        client.execute(STAMP_EXPORTS, &[&None::<&str>]).await?;
        Ok(())
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

/// An import's copy written in a transaction of its own, rolled back
/// whole when the tables refuse any of it.
async fn copied(client: &Client, row: &Value) -> Result<(), Error> {
    client.batch_execute("BEGIN").await?;
    match import::copy(client, row).await {
        Ok(()) => client.batch_execute("COMMIT").await,
        Err(e) => {
            client.batch_execute("ROLLBACK").await?;
            Err(e)
        }
    }
}

/// What the tables refuse — a CHECK, a foreign key, a NOT NULL, of class 23,
/// or a value they cannot hold, of class 22 — rejects the command; any other
/// error fails the step.
fn judged(error: Error) -> ProjectionError {
    match error.as_db_error() {
        Some(db)
            if ["22", "23"]
                .iter()
                .any(|class| db.code().code().starts_with(class)) =>
        {
            ProjectionError::Refused(db.message().to_owned())
        }
        _ => error.into(),
    }
}
