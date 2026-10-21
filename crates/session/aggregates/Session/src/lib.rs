//! The invariants of the `Session` aggregate
//! (`crates/session/aggregates/Session/aggregate.json`), which its rules
//! write in words: the engine judges here what one row of a session and its
//! command can say, on the row's state after the command. The tables
//! (`crates/data/migrations/0006_session.sql`) hold the relations, the value
//! sets and the two views the rules read.
//!
//! What a session's own row can say is judged here; the rest waits for the
//! DataCapabilities that write these tables, which prepare a command against
//! the rows it needs. `present-only-pj` and `scene-links-same-campaign` read
//! the character, element or media linked, rows of the Entity and Media
//! aggregates: here, the roster and the media of a session are never
//! stored — the views compute them, so they follow every later presence and
//! every change of place. `scene-order-contiguous` reads the session's other
//! scenes: here, a scene's rank is one of 1 to n. `previous-summary-visible`
//! is a read: the tables keep a closed session's summary and journal for the
//! next one.
//!
//! A scene is a row of its own, written into its tables in the transaction
//! that applies it ([`Scenes`]): its rank is settled there, after the
//! session's last, and the elements it links are judged there against the
//! session's campaign.

use std::collections::BTreeMap;
use std::sync::LazyLock;

use harness_dataguard::aggregate::Aggregate;
use harness_dataguard::command::{Effect, Entry};
use harness_dataguard::plugins::{Judge, Plugin};
use serde_json::Value;

mod scenes;

pub use scenes::{
    LINKS, NOTES, PARTITION, SAME_CAMPAIGN, SCENE_ID, SCENE_SESSION_ID, SCENE_TITLE, Scenes,
};

const AGGREGATE: &str = "Session";

/// The aggregate's document (contract I), as the engine registers it.
pub const DOCUMENT: &str = include_str!("../aggregate.json");

/// A session's status, in the order it passes through them.
const STATUS: &str = "sessionStatus";
const CLOSED: &str = "closed";
const LIFECYCLE: [&str; 3] = ["prep", "live", CLOSED];

/// The fields of a journal line, which a closed session no longer takes.
const JOURNAL: [&str; 5] = [
    "sessionJournalLineId",
    "sessionJournalLineSessionId",
    "sessionJournalLineSceneId",
    "spokenAt",
    "text",
];

/// What the close of a session records: the microphone, off; when; the
/// summary, with the model that wrote it.
const MICROPHONE: &str = "microphoneOn";
const CLOSED_AT: &str = "closedAt";
const SUMMARY: &str = "summary";
const SUMMARY_MODEL: &str = "summaryModel";

/// A scene's rank in its session.
const SCENE_POSITION: &str = "scenePosition";

/// An entry of the « à créer » list: its id and status, which say a row is
/// one; the element it created; what the analysis of a brief writes it with.
const TO_CREATE_STATUS: &str = "entityToCreateStatus";
const TO_CREATE: [&str; 2] = ["entityToCreateId", TO_CREATE_STATUS];
const A_CREER: &str = "a_creer";
const CREEE: &str = "creee";
const TO_CREATE_STATUSES: [&str; 2] = [A_CREER, CREEE];
const CREATED: &str = "createdEntityId";
const TYPE: &str = "type";
const TO_CREATE_TYPES: [&str; 3] = ["pnj", "lieu", "evenement"];
const EXCERPT: &str = "excerpt";

/// The characters a session offers: `session_roster_view`'s, never stored.
const ROSTER: [&str; 3] = [
    "sessionRosterViewSessionId",
    "sessionRosterViewCharacterId",
    "via",
];

/// The media of a session: `session_media_view`'s, never stored.
const SESSION_MEDIA: [&str; 3] = [
    "sessionMediaViewSessionId",
    "sessionMediaViewMediaId",
    "viaPlaceId",
];

/// The fields the aggregate declares: the only ones a command may write.
static DECLARED: LazyLock<Vec<String>> = LazyLock::new(|| {
    Aggregate::parse(DOCUMENT)
        .expect("Session's aggregate.json conforms to contract I")
        .fields
        .into_keys()
        .collect()
});

/// One invariant: its id, and whether it holds on the state the command
/// leaves.
type Check = (&'static str, fn(&BTreeMap<String, Value>, &Entry) -> bool);

const CHECKS: [Check; 8] = [
    ("present-only-pj", present_only_pj),
    ("scene-order-contiguous", scene_order_contiguous),
    ("session-media-by-place", session_media_by_place),
    ("brief-no-auto-creation", brief_no_auto_creation),
    (
        "to-create-leaves-once-created",
        to_create_leaves_once_created,
    ),
    ("status-lifecycle", status_lifecycle),
    ("close-session", close_session),
    ("text-only-no-audio", text_only_no_audio),
];

/// The aggregate's invariants as code: its judge.
pub struct Session;

impl Judge for Session {
    fn violated(&self, state: &BTreeMap<String, Value>, entry: &Entry) -> Vec<String> {
        // A deletion is the engine's tombstone: nothing of the row is left to
        // judge.
        if entry.effect == Effect::Delete {
            return Vec::new();
        }
        CHECKS
            .iter()
            .filter(|(_, holds)| !holds(state, entry))
            .map(|(id, _)| (*id).to_string())
            .collect()
    }
}

/// What the aggregate plugs into the engine: its judge, and its scenes
/// written in the transaction that applies them.
#[must_use]
pub fn plugin() -> Plugin {
    Plugin::new()
        .judge(AGGREGATE, Session)
        .project_pg(AGGREGATE, Scenes)
}

/// `field`'s value in `row`, an absent field and a null being the same.
fn value<'a>(row: &'a BTreeMap<String, Value>, field: &str) -> &'a Value {
    row.get(field).unwrap_or(&Value::Null)
}

/// Whether `row` holds a value for `field`: a `null` is none.
fn set(row: &BTreeMap<String, Value>, field: &str) -> bool {
    !value(row, field).is_null()
}

/// A text holding more than blanks.
fn filled(value: &Value) -> bool {
    value.as_str().is_some_and(|text| !text.trim().is_empty())
}

/// The fields the command writes: its `touches`, and its payload's keys.
fn written(entry: &Entry) -> impl Iterator<Item = String> + '_ {
    entry
        .fields()
        .into_iter()
        .chain(entry.payload.keys().cloned())
}

/// The roster is never stored: the present PCs are offered as
/// `session_roster_view` computes them, those checked later included.
fn present_only_pj(state: &BTreeMap<String, Value>, _entry: &Entry) -> bool {
    ROSTER.iter().all(|field| !set(state, field))
}

/// A scene's rank, when the row has one, is one of 1 to n: a whole number
/// from 1.
fn scene_order_contiguous(state: &BTreeMap<String, Value>, _entry: &Entry) -> bool {
    let position = value(state, SCENE_POSITION);
    position.is_null() || position.as_u64().is_some_and(|rank| rank >= 1)
}

/// A session's media are never stored: `session_media_view` computes them,
/// so they follow every change of place.
fn session_media_by_place(state: &BTreeMap<String, Value>, _entry: &Entry) -> bool {
    SESSION_MEDIA.iter().all(|field| !set(state, field))
}

/// The analysis of a brief creates no element: an entry is written « à
/// créer », with no element, with its type and its excerpt.
fn brief_no_auto_creation(state: &BTreeMap<String, Value>, entry: &Entry) -> bool {
    // Written now: an insert, or an upsert of a row the engine does not hold.
    let born = entry.effect == Effect::Insert
        || (entry.effect == Effect::Upsert && entry.projection.confirmed.is_empty());
    if !born || !TO_CREATE.iter().any(|field| set(state, field)) {
        return true;
    }
    value(state, TO_CREATE_STATUS).as_str() == Some(A_CREER)
        && !set(state, CREATED)
        && value(state, TYPE)
            .as_str()
            .is_some_and(|kind| TO_CREATE_TYPES.contains(&kind))
        && filled(value(state, EXCERPT))
}

/// An entry is « creee » if and only if it names the element it created.
fn to_create_leaves_once_created(state: &BTreeMap<String, Value>, _entry: &Entry) -> bool {
    let status = value(state, TO_CREATE_STATUS);
    let known = status.is_null()
        || status
            .as_str()
            .is_some_and(|status| TO_CREATE_STATUSES.contains(&status));
    known && (status.as_str() == Some(CREEE)) == set(state, CREATED)
}

/// A status's place in the lifecycle.
fn rank(status: &Value) -> Option<usize> {
    let status = status.as_str()?;
    LIFECYCLE.iter().position(|known| *known == status)
}

/// A session goes from prep to live, then to closed, one step at a time and
/// never back; once closed, it takes no journal line.
fn status_lifecycle(state: &BTreeMap<String, Value>, entry: &Entry) -> bool {
    let status = value(state, STATUS);
    let before = value(&entry.projection.confirmed, STATUS);
    let forward = match (rank(before), rank(status)) {
        (_, None) => status.is_null() && before.is_null(),
        (None, Some(_)) => true,
        (Some(before), Some(after)) => after == before || after == before + 1,
    };
    let journal = before.as_str() == Some(CLOSED)
        && written(entry).any(|field| JOURNAL.contains(&field.as_str()));
    forward && !journal
}

/// A closed session has its microphone off and its close dated; a summary
/// it records names the summary model that wrote it.
fn close_session(state: &BTreeMap<String, Value>, _entry: &Entry) -> bool {
    value(state, STATUS).as_str() != Some(CLOSED)
        || (value(state, MICROPHONE) == &Value::Bool(false)
            && set(state, CLOSED_AT)
            && (!set(state, SUMMARY) || filled(value(state, SUMMARY_MODEL))))
}

/// A command writes only fields the aggregate declares, each a text, a
/// number, a boolean or a null: no field holds audio, nor anything but what
/// a column types. One list is a column's values: the ids of the elements a
/// scene links, one `scene_entity` row each.
fn text_only_no_audio(_state: &BTreeMap<String, Value>, entry: &Entry) -> bool {
    entry.payload.iter().all(|(field, value)| {
        DECLARED.contains(field)
            && match value {
                Value::Object(_) => false,
                Value::Array(ids) => {
                    field == LINKS && ids.iter().all(|id| id.as_str().is_some_and(uuid))
                }
                _ => true,
            }
    })
}

/// A uuid written 8-4-4-4-12 in hex digits, in either case: what a
/// session's, a scene's or an element's id is.
#[must_use]
pub fn uuid(id: &str) -> bool {
    id.len() == 36
        && id.char_indices().all(|(at, c)| match at {
            8 | 13 | 18 | 23 => c == '-',
            _ => c.is_ascii_hexdigit(),
        })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use harness_dataguard::command::Entry;
    use harness_dataguard::plugins::Judge;
    use serde_json::{Value, json};

    use super::{
        CHECKS, CLOSED_AT, CREATED, DECLARED, EXCERPT, JOURNAL, MICROPHONE, ROSTER, SCENE_POSITION,
        SESSION_MEDIA, STATUS, SUMMARY, SUMMARY_MODEL, Session, TO_CREATE, TYPE,
    };

    /// A command of `effect` on `aggregate`'s row.
    fn entry(aggregate: &str, effect: &str) -> Entry {
        serde_json::from_value(json!({
            "command": "c1",
            "data_capability": "test.write@1",
            "by": "dm",
            "partition": format!("{aggregate}/s1"),
            "position": 1,
            "based_on": { "version": 0 },
            "payload": {},
            "effect": effect,
            "touches": [],
            "mode": "overwrite",
            "projection": { "confirmed": {}, "pending_ahead": [] },
            "state": "queued",
        }))
        .expect("an entry")
    }

    #[test]
    fn the_judge_leaves_a_tombstone_alone() {
        // A closed session with its microphone on, at rank 0: what a session
        // may not be, and what another aggregate's row may well hold.
        let state: BTreeMap<String, Value> = [
            (STATUS.to_string(), json!("closed")),
            (MICROPHONE.to_string(), json!(true)),
            (SCENE_POSITION.to_string(), json!(0)),
        ]
        .into_iter()
        .collect();
        assert_eq!(
            Session.violated(&state, &entry("Session", "update")),
            ["scene-order-contiguous", "close-session"]
        );
        // Another aggregate's row is never handed to this judge: the engine
        // calls the judges of the aggregate written alone.
        assert!(
            Session
                .violated(&state, &entry("Session", "delete"))
                .is_empty()
        );
    }

    #[test]
    fn the_checks_know_the_fields_and_invariants_the_aggregate_declares() {
        let aggregate: Value =
            serde_json::from_str(include_str!("../aggregate.json")).expect("contract I");
        let fields = aggregate["fields"].as_object().expect("fields");
        for field in [
            STATUS,
            MICROPHONE,
            CLOSED_AT,
            SUMMARY,
            SUMMARY_MODEL,
            SCENE_POSITION,
            CREATED,
            TYPE,
            EXCERPT,
        ]
        .iter()
        .chain(JOURNAL.iter())
        .chain(TO_CREATE.iter())
        .chain(ROSTER.iter())
        .chain(SESSION_MEDIA.iter())
        {
            assert!(fields.contains_key(*field), "{field} is not declared");
            assert!(DECLARED.iter().any(|declared| declared == field), "{field}");
        }
        // What the views compute is derived, never written.
        for field in ROSTER.iter().chain(SESSION_MEDIA.iter()) {
            assert_eq!(fields[*field]["kind"], "derived", "{field}");
        }
        let declared: Vec<&str> = aggregate["invariants"]
            .as_array()
            .expect("invariants")
            .iter()
            .map(|invariant| invariant["id"].as_str().expect("an id"))
            .collect();
        for (id, _) in CHECKS {
            assert!(declared.contains(&id), "{id} is not declared");
        }
    }
}
