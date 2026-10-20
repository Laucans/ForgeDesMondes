//! What a new scene is prepared against beyond its own row: each session's
//! campaign, and each scene's session and rank — whether its session is one
//! the product holds, and where its next scene goes
//! (`scene-order-contiguous`). The engine judges a row alone, and this is
//! the rest of its session. The places it links are judged as a place put in
//! a scene is (`crate::scene_places`).
//!
//! It follows what the engine applies: [`crate::versioning::Versioning`]
//! records each write of a session or a scene once it is applied, over the
//! engine's memory and over a database alike, and forgets one when it is
//! removed. Over a database it also takes what the tables hold — the
//! fixtures' rows, which no command wrote — at a start, and of a session
//! each time a scene of it is prepared ([`crate::Rows::refresh_session`]).
//! Sessions and scenes are told apart by their id parsed as a uuid: the
//! same id in another case is the same one.

use std::collections::BTreeMap;
use std::sync::RwLock;

use add_scene::{POSITION, SESSION};
use serde_json::Value;

use crate::aliases::parsed;

/// A session's row: the campaign it is of.
const SESSION_CAMPAIGN: &str = "sessionCampaignId";

/// What is known of the sessions, by id.
#[derive(Debug, Default)]
struct Facts {
    /// A session's campaign, in lower case.
    sessions: BTreeMap<u128, String>,
    /// A scene's session and rank.
    scenes: BTreeMap<u128, (u128, u64)>,
}

/// The sessions and scenes the engine has applied, and those the tables
/// hold.
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
    /// campaign, a scene's session and rank. A session's row may hold none
    /// of them: a write of its status alone.
    pub fn record(&self, aggregate: &str, id: &str, row: &BTreeMap<String, Value>) {
        let Some(key) = parsed(id).filter(|_| aggregate == "Session") else {
            return;
        };
        let mut facts = self.facts.write().unwrap_or_else(|e| e.into_inner());
        if let Some(campaign) = row
            .get(SESSION_CAMPAIGN)
            .and_then(Value::as_str)
            .filter(|campaign| parsed(campaign).is_some())
        {
            facts.sessions.insert(key, campaign.to_lowercase());
        }
        if let Some(session) = uuid_in(row, SESSION) {
            let rank = row.get(POSITION).and_then(Value::as_u64).unwrap_or(0);
            facts.scenes.insert(key, (session, rank));
        }
    }

    /// The session or scene `id` is no more.
    pub fn forget(&self, id: &str) {
        if let Some(id) = parsed(id) {
            let mut facts = self.facts.write().unwrap_or_else(|e| e.into_inner());
            facts.sessions.remove(&id);
            facts.scenes.remove(&id);
        }
    }

    /// The campaign of `session`, in lower case, when it is known: a session
    /// the engine or the tables hold.
    pub fn campaign_of(&self, session: &str) -> Option<String> {
        let session = parsed(session)?;
        let facts = self.facts.read().unwrap_or_else(|e| e.into_inner());
        facts.sessions.get(&session).cloned()
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
    const ARRIVEE: &str = "5c000000-0000-4000-8000-000000000001";
    const ROUTE: &str = "5c000000-0000-4000-8000-000000000002";

    fn row(fields: Value) -> BTreeMap<String, Value> {
        let Value::Object(fields) = fields else {
            panic!("a row is an object");
        };
        fields.into_iter().collect()
    }

    #[test]
    fn a_session_s_campaign_is_known_whatever_the_case_of_its_id() {
        let scenes = Scenes::default();
        scenes.record(
            "Session",
            SEANCE,
            &row(json!({ "sessionCampaignId": VALOMBRE.to_uppercase() })),
        );
        assert_eq!(
            scenes.campaign_of(&SEANCE.to_uppercase()).as_deref(),
            Some(VALOMBRE)
        );
        assert_eq!(scenes.campaign_of(ROUTE), None);
        assert_eq!(scenes.campaign_of("seance-4"), None);
        // A campaign that is no id, or another aggregate's row, is no
        // session's campaign.
        scenes.record(
            "Session",
            ROUTE,
            &row(json!({ "sessionCampaignId": "valombre" })),
        );
        assert_eq!(scenes.campaign_of(ROUTE), None);
        scenes.record("Media", ROUTE, &row(json!({ "sessionCampaignId": BRUME })));
        assert_eq!(scenes.campaign_of(ROUTE), None);
        scenes.forget(SEANCE);
        assert_eq!(scenes.campaign_of(SEANCE), None);
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
