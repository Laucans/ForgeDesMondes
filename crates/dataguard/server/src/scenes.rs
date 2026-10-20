//! What a scene is prepared and judged against beyond its own row: each
//! session's campaign, and each scene's session and rank — whether a new
//! scene's session is one the product holds, where its next scene goes, and
//! whether a reorder lists every scene of its session, each once
//! (`scene-order-contiguous`). The engine judges a row alone, and this is
//! the rest of its session. The places a new scene links are judged as a
//! place put in a scene is (`crate::scene_places`).
//!
//! It follows what the engine applies: [`crate::versioning::Versioning`]
//! records each write of a session or a scene once it is applied, over the
//! engine's memory and over a database alike, and forgets one when it is
//! removed. Over a database it also takes what the tables hold — the
//! fixtures' rows, which no command wrote — at a start, of a session each
//! time a scene of it is prepared ([`crate::Rows::refresh_session`]), and of
//! its scenes each time a reorder of it is submitted
//! ([`crate::Rows::refresh_scenes`]): taken beside what it knows, never over
//! it, so a scene applied while the tables were read is not lost. Sessions
//! and scenes are told apart by their id parsed as a uuid: the same id in
//! another case is the same one.

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

    /// The session or scene `id` is no more — nor, when `id` names a
    /// session, are its scenes: they go with it.
    pub fn forget(&self, id: &str) {
        if let Some(id) = parsed(id) {
            let mut facts = self.facts.write().unwrap_or_else(|e| e.into_inner());
            facts.sessions.remove(&id);
            facts
                .scenes
                .retain(|scene, (session, _)| *scene != id && *session != id);
        }
    }

    /// Records each `(scene, session)` pair the tables and the engine's rows
    /// hold, beside the scenes known — a rank known kept: the ones that do
    /// not parse left out.
    pub fn take(&self, scenes: &[(String, String)]) {
        let mut facts = self.facts.write().unwrap_or_else(|e| e.into_inner());
        for (scene, session) in scenes {
            if let (Some(scene), Some(session)) = (parsed(scene), parsed(session)) {
                let rank = facts.scenes.get(&scene).map_or(0, |(_, rank)| *rank);
                facts.scenes.insert(scene, (session, rank));
            }
        }
    }

    /// Whether `order` is every scene of `session`, each once: a scene it
    /// omits, repeats, takes from another session — or an id that is no
    /// scene — and it is not.
    pub fn orders(&self, session: &str, order: &[Value]) -> bool {
        let Some(session) = parsed(session) else {
            return false;
        };
        let facts = self.facts.read().unwrap_or_else(|e| e.into_inner());
        let mut ranked = Vec::with_capacity(order.len());
        for scene in order {
            match scene.as_str().and_then(parsed) {
                Some(scene) if facts.scenes.get(&scene).map(|(of, _)| *of) == Some(session) => {
                    ranked.push(scene);
                }
                _ => return false,
            }
        }
        ranked.sort_unstable();
        ranked.dedup();
        ranked.len() == order.len()
            && facts
                .scenes
                .values()
                .filter(|(of, _)| *of == session)
                .count()
                == ranked.len()
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
    const AUTRE_SEANCE: &str = "51000000-0000-4000-8000-000000000002";
    const GUE: &str = "5c000000-0000-4000-8000-000000000003";
    const AILLEURS: &str = "5c000000-0000-4000-8000-000000000009";

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

    fn scene(id: &str, session: &str) -> BTreeMap<String, Value> {
        row(json!({ "sceneId": id, "sceneSessionId": session, "scenePosition": 1 }))
    }

    /// `id` recorded as a scene of `session`, as its partition's write.
    fn record_scene(scenes: &Scenes, id: &str, session: &str) {
        scenes.record("Session", id, &scene(id, session));
    }

    fn order(ids: &[&str]) -> Vec<Value> {
        ids.iter().map(|id| json!(id)).collect()
    }

    /// Session 1's three scenes, and a scene of another session.
    fn scenes() -> Scenes {
        let scenes = Scenes::default();
        for id in [ARRIVEE, ROUTE, GUE] {
            record_scene(&scenes, id, SEANCE);
        }
        record_scene(&scenes, AILLEURS, AUTRE_SEANCE);
        scenes
    }

    #[test]
    fn an_order_is_every_scene_of_its_session_each_once_whatever_the_case() {
        let scenes = scenes();
        assert!(scenes.orders(SEANCE, &order(&[GUE, ARRIVEE, ROUTE])));
        assert!(scenes.orders(
            &SEANCE.to_uppercase(),
            &order(&[&GUE.to_uppercase(), ARRIVEE, ROUTE])
        ));
        assert!(scenes.orders(AUTRE_SEANCE, &order(&[AILLEURS])));
    }

    #[test]
    fn an_order_omitting_repeating_or_borrowing_a_scene_is_none() {
        let scenes = scenes();
        // Omits the ford.
        assert!(!scenes.orders(SEANCE, &order(&[ROUTE, ARRIVEE])));
        // Repeats the road, in place of the ford.
        assert!(!scenes.orders(SEANCE, &order(&[ROUTE, ARRIVEE, ROUTE])));
        // Repeats the road, beside every scene.
        assert!(!scenes.orders(SEANCE, &order(&[ROUTE, ARRIVEE, GUE, ROUTE])));
        // Takes a scene of another session.
        assert!(!scenes.orders(SEANCE, &order(&[GUE, ARRIVEE, ROUTE, AILLEURS])));
        assert!(!scenes.orders(SEANCE, &order(&[GUE, ARRIVEE, AILLEURS])));
        // Names what is no scene, or no id at all.
        assert!(!scenes.orders(SEANCE, &order(&[GUE, ARRIVEE, ROUTE, "sc1"])));
        assert!(!scenes.orders(SEANCE, &[json!(GUE), json!(ARRIVEE), json!(1)]));
        // Of a session that is none, or no uuid.
        assert!(!scenes.orders("seance-1", &order(&[GUE, ARRIVEE, ROUTE])));
        assert!(!scenes.orders(
            "51000000-0000-4000-8000-0000000000ff",
            &order(&[GUE, ARRIVEE, ROUTE])
        ));
    }

    #[test]
    fn a_row_holding_no_scene_records_none_and_a_removal_forgets_it() {
        let scenes = scenes();
        // A session's own row, its status written alone, is no scene.
        scenes.record(
            "Session",
            SEANCE,
            &row(json!({ "sessionId": SEANCE, "sessionStatus": "live" })),
        );
        scenes.record("Session", "sc4", &row(json!({ "sceneSessionId": SEANCE })));
        assert!(scenes.orders(SEANCE, &order(&[GUE, ARRIVEE, ROUTE])));
        scenes.forget(&GUE.to_uppercase());
        assert!(scenes.orders(SEANCE, &order(&[ARRIVEE, ROUTE])));
        // A session removed takes its scenes with it.
        scenes.forget(SEANCE);
        assert!(!scenes.orders(SEANCE, &order(&[ARRIVEE, ROUTE])));
        assert!(scenes.orders(AUTRE_SEANCE, &order(&[AILLEURS])));
        scenes.forget("not-an-id");
        assert!(scenes.orders(AUTRE_SEANCE, &order(&[AILLEURS])));
    }

    #[test]
    fn the_scenes_the_tables_hold_are_taken_beside_the_ones_known() {
        let scenes = Scenes::default();
        record_scene(&scenes, ARRIVEE, SEANCE);
        scenes.take(&[
            (ROUTE.to_uppercase(), SEANCE.to_string()),
            (GUE.to_string(), SEANCE.to_uppercase()),
            (AILLEURS.to_string(), AUTRE_SEANCE.to_string()),
            ("sc9".to_string(), SEANCE.to_string()),
            (ARRIVEE.to_string(), "seance-1".to_string()),
        ]);
        assert!(scenes.orders(SEANCE, &order(&[GUE, ARRIVEE, ROUTE])));
        assert!(scenes.orders(AUTRE_SEANCE, &order(&[AILLEURS])));
        // Taken again, nothing known is lost.
        scenes.take(&[]);
        assert!(scenes.orders(SEANCE, &order(&[GUE, ARRIVEE, ROUTE])));
        // A rank known is kept, one the tables name alone is none.
        scenes.record(
            "Session",
            ARRIVEE,
            &row(json!({ "sceneSessionId": SEANCE, "scenePosition": 2 })),
        );
        scenes.take(&[(ARRIVEE.to_string(), SEANCE.to_string())]);
        assert_eq!(scenes.next_position(SEANCE, ROUTE), 3);
    }
}
