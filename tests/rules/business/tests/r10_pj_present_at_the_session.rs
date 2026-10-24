//! Rule R10: PJ présents à la séance.
mod common;

use common::*;

// given: une séance avec deux PJ cochés présents sur quatre
// when: le DM ouvre la DM view ou une battlemap
// then: seuls ces deux PJ sont proposés, et un PJ ajouté plus tard l'est aussi.
#[test]
fn r10_pj_present_at_the_session() {
    rule(
        "R10",
        &[
            "data-capability:session.createSession",
            "data-capability:session.addPresentCharacter",
            "capability:session.getSession",
            "capability:session.getLiveSession",
            "capability:table.getBattlemapInstance",
            "capability:campaign.listCharacters",
            "capability:session.listScenes",
            "capability:session.listSessions",
            "data-capability:session.addScene",
            "data-capability:table.createBattlemapInstance",
            "data-capability:campaign.createCampaign",
            "data-capability:campaign.saveEntity",
            "capability:campaign.listCampaigns",
            "data-capability:media.uploadMedia",
            "capability:media.listMedia",
        ],
        |s| {
            // A campaign of its own with the four PJ of the fixtures (a
            // command takes stored ids, never the fixtures' slugs), two of
            // them ticked present.
            let campaign = new_campaign(s, "campagne")?;
            let cast = new_cast(s, &campaign, &["elowen", "brannoc", "sirielle", "kael"])?;
            let battlemap = new_battlemap(s, &campaign)?;
            let session = new_session(s, &campaign, &[cast.id("elowen"), cast.id("kael")])?;
            let scene = new_scene(s, &session, json!({}))?;
            let map = new_instance(s, &session, &scene, &battlemap)?;
            let pj_ids: Vec<String> = characters(s, &campaign)?
                .iter()
                .filter(|c| text(c, "kind") == "pj")
                .map(|c| text(c, "id").to_string())
                .collect();
            check(pj_ids.len() >= 4, "the fixtures hold four PJ")?;

            // What the DM view and the battlemap propose, as a sorted list of PJ.
            let proposed = |s: &Server| -> Result<[Vec<String>; 3], String> {
                let sorted = |mut v: Vec<String>| {
                    v.sort();
                    v
                };
                let ids = |v: &Value| -> Vec<String> {
                    v.as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|x| x.as_str().map(String::from))
                        .collect()
                };
                let prep = s.read("session.getSession", json!({ "id": session }))?;
                let live = s.read("session.getLiveSession", json!({ "id": session }))?;
                let roster = s.read("table.getBattlemapInstance", json!({ "instance": map }))?;
                let on_map: Vec<String> = ids(&instance(&roster)["roster"])
                    .into_iter()
                    .filter(|c| pj_ids.contains(c))
                    .collect();
                Ok([
                    sorted(ids(&prep["present"])),
                    sorted(ids(&live["session"]["present"])),
                    sorted(on_map),
                ])
            };

            let mut two = vec![cast.id("elowen").to_string(), cast.id("kael").to_string()];
            two.sort();
            let [prep, live, on_map] = proposed(s)?;
            check(
                prep == two && live == two && on_map == two,
                "only the two ticked PJ may be proposed",
            )?;

            apply(
                s,
                "session.addPresentCharacter",
                json!({ "character": cast.id("sirielle") }),
                Some(&session),
            )?;
            let mut three = vec![
                cast.id("elowen").to_string(),
                cast.id("kael").to_string(),
                cast.id("sirielle").to_string(),
            ];
            three.sort();
            let [prep, live, on_map] = proposed(s)?;
            check(
                prep == three && live == three && on_map == three,
                "the PJ added later must be proposed too",
            )?;

            Ok(())
        },
    );
}
