//! Rule R94: Rail repliable et tiroir sur petit écran.
mod common;

use common::*;

// given: la fiche d'entité ouverte avec son rail déplié
// when: le DM replie le rail et recharge la page, puis réduit la fenêtre sous 960 px, ouvre le tiroir et le referme
// then: le rail est toujours replié après le rechargement, et sous 960 px le tiroir s'ouvre au-dessus de la fiche puis se referme sans la masquer
#[test]
fn r94_rail_folds_and_becomes_a_drawer() {
    rule(
        "R94",
        &[
            "capability:campaign.listCharacters",
            "capability:campaign.listPlaces",
            "capability:campaign.listEvents",
        ],
        |s| {
            // The folding, its memory and the drawer are the page's: tests/rules/journey/R94-*.spec.ts walks them.
            // Here, what the rail has to show once unfolded: the four categories, filled.
            let chars = characters(s, CAMPAIGN)?;
            check(
                chars.iter().any(|c| text(c, "kind") == "pj"),
                "the rail has no PJ to list",
            )?;
            check(
                chars.iter().any(|c| text(c, "kind") == "pnj"),
                "the rail has no PNJ to list",
            )?;
            check(
                !places(s, CAMPAIGN)?.is_empty(),
                "the rail has no place to list",
            )?;
            check(
                !events(s, CAMPAIGN)?.is_empty(),
                "the rail has no event to list",
            )?;

            Ok(())
        },
    );
}
