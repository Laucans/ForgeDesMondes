//! Rule R102: Lieux et événements reconnus dans le brief.
mod common;

use common::*;

// given: une campagne qui contient le lieu « La Taverne du Sanglier » mais pas « Le Pont des Pendus »
// when: le DM raconte un brief qui cite la taverne, le pont et l'événement « l'embuscade »
// then: la taverne est liée à la scène correspondante comme lieu existant, et le pont et l'embuscade apparaissent dans la liste « à créer » avec leur type et l'extrait du brief
#[test]
fn r102_places_and_events_recognised_in_the_brief() {
    rule(
        "R102",
        &[
            "data-capability:ai.analyzeBrief",
            "capability:session.listScenes",
            "capability:campaign.listEntitiesToCreate",
            "capability:campaign.listPlaces",
            "capability:campaign.listCampaigns",
            "capability:session.listSessions",
            "data-capability:campaign.createCampaign",
            "data-capability:campaign.saveEntity",
            "data-capability:session.createSession",
        ],
        |s| {
            // A campaign of its own, with the tavern and without the bridge.
            let campaign = new_campaign(s, "campagne")?;
            apply(
                s,
                "campaign.saveEntity",
                json!({ "campaign": campaign, "entity": { "kind": "lieu", "name": "La Taverne du Sanglier", "aliases": ["la taverne"] } }),
                None,
            )?;
            let tavern = text(
                by_name(&places(s, &campaign)?, "La Taverne du Sanglier").ok_or("no tavern")?,
                "id",
            )
            .to_string();
            let session = new_session(s, &campaign, &[])?;
            let brief = "On commence à la Taverne du Sanglier. Puis le groupe prend la route et tombe sur le Pont des Pendus, où a lieu l'embuscade.";

            apply(
                s,
                "ai.analyzeBrief",
                json!({ "session": session, "text": brief }),
                None,
            )?;

            let scenes = items(
                &s.read("session.listScenes", json!({ "session": session }))?,
                "scenes",
            );
            check(
                scenes.iter().any(|sc| {
                    sc["places"]
                        .as_array()
                        .is_some_and(|p| p.iter().any(|x| *x == tavern))
                }),
                "the tavern is not linked to a scene as an existing place",
            )?;
            let to_create = items(
                &s.read(
                    "campaign.listEntitiesToCreate",
                    json!({ "campaign": campaign }),
                )?,
                "items",
            );
            let bridge = to_create
                .iter()
                .find(|e| text(e, "name").contains("Pont des Pendus"))
                .ok_or("the bridge is not to create")?;
            check(
                text(bridge, "type") == "lieu",
                "the bridge is not typed as a place",
            )?;
            check(
                text(bridge, "excerpt").contains("Pont"),
                "the bridge has no excerpt of the brief",
            )?;
            let ambush = to_create
                .iter()
                .find(|e| text(e, "name").to_lowercase().contains("embuscade"))
                .ok_or("the ambush is not to create")?;
            check(
                text(ambush, "type") == "événement",
                "the ambush is not typed as an event",
            )?;
            check(
                text(ambush, "excerpt").contains("embuscade"),
                "the ambush has no excerpt of the brief",
            )?;
            check(
                !to_create
                    .iter()
                    .any(|e| text(e, "name").contains("Taverne")),
                "the existing tavern is listed to create",
            )?;

            Ok(())
        },
    );
}
