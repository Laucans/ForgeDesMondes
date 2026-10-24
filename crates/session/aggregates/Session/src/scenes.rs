//! A session's scenes in their tables, `scene` and `scene_entity`
//! (`crates/data/migrations/0006_session.sql`), written by [`Scenes`] inside
//! the transaction that applies a write of a scene.
//!
//! A scene is a partition of its own, `Session/<its id>`, whose row holds
//! `sceneId`: its session, its title, its notes, and the elements it links,
//! in order, as one list of ids under `entityId`. Any other row of the
//! aggregate — a session's, an entry « à créer » — is not a scene's, and
//! writes nothing here.
//!
//! - Its rank is settled here, never at submit: the session's row is locked
//!   until the commit and the scene takes the rank after the session's last,
//!   so ranks go from 1 to n whatever was queued side by side
//!   (`scene-order-contiguous`), and none is rejected for it.
//! - The elements it links belong to the session's campaign
//!   (`scene-links-same-campaign`): one that does not, or that the tables do
//!   not hold, rejects the command, and nothing of it is written. Each is
//!   linked once, ranked in the list's order.
//! - The media of a session are never written: `session_media_view`
//!   computes them from the places its scenes link (`session-media-by-place`).
//!
//! A scene written again keeps its rank and takes its row's title, notes
//! and links: those its list no longer names are unlinked, the others
//! ranked again. A scene's tombstone takes it out of `scene`, its links
//! with it, and the scenes after it each move up one rank, so ranks still
//! go from 1 to n.

use async_trait::async_trait;
use harness_dataguard::plugins::{ProjectPg, ProjectionError};
use harness_dataguard::store::Write;
use harness_dataguard::tokio_postgres::{Error, Transaction};
use serde_json::Value;

/// The fields of a scene's row.
pub const SCENE_ID: &str = "sceneId";
pub const SCENE_SESSION_ID: &str = "sceneSessionId";
pub const SCENE_TITLE: &str = "sceneTitle";
pub const NOTES: &str = "notes";
/// The elements a scene links, in order: one list of ids.
pub const LINKS: &str = "entityId";

/// The partitions of the aggregate: `Session/<id>`.
pub const PARTITION: &str = "Session/";

/// The invariant an element of another campaign breaks.
pub const SAME_CAMPAIGN: &str = "scene-links-same-campaign";

/// The session `$1` names, its row locked until the commit: its campaign.
const SESSION: &str = "SELECT campaign_id::text FROM session WHERE id = $1::text::uuid FOR UPDATE";

/// The scene `$1` names taken out — its links cascade — and the scenes of
/// its session after it moved up one rank.
const REMOVE_SCENE: &str = "WITH gone AS (
        DELETE FROM scene WHERE id::text = lower($1) RETURNING session_id, position)
    UPDATE scene s SET position = s.position - 1
    FROM gone WHERE s.session_id = gone.session_id AND s.position > gone.position";

/// How many of the ids `$2` lists name no element of the campaign `$1`.
const STRANGERS: &str = "SELECT count(*) FROM unnest($2::text[]) AS linked(id)
    WHERE NOT EXISTS (SELECT 1 FROM entity e
        WHERE e.id::text = lower(linked.id) AND e.campaign_id = $1::text::uuid)";

/// The scene, after the session's last; written again, its title and its
/// notes, its rank kept.
const ADD_SCENE: &str = "INSERT INTO scene (id, session_id, position, title, notes)
    SELECT $1::text::uuid, $2::text::uuid, coalesce(max(s.position), 0) + 1, $3, $4
    FROM scene s WHERE s.session_id = $2::text::uuid
    ON CONFLICT (id) DO UPDATE SET title = excluded.title, notes = excluded.notes";

/// The links of the scene `$1` its list `$2` no longer names.
const UNLINK: &str = "DELETE FROM scene_entity
    WHERE scene_id = $1::text::uuid AND NOT entity_id::text = ANY (
        SELECT lower(linked.id) FROM unnest($2::text[]) AS linked(id))";

/// The elements it links, ranked in the list's order.
const LINK: &str = "INSERT INTO scene_entity (scene_id, entity_id, position)
    SELECT $1::text::uuid, lower(linked.id)::uuid, linked.rank::integer
    FROM unnest($2::text[]) WITH ORDINALITY AS linked(id, rank)
    ON CONFLICT (scene_id, entity_id) DO UPDATE SET position = excluded.position";

/// The Session aggregate's projection of its scenes into their tables.
pub struct Scenes;

#[async_trait]
impl ProjectPg for Scenes {
    /// The scene a write leaves, with its links; a tombstone's scene taken
    /// out. A row that is no scene's writes nothing.
    async fn project(&self, write: &Write, tx: &Transaction<'_>) -> Result<(), ProjectionError> {
        if write.tombstone {
            let id = write.partition.strip_prefix(PARTITION).unwrap_or_default();
            tx.execute(REMOVE_SCENE, &[&id]).await.map_err(judged)?;
            return Ok(());
        }
        let text = |field: &str| write.values.get(field).and_then(Value::as_str);
        let Some(scene) = text(SCENE_ID) else {
            return Ok(());
        };
        let session = text(SCENE_SESSION_ID).unwrap_or_default();
        let Some(campaign) = tx.query_opt(SESSION, &[&session]).await.map_err(judged)? else {
            return Err(ProjectionError::Refused(format!(
                "{SCENE_SESSION_ID} : la séance {session} n'existe pas"
            )));
        };
        let campaign: String = campaign.try_get(0)?;
        let links = linked(&write.values);
        let strangers: i64 = tx
            .query_one(STRANGERS, &[&campaign, &links])
            .await
            .map_err(judged)?
            .try_get(0)?;
        if strangers > 0 {
            return Err(ProjectionError::Refused(format!(
                "{SAME_CAMPAIGN} : une scène ne lie que des éléments de la campagne de sa séance"
            )));
        }
        tx.execute(
            ADD_SCENE,
            &[&scene, &session, &text(SCENE_TITLE), &text(NOTES)],
        )
        .await
        .map_err(judged)?;
        for sql in [UNLINK, LINK] {
            tx.execute(sql, &[&scene, &links]).await.map_err(judged)?;
        }
        Ok(())
    }
}

/// The ids a scene's row links, in order: none when it holds no list.
fn linked(row: &std::collections::BTreeMap<String, Value>) -> Vec<String> {
    row.get(LINKS)
        .and_then(Value::as_array)
        .map(|ids| {
            ids.iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

/// What the tables refuse — a value no column takes (class 22: an id that
/// is no uuid), a NOT NULL, a foreign key, a CHECK (class 23) — rejects the
/// command; any other error fails the step.
pub(crate) fn judged(error: Error) -> ProjectionError {
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

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{LINKS, linked};

    #[test]
    fn a_scene_links_the_ids_of_its_list_in_order_and_nothing_else() {
        let row = |value| [(LINKS.to_owned(), value)].into_iter().collect();
        assert_eq!(linked(&row(json!(["b", "a"]))), ["b", "a"]);
        assert!(linked(&row(json!("a"))).is_empty());
        assert!(linked(&std::collections::BTreeMap::new()).is_empty());
    }
}
