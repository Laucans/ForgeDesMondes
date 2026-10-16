//! Rule R108: État approximatif des PNJ.
mod common;

use common::*;

// given: un PNJ visible de 20 PV max
// when: ses PV courants passent à 20, puis à 13, puis à 10, puis à 0
// then: la player view affiche successivement « indemne », « blessé » (au-dessus de la moitié), « en sang » (à la moitié ou moins), puis le token grisé et marqué mort, sans aucun chiffre de PV
#[test]
fn r108_approximate_state_of_npcs() {
    rule(
        "R108",
        &[
            "data-capability:campaign.saveEntity",
            "data-capability:table.placeTokens",
            "data-capability:table.changeHitPoints",
            "capability:table.getBattlemapInstance",
        ],
        |s| {
            let campaign = new_campaign(s, "campagne")?;
            let id = new_character(s, &campaign, "pnj", json!({ "pvMax": 20 }))?;
            let session = new_session(s, &campaign, &[])?;
            let scene = new_scene(s, &session, json!({}))?;
            let map = new_instance(s, &session, &scene, "bm-gue")?;
            place(s, &map, &id, (5, 5), 1)?;
            let token = tokens(s, &map, "dm")?[0].clone();

            // 20 PV, then 13, 10 and 0.
            for (delta, state) in [
                (0, "indemne"),
                (-7, "blessé"),
                (-3, "en sang"),
                (-10, "mort"),
            ] {
                if delta != 0 {
                    apply(
                        s,
                        "table.changeHitPoints",
                        json!({ "token": token["id"], "delta": delta }),
                        Some(&map),
                    )?;
                }
                let seen = tokens(s, &map, "player")?;
                let shown =
                    by_id(&seen, text(&token, "id")).ok_or("the NPC left the player view")?;
                let said = text(shown, "status").to_lowercase();
                check(
                    said == state,
                    &format!("expected « {state} », the player view says « {said} »"),
                )?;
                check(
                    shown.get("hp").is_none_or(Value::is_null)
                        && shown.get("hpMax").is_none_or(Value::is_null),
                    "the PV reach the player view",
                )?;
                if state == "mort" {
                    check(shown["dead"] == true, "the token is not marked dead")?;
                }
            }

            Ok(())
        },
    );
}
