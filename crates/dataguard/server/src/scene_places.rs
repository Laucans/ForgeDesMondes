//! A place put in a scene (`session.addPlaceToSession`), judged against what
//! its own row cannot say: that the scene is the session's, and the place an
//! element of the session's campaign (`scene-links-same-campaign`). The
//! engine judges a row alone, and a link's row holds its scene, its place
//! and its rank — never their campaigns.
//!
//! So the server reads them when it prepares the link ([`crate::Rows::place_facts`]):
//! what the tables hold of the session, the scene and the place, and —
//! where the tables hold nothing, over the engine's memory always — what the
//! engine holds of them. It keeps the verdict under the link's id in
//! [`Links`], which [`ScenePlaces`] reads when the engine judges the
//! command: at its submission and when it is applied. A link no preparation
//! judged — one submitted to the engine directly, or queued before a start
//! and not judged again since ([`crate::Rows::rejudge_links`]) — is held to
//! nothing known, and rejected.

use std::collections::BTreeMap;
use std::sync::{Arc, RwLock};

use add_place_to_session::{PLACE, SCENE, Scene, link_of, names};
use harness_dataguard::command::{Effect, Entry};
use harness_dataguard::guard::Invariants;
use serde_json::Value;

const AGGREGATE: &str = "Session";

/// The invariant this judge holds the links to.
pub const SAME_CAMPAIGN: &str = "scene-links-same-campaign";

/// What is known of a link before it is submitted: its session's campaign,
/// the scene it joins and that scene's session, the place's campaign and
/// type. `None` is nothing known.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Facts {
    /// The session's campaign.
    pub session_campaign: Option<String>,
    /// The scene the place joins — the one named, else the session's first —
    /// with the elements it links.
    pub scene: Option<Scene>,
    /// That scene's session.
    pub scene_session: Option<String>,
    /// The place's campaign.
    pub place_campaign: Option<String>,
    /// The place's type.
    pub place_type: Option<String>,
}

impl Facts {
    /// Whether the scene is `session`'s and the place an element of its
    /// campaign: nothing known is neither.
    pub fn same_campaign(&self, session: &str) -> bool {
        let same = |a: &Option<String>, b: Option<&str>| {
            a.as_deref()
                .zip(b)
                .is_some_and(|(a, b)| a.eq_ignore_ascii_case(b))
        };
        same(&self.scene_session, Some(session))
            && same(&self.place_campaign, self.session_campaign.as_deref())
    }
}

/// The verdicts of the links prepared, by link id.
#[derive(Debug, Default)]
pub struct Links {
    verdicts: RwLock<BTreeMap<String, bool>>,
}

impl Links {
    /// Keeps whether the link `id` names joins a scene of its session to a
    /// place of its campaign.
    pub fn judged(&self, id: &str, same_campaign: bool) {
        let mut verdicts = self.verdicts.write().unwrap_or_else(|e| e.into_inner());
        verdicts.insert(id.to_ascii_lowercase(), same_campaign);
    }

    /// The verdict kept for the link `id` names, when one was.
    fn verdict(&self, id: &str) -> Option<bool> {
        let verdicts = self.verdicts.read().unwrap_or_else(|e| e.into_inner());
        verdicts.get(&id.to_ascii_lowercase()).copied()
    }
}

/// The judge of a place put in a scene, reading the verdicts of [`Links`].
pub struct ScenePlaces {
    links: Arc<Links>,
}

impl ScenePlaces {
    /// The judge reading `links`.
    pub fn new(links: Arc<Links>) -> Self {
        Self { links }
    }
}

/// `field`'s text in `row`.
fn text<'a>(row: &'a BTreeMap<String, Value>, field: &str) -> Option<&'a str> {
    row.get(field).and_then(Value::as_str)
}

impl Invariants for ScenePlaces {
    fn violated(
        &self,
        aggregate: &str,
        state: &BTreeMap<String, Value>,
        entry: &Entry,
    ) -> Vec<String> {
        if aggregate != AGGREGATE
            || entry.effect == Effect::Delete
            || !names(&entry.data_capability)
        {
            return Vec::new();
        }
        let id = entry.partition.strip_prefix("Session/").unwrap_or_default();
        // The row links what its id names, and the preparation found the
        // scene the session's and the place of its campaign.
        let holds = link_of(id).is_some_and(|(_, scene, place)| {
            text(state, SCENE).is_some_and(|s| s.eq_ignore_ascii_case(scene))
                && text(state, PLACE).is_some_and(|p| p.eq_ignore_ascii_case(place))
        }) && self.links.verdict(id) == Some(true);
        if holds {
            Vec::new()
        } else {
            vec![SAME_CAMPAIGN.to_string()]
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    const VALOMBRE: &str = "c1000000-0000-4000-8000-000000000001";
    const SEANCE: &str = "51000000-0000-4000-8000-000000000004";
    const ARRIVEE: &str = "5c000000-0000-4000-8000-000000000001";
    const TAVERNE: &str = "e1000000-0000-4000-8000-000000000003";

    fn known() -> Facts {
        Facts {
            session_campaign: Some(VALOMBRE.into()),
            scene: Some(Scene {
                id: ARRIVEE.into(),
                links: BTreeMap::new(),
            }),
            scene_session: Some(SEANCE.to_uppercase()),
            place_campaign: Some(VALOMBRE.into()),
            place_type: Some("lieu".into()),
        }
    }

    #[test]
    fn scene_links_same_campaign_holds_for_a_scene_of_the_session_and_a_place_of_its_campaign() {
        assert!(known().same_campaign(SEANCE));
        let other = "c1000000-0000-4000-8000-000000000002";
        for facts in [
            Facts {
                place_campaign: Some(other.into()),
                ..known()
            },
            Facts {
                scene_session: Some("51000000-0000-4000-8000-000000000005".into()),
                ..known()
            },
            // Nothing known is none of the campaign's.
            Facts {
                place_campaign: None,
                ..known()
            },
            Facts {
                session_campaign: None,
                place_campaign: None,
                ..known()
            },
            Facts {
                scene_session: None,
                ..known()
            },
        ] {
            assert!(!facts.same_campaign(SEANCE), "{facts:?}");
        }
    }

    /// A link's command, as the engine judges it.
    fn entry(data_capability: &str, partition: &str, effect: &str) -> Entry {
        serde_json::from_value(json!({
            "command": "c1",
            "data_capability": data_capability,
            "by": "dm",
            "partition": partition,
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
    fn scene_links_same_campaign_rejects_a_link_no_preparation_found_of_the_campaign() {
        let links = Arc::new(Links::default());
        let judge = ScenePlaces::new(links.clone());
        let id = format!("{SEANCE}:{ARRIVEE}:{TAVERNE}");
        let partition = format!("Session/{id}");
        let state: BTreeMap<String, Value> = [
            (SCENE.to_string(), json!(ARRIVEE)),
            (PLACE.to_string(), json!(TAVERNE.to_uppercase())),
        ]
        .into_iter()
        .collect();
        let link = entry("session.addPlaceToSession@1", &partition, "insert");
        // Never prepared: nothing known.
        assert_eq!(judge.violated(AGGREGATE, &state, &link), [SAME_CAMPAIGN]);
        links.judged(&id.to_uppercase(), true);
        assert!(judge.violated(AGGREGATE, &state, &link).is_empty());
        // A row linking anything but what its id names.
        let mut elsewhere = state.clone();
        elsewhere.insert(PLACE.into(), json!("e1000000-0000-4000-8000-000000000009"));
        assert_eq!(
            judge.violated(AGGREGATE, &elsewhere, &link),
            [SAME_CAMPAIGN]
        );
        links.judged(&id, false);
        assert_eq!(judge.violated(AGGREGATE, &state, &link), [SAME_CAMPAIGN]);
        // Another command, another aggregate, a tombstone: not this judge's.
        let other = entry("session.addScene@1", &partition, "insert");
        assert!(judge.violated(AGGREGATE, &state, &other).is_empty());
        assert!(judge.violated("Entity", &state, &link).is_empty());
        let removed = entry("session.addPlaceToSession@1", &partition, "delete");
        assert!(judge.violated(AGGREGATE, &state, &removed).is_empty());
    }
}
