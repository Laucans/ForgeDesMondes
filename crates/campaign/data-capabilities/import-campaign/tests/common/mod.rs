//! What the engine's tests and PostgreSQL's share: the cases of
//! `CampaignArchive@2` run through a real engine, which draws the ids the
//! case gives and reads the clock the case gives.

use std::collections::VecDeque;
use std::sync::Mutex;

use harness_dataguard::store::Ids;
use serde_json::{Value, json};

/// The Concept this unit's copy is held to.
pub const CONCEPT: &str = "CampaignArchive@2";

/// The ids of a case's `new_ids`, in the order the unit draws them: one for
/// each id its archive holds, by the id it replaces.
pub struct Given(Mutex<VecDeque<String>>);

impl Given {
    /// The ids `input` (a case's input) gives.
    pub fn of(input: &Value) -> Self {
        let wire = json!({ "archive": input["archive"] });
        let request =
            import_campaign::request(&serde_json::from_value(wire).expect("an object"), None)
                .expect("a well formed archive");
        let new_ids = input["new_ids"].as_object().expect("new ids");
        Self(Mutex::new(
            request
                .ids
                .iter()
                .map(|old| new_ids[old].as_str().expect("an id").to_owned())
                .collect(),
        ))
    }
}

impl Ids for Given {
    fn fresh(&self) -> String {
        self.0
            .lock()
            .expect("the ids")
            .pop_front()
            .expect("an id for every id of the archive")
    }
}

/// `rfc3339` (`2026-10-20T14:00:00.000Z`, as the Concept's cases write the
/// time of an import) in milliseconds since the epoch.
pub fn millis(rfc3339: &str) -> u64 {
    let number = |from: usize, to: usize| -> i64 { rfc3339[from..to].parse().expect("a number") };
    let (year, month, day) = (number(0, 4), number(5, 7), number(8, 10));
    let year = year - i64::from(month <= 2);
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let day_of_year = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    let days = era * 146_097 + day_of_era - 719_468;
    let seconds = days * 86_400 + number(11, 13) * 3600 + number(14, 16) * 60 + number(17, 19);
    let millis = u64::try_from(seconds * 1000 + number(20, 23)).expect("after the epoch");
    assert_eq!(
        import_campaign::timestamp(millis),
        rfc3339,
        "a time to the millisecond"
    );
    millis
}
