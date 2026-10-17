//! Rule R97: Médias rattachés à la séance par leur lieu.
mod common;

use common::*;

// given: une battlemap et une illustration associées à « La Taverne du Sanglier », absente de la séance 4, une seconde battlemap associée à un autre lieu, et un média sans lieu
// when: le DM ajoute la taverne à une scène de la séance 4 depuis la préparation de séance, puis ouvre la page Médias et y change le lieu de la seconde battlemap pour « La Taverne du Sanglier »
// then: la battlemap et l'illustration de la taverne sont associées à la séance 4 sur les deux pages, la page Médias affiche « Associé à la séance 4 » sur leurs cartes, la seconde battlemap le devient aussi après son changement de lieu, et le média sans lieu reste valide sans association à la séance
#[test]
fn r97_media_attached_to_the_session_by_place() {
    rule(
        "R97",
        &[
            "data-capability:media.uploadMedia",
            "data-capability:media.setMediaPlace",
            "data-capability:session.addScene",
            "capability:media.listMedia",
            "capability:session.listScenes",
            "capability:campaign.listPlaces",
            "capability:campaign.listCampaigns",
            "capability:session.listSessions",
            "data-capability:campaign.createCampaign",
            "data-capability:campaign.saveEntity",
            "data-capability:session.createSession",
        ],
        |s| {
            let campaign = new_campaign(s, "campagne")?;
            for name in ["La Taverne du Sanglier", "La Tour d'Ébène"] {
                apply(
                    s,
                    "campaign.saveEntity",
                    json!({ "campaign": campaign, "entity": { "kind": "lieu", "name": name } }),
                    None,
                )?;
            }
            let spots = places(s, &campaign)?;
            let tavern = text(
                by_name(&spots, "La Taverne du Sanglier").ok_or("no tavern")?,
                "id",
            )
            .to_string();
            let tower =
                text(by_name(&spots, "La Tour d'Ébène").ok_or("no tower")?, "id").to_string();

            // A battlemap and an illustration of the tavern, another battlemap elsewhere, a medium with no place.
            let mut ids = std::collections::HashMap::new();
            for (key, kind, place) in [
                ("map", "battlemap", Some(&tavern)),
                ("art", "illustration", Some(&tavern)),
                ("other", "battlemap", Some(&tower)),
                ("none", "illustration", None),
            ] {
                let title = unique(key);
                apply(
                    s,
                    "media.uploadMedia",
                    json!({ "campaign": campaign, "kind": kind, "title": title, "place": place }),
                    None,
                )?;
                let listed = s.read("media.listMedia", json!({ "campaign": campaign }))?;
                let mut all = items(&listed, "battlemaps");
                all.extend(items(&listed, "illustrations"));
                ids.insert(
                    key,
                    by_name_or_title(&all, &title).ok_or("the medium is not listed")?,
                );
            }
            let session = new_session(s, &campaign, &[])?;
            let attached = |s: &Server, key: &str| -> Result<bool, String> {
                let listed = s.read("media.listMedia", json!({ "campaign": campaign }))?;
                let mut all = items(&listed, "battlemaps");
                all.extend(items(&listed, "illustrations"));
                let medium = by_id(&all, &ids[key]).ok_or("the medium is not listed")?;
                Ok(medium["sessions"]
                    .as_array()
                    .is_some_and(|x| x.iter().any(|v| *v == session)))
            };
            check(
                !attached(s, "map")? && !attached(s, "art")?,
                "the tavern's media are attached before the tavern is in the session",
            )?;

            // The DM adds the tavern to a scene from the preparation page.
            let scene = new_scene(s, &session, json!({ "places": [tavern] }))?;
            check(
                attached(s, "map")? && attached(s, "art")?,
                "the media of the tavern are not attached to the session",
            )?;
            let scenes = items(
                &s.read("session.listScenes", json!({ "session": session }))?,
                "scenes",
            );
            let on_scene = by_id(&scenes, &scene).ok_or("the scene is missing")?["media"].clone();
            check(
                on_scene.as_array().is_some_and(|m| {
                    m.iter().any(|v| *v == ids["map"]) && m.iter().any(|v| *v == ids["art"])
                }),
                "the preparation page does not show the tavern's media",
            )?;
            check(
                !attached(s, "other")?,
                "a medium of another place is attached",
            )?;

            // On the media page, the second battlemap moves to the tavern.
            apply(
                s,
                "media.setMediaPlace",
                json!({ "place": tavern }),
                Some(&ids["other"]),
            )?;
            check(
                attached(s, "other")?,
                "changing the place did not attach the medium",
            )?;

            // The medium with no place stays valid, attached to nothing.
            check(
                !attached(s, "none")?,
                "a medium with no place is attached to the session",
            )?;

            Ok(())
        },
    );
}
