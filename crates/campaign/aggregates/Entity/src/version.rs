//! An element's versions (EntityVersion@1), as they are settled once a save
//! is applied.
//!
//! A save is numbered, snapshotted and diffed when it is prepared, against
//! the row the commands queued ahead of it will leave: what the DataGuard
//! judges, and what the engine's row keeps. The version the element really
//! takes is settled when the save is applied, after the last version the
//! element really has: a command ahead that was cancelled or rejected leaves
//! no gap in the numbers and no change in the diff that never happened. The
//! tables settle it in the apply transaction (`crate::projection`), the
//! engine's memory as it observes the write ([`crate::History`]): the same
//! computation, [`settled`].

use std::collections::BTreeSet;

use serde_json::{Map, Value};

use crate::aliases::Row;

/// The row's field holding the element's aliases; a snapshot holds them
/// under `aliases`.
const ALIAS: &str = "alias";

/// The fields of the element and of its sheet a snapshot holds, beside its
/// `aliases`: what the DM sees and restores — never an id or a date.
const SNAPSHOT: [&str; 24] = [
    "type",
    "name",
    "description",
    "notes",
    "playerId",
    "initialPrompt",
    "background",
    "behavior",
    "strength",
    "dexterity",
    "constitution",
    "intelligence",
    "wisdom",
    "charisma",
    "armorClass",
    "hpMax",
    "spells",
    "actions",
    "portraitMediaId",
    "tokenMediaId",
    "tokenCrop",
    "tokenColor",
    "dndbeyondId",
    "dndbeyondSyncedAt",
];

/// One version of an element, as it was written: never changed since.
#[derive(Clone, Debug, PartialEq)]
pub struct Kept {
    /// Its id.
    pub id: String,
    /// Its number in the element's history, from 1.
    pub number: u64,
    /// The element, its sheet and its aliases as it left them.
    pub snapshot: Map<String, Value>,
    /// What it changed from the version before, `{field, old, new}`.
    pub diff: Vec<Value>,
}

impl Kept {
    /// `row` carrying this version: its number, its snapshot and its diff.
    #[must_use]
    pub fn stamped(&self, row: &Row) -> Row {
        let mut stamped = row.clone();
        stamped.insert("versionNumber".into(), self.number.into());
        stamped.insert("snapshot".into(), Value::Object(self.snapshot.clone()));
        stamped.insert("diff".into(), Value::Array(self.diff.clone()));
        stamped
    }
}

/// The version `row` — an element's row as a save leaves it — adds after
/// `last`, the last version the element has (`None`: it has none): the
/// number after it, the snapshot of `row`, the diff from `last`'s snapshot.
/// `None` when the row carries no version id.
#[must_use]
pub fn settled(last: Option<&Kept>, row: &Row) -> Option<Kept> {
    let id = row.get("entityVersionId")?.as_str()?.to_owned();
    let empty = Map::new();
    let (number, previous) = last.map_or((1, &empty), |last| {
        (last.number.saturating_add(1), &last.snapshot)
    });
    let snapshot = snapshot_of(row);
    let diff = diff(previous, &snapshot);
    Some(Kept {
        id,
        number,
        snapshot,
        diff,
    })
}

/// The snapshot of an element's row: every field of the element and of its
/// sheet it holds, and its `aliases`.
#[must_use]
pub fn snapshot_of(row: &Row) -> Map<String, Value> {
    let mut snapshot: Map<String, Value> = SNAPSHOT
        .iter()
        .filter_map(|field| row.get(*field).map(|v| ((*field).to_owned(), v.clone())))
        .collect();
    if let Some(aliases) = row.get(ALIAS) {
        snapshot.insert("aliases".into(), aliases.clone());
    }
    snapshot
}

/// The field-by-field difference from `previous` to `snapshot`
/// (EntityVersion@1): a field whose value differs, an absent field and a
/// `null` being the same, sorted by field in code point order.
#[must_use]
pub fn diff(previous: &Map<String, Value>, snapshot: &Map<String, Value>) -> Vec<Value> {
    let fields: BTreeSet<&String> = previous.keys().chain(snapshot.keys()).collect();
    fields
        .into_iter()
        .filter_map(|field| {
            let old = previous.get(field).unwrap_or(&Value::Null);
            let new = snapshot.get(field).unwrap_or(&Value::Null);
            (!same(old, new)).then(|| {
                let mut change = Map::new();
                change.insert("field".into(), field.clone().into());
                change.insert("old".into(), old.clone());
                change.insert("new".into(), new.clone());
                Value::Object(change)
            })
        })
        .collect()
}

/// Two JSON values compared as JSON, numbers by value (15 and 15.0 are the
/// same number).
fn same(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(a), Value::Number(b)) => {
            a == b || matches!((a.as_f64(), b.as_f64()), (Some(x), Some(y)) if x == y)
        }
        (Value::Array(a), Value::Array(b)) => {
            a.len() == b.len() && a.iter().zip(b).all(|(a, b)| same(a, b))
        }
        (Value::Object(a), Value::Object(b)) => {
            a.len() == b.len()
                && a.iter()
                    .all(|(key, a)| b.get(key).is_some_and(|b| same(a, b)))
        }
        _ => a == b,
    }
}

/// `millis` since the epoch as an element's dates are written: RFC 3339, in
/// UTC, to the millisecond (`2026-10-17T10:00:00.000Z`).
#[must_use]
pub fn timestamp(millis: u64) -> String {
    let seconds = millis / 1000;
    let (year, month, day) = civil(seconds / 86_400);
    let time = seconds % 86_400;
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{:03}Z",
        time / 3600,
        time % 3600 / 60,
        time % 60,
        millis % 1000
    )
}

/// The date `days` after 1970-01-01 (H. Hinnant's `civil_from_days`).
const fn civil(days: u64) -> (u64, u64, u64) {
    let days = days + 719_468;
    let era = days / 146_097;
    let doe = days % 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + if month <= 2 { 1 } else { 0 };
    (year, month, day)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn row(value: Value) -> Row {
        let Value::Object(map) = value else {
            panic!("a row is an object");
        };
        map.into_iter().collect()
    }

    fn kept(number: u64, snapshot: Value) -> Kept {
        let Value::Object(snapshot) = snapshot else {
            panic!("a snapshot is an object");
        };
        Kept {
            id: format!("v{number}"),
            number,
            snapshot,
            diff: Vec::new(),
        }
    }

    #[test]
    fn a_first_version_is_1_and_its_diff_is_the_whole_snapshot() {
        let saved = row(json!({
            "entityId": "e1", "entityVersionId": "v1", "type": "pnj", "name": "Morlox",
            "alias": ["le gobelin"], "updatedAt": "2026-10-17T10:00:00.000Z",
        }));
        let version = settled(None, &saved).expect("a version");
        assert_eq!(version.id, "v1");
        assert_eq!(version.number, 1);
        assert_eq!(
            Value::Object(version.snapshot.clone()),
            json!({ "type": "pnj", "name": "Morlox", "aliases": ["le gobelin"] })
        );
        let fields: Vec<&Value> = version.diff.iter().map(|c| &c["field"]).collect();
        assert_eq!(fields, [&json!("aliases"), &json!("name"), &json!("type")]);
    }

    #[test]
    fn a_version_follows_the_last_one_kept_whatever_the_row_was_numbered() {
        let last = kept(4, json!({ "type": "pnj", "name": "Morlox", "notes": "v4" }));
        let saved = row(json!({
            "entityVersionId": "v9", "versionNumber": 9, "type": "pnj", "name": "Morlox",
            "notes": "v5", "snapshot": { "notes": "never applied" },
        }));
        let version = settled(Some(&last), &saved).expect("a version");
        assert_eq!(version.number, 5);
        assert_eq!(
            version.diff,
            [json!({ "field": "notes", "old": "v4", "new": "v5" })]
        );
        let stamped = version.stamped(&saved);
        assert_eq!(stamped["versionNumber"], 5);
        assert_eq!(stamped["snapshot"]["notes"], "v5");
        assert_eq!(stamped["diff"], json!(version.diff));
        assert_eq!(stamped["name"], "Morlox");
        assert_eq!(
            settled(Some(&kept(u64::MAX, json!({}))), &saved).map(|v| v.number),
            Some(u64::MAX)
        );
    }

    #[test]
    fn a_row_carrying_no_version_id_settles_none() {
        assert_eq!(settled(None, &row(json!({ "name": "Morlox" }))), None);
        assert_eq!(settled(None, &row(json!({ "entityVersionId": 7 }))), None);
    }

    #[test]
    fn a_diff_compares_numbers_by_value_inside_lists_and_objects_too() {
        let snapshot = |value: Value| match value {
            Value::Object(map) => map,
            _ => panic!("a snapshot is an object"),
        };
        let before = snapshot(json!({
            "hpMax": 15, "spells": [1, { "level": 2 }], "tokenCrop": { "x": 1 },
            "name": "Morlox", "notes": null,
        }));
        let same_values = snapshot(json!({
            "hpMax": 15.0, "spells": [1.0, { "level": 2.0 }], "tokenCrop": { "x": 1.0 },
            "name": "Morlox",
        }));
        assert_eq!(diff(&before, &same_values), Vec::<Value>::new());
        for (field, changed) in [
            ("hpMax", json!(16)),
            ("spells", json!([1, { "level": 3 }])),
            ("spells", json!([1])),
            ("spells", json!([1, { "level": 2 }, 3])),
            ("tokenCrop", json!({ "x": 2 })),
            ("tokenCrop", json!({ "y": 1 })),
            ("tokenCrop", json!({ "x": 1, "y": 1 })),
            ("name", json!("Morlo")),
            ("name", json!(15)),
        ] {
            let mut after = same_values.clone();
            after.insert(field.into(), changed.clone());
            assert_eq!(
                diff(&before, &after),
                [json!({ "field": field, "old": before[field], "new": changed })],
                "{field}: {changed}"
            );
        }
    }

    /// The date `days` after 1970-01-01, counted a day at a time.
    fn counted(days: u64) -> (u64, u64, u64) {
        let leap = |year: u64| {
            (year.is_multiple_of(4) && !year.is_multiple_of(100)) || year.is_multiple_of(400)
        };
        let (mut year, mut left) = (1970, days);
        loop {
            let length = if leap(year) { 366 } else { 365 };
            if left < length {
                break;
            }
            left -= length;
            year += 1;
        }
        let mut month = 1;
        for length in [
            31,
            if leap(year) { 29 } else { 28 },
            31,
            30,
            31,
            30,
            31,
            31,
            30,
            31,
            30,
            31,
        ] {
            if left < length {
                break;
            }
            left -= length;
            month += 1;
        }
        (year, month, left + 1)
    }

    #[test]
    fn every_day_from_1970_to_2500_is_its_calendar_date() {
        for days in 0..193_944 {
            assert_eq!(civil(days), counted(days), "{days}");
        }
    }

    #[test]
    fn a_timestamp_is_rfc_3339_in_utc_to_the_millisecond() {
        assert_eq!(timestamp(0), "1970-01-01T00:00:00.000Z");
        assert_eq!(timestamp(1_700_000_123_456), "2023-11-14T22:15:23.456Z");
        assert_eq!(timestamp(1_709_164_799_999), "2024-02-28T23:59:59.999Z");
        assert_eq!(timestamp(1_709_164_800_007), "2024-02-29T00:00:00.007Z");
        assert_eq!(timestamp(951_782_400_000), "2000-02-29T00:00:00.000Z");
        assert_eq!(timestamp(1_792_234_861_010), "2026-10-17T11:01:01.010Z");
    }
}
