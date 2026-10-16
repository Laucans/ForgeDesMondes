//! Rule R22: Combat et carrousel d'initiative.
mod common;

use common::*;

// given: deux PNJ à égalité d'initiative
// when: le combat démarre
// then: ils sont départagés par la DEX puis au hasard, le carrousel apparaît sur les deux vues, et « Suivant » fait avancer le tour et le compteur de rounds.
#[test]
fn r22_combat_and_initiative_carousel() {
    rule(
        "R22",
        &[
            "data-capability:table.placeTokens",
            "data-capability:table.startCombat",
            "data-capability:table.nextTurn",
            "capability:table.getBattlemapInstance",
        ],
        |s| {
            let map = fresh_instance(s, &["elowen"])?;
            place(s, &map, "elowen", (1, 1), 1)?;
            place(s, &map, "bandit", (5, 5), 1)?;
            place(s, &map, "vesna", (6, 5), 1)?;
            let all = tokens(s, &map, "dm")?;
            let (pj, bandit, boss) = (
                of_character(&all, "elowen")[0].clone(),
                of_character(&all, "bandit")[0].clone(),
                of_character(&all, "vesna")[0].clone(),
            );

            // Both NPC reach 14: the bandit rolls 13 with DEX 12 (+1), the boss 11 with DEX 16 (+3).
            apply(
                s,
                "table.startCombat",
                json!({ "initiative": { "elowen": 18 }, "rolls": { text(&bandit, "id"): 13, text(&boss, "id"): 11 } }),
                Some(&map),
            )?;
            let started = combat(s, &map, "dm")?;
            let line = order(&started);
            let rank = |t: &Value| line.iter().position(|id| id == text(t, "id"));
            check(
                rank(&boss) < rank(&bandit),
                "the tie was not broken by the DEX",
            )?;
            check(rank(&pj) == Some(0), "the PJ's initiative was not used")?;

            // The carousel shows on both views.
            check(
                !combat(s, &map, "player")?.is_null(),
                "no carousel on the player view",
            )?;
            check(
                started["round"] == 1,
                "the combat does not start at round 1",
            )?;

            // « Suivant » moves the turn along, and the round counter after a lap.
            let mut seen = vec![text(&started, "current").to_string()];
            for _ in 0..3 {
                apply(s, "table.nextTurn", json!({}), Some(&map))?;
                seen.push(text(&combat(s, &map, "dm")?, "current").to_string());
            }
            check(
                seen == line
                    .iter()
                    .cloned()
                    .chain(line.first().cloned())
                    .collect::<Vec<_>>(),
                "the turn does not follow the order",
            )?;
            check(
                combat(s, &map, "dm")?["round"] == 2,
                "the round counter did not advance after a lap",
            )?;

            Ok(())
        },
    );
}
