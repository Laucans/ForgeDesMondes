//! Rule R21: PV, états, mort et visibilité des tokens.
mod common;

use common::*;

// given: un PNJ dont les PV tombent à 0
// when: le DM valide
// then: son token est grisé, marqué mort et sauté dans l'initiative, et les PV des PJ ne sont jamais suivis.
#[test]
fn r21_hit_points_states_death_and_visibility() {
    rule(
        "R21",
        &[
            "data-capability:table.placeTokens",
            "data-capability:table.setTokenVisibility",
            "data-capability:table.setTokenCondition",
            "data-capability:table.changeHitPoints",
            "data-capability:table.startCombat",
            "data-capability:table.nextTurn",
            "data-capability:table.removeToken",
            "capability:table.getBattlemapInstance",
        ],
        |s| {
            let map = fresh_instance(s, &["elowen"])?;
            place(s, &map, "elowen", (1, 1), 1)?;
            place(s, &map, "bandit", (5, 5), 2)?;
            let all = tokens(s, &map, "dm")?;
            let pj = of_character(&all, "elowen")[0].clone();
            let bandits = of_character(&all, "bandit");
            let (first, second) = (bandits[0].clone(), bandits[1].clone());
            let reread = |s: &Server, id: &Value| -> Result<Value, String> {
                let list = tokens(s, &map, "dm")?;
                by_id(&list, text(id, "id"))
                    .cloned()
                    .ok_or_else(|| "a token vanished".to_string())
            };

            // A visible/hidden switch and state badges.
            apply(
                s,
                "table.setTokenVisibility",
                json!({ "token": first["id"], "visible": false }),
                Some(&map),
            )?;
            check(
                reread(s, &first)?["visible"] == false,
                "the token was not hidden",
            )?;
            apply(
                s,
                "table.setTokenVisibility",
                json!({ "token": first["id"], "visible": true }),
                Some(&map),
            )?;
            check(
                reread(s, &first)?["visible"] == true,
                "the token was not shown again",
            )?;
            apply(
                s,
                "table.setTokenCondition",
                json!({ "token": first["id"], "state": "Empoisonné", "on": true }),
                Some(&map),
            )?;
            check(
                reread(s, &first)?["states"] == json!(["Empoisonné"]),
                "the state badge was not set",
            )?;

            // The PV of a PJ are never followed.
            let _ = s.command(
                "table.changeHitPoints",
                json!({ "token": pj["id"], "delta": -5 }),
                Some(&map),
            )?;
            check(
                reread(s, &pj)?["hp"].is_null(),
                "the PV of a PJ are followed",
            )?;

            // A NPC at 0 PV is dead and skipped by the initiative.
            apply(
                s,
                "table.startCombat",
                json!({ "initiative": { "elowen": 15 }, "rolls": { text(&first, "id"): 10, text(&second, "id"): 5 } }),
                Some(&map),
            )?;
            let max = second["hpMax"].as_i64().ok_or("a NPC has no PV")?;
            apply(
                s,
                "table.changeHitPoints",
                json!({ "token": second["id"], "delta": -max }),
                Some(&map),
            )?;
            let dead = reread(s, &second)?;
            check(
                dead["hp"] == 0 && dead["dead"] == true,
                "the token is not dead at 0 PV",
            )?;
            let seen_by_players = tokens(s, &map, "player")?;
            let on_player_view = by_id(&seen_by_players, text(&second, "id"))
                .ok_or("the dead token left the player view")?;
            check(
                on_player_view["dead"] == true,
                "the player view does not mark the token dead",
            )?;
            for _ in 0..6 {
                apply(s, "table.nextTurn", json!({}), Some(&map))?;
                let current = combat(s, &map, "dm")?;
                check(
                    text(&current, "current") != text(&second, "id"),
                    "the initiative stops on a dead token",
                )?;
            }

            // The DM removes it in one click.
            apply(
                s,
                "table.removeToken",
                json!({ "token": second["id"] }),
                Some(&map),
            )?;
            check(
                by_id(&tokens(s, &map, "dm")?, text(&second, "id")).is_none(),
                "the dead token was not removed",
            )?;

            Ok(())
        },
    );
}
