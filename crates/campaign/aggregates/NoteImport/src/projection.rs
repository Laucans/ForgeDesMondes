//! A note's or a proposal's write, carried into the product's tables in the
//! transaction that applies it (`ProjectPg`).
//!
//! An analysis of notes (`ai.extractFromNotes`) writes a partition per
//! imported note, `NoteImport/<its id>`, and one per proposal: once the
//! note is applied, [`Tables`] adds its `imported_note` row and the
//! `note_proposal` row of every proposal of it applied — whichever the
//! engine applied first. A validation of a note's proposals
//! (`campaign.createEntitiesFromNote`) writes each proposal's partition
//! again, its check and the element it created: [`Tables`] writes
//! `note_proposal.checked` and `created_entity_id` — the element's once the
//! tables hold it (the Entity aggregate's projection records it when its
//! element comes after). The statements write only what the tables can hold
//! — a note of a campaign they hold, a proposal of a note they hold, never
//! an element in place of another — so an error is the store failing: the
//! apply rolls back and the step fails.
//!
//! When the store starts ([`Tables`]'s `warm`), what the engine holds and
//! the tables do not is projected again — a note applied before its
//! projection ran in the transaction — and every proposal the engine holds
//! is recorded among its note's ([`Notes`]). Tables not migrated yet hold
//! none.

use std::sync::Arc;

use async_trait::async_trait;
use harness_dataguard::plugins::{ProjectPg, ProjectionError};
use harness_dataguard::store::Write;
use harness_dataguard::tokio_postgres::types::ToSql;
use harness_dataguard::tokio_postgres::{Client, Error, GenericClient, Transaction};
use serde_json::Value;

use crate::notes::Notes;

/// The imported notes the engine holds and the tables do not — of the note
/// `$1` names, or of every one when it is null. A note of a campaign the
/// tables do not hold, or whose ids are no uuids, is none they could hold.
const ADD_NOTES: &str =
    "INSERT INTO imported_note (id, campaign_id, raw_text, created_at, analyzed_at)
    SELECT (r.fields ->> 'importedNoteId')::uuid, c.id, r.fields ->> 'rawText',
        coalesce((r.fields ->> 'createdAt')::timestamptz, now()),
        (r.fields ->> 'analyzedAt')::timestamptz
    FROM dataguard_rows r
    JOIN campaign c ON c.id::text = lower(r.fields ->> 'campaignId')
    WHERE r.partition LIKE 'NoteImport/%' AND NOT r.tombstoned
        AND ($1::text IS NULL OR lower(r.fields ->> 'importedNoteId') = lower($1::text))
        AND pg_input_is_valid(r.fields ->> 'importedNoteId', 'uuid')
        AND r.fields ->> 'rawText' IS NOT NULL
        AND pg_input_is_valid(coalesce(r.fields ->> 'createdAt', 'now'), 'timestamptz')
        AND pg_input_is_valid(coalesce(r.fields ->> 'analyzedAt', 'now'), 'timestamptz')
        AND NOT EXISTS (SELECT 1 FROM imported_note n
            WHERE n.id::text = lower(r.fields ->> 'importedNoteId'))";

/// The proposals the engine holds and the tables do not, of a note they hold
/// — the one `$1` names, or every one when it is null. The element a
/// proposal recognised and the tables no longer hold is forgotten, as its
/// relation's `onDelete nullify` would have.
const ADD_PROPOSALS: &str = "INSERT INTO note_proposal
        (id, note_id, type, name, excerpt, existing_entity_id, checked)
    SELECT (r.fields ->> 'noteProposalId')::uuid, n.id, r.fields ->> 'type',
        r.fields ->> 'name', coalesce(r.fields ->> 'excerpt', ''), e.id,
        coalesce((r.fields ->> 'checked')::boolean, e.id IS NULL)
    FROM dataguard_rows r
    JOIN imported_note n ON n.id::text = lower(r.fields ->> 'noteId')
    LEFT JOIN entity e ON e.id::text = lower(r.fields ->> 'existingEntityId')
    WHERE r.partition LIKE 'NoteImport/%' AND NOT r.tombstoned
        AND ($1::text IS NULL OR n.id::text = lower($1::text))
        AND pg_input_is_valid(r.fields ->> 'noteProposalId', 'uuid')
        AND r.fields ->> 'type' IN ('pj', 'pnj', 'lieu', 'evenement')
        AND r.fields ->> 'name' IS NOT NULL
        AND pg_input_is_valid(coalesce(r.fields ->> 'checked', 'true'), 'boolean')
        AND NOT EXISTS (SELECT 1 FROM note_proposal p
            WHERE p.id::text = lower(r.fields ->> 'noteProposalId'))
    ORDER BY r.partition";

/// The checked state and the created element the engine holds of each
/// proposal the tables hold — of the note `$1` names, or of every one when
/// it is null — where they differ: a validation applied. An element the
/// tables do not hold yet is recorded once they do (the Entity aggregate's
/// projection); one recorded is never replaced, nor is a proposal that
/// created one unchecked (`no-creation-before-validation`).
const STAMP_PROPOSALS: &str = "UPDATE note_proposal p
    SET checked = s.checked,
        created_entity_id = coalesce(e.id, p.created_entity_id)
    FROM (SELECT r.partition, lower(r.fields ->> 'createdEntityId') AS created,
            CASE r.fields ->> 'checked' WHEN 'true' THEN true WHEN 'false' THEN false END AS checked
        FROM dataguard_rows r
        WHERE r.partition LIKE 'NoteImport/%' AND NOT r.tombstoned) s
    LEFT JOIN entity e ON e.id::text = s.created
    WHERE s.partition = 'NoteImport/' || p.id::text AND s.checked IS NOT NULL
        AND ($1::text IS NULL OR p.note_id::text = lower($1::text))
        AND (p.created_entity_id IS NULL OR e.id IS NULL OR e.id = p.created_entity_id)
        AND (s.checked OR coalesce(e.id, p.created_entity_id) IS NULL)
        AND (p.checked <> s.checked
            OR p.created_entity_id IS DISTINCT FROM coalesce(e.id, p.created_entity_id))";

/// Every proposal the engine holds, with its note: what [`Notes`] starts
/// from.
const PROPOSALS: &str =
    "SELECT r.fields ->> 'noteId', substr(r.partition, length('NoteImport/') + 1)
    FROM dataguard_rows r
    WHERE r.partition LIKE 'NoteImport/%' AND NOT r.tombstoned
        AND jsonb_typeof(r.fields -> 'noteId') = 'string'";

/// Whether the relation `$1` names exists: a table not migrated yet does not.
const EXISTS: &str = "SELECT to_regclass($1::text) IS NOT NULL";

/// The NoteImport aggregate's projection into the product's tables, keeping
/// the proposals of each note it starts from.
pub struct Tables {
    /// Every note's proposals, which a start reads from the engine's rows.
    pub notes: Arc<Notes>,
}

/// The imported note a live write of the aggregate is of: the note's own
/// id, or the note a proposal is of.
fn noted(write: &Write) -> Option<&str> {
    if write.tombstone || !write.partition.starts_with("NoteImport/") {
        return None;
    }
    ["importedNoteId", "noteId"]
        .iter()
        .find_map(|field| write.values.get(*field).and_then(Value::as_str))
}

/// What the engine holds of the note `note` names — of every one, `None` —
/// and the tables do not, written through `link`: the apply's transaction,
/// or a client inside the transaction it opened.
async fn written(link: &impl GenericClient, note: Option<&str>) -> Result<(), Error> {
    let parameters: [&(dyn ToSql + Sync); 1] = [&note];
    for sql in [ADD_NOTES, ADD_PROPOSALS, STAMP_PROPOSALS] {
        link.execute(sql, &parameters).await?;
    }
    Ok(())
}

#[async_trait]
impl ProjectPg for Tables {
    async fn project(&self, write: &Write, tx: &Transaction<'_>) -> Result<(), ProjectionError> {
        // A removal is the engine's tombstone: the tables keep the note.
        let Some(note) = noted(write) else {
            return Ok(());
        };
        Ok(written(tx, Some(note)).await?)
    }

    async fn warm(&self, client: &Client) -> Result<(), ProjectionError> {
        let migrated = client
            .query_one(EXISTS, &[&"note_proposal"])
            .await?
            .get::<_, bool>(0);
        if !migrated {
            return Ok(());
        }
        // One transaction: a note and its proposals, or nothing.
        let repaired = async {
            client.batch_execute("BEGIN").await?;
            match written(client, None).await {
                Ok(()) => client.batch_execute("COMMIT").await,
                Err(e) => {
                    client.batch_execute("ROLLBACK").await?;
                    Err(e)
                }
            }
        };
        if let Err(e) = repaired.await {
            // Its SQLSTATE alone: a value may be the DM's.
            eprintln!(
                "projection: imported notes not projected: {}",
                e.as_db_error().map_or("-", |db| db.code().code())
            );
        }
        for row in client.query(PROPOSALS, &[]).await? {
            self.notes.record(row.get(0), row.get(1));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn a_live_write_of_a_note_or_a_proposal_names_its_note() {
        let write = |partition: &str, values: Value, tombstone| Write {
            partition: partition.to_string(),
            values: serde_json::from_value(values).expect("an object"),
            tombstone,
        };
        let note = "a1000000-0000-4000-8000-000000000001";
        let noted_by = |values, tombstone| {
            noted(&write(&format!("NoteImport/{note}"), values, tombstone)).map(str::to_owned)
        };
        assert_eq!(
            noted_by(json!({ "importedNoteId": note }), false).as_deref(),
            Some(note)
        );
        assert_eq!(
            noted_by(json!({ "noteProposalId": "p", "noteId": note }), false).as_deref(),
            Some(note)
        );
        assert_eq!(noted_by(json!({ "importedNoteId": note }), true), None);
        assert_eq!(noted_by(json!({ "checked": true }), false), None);
        assert_eq!(
            noted(&write("AiSettings/1", json!({ "noteId": note }), false)),
            None
        );
    }
}
