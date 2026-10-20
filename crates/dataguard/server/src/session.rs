//! The invariants of the `Session` aggregate
//! (`crates/dataguard/aggregates/Session/aggregate.json`), which its rules
//! write in words: the engine judges here what one row of a session and its
//! command can say, on the row's state after the command. The tables
//! (`crates/data/migrations/0006_session.sql`) hold the relations, the value
//! sets and the two views the rules read.
//!
//! What a session's own row can say is judged here; the rest waits for the
//! DataCapabilities that write these tables, which prepare a command against
//! the rows it needs. `present-only-pj` reads the character linked, a row of
//! the Entity aggregate: here, the roster and the media of a session are
//! never stored — the views compute them, so they follow every later
//! presence and every change of place. `scene-links-same-campaign` reads the
//! campaign of a scene's session and of each element it links, which
//! [`Scenes`] keeps. `scene-order-contiguous` reads the session's other
//! scenes: here, a scene's rank is one of 1 to n, and a new scene's is
//! settled after the session's last when it is applied
//! (`crate::versioning::Versioning`). `previous-summary-visible` is a read:
//! the tables keep a closed session's summary and journal for the next one.

use std::collections::BTreeMap;
use std::sync::{Arc, LazyLock};

use add_scene::{LINKED, SESSION as SCENE_SESSION};
use harness_dataguard::aggregate::Aggregate;
use harness_dataguard::command::{Effect, Entry};
use harness_dataguard::guard::Invariants;
use serde_json::Value;

use crate::scenes::Scenes;

const AGGREGATE: &str = "Session";

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
    Aggregate::parse(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../aggregates/Session/aggregate.json"
    )))
    .expect("Session's aggregate.json conforms to contract I")
    .fields
    .into_keys()
    .collect()
});

/// One invariant: its id, and whether it holds on the state the command
/// leaves.
type Check = (&'static str, fn(&BTreeMap<String, Value>, &Entry) -> bool);

/// The invariant [`Session`] judges with what [`Scenes`] keeps.
const SAME_CAMPAIGN: &str = "scene-links-same-campaign";

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

/// The invariants `plant()` binds the engine with, `scene-links-same-campaign`
/// reading `scenes`.
pub struct Session {
    scenes: Arc<Scenes>,
}

impl Session {
    /// The judge of the Session aggregate, reading `scenes`.
    pub fn new(scenes: Arc<Scenes>) -> Self {
        Self { scenes }
    }

    /// The elements a scene links belong to its session's campaign: judged
    /// when the command writes the links or the session of a scene.
    fn links_same_campaign(&self, state: &BTreeMap<String, Value>, entry: &Entry) -> bool {
        let Some(session) = value(state, SCENE_SESSION).as_str() else {
            return true;
        };
        if !written(entry).any(|field| field == LINKED || field == SCENE_SESSION) {
            return true;
        }
        let linked: Vec<&str> = match value(state, LINKED) {
            Value::Array(ids) => ids.iter().filter_map(Value::as_str).collect(),
            Value::String(id) => vec![id.as_str()],
            _ => Vec::new(),
        };
        self.scenes.same_campaign(session, linked.into_iter())
    }
}

impl Invariants for Session {
    fn violated(
        &self,
        aggregate: &str,
        state: &BTreeMap<String, Value>,
        entry: &Entry,
    ) -> Vec<String> {
        // A deletion is the engine's tombstone: nothing of the row is left to
        // judge.
        if aggregate != AGGREGATE || entry.effect == Effect::Delete {
            return Vec::new();
        }
        let mut violated: Vec<String> = CHECKS
            .iter()
            .filter(|(_, holds)| !holds(state, entry))
            .map(|(id, _)| (*id).to_string())
            .collect();
        if !self.links_same_campaign(state, entry) {
            violated.push(SAME_CAMPAIGN.to_string());
        }
        violated
    }
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
/// number, a boolean or a null — the elements a scene links, a list of their
/// ids: no field holds audio, nor anything but what a column types.
fn text_only_no_audio(_state: &BTreeMap<String, Value>, entry: &Entry) -> bool {
    entry.payload.iter().all(|(field, value)| {
        DECLARED.contains(field)
            && match value {
                Value::Array(ids) => field == LINKED && ids.iter().all(Value::is_string),
                Value::Object(_) => false,
                _ => true,
            }
    })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::Arc;

    use harness_dataguard::command::Entry;
    use harness_dataguard::guard::Invariants;
    use serde_json::{Value, json};

    use super::{
        CHECKS, CLOSED_AT, CREATED, DECLARED, EXCERPT, JOURNAL, LINKED, MICROPHONE, ROSTER,
        SAME_CAMPAIGN, SCENE_POSITION, SCENE_SESSION, SESSION_MEDIA, STATUS, SUMMARY,
        SUMMARY_MODEL, Session, TO_CREATE, TYPE,
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
    fn the_judge_leaves_another_aggregate_s_row_and_a_tombstone_alone() {
        // A closed session with its microphone on, at rank 0: what a session
        // may not be, and what another aggregate's row may well hold.
        let state: BTreeMap<String, Value> = [
            (STATUS.to_string(), json!("closed")),
            (MICROPHONE.to_string(), json!(true)),
            (SCENE_POSITION.to_string(), json!(0)),
        ]
        .into_iter()
        .collect();
        let judge = Session::new(Arc::default());
        assert_eq!(
            judge.violated("Session", &state, &entry("Session", "update")),
            ["scene-order-contiguous", "close-session"]
        );
        assert!(
            judge
                .violated("Campaign", &state, &entry("Campaign", "update"))
                .is_empty()
        );
        assert!(
            judge
                .violated("Session", &state, &entry("Session", "delete"))
                .is_empty()
        );
    }

    #[test]
    fn the_checks_know_the_fields_and_invariants_the_aggregate_declares() {
        let aggregate: Value =
            serde_json::from_str(include_str!("../../aggregates/Session/aggregate.json"))
                .expect("contract I");
        let fields = aggregate["fields"].as_object().expect("fields");
        for field in [
            STATUS,
            MICROPHONE,
            CLOSED_AT,
            SUMMARY,
            SUMMARY_MODEL,
            SCENE_POSITION,
            SCENE_SESSION,
            LINKED,
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
        for id in CHECKS.iter().map(|(id, _)| *id).chain([SAME_CAMPAIGN]) {
            assert!(declared.contains(&id), "{id} is not declared");
        }
    }
}
