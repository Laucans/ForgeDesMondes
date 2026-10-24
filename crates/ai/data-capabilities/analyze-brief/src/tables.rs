//! A brief, its entries « à créer » and the media its scenes link in their
//! tables, `oral_brief`, `entity_to_create` and `scene_media`
//! (`crates/data/migrations/0006_session.sql`), written by [`BriefTables`]
//! inside the transaction that applies a write of the Session aggregate.
//!
//! Each is a partition of its own, `Session/<its id>`, whose row says what it
//! is: `oralBriefId` a brief, `entityToCreateId` an entry,
//! `sceneMediaSceneId` and `sceneMediaMediaId` a media linked to a scene.
//! Any other row of the aggregate — a session's, a scene's — writes nothing
//! here; the scenes are the aggregate's own projection's.
//!
//! - A brief is text alone (`text-only-no-audio`): its transcript, its
//!   duration and its dates.
//! - An entry is written as the analysis left it, waiting to be created
//!   (`brief-no-auto-creation`, judged by the aggregate before).
//! - A media is linked to a scene only when it belongs to the campaign of
//!   the scene's session (`scene-links-same-campaign`): any other rejects
//!   the command, and nothing of it is written.
//!
//! A row is written once: a later write of its partition — the whole row,
//! whatever the command changed — finds it there and leaves it. A
//! tombstone writes nothing.

use async_trait::async_trait;
use harness_dataguard::plugins::{ProjectPg, ProjectionError};
use harness_dataguard::store::Write;
use harness_dataguard::tokio_postgres::Error;
use harness_dataguard::tokio_postgres::Transaction;
use serde_json::Value;

/// The fields of a brief's row.
pub const ORAL_BRIEF_ID: &str = "oralBriefId";
pub const ORAL_BRIEF_SESSION_ID: &str = "oralBriefSessionId";
pub const TRANSCRIPT: &str = "transcript";
pub const DURATION_SECONDS: &str = "durationSeconds";
pub const RECORDED_AT: &str = "recordedAt";
pub const ANALYZED_AT: &str = "analyzedAt";

/// The fields of a media linked to a scene.
pub const SCENE_MEDIA_SCENE_ID: &str = "sceneMediaSceneId";
pub const SCENE_MEDIA_MEDIA_ID: &str = "sceneMediaMediaId";

/// The fields of an entry « à créer », beside `type`, `name` and `excerpt`.
pub const TO_CREATE_ID: &str = "entityToCreateId";
pub const TO_CREATE_CAMPAIGN_ID: &str = "entityToCreateCampaignId";
pub const TO_CREATE_SESSION_ID: &str = "entityToCreateSessionId";
pub const BRIEF_ID: &str = "briefId";
pub const TO_CREATE_SCENE_ID: &str = "entityToCreateSceneId";
pub const TO_CREATE_STATUS: &str = "entityToCreateStatus";
pub const CREATED_AT: &str = "createdAt";

/// The brief, unless the table holds it already.
const ADD_BRIEF: &str = "INSERT INTO oral_brief
        (id, session_id, transcript, duration_seconds, recorded_at, analyzed_at)
    VALUES ($1::text::uuid, $2::text::uuid, $3, $4::text::integer,
        $5::text::timestamptz, $6::text::timestamptz)
    ON CONFLICT (id) DO NOTHING";

/// The entry, unless the table holds it already.
const ADD_ENTRY: &str = "INSERT INTO entity_to_create
        (id, campaign_id, session_id, brief_id, scene_id, type, name, excerpt, status, created_at)
    VALUES ($1::text::uuid, $2::text::uuid, $3::text::uuid, $4::text::uuid, $5::text::uuid,
        $6, $7, $8, $9, $10::text::timestamptz)
    ON CONFLICT (id) DO NOTHING";

/// Whether the media `$2` belongs to the campaign of the session of the
/// scene `$1`.
const MEDIA_OF_CAMPAIGN: &str = "SELECT EXISTS (SELECT 1 FROM scene s
        JOIN session se ON se.id = s.session_id
        JOIN media m ON m.campaign_id = se.campaign_id
    WHERE s.id = $1::text::uuid AND m.id = $2::text::uuid)";

/// The media linked to the scene, unless it is already.
const LINK_MEDIA: &str = "INSERT INTO scene_media (scene_id, media_id)
    VALUES ($1::text::uuid, $2::text::uuid)
    ON CONFLICT (scene_id, media_id) DO NOTHING";

/// The projection of a brief, its entries and the media its scenes link.
pub struct BriefTables;

#[async_trait]
impl ProjectPg for BriefTables {
    /// The brief, the entry or the media link a write leaves, once; a row
    /// that is none of them writes nothing.
    async fn project(&self, write: &Write, tx: &Transaction<'_>) -> Result<(), ProjectionError> {
        if write.tombstone {
            return Ok(());
        }
        let text = |field: &str| write.values.get(field).and_then(Value::as_str);
        if let Some(brief) = text(ORAL_BRIEF_ID) {
            let duration = write
                .values
                .get(DURATION_SECONDS)
                .and_then(Value::as_u64)
                .unwrap_or(0)
                .to_string();
            tx.execute(
                ADD_BRIEF,
                &[
                    &brief,
                    &text(ORAL_BRIEF_SESSION_ID),
                    &text(TRANSCRIPT),
                    &duration,
                    &text(RECORDED_AT),
                    &text(ANALYZED_AT),
                ],
            )
            .await
            .map_err(judged)?;
        }
        if let (Some(scene), Some(media)) = (text(SCENE_MEDIA_SCENE_ID), text(SCENE_MEDIA_MEDIA_ID))
        {
            let same: bool = tx
                .query_one(MEDIA_OF_CAMPAIGN, &[&scene, &media])
                .await
                .map_err(judged)?
                .try_get(0)?;
            if !same {
                return Err(ProjectionError::Refused(format!(
                    "{} : une scène ne lie que des médias de la campagne de sa séance",
                    aggregate_session::SAME_CAMPAIGN
                )));
            }
            tx.execute(LINK_MEDIA, &[&scene, &media])
                .await
                .map_err(judged)?;
        }
        if let Some(entry) = text(TO_CREATE_ID) {
            tx.execute(
                ADD_ENTRY,
                &[
                    &entry,
                    &text(TO_CREATE_CAMPAIGN_ID),
                    &text(TO_CREATE_SESSION_ID),
                    &text(BRIEF_ID),
                    &text(TO_CREATE_SCENE_ID),
                    &text("type"),
                    &text("name"),
                    &text("excerpt"),
                    &text(TO_CREATE_STATUS),
                    &text(CREATED_AT),
                ],
            )
            .await
            .map_err(judged)?;
        }
        Ok(())
    }
}

/// What the tables refuse — a value no column takes (class 22: an id that
/// is no uuid), a NOT NULL, a foreign key, a CHECK (class 23) — rejects the
/// command; any other error fails the step.
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
