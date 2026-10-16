//! Rule R23: État de la carte conservé.
mod common;

use common::*;

// given: un combat en cours sur une battlemap
// when: l'application est fermée puis rouverte
// then: positions, PV, états et initiative sont identiques, et « Réinitialiser la carte » remet l'état à zéro.
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
        ],
        |s| {
            let map = fresh_instance(s, &["elowen"])?;
            let initial = s.read("table.getBattlemapInstance", json!({ "instance": map }))?;
            place(s, &map, "elowen", (1, 1), 1)?;
            place(s, &map, "bandit", (5, 5), 2)?;
            let all = tokens(s, &map, "dm")?;
            let pj = of_character(&all, "elowen")[0].clone();
            let bandits = of_character(&all, "bandit");
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
                json!({ "initiative": { "elowen": 15 }, "rolls": { text(&bandits[0], "id"): 9, text(&bandits[1], "id"): 4 } }),
                Some(&map),
            )?;
            let kept = s.read("table.getBattlemapInstance", json!({ "instance": map }))?;

            // The state lives in the product's store, not in the page: a fresh read after the app was closed.
            let reopened = s.read("table.getBattlemapInstance", json!({ "instance": map }))?;
            check(
                reopened == kept,
                "the state of the map changed between two openings",
            )?;
            let moved = by_id(&tokens(s, &map, "dm")?, text(&pj, "id"))
                .cloned()
                .ok_or("the PJ token vanished")?;
            check(moved["q"] == 3 && moved["r"] == 2, "the position was lost")?;
            let hurt = by_id(&tokens(s, &map, "dm")?, text(&bandits[0], "id"))
                .cloned()
                .ok_or("a token vanished")?;
            check(
                hurt["hp"] == hurt["hpMax"].as_i64().unwrap_or(0) - 4,
                "the PV were lost",
            )?;
            let scared = by_id(&tokens(s, &map, "dm")?, text(&bandits[1], "id"))
                .cloned()
                .ok_or("a token vanished")?;
            check(
                scared["states"] == json!(["Effrayé"]),
                "the states were lost",
            )?;
            let line = order(&combat(s, &map, "dm")?);
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

            Ok(())
        },
    );
}
