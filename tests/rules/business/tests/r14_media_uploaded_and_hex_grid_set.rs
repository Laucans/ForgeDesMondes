//! Rule R14: Médias téléversés et grille hexagonale réglée.
mod common;

use common::*;

// given: une battlemap téléversée
// when: le DM règle taille, décalage, orientation, couleur et opacité de la grille
// then: l'aperçu se met à jour en direct et un hex vaut toujours 5 pieds.
#[test]
fn r14_media_uploaded_and_hex_grid_set() {
    rule(
        "R14",
        &[
            "data-capability:media.uploadMedia",
            "data-capability:media.saveGridSettings",
            "capability:media.listMedia",
            "capability:media.getBattlemap",
            "capability:campaign.listCampaigns",
            "data-capability:campaign.createCampaign",
        ],
        |s| {
            let campaign = new_campaign(s, "campagne")?;
            let title = unique("Battlemap de test");
            apply(
                s,
                "media.uploadMedia",
                json!({ "campaign": campaign, "kind": "battlemap", "title": title, "width": 900, "height": 600 }),
                None,
            )?;
            let media = s.read("media.listMedia", json!({ "campaign": campaign }))?;
            let id = by_name_or_title(&items(&media, "battlemaps"), &title)
                .ok_or("the uploaded battlemap is not listed")?;
            let grid = |s: &Server| -> Result<Value, String> {
                let data = s.read("media.getBattlemap", json!({ "id": id }))?;
                Ok(data.get("battlemap").unwrap_or(&data)["grid"].clone())
            };
            check(
                grid(s)?["feetPerHex"] == 5,
                "a hex is not 5 feet at the start",
            )?;

            let set = json!({ "size": 40, "ox": 5, "oy": 7, "orient": "flat", "color": "accent", "opacity": 70 });
            apply(s, "media.saveGridSettings", set.clone(), Some(&id))?;
            let saved = grid(s)?;
            for key in ["size", "ox", "oy", "orient", "color", "opacity"] {
                check(
                    saved[key] == set[key],
                    &format!("the grid's {key} was not kept"),
                )?;
            }
            check(
                saved["feetPerHex"] == 5,
                "a hex is no longer 5 feet once the grid is set",
            )?;

            Ok(())
        },
    );
}
