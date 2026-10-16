//! Rule R101: Image de token par défaut.
mod common;

use common::*;

// given: un PNJ « Bandit » sans portrait ni image de token, placé en trois instances
// when: les tokens s'affichent sur la battlemap
// then: chacun est une pastille de couleur portant « B » et son numéro (1, 2, 3), et dès qu'un portrait est ajouté et recadré, les tokens montrent ce recadrage
#[test]
fn r101_default_token_image() {
    rule(
        "R101",
        &[
            "data-capability:campaign.saveEntity",
            "data-capability:table.placeTokens",
            "capability:table.getBattlemapInstance",
        ],
        |s| {
            let campaign = new_campaign(s, "campagne")?;
            let id = new_character(
                s,
                &campaign,
                "pnj",
                json!({ "name": unique("Bandit"), "pvMax": 11, "color": "danger" }),
            )?;
            let session = new_session(s, &campaign, &[])?;
            let scene = new_scene(s, &session, json!({}))?;
            let map = new_instance(s, &session, &scene, "bm-gue")?;
            place(s, &map, &id, (5, 5), 3)?;

            // No portrait, no token image: a coloured disc with the initial and the number.
            for view in ["dm", "player"] {
                let discs = tokens(s, &map, view)?;
                check(discs.len() == 3, "three instances expected")?;
                let mut numbers: Vec<u64> =
                    discs.iter().filter_map(|t| t["num"].as_u64()).collect();
                numbers.sort_unstable();
                check(
                    numbers == [1, 2, 3],
                    &format!("{view}: the instances are not numbered 1, 2, 3"),
                )?;
                for t in &discs {
                    check(
                        text(t, "initials").starts_with('B'),
                        &format!("{view}: the disc does not carry « B »"),
                    )?;
                    check(
                        !text(t, "color").is_empty(),
                        &format!("{view}: the disc has no colour"),
                    )?;
                    check(
                        t["portrait"] != true && t.get("crop").is_none_or(Value::is_null),
                        &format!("{view}: the disc shows a portrait"),
                    )?;
                }
            }

            // A portrait is added and cropped: the tokens show that crop.
            let crop = json!({ "zoom": 150, "x": 40, "y": 30 });
            apply(
                s,
                "campaign.saveEntity",
                json!({ "campaign": campaign, "entity": { "id": id, "portrait": true, "crop": crop } }),
                None,
            )?;
            for view in ["dm", "player"] {
                for t in tokens(s, &map, view)? {
                    check(
                        t["crop"] == crop && t["portrait"] == true,
                        &format!("{view}: a token does not show the portrait's crop"),
                    )?;
                }
            }

            Ok(())
        },
    );
}
