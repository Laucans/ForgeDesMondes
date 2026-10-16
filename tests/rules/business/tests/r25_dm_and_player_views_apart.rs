//! Rule R25: Séparation des vues DM et joueurs.
mod common;

use common::*;

// given: un token caché et un PNJ blessé
// when: la player view s'affiche
// then: ni notes, ni PV de PNJ, ni token caché n'apparaissent, l'état du PNJ est seulement « blessé », et un token révélé reprend sa place dans l'initiative.
#[test]
fn r25_dm_and_player_views_apart() {
    rule(
        "R25",
        &[
            "data-capability:table.placeTokens",
            "data-capability:table.setTokenVisibility",
            "data-capability:table.changeHitPoints",
            "data-capability:table.startCombat",
            "capability:table.getBattlemapInstance",
            "capability:live.getDiffusion",
        ],
        |s| {
            let map = fresh_instance(s, &["elowen"])?;
            place(s, &map, "elowen", (1, 1), 1)?;
            place(s, &map, "bandit", (5, 5), 1)?;
            place(s, &map, "vesna", (6, 5), 1)?;
            let all = tokens(s, &map, "dm")?;
            let (bandit, hidden) = (
                of_character(&all, "bandit")[0].clone(),
                of_character(&all, "vesna")[0].clone(),
            );
            apply(
                s,
                "table.setTokenVisibility",
                json!({ "token": hidden["id"], "visible": false }),
                Some(&map),
            )?;
            // 11 PV max, 4 lost: above half, so « blessé ».
            apply(
                s,
                "table.changeHitPoints",
                json!({ "token": bandit["id"], "delta": -4 }),
                Some(&map),
            )?;
            apply(
                s,
                "table.startCombat",
                json!({ "initiative": { "elowen": 20 }, "rolls": { text(&bandit, "id"): 10, text(&hidden, "id"): 11 } }),
                Some(&map),
            )?;

            // The player view shows neither notes, NPC PV nor hidden token.
            let shown = tokens(s, &map, "player")?;
            check(
                by_id(&shown, text(&hidden, "id")).is_none(),
                "a hidden token shows on the player view",
            )?;
            let wounded = by_id(&shown, text(&bandit, "id")).ok_or("the visible NPC is missing")?;
            check(
                wounded.get("hp").is_none_or(Value::is_null)
                    && wounded.get("hpMax").is_none_or(Value::is_null),
                "the PV of a NPC reach the player view",
            )?;
            check(
                text(wounded, "status").to_lowercase() == "blessé",
                "the NPC's state is not « blessé »",
            )?;
            let dm_line = order(&combat(s, &map, "dm")?);
            let player_line = order(&combat(s, &map, "player")?);
            check(
                !player_line.contains(&text(&hidden, "id").to_string()),
                "a hidden token is in the player carousel",
            )?;
            let raw = format!(
                "{} {}",
                s.read(
                    "table.getBattlemapInstance",
                    json!({ "instance": map, "view": "player" })
                )?,
                s.read("live.getDiffusion", json!({ "view": "player" }))?
            );
            for c in items(&fixture("entities"), "characters") {
                let notes = text(&c, "notes");
                check(
                    notes.is_empty() || !raw.contains(notes),
                    "notes reach the player view",
                )?;
            }

            // A token shown again takes its place in the initiative.
            apply(
                s,
                "table.setTokenVisibility",
                json!({ "token": hidden["id"], "visible": true }),
                Some(&map),
            )?;
            check(
                order(&combat(s, &map, "player")?) == dm_line,
                "the revealed token did not take its place",
            )?;

            Ok(())
        },
    );
}
