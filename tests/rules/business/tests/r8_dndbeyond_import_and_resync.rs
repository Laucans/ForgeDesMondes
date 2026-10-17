//! Rule R8: Import et resynchronisation D&D Beyond.
mod common;

use common::*;

// given: un PJ importé de D&D Beyond avec alias, notes et token locaux
// when: le DM clique « Resynchroniser » après une montée de niveau
// then: stats, PV et sorts sont à jour et alias, notes et token sont inchangés.
#[test]
fn r8_dndbeyond_import_and_resync() {
    rule(
        "R8",
        &[
            "data-capability:dndbeyond.resyncCharacter",
            "capability:campaign.getEntity",
        ],
        |s| {
            let id = "elowen";
            let before = get_entity(s, id)?;
            check(
                !text(&before["ddb"], "url").is_empty(),
                "the PJ was not imported from D&D Beyond",
            )?;
            let level_up = &fixture("EntityEditor")["ddbResync"][id];

            apply(s, "dndbeyond.resyncCharacter", json!({}), Some(id))?;
            let after = get_entity(s, id)?;

            let patch = &level_up["patch"];
            check(
                after["pvMax"] == patch["pvMax"],
                "the PV are not up to date",
            )?;
            check(
                after["stats"]["SAG"] == patch["stats"]["SAG"],
                "the stats are not up to date",
            )?;
            let spells: Vec<&str> = after["features"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|f| text(f, "name"))
                .collect();
            for gained in ["Explorateur né amélioré", "Communication avec les animaux"] {
                check(
                    spells.contains(&gained),
                    &format!("{gained} is missing after the level-up"),
                )?;
            }
            for local in ["aliases", "notes", "crop", "portrait", "initials", "color"] {
                check(
                    after[local] == before[local],
                    &format!("{local} changed by the resync"),
                )?;
            }

            Ok(())
        },
    );
}
