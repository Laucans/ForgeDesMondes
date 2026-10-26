//! Rule R23: État de la carte conservé.
mod common;

use common::*;

// given: un combat en cours sur une battlemap
// when: l'application est fermée puis rouverte
// then: positions, PV, états et initiative sont identiques, et « Réinitialiser la carte » vide la carte : plus aucun token ni ordre d'initiative, la carte et sa grille restent affichées.
#[test]
fn r23_map_state_kept() {
    rule(
        "R23",
        &[
            "data-capability:table.placeTokens",
            "data-capability:table.moveToken",
            "data-capability:table.changeHitPoints",
            "data-capability:table.setTokenCondition",
            "data-capability:table.startCombat",
            "data-capability:table.resetBattlemap",
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
            let (map, cast) = fresh_instance(s, &["elowen", "bandit"])?;
            let initial = s.read("table.getBattlemapInstance", json!({ "instance": map }))?;
            place(s, &map, cast.id("elowen"), (1, 1), 1)?;
            place(s, &map, cast.id("bandit"), (5, 5), 2)?;
            let all = tokens(s, &map, "dm")?;
            let pj = of_character(&all, cast.id("elowen"))[0].clone();
            let bandits = of_character(&all, cast.id("bandit"));
            apply(
                s,
                "table.moveToken",
                json!({ "token": pj["id"], "q": 3, "r": 2 }),
                Some(&map),
            )?;
            apply(
                s,
                "table.changeHitPoints",
                json!({ "token": bandits[0]["id"], "delta": -4 }),
                Some(&map),
            )?;
            apply(
                s,
                "table.setTokenCondition",
                json!({ "token": bandits[1]["id"], "state": "Effrayé", "on": true }),
                Some(&map),
            )?;
            apply(
                s,
                "table.startCombat",
                json!({ "initiative": { cast.id("elowen"): 15 }, "rolls": { text(&bandits[0], "id"): 9, text(&bandits[1], "id"): 4 } }),
                Some(&map),
            )?;
            let kept = s.read(
                "table.getBattlemapInstance",
                json!({ "instance": map, "view": "dm" }),
            )?;

            // The app is closed, then reopened: a read of its own, carrying nothing of the session that wrote.
            let reopened = read_reopened(
                "table.getBattlemapInstance",
                json!({ "instance": map, "view": "dm" }),
            )?;
            let state = instance(&reopened)
                .get("state")
                .cloned()
                .unwrap_or(Value::Null);
            check(
                state == instance(&kept)["state"],
                "the state of the map is not the one that was kept",
            )?;
            let reopened_tokens = state["tokens"].as_array().cloned().unwrap_or_default();
            let moved = by_id(&reopened_tokens, text(&pj, "id")).ok_or("the PJ token vanished")?;
            check(moved["q"] == 3 && moved["r"] == 2, "the position was lost")?;
            let hurt =
                by_id(&reopened_tokens, text(&bandits[0], "id")).ok_or("a token vanished")?;
            check(
                hurt["hp"] == hurt["hpMax"].as_i64().unwrap_or(0) - 4,
                "the PV were lost",
            )?;
            let scared =
                by_id(&reopened_tokens, text(&bandits[1], "id")).ok_or("a token vanished")?;
            check(
                scared["states"] == json!(["Effrayé"]),
                "the states were lost",
            )?;
            let line = order(&state["combat"]);
            check(
                line.len() == 3 && line[0] == text(&pj, "id"),
                "the initiative was lost",
            )?;

            apply(s, "table.resetBattlemap", json!({}), Some(&map))?;
            let reset = s.read("table.getBattlemapInstance", json!({ "instance": map }))?;
            check(
                instance(&reset)["state"] == instance(&initial)["state"],
                "the map was not set back to zero",
            )?;
            check(
                instance(&reset)["state"]["tokens"]
                    .as_array()
                    .is_none_or(Vec::is_empty)
                    && instance(&reset)["state"]["combat"].is_null(),
                "a token or the initiative order is left on the map",
            )?;

            Ok(())
        },
    );
}
