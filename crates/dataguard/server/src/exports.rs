//! The archives of the exports submitted (`campaign.exportCampaign`), kept
//! until their command is answered applied.
//!
//! An export's archive is built when the command is submitted, from what the
//! tables hold then; the stamp is the applier's. [`Exports`] keeps each
//! archive under its command's id, and the command's result answers it once
//! the stamp is applied — never before, never for an export rejected: a
//! failed export gives no archive, and changes nothing. The last
//! [`KEPT`] are kept, in memory: an archive the DM did not take before the
//! server stopped is made again by exporting again.

use std::collections::VecDeque;
use std::sync::Mutex;

use export_campaign::Archive;

/// How many archives are kept: one per export a DM may follow at once, and
/// then some.
pub const KEPT: usize = 16;

/// The archives of the last exports, by command id.
#[derive(Default)]
pub struct Exports {
    kept: Mutex<VecDeque<(String, Archive)>>,
}

impl Exports {
    /// Keeps `archive` under `command`, forgetting the oldest beyond
    /// [`KEPT`]. A command already keeping one keeps its first: the same
    /// idempotency key replayed is the same export.
    pub fn keep(&self, command: &str, archive: Archive) {
        let mut kept = self.kept.lock().unwrap_or_else(|e| e.into_inner());
        if kept.iter().any(|(id, _)| id == command) {
            return;
        }
        kept.push_back((command.to_owned(), archive));
        while kept.len() > KEPT {
            kept.pop_front();
        }
    }

    /// The archive kept under `command`, when there is one.
    pub fn of(&self, command: &str) -> Option<Archive> {
        let kept = self.kept.lock().unwrap_or_else(|e| e.into_inner());
        kept.iter()
            .find(|(id, _)| id == command)
            .map(|(_, archive)| archive.clone())
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn archive(file: &str) -> Archive {
        Archive {
            file: file.to_owned(),
            content: json!({ "format": "campaign-archive" }),
        }
    }

    #[test]
    fn an_archive_is_answered_under_its_command_and_a_replay_keeps_the_first() {
        let exports = Exports::default();
        exports.keep("c_1", archive("first.json"));
        exports.keep("c_1", archive("replayed.json"));
        assert_eq!(exports.of("c_1"), Some(archive("first.json")));
        assert_eq!(exports.of("c_2"), None);
    }

    #[test]
    fn only_the_last_ones_are_kept() {
        let exports = Exports::default();
        for i in 0..=KEPT {
            exports.keep(&format!("c_{i}"), archive(&format!("{i}.json")));
        }
        assert_eq!(exports.of("c_0"), None);
        assert_eq!(exports.of("c_1"), Some(archive("1.json")));
        assert_eq!(
            exports.of(&format!("c_{KEPT}")),
            Some(archive(&format!("{KEPT}.json")))
        );
    }
}
