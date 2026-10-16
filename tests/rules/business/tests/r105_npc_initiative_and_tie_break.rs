//! Rule R105: Initiative des PNJ et départage.
mod common;

use common::*;

// given: deux PNJ qui obtiennent tous deux 14 à l'initiative, l'un avec DEX 16 et l'autre avec DEX 12, et un PJ dont le DM saisit 14
// when: le DM lance le combat
// then: chaque PNJ a une initiative comprise entre 1 + son modificateur et 20 + son modificateur, et le PNJ à DEX 16 passe avant celui à DEX 12 ; une égalité qui subsiste à DEX égale est tranchée au hasard
#[test]
fn r105_npc_initiative_and_tie_break() {
    rule(
        "R105",
        &[
            "data-capability:table.placeTokens",
            "data-capability:table.startCombat",
            "capability:table.getBattlemapInstance",
        ],
        |s| {
            // DEX 16 is the boss (+3), DEX 12 the bandit (+1): both total 14, the PJ enters 14.
            let map = fresh_instance(s, &["elowen"])?;
            place(s, &map, "elowen", (1, 1), 1)?;
            place(s, &map, "bandit", (5, 5), 1)?;
            place(s, &map, "vesna", (6, 5), 1)?;
            let all = tokens(s, &map, "dm")?;
            let (bandit, boss) = (
                of_character(&all, "bandit")[0].clone(),
                of_character(&all, "vesna")[0].clone(),
            );
            apply(
                s,
                "table.startCombat",
                json!({ "initiative": { "elowen": 14 }, "rolls": { text(&bandit, "id"): 13, text(&boss, "id"): 11 } }),
                Some(&map),
            )?;
            let started = combat(s, &map, "dm")?;
            let line = order(&started);
            let rank = |t: &Value| line.iter().position(|id| id == text(t, "id"));
            check(
                rank(&boss) < rank(&bandit),
                "the DEX 16 NPC does not come before the DEX 12 NPC",
            )?;
            let init_of = |t: &Value| -> Option<i64> {
                started["order"]
                    .as_array()?
                    .iter()
                    .find(|o| text(o, "id") == text(t, "id"))?["init"]
                    .as_i64()
            };
            check(
                init_of(&boss) == Some(14) && init_of(&bandit) == Some(14),
                "both NPC must total 14",
            )?;

            // Left to the dice, each NPC lands within 1 + modifier and 20 + modifier.
            let map = fresh_instance(s, &["elowen"])?;
            place(s, &map, "elowen", (1, 1), 1)?;
            place(s, &map, "bandit", (5, 5), 1)?;
            place(s, &map, "vesna", (6, 5), 1)?;
            let all = tokens(s, &map, "dm")?;
            let (bandit, boss) = (
                of_character(&all, "bandit")[0].clone(),
                of_character(&all, "vesna")[0].clone(),
            );
            apply(
                s,
                "table.startCombat",
                json!({ "initiative": { "elowen": 14 } }),
                Some(&map),
            )?;
            let rolled = combat(s, &map, "dm")?;
            for (t, modifier) in [(&bandit, 1), (&boss, 3)] {
                let init = rolled["order"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .find(|o| text(o, "id") == text(t, "id"))
                    .and_then(|o| o["init"].as_i64())
                    .ok_or("a NPC has no initiative")?;
                check(
                    (1 + modifier..=20 + modifier).contains(&init),
                    "an NPC's initiative is out of d20 + modifier",
                )?;
            }

            // Equal totals and equal DEX: the tie is settled at random, so both orders must come up.
            let mut firsts = std::collections::HashSet::new();
            for _ in 0..24 {
                let map = fresh_instance(s, &[])?;
                place(s, &map, "bandit", (5, 5), 2)?;
                let pair = tokens(s, &map, "dm")?;
                let rolls = json!({ text(&pair[0], "id"): 10, text(&pair[1], "id"): 10 });
                apply(
                    s,
                    "table.startCombat",
                    json!({ "initiative": {}, "rolls": rolls }),
                    Some(&map),
                )?;
                let first = order(&combat(s, &map, "dm")?)
                    .first()
                    .cloned()
                    .unwrap_or_default();
                let name = tokens(s, &map, "dm")?
                    .iter()
                    .find(|t| text(t, "id") == first)
                    .map(|t| text(t, "name").to_string())
                    .unwrap_or_default();
                firsts.insert(name);
            }
            check(firsts.len() == 2, "an exact tie is not settled at random")?;

            Ok(())
        },
    );
}
