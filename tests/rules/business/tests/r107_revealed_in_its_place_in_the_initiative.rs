//! Rule R107: Révélation à sa place dans l'initiative.
mod common;

use common::*;

// given: un combat où Chef bandit 1, caché, a la deuxième initiative et figure avec une icône dans le carrousel DM seulement
// when: le DM le rend visible
// then: il apparaît en deuxième position dans le carrousel de la player view, et l'ordre des autres participants n'a pas changé
#[test]
fn r107_revealed_in_its_place_in_the_initiative() {
    rule(
        "R107",
        &[
            "data-capability:table.placeTokens",
            "data-capability:table.setTokenVisibility",
            "data-capability:table.startCombat",
            "capability:table.getBattlemapInstance",
            "capability:session.listScenes",
            "capability:session.listSessions",
            "data-capability:session.addScene",
            "data-capability:session.createSession",
            "data-capability:table.createBattlemapInstance",
            "data-capability:campaign.createCampaign",
            "data-capability:campaign.saveEntity",
            "capability:campaign.listCampaigns",
            "capability:campaign.listCharacters",
            "data-capability:media.uploadMedia",
            "capability:media.listMedia",
        ],
        |s| {
            let (map, cast) = fresh_instance(s, &["elowen", "kael", "bandit", "vesna"])?;
            place(s, &map, cast.id("elowen"), (1, 1), 1)?;
            place(s, &map, cast.id("kael"), (2, 1), 1)?;
            place(s, &map, cast.id("bandit"), (5, 5), 1)?;
            place(s, &map, cast.id("vesna"), (6, 5), 1)?;
            let all = tokens(s, &map, "dm")?;
            let (bandit, boss) = (
                of_character(&all, cast.id("bandit"))[0].clone(),
                of_character(&all, cast.id("vesna"))[0].clone(),
            );
            apply(
                s,
                "table.setTokenVisibility",
                json!({ "token": boss["id"], "visible": false }),
                Some(&map),
            )?;

            // The hidden boss gets the second initiative: 15 behind Elowen's 18, ahead of Kael's 10.
            apply(
                s,
                "table.startCombat",
                json!({ "initiative": { cast.id("elowen"): 18, cast.id("kael"): 10 }, "rolls": { text(&boss, "id"): 12, text(&bandit, "id"): 3 } }),
                Some(&map),
            )?;
            let dm_line = order(&combat(s, &map, "dm")?);
            check(
                dm_line.get(1).map(String::as_str) == Some(text(&boss, "id")),
                "the hidden token is not second on the DM carousel",
            )?;
            let player_before = order(&combat(s, &map, "player")?);
            check(
                !player_before.iter().any(|id| id == text(&boss, "id")),
                "the hidden token is on the player carousel",
            )?;
            check(
                player_before.len() == dm_line.len() - 1,
                "the player carousel lost another participant",
            )?;

            apply(
                s,
                "table.setTokenVisibility",
                json!({ "token": boss["id"], "visible": true }),
                Some(&map),
            )?;
            let player_after = order(&combat(s, &map, "player")?);
            check(
                player_after.get(1).map(String::as_str) == Some(text(&boss, "id")),
                "the revealed token is not second on the player carousel",
            )?;
            let others: Vec<&String> = player_after
                .iter()
                .filter(|id| *id != text(&boss, "id"))
                .collect();
            check(
                others == player_before.iter().collect::<Vec<_>>(),
                "the order of the others changed",
            )?;

            Ok(())
        },
    );
}
