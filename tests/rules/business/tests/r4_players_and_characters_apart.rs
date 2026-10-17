//! Rule R4: Joueurs et personnages distincts.
mod common;

use common::*;

// given: un PJ lié à un joueur et un PNJ
// when: le DM ouvre les onglets de la campagne
// then: chacun apparaît dans le bon onglet avec sa fiche (caractéristiques, CA, PV, sorts, actions), son token et ses notes.
#[test]
fn r4_players_and_characters_apart() {
    rule(
        "R4",
        &[
            "capability:campaign.listPlayers",
            "capability:campaign.listCharacters",
            "capability:campaign.listPlaces",
            "capability:campaign.listEvents",
            "capability:campaign.getEntity",
        ],
        |s| {
            let players = items(
                &s.read("campaign.listPlayers", json!({ "campaign": CAMPAIGN }))?,
                "players",
            );
            let chars = characters(s, CAMPAIGN)?;
            let other_tabs: Vec<String> = places(s, CAMPAIGN)?
                .iter()
                .chain(events(s, CAMPAIGN)?.iter())
                .map(|e| text(e, "id").to_string())
                .collect();
            let known = items(&fixture("entities"), "characters");

            let pj = by_id(&chars, "elowen").ok_or("the PJ is not among the characters")?;
            check(text(pj, "kind") == "pj", "the PJ is not a PJ")?;
            check(
                by_id(&players, text(pj, "player")).is_some(),
                "the PJ is linked to no listed player",
            )?;
            let pnj = by_id(&chars, "morlox").ok_or("the PNJ is not among the characters")?;
            check(text(pnj, "kind") == "pnj", "the PNJ is not a PNJ")?;
            check(
                pnj.get("player").is_none_or(Value::is_null),
                "the PNJ is linked to a player",
            )?;

            for id in ["elowen", "morlox"] {
                let sheet = get_entity(s, id)?;
                for stat in ["FOR", "DEX", "CON", "INT", "SAG", "CHA"] {
                    check(
                        sheet
                            .pointer(&format!("/stats/{stat}"))
                            .and_then(Value::as_i64)
                            .is_some(),
                        &format!("{id} has no {stat}"),
                    )?;
                }
                check(sheet.get("ca").and_then(Value::as_i64).is_some(), "no CA")?;
                check(
                    sheet.get("pvMax").and_then(Value::as_i64).is_some(),
                    "no PV",
                )?;
                check(
                    sheet
                        .get("features")
                        .and_then(Value::as_array)
                        .is_some_and(|f| !f.is_empty()),
                    "no spells and actions",
                )?;
                check(!text(&sheet, "initials").is_empty(), "no token")?;
                let notes = by_id(&known, id).map_or("", |c| text(c, "notes"));
                check(text(&sheet, "notes") == notes, "the notes differ")?;
                check(
                    !other_tabs.iter().any(|o| o == id),
                    "a character is among places or events",
                )?;
            }

            Ok(())
        },
    );
}
