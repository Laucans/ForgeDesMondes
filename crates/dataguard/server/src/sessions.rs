//! What a session is judged against beyond its own row: each campaign's
//! kind, its sessions and their positions, its characters and their types —
//! what `present-only-pj` and `oneshot-single-session` read, and where the
//! next session of a campaign goes. The engine judges a row alone, and this
//! is the rest of its campaign.
//!
//! It follows what the engine applies: [`crate::versioning::Versioning`]
//! records each write of a campaign, an element or a session once it is
//! applied, over the engine's memory and over a database alike, and forgets
//! an element or a session when it is removed. Over a database it also
//! takes what the tables hold of a campaign — the fixtures' rows, which no
//! command wrote — each time a session of that campaign is prepared
//! ([`crate::Rows::refresh`]). Campaigns, sessions and elements are told
//! apart by their id parsed as a uuid: the same id in another case is the
//! same one.

use std::collections::BTreeMap;
use std::sync::RwLock;

use serde_json::Value;

use crate::aliases::parsed;

/// A campaign that is a oneshot: one session, never a second.
pub const ONESHOT: &str = "oneshot";

/// The type of element that is a PC.
const PC: &str = "pj";

/// What is known of the campaigns, by id.
#[derive(Debug, Default)]
struct Facts {
    /// A campaign's kind.
    kinds: BTreeMap<u128, String>,
    /// A session's campaign and position.
    sessions: BTreeMap<u128, (u128, u64)>,
    /// An element's campaign and type.
    elements: BTreeMap<u128, (u128, String)>,
}

/// The campaigns' kinds, sessions and elements the engine has applied, and
/// those the tables hold.
#[derive(Debug, Default)]
pub struct Sessions {
    facts: RwLock<Facts>,
}

/// The text `field` holds in `row`.
fn text<'a>(row: &'a BTreeMap<String, Value>, field: &str) -> Option<&'a str> {
    row.get(field).and_then(Value::as_str)
}

impl Sessions {
    /// The campaign `id` is of `kind`.
    pub fn campaign(&self, id: &str, kind: &str) {
        if let (Some(id), Ok(mut facts)) = (parsed(id), self.facts.write()) {
            facts.kinds.insert(id, kind.to_owned());
        }
    }

    /// The session `id` is of `campaign`, at `position`.
    pub fn session(&self, id: &str, campaign: &str, position: u64) {
        if let (Some(id), Some(campaign), Ok(mut facts)) =
            (parsed(id), parsed(campaign), self.facts.write())
        {
            facts.sessions.insert(id, (campaign, position));
        }
    }

    /// The element `id` is of `campaign`, of type `kind`.
    pub fn element(&self, id: &str, campaign: &str, kind: &str) {
        if let (Some(id), Some(campaign), Ok(mut facts)) =
            (parsed(id), parsed(campaign), self.facts.write())
        {
            facts.elements.insert(id, (campaign, kind.to_owned()));
        }
    }

    /// The campaign, session or element `id` is no more.
    pub fn forget(&self, id: &str) {
        if let (Some(id), Ok(mut facts)) = (parsed(id), self.facts.write()) {
            facts.kinds.remove(&id);
            facts.sessions.remove(&id);
            facts.elements.remove(&id);
        }
    }

    /// Records what the row `id` names of `aggregate` says: a campaign's kind,
    /// a session's campaign and position, an element's campaign and type.
    pub fn record(&self, aggregate: &str, id: &str, row: &BTreeMap<String, Value>) {
        match aggregate {
            "Campaign" => {
                if let Some(kind) = text(row, "kind") {
                    self.campaign(id, kind);
                }
            }
            "Session" => {
                if let Some(campaign) = text(row, "sessionCampaignId") {
                    let position = row
                        .get("sessionPosition")
                        .and_then(Value::as_u64)
                        .unwrap_or_default();
                    self.session(id, campaign, position);
                }
            }
            "Entity" => {
                if let (Some(campaign), Some(kind)) =
                    (text(row, "entityCampaignId"), text(row, "type"))
                {
                    self.element(id, campaign, kind);
                }
            }
            _ => {}
        }
    }

    /// Whether `campaign` is one whose kind is known: a campaign the engine or
    /// the tables hold.
    pub fn known(&self, campaign: &str) -> bool {
        let Some(campaign) = parsed(campaign) else {
            return false;
        };
        self.facts
            .read()
            .is_ok_and(|facts| facts.kinds.contains_key(&campaign))
    }

    /// Whether `campaign` is a oneshot.
    pub fn oneshot(&self, campaign: &str) -> bool {
        let Some(campaign) = parsed(campaign) else {
            return false;
        };
        self.facts
            .read()
            .is_ok_and(|facts| facts.kinds.get(&campaign).is_some_and(|k| k == ONESHOT))
    }

    /// How many sessions `campaign` has besides `session`.
    pub fn others(&self, campaign: &str, session: &str) -> usize {
        let (Some(campaign), session) = (parsed(campaign), parsed(session)) else {
            return 0;
        };
        self.facts.read().map_or(0, |facts| {
            facts
                .sessions
                .iter()
                .filter(|(id, (of, _))| *of == campaign && Some(**id) != session)
                .count()
        })
    }

    /// The position after the last of `campaign`'s sessions besides
    /// `session`: 1 for its first.
    pub fn next_position(&self, campaign: &str, session: &str) -> u64 {
        let (Some(campaign), session) = (parsed(campaign), parsed(session)) else {
            return 1;
        };
        self.facts.read().map_or(1, |facts| {
            facts
                .sessions
                .iter()
                .filter(|(id, (of, _))| *of == campaign && Some(**id) != session)
                .map(|(_, (_, position))| *position)
                .max()
                .unwrap_or(0)
                + 1
        })
    }

    /// Whether `character` is a PC of `campaign`.
    pub fn pc_of(&self, character: &str, campaign: &str) -> bool {
        let (Some(character), Some(campaign)) = (parsed(character), parsed(campaign)) else {
            return false;
        };
        self.facts.read().is_ok_and(|facts| {
            facts
                .elements
                .get(&character)
                .is_some_and(|(of, kind)| *of == campaign && kind == PC)
        })
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    const VALOMBRE: &str = "c1000000-0000-4000-8000-000000000001";
    const BRUME: &str = "c1000000-0000-4000-8000-000000000002";
    const ELOWEN: &str = "e1000000-0000-4000-8000-000000000001";
    const MORLOX: &str = "e1000000-0000-4000-8000-000000000003";
    const SEANCE: &str = "51000000-0000-4000-8000-000000000001";
    const SUITE: &str = "51000000-0000-4000-8000-000000000002";

    fn row(fields: Value) -> BTreeMap<String, Value> {
        let Value::Object(fields) = fields else {
            panic!("a row is an object");
        };
        fields.into_iter().collect()
    }

    #[test]
    fn a_pc_is_one_of_its_own_campaign_whatever_the_case_of_its_id() {
        let sessions = Sessions::default();
        sessions.record(
            "Entity",
            ELOWEN,
            &row(json!({ "entityCampaignId": VALOMBRE, "type": "pj" })),
        );
        sessions.record(
            "Entity",
            MORLOX,
            &row(json!({ "entityCampaignId": VALOMBRE, "type": "pnj" })),
        );
        assert!(sessions.pc_of(&ELOWEN.to_uppercase(), VALOMBRE));
        assert!(!sessions.pc_of(ELOWEN, BRUME));
        assert!(!sessions.pc_of(MORLOX, VALOMBRE));
        assert!(!sessions.pc_of("elowen", VALOMBRE));
        sessions.forget(ELOWEN);
        assert!(!sessions.pc_of(ELOWEN, VALOMBRE));
    }

    #[test]
    fn a_campaign_s_sessions_are_counted_and_the_next_goes_after_the_last() {
        let sessions = Sessions::default();
        assert_eq!(sessions.next_position(VALOMBRE, SUITE), 1);
        sessions.record(
            "Session",
            SEANCE,
            &row(json!({ "sessionCampaignId": VALOMBRE, "sessionPosition": 3 })),
        );
        assert_eq!(sessions.others(VALOMBRE, SUITE), 1);
        assert_eq!(sessions.others(VALOMBRE, SEANCE), 0);
        assert_eq!(sessions.others(BRUME, SUITE), 0);
        assert_eq!(sessions.next_position(VALOMBRE, SUITE), 4);
        // A session is not after itself.
        assert_eq!(sessions.next_position(VALOMBRE, SEANCE), 1);
        sessions.forget(SEANCE);
        assert_eq!(sessions.others(VALOMBRE, SUITE), 0);
    }

    #[test]
    fn a_campaign_is_a_oneshot_once_its_kind_says_so() {
        let sessions = Sessions::default();
        assert!(!sessions.oneshot(BRUME));
        assert!(!sessions.known(BRUME));
        sessions.record("Campaign", BRUME, &row(json!({ "kind": "oneshot" })));
        assert!(sessions.known(&BRUME.to_uppercase()));
        sessions.record("Campaign", VALOMBRE, &row(json!({ "kind": "campagne" })));
        assert!(sessions.oneshot(BRUME));
        assert!(!sessions.oneshot(VALOMBRE));
        // What another aggregate's row says is not a campaign's kind.
        sessions.record("Media", VALOMBRE, &row(json!({ "kind": "oneshot" })));
        assert!(!sessions.oneshot(VALOMBRE));
        // A campaign removed is no longer known.
        sessions.forget(BRUME);
        assert!(!sessions.known(BRUME));
        assert!(!sessions.oneshot(BRUME));
    }
}
