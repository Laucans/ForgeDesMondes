//! What a scene is judged against beyond its own row: each session's
//! campaign, each element's campaign, and each scene's session and rank —
//! what `scene-links-same-campaign` reads, and where a session's next scene
//! goes (`scene-order-contiguous`). The engine judges a row alone, and this
//! is the rest of its session.
//!
//! It follows what the engine applies: [`crate::versioning::Versioning`]
//! records each write of a session, a scene or an element once it is
//! applied, over the engine's memory and over a database alike, and forgets
//! one when it is removed. Over a database it also takes what the tables
//! hold — the fixtures' rows, which no command wrote — at a start, and of a
//! session each time a scene of it is prepared ([`crate::Rows::refresh_session`]).
//! Sessions, scenes and elements are told apart by their id parsed as a
//! uuid: the same id in another case is the same one.

use std::collections::BTreeMap;
use std::sync::RwLock;

use add_scene::{POSITION, SESSION};
use serde_json::Value;

use crate::aliases::parsed;

/// A session's row: the campaign it is of.
const SESSION_CAMPAIGN: &str = "sessionCampaignId";

/// An element's row: the campaign it is of.
const ELEMENT_CAMPAIGN: &str = "entityCampaignId";

/// What is known of the sessions, by id.
#[derive(Debug, Default)]
struct Facts {
    /// A session's campaign.
    sessions: BTreeMap<u128, u128>,
    /// An element's campaign.
    elements: BTreeMap<u128, u128>,
    /// A scene's session and rank.
    scenes: BTreeMap<u128, (u128, u64)>,
}

/// The sessions, elements and scenes the engine has applied, and those the
/// tables hold.
#[derive(Debug, Default)]
pub struct Scenes {
    facts: RwLock<Facts>,
}

/// The id `field` holds in `row`, parsed.
fn uuid_in(row: &BTreeMap<String, Value>, field: &str) -> Option<u128> {
    row.get(field).and_then(Value::as_str).and_then(parsed)
}

impl Scenes {
    /// Records what the row `id` names of `aggregate` says: a session's
    /// campaign, a scene's session and rank, an element's campaign. A
    /// session's row may hold none of them: a write of its status alone.
    pub fn record(&self, aggregate: &str, id: &str, row: &BTreeMap<String, Value>) {
        let Some(key) = parsed(id) else {
            return;
        };
        let mut facts = self.facts.write().unwrap_or_else(|e| e.into_inner());
        match aggregate {
            "Session" => {
                if let Some(campaign) = uuid_in(row, SESSION_CAMPAIGN) {
                    facts.sessions.insert(key, campaign);
                }
                if let Some(session) = uuid_in(row, SESSION) {
                    let rank = row.get(POSITION).and_then(Value::as_u64).unwrap_or(0);
                    facts.scenes.insert(key, (session, rank));
                }
            }
            "Entity" => {
                if let Some(campaign) = uuid_in(row, ELEMENT_CAMPAIGN) {
                    facts.elements.insert(key, campaign);
                }
            }
            _ => {}
        }
    }

    /// The session, scene or element `id` is no more.
    pub fn forget(&self, id: &str) {
        if let Some(id) = parsed(id) {
            let mut facts = self.facts.write().unwrap_or_else(|e| e.into_inner());
            facts.sessions.remove(&id);
            facts.elements.remove(&id);
            facts.scenes.remove(&id);
        }
    }

    /// Whether `session` is one whose campaign is known: a session the
    /// engine or the tables hold.
    pub fn knows(&self, session: &str) -> bool {
        self.campaign_of(session).is_some()
    }

    /// The campaign of `session`, when it is known.
    fn campaign_of(&self, session: &str) -> Option<u128> {
        let session = parsed(session)?;
        let facts = self.facts.read().unwrap_or_else(|e| e.into_inner());
        facts.sessions.get(&session).copied()
    }

    /// Whether every one of `elements` is an element of `session`'s
    /// campaign: an element, or a session, nobody knows is none.
    pub fn same_campaign<'a>(
        &self,
        session: &str,
        mut elements: impl Iterator<Item = &'a str>,
    ) -> bool {
        let campaign = self.campaign_of(session);
        let facts = self.facts.read().unwrap_or_else(|e| e.into_inner());
        elements.all(|element| {
            campaign.is_some()
                && parsed(element).and_then(|e| facts.elements.get(&e).copied()) == campaign
        })
    }

    /// The rank after the last of `session`'s scenes besides `scene`: 1 for
    /// its first.
    pub fn next_position(&self, session: &str, scene: &str) -> u64 {
        let (Some(session), scene) = (parsed(session), parsed(scene)) else {
            return 1;
        };
        let facts = self.facts.read().unwrap_or_else(|e| e.into_inner());
        facts
            .scenes
            .iter()
            .filter(|(id, (of, _))| *of == session && Some(**id) != scene)
            .map(|(_, (_, rank))| *rank)
            .max()
            .unwrap_or(0)
            + 1
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    const VALOMBRE: &str = "c1000000-0000-4000-8000-000000000001";
    const BRUME: &str = "c1000000-0000-4000-8000-000000000002";
    const SEANCE: &str = "51000000-0000-4000-8000-000000000001";
    const TAVERNE: &str = "e1000000-0000-4000-8000-000000000003";
    const PHARE: &str = "e1000000-0000-4000-8000-000000000009";
    const ARRIVEE: &str = "5c000000-0000-4000-8000-000000000001";
    const ROUTE: &str = "5c000000-0000-4000-8000-000000000002";

    fn row(fields: Value) -> BTreeMap<String, Value> {
        let Value::Object(fields) = fields else {
            panic!("a row is an object");
        };
        fields.into_iter().collect()
    }

    #[test]
    fn an_element_links_a_scene_of_its_own_campaign_only_whatever_the_case_of_its_id() {
        let scenes = Scenes::default();
        scenes.record(
            "Session",
            SEANCE,
            &row(json!({ "sessionCampaignId": VALOMBRE })),
        );
        scenes.record(
            "Entity",
            TAVERNE,
            &row(json!({ "entityCampaignId": VALOMBRE })),
        );
        scenes.record("Entity", PHARE, &row(json!({ "entityCampaignId": BRUME })));
        assert!(scenes.knows(&SEANCE.to_uppercase()));
        assert!(scenes.same_campaign(SEANCE, [TAVERNE.to_uppercase().as_str()].into_iter()));
        assert!(scenes.same_campaign(SEANCE, std::iter::empty()));
        assert!(!scenes.same_campaign(SEANCE, [TAVERNE, PHARE].into_iter()));
        // An element, or a session, nobody knows is none of the campaign's.
        assert!(!scenes.same_campaign(SEANCE, ["taverne-sanglier"].into_iter()));
        assert!(!scenes.same_campaign(ROUTE, [TAVERNE].into_iter()));
        // What another aggregate's row says is no element's campaign.
        scenes.record(
            "Media",
            PHARE,
            &row(json!({ "entityCampaignId": VALOMBRE })),
        );
        assert!(!scenes.same_campaign(SEANCE, [PHARE].into_iter()));
        scenes.forget(TAVERNE);
        assert!(!scenes.same_campaign(SEANCE, [TAVERNE].into_iter()));
        scenes.forget(SEANCE);
        assert!(!scenes.knows(SEANCE));
    }

    #[test]
    fn a_session_s_next_scene_goes_after_its_last() {
        let scenes = Scenes::default();
        assert_eq!(scenes.next_position(SEANCE, ROUTE), 1);
        scenes.record(
            "Session",
            ARRIVEE,
            &row(json!({ "sceneSessionId": SEANCE, "scenePosition": 2 })),
        );
        assert_eq!(scenes.next_position(&SEANCE.to_uppercase(), ROUTE), 3);
        // A scene is not after itself, nor is another session's.
        assert_eq!(scenes.next_position(SEANCE, ARRIVEE), 1);
        assert_eq!(scenes.next_position(VALOMBRE, ROUTE), 1);
        // A session's own row, its status written alone, is no scene.
        scenes.record("Session", SEANCE, &row(json!({ "sessionStatus": "live" })));
        assert_eq!(scenes.next_position(SEANCE, ROUTE), 3);
        scenes.forget(ARRIVEE);
        assert_eq!(scenes.next_position(SEANCE, ROUTE), 1);
    }
}
