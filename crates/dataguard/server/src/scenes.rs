//! What a reorder of a session's scenes is judged against beyond its own
//! row: which session each scene is of — what `scene-order-contiguous`
//! reads, so an order is every scene of its session, each once. The engine
//! judges a row alone, and this is the rest of its session.
//!
//! It follows what the engine applies: [`crate::versioning::Versioning`]
//! records each write of the Session aggregate once it is applied, over the
//! engine's memory and over a database alike, and forgets one when it is
//! removed. Over a database it also takes what the tables and the engine's
//! rows hold — the fixtures' scenes, which no command wrote — at a start,
//! and of a session each time a reorder of it is submitted
//! ([`crate::Rows::refresh_scenes`]): taken beside what it knows, never over
//! it, so a scene applied while the tables were read is not lost. Scenes and
//! sessions are told apart by their id parsed as a uuid: the same id in
//! another case is the same one.

use std::collections::BTreeMap;
use std::sync::RwLock;

use serde_json::Value;

use crate::aliases::parsed;

/// A scene's row: its id.
pub const SCENE: &str = "sceneId";

/// A scene's row: the session it is of.
pub const SCENE_SESSION: &str = "sceneSessionId";

/// The scenes known, each with its session.
#[derive(Debug, Default)]
pub struct Scenes {
    sessions: RwLock<BTreeMap<u128, u128>>,
}

/// The id `field` holds in `row`, parsed.
fn uuid_in(row: &BTreeMap<String, Value>, field: &str) -> Option<u128> {
    row.get(field).and_then(Value::as_str).and_then(parsed)
}

impl Scenes {
    /// Records the scene a row of the Session aggregate holds, with its
    /// session: a row holding no scene — a session's own, its status
    /// written alone — records nothing.
    pub fn record(&self, row: &BTreeMap<String, Value>) {
        if let (Some(scene), Some(session)) = (uuid_in(row, SCENE), uuid_in(row, SCENE_SESSION)) {
            let mut sessions = self.sessions.write().unwrap_or_else(|e| e.into_inner());
            sessions.insert(scene, session);
        }
    }

    /// The scene `id` names is no more — nor, when `id` names a session, are
    /// its scenes: they go with it.
    pub fn forget(&self, id: &str) {
        let Some(id) = parsed(id) else {
            return;
        };
        let mut sessions = self.sessions.write().unwrap_or_else(|e| e.into_inner());
        sessions.retain(|scene, session| *scene != id && *session != id);
    }

    /// Records each `(scene, session)` pair the tables and the engine's rows
    /// hold, beside the scenes known: the ones that do not parse left out.
    pub fn take(&self, scenes: &[(String, String)]) {
        let mut sessions = self.sessions.write().unwrap_or_else(|e| e.into_inner());
        for (scene, session) in scenes {
            if let (Some(scene), Some(session)) = (parsed(scene), parsed(session)) {
                sessions.insert(scene, session);
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
        let sessions = self.sessions.read().unwrap_or_else(|e| e.into_inner());
        let mut ranked = Vec::with_capacity(order.len());
        for scene in order {
            match scene.as_str().and_then(parsed) {
                Some(scene) if sessions.get(&scene) == Some(&session) => ranked.push(scene),
                _ => return false,
            }
        }
        ranked.sort_unstable();
        ranked.dedup();
        ranked.len() == order.len()
            && sessions.values().filter(|of| **of == session).count() == ranked.len()
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    const SEANCE: &str = "51000000-0000-4000-8000-000000000001";
    const AUTRE_SEANCE: &str = "51000000-0000-4000-8000-000000000002";
    const ARRIVEE: &str = "5c000000-0000-4000-8000-000000000001";
    const ROUTE: &str = "5c000000-0000-4000-8000-000000000002";
    const GUE: &str = "5c000000-0000-4000-8000-000000000003";
    const AILLEURS: &str = "5c000000-0000-4000-8000-000000000009";

    fn row(fields: Value) -> BTreeMap<String, Value> {
        let Value::Object(fields) = fields else {
            panic!("a row is an object");
        };
        fields.into_iter().collect()
    }

    fn scene(id: &str, session: &str) -> BTreeMap<String, Value> {
        row(json!({ "sceneId": id, "sceneSessionId": session, "scenePosition": 1 }))
    }

    fn order(ids: &[&str]) -> Vec<Value> {
        ids.iter().map(|id| json!(id)).collect()
    }

    /// Session 1's three scenes, and a scene of another session.
    fn scenes() -> Scenes {
        let scenes = Scenes::default();
        for id in [ARRIVEE, ROUTE, GUE] {
            scenes.record(&scene(id, SEANCE));
        }
        scenes.record(&scene(AILLEURS, AUTRE_SEANCE));
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
        scenes.record(&row(
            json!({ "sessionId": SEANCE, "sessionStatus": "live" }),
        ));
        scenes.record(&row(json!({ "sceneId": "sc4", "sceneSessionId": SEANCE })));
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
        scenes.record(&scene(ARRIVEE, SEANCE));
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
    }
}
