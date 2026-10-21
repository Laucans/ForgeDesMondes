//! The commands a `Prepare` of the aggregate's DataCapabilities asks the
//! engine to submit after its own (`Prepared.then`), each kept here until
//! its own `Prepare` takes it back.
//!
//! An analysis of notes is a command per proposal beside the note's, and a
//! validation a command per proposal it checks or unchecks: the first is
//! the one submitted, the others its follow-ups, which the engine submits
//! through the same DataCapability, and so through the same `Prepare`. A
//! follow-up is submitted with an empty payload — what the author sent was
//! the first one's, and the engine checks no key of it — and names the
//! partition it writes; the `Prepare` finds there what it announced, and
//! writes it as it is. A submission of the author always carries a payload,
//! so it is never mistaken for one.

use std::collections::BTreeMap;
use std::sync::Mutex;

use harness_dataguard::api::Submission;
use serde_json::Value;

/// One partition's row, as a command writes it.
type Row = BTreeMap<String, Value>;

/// The follow-ups announced and not yet taken back, by the id of the
/// partition each writes.
#[derive(Debug, Default)]
pub struct Announced {
    commands: Mutex<BTreeMap<String, Row>>,
}

impl Announced {
    /// The follow-up of `data_capability` writing `payload` on the row
    /// `target_id` names, announced: the submission the `Prepare` hands the
    /// engine. One announced on the same row before, and never taken back,
    /// gives way to it.
    pub fn follow_up(&self, data_capability: &str, target_id: &str, payload: Row) -> Submission {
        self.commands
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(target_id.to_owned(), payload);
        Submission {
            data_capability: data_capability.to_owned(),
            target_id: Some(target_id.to_owned()),
            ..Submission::default()
        }
    }

    /// What the follow-up submitted with `payload` on the row `target_id`
    /// names writes, taken back: `None` for a submission that is no
    /// follow-up announced.
    pub fn take(&self, target_id: Option<&str>, payload: &Row) -> Option<Row> {
        let target_id = target_id.filter(|_| payload.is_empty())?;
        self.commands
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(target_id)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn row(value: Value) -> Row {
        serde_json::from_value(value).expect("an object")
    }

    #[test]
    fn a_follow_up_names_its_row_carries_no_payload_and_is_taken_back_once() {
        let announced = Announced::default();
        let submission = announced.follow_up(
            "campaign.createEntitiesFromNote",
            "p1",
            row(json!({ "checked": true })),
        );
        assert_eq!(
            submission.data_capability,
            "campaign.createEntitiesFromNote"
        );
        assert_eq!(submission.target_id.as_deref(), Some("p1"));
        assert!(submission.payload.is_empty());
        assert_eq!(submission.idempotency_key, None);

        // The author's own submission, with a payload, takes nothing.
        let wire = row(json!({ "campaign": "c1" }));
        assert_eq!(announced.take(Some("p1"), &wire), None);
        assert_eq!(announced.take(None, &Row::new()), None);
        assert_eq!(announced.take(Some("p2"), &Row::new()), None);
        assert_eq!(
            announced.take(Some("p1"), &Row::new()),
            Some(row(json!({ "checked": true })))
        );
        assert_eq!(announced.take(Some("p1"), &Row::new()), None, "once");
    }

    #[test]
    fn a_row_announced_again_writes_the_last_payload() {
        let announced = Announced::default();
        announced.follow_up("x", "p1", row(json!({ "checked": true })));
        announced.follow_up("x", "p1", row(json!({ "checked": false })));
        assert_eq!(
            announced.take(Some("p1"), &Row::new()),
            Some(row(json!({ "checked": false })))
        );
    }
}
