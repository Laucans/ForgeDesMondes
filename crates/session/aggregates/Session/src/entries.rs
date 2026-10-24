//! An entry « à créer » once created, in its table, `entity_to_create`
//! (`crates/data/migrations/0006_session.sql`), written by [`Entries`] inside
//! the transaction that applies a write of the entry.
//!
//! An entry is a partition of its own, `Session/<its id>`. Its row is born
//! with the analysis of a brief, which writes it into the table; what
//! follows — `ai.createFromBrief` stamping it « creee » with the element it
//! became and, when the DM refined it, their refinement — is written here,
//! so the campaign's to-create list, which reads the table, no longer offers
//! it. Any other row of the aggregate — a session's, a scene's — holds none
//! of these fields and writes nothing here.
//!
//! - Only the fields the row holds are written: a write of the status alone
//!   leaves the element and the refinement as they were, and a `null` takes
//!   the element or the refinement out; a status is never taken out.
//! - An entry the table does not hold is not made here: the analysis that
//!   writes entries does. The table's own checks judge the rest — a status
//!   it does not know, an element it does not hold — and reject the command,
//!   nothing of it written.
//!
//! A tombstone writes nothing.

use async_trait::async_trait;
use harness_dataguard::plugins::{ProjectPg, ProjectionError};
use harness_dataguard::store::Write;
use harness_dataguard::tokio_postgres::Transaction;
use serde_json::Value;

use crate::PARTITION;
use crate::scenes::judged;

/// The fields of an entry the stamp of its creation writes.
const STATUS: &str = "entityToCreateStatus";
const CREATED: &str = "createdEntityId";
const REFINEMENT: &str = "refinementText";

/// The entry `$1`, its status (`$2`, kept when null), its refinement (`$4`,
/// when `$3` says the row holds one) and its element (`$6`, when `$5` does).
const STAMP: &str = "UPDATE entity_to_create SET
        status = coalesce($2::text, status),
        refinement_text = CASE WHEN $3::boolean THEN $4::text ELSE refinement_text END,
        created_entity_id = CASE WHEN $5::boolean THEN $6::text::uuid ELSE created_entity_id END
    WHERE id = $1::text::uuid";

/// The Session aggregate's projection of an entry's creation into its table.
pub struct Entries;

#[async_trait]
impl ProjectPg for Entries {
    /// The status, the element and the refinement a write leaves on its
    /// entry. A row that holds none of them, or a tombstone, writes nothing.
    async fn project(&self, write: &Write, tx: &Transaction<'_>) -> Result<(), ProjectionError> {
        let values = &write.values;
        if write.tombstone
            || ![STATUS, CREATED, REFINEMENT]
                .iter()
                .any(|f| values.contains_key(*f))
        {
            return Ok(());
        }
        let entry = write.partition.strip_prefix(PARTITION).unwrap_or_default();
        let status = text(values.get(STATUS), STATUS)?;
        let (refined, refinement) = column(values.get(REFINEMENT), REFINEMENT)?;
        let (created, element) = column(values.get(CREATED), CREATED)?;
        tx.execute(
            STAMP,
            &[&entry, &status, &refined, &refinement, &created, &element],
        )
        .await
        .map_err(judged)?;
        Ok(())
    }
}

/// `field`'s value when it is a text or a null.
fn text<'a>(value: Option<&'a Value>, field: &str) -> Result<Option<&'a str>, ProjectionError> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(text)) => Ok(Some(text)),
        Some(_) => Err(ProjectionError::Refused(format!(
            "{field} : un texte est attendu"
        ))),
    }
}

/// Whether the row holds `field`, and its text: a `null` is held, as none.
fn column<'a>(
    value: Option<&'a Value>,
    field: &str,
) -> Result<(bool, Option<&'a str>), ProjectionError> {
    Ok((value.is_some(), text(value, field)?))
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::{column, text};

    #[test]
    fn a_field_is_a_text_a_null_or_absent_and_nothing_else() {
        assert_eq!(text(Some(&json!("a")), "f").unwrap(), Some("a"));
        assert_eq!(text(Some(&Value::Null), "f").unwrap(), None);
        assert_eq!(text(None, "f").unwrap(), None);
        assert!(text(Some(&json!(3)), "f").is_err());
        assert!(text(Some(&json!(["a"])), "f").is_err());
    }

    #[test]
    fn a_column_is_written_when_the_row_holds_it_even_as_a_null() {
        assert_eq!(column(Some(&json!("a")), "f").unwrap(), (true, Some("a")));
        assert_eq!(column(Some(&Value::Null), "f").unwrap(), (true, None));
        assert_eq!(column(None, "f").unwrap(), (false, None));
        assert!(column(Some(&json!(true)), "f").is_err());
    }
}
