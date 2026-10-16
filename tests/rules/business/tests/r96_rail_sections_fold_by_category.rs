//! Rule R96: Sections du rail repliables par catégorie.
mod common;

use common::*;

// given: la fiche d'entité ouverte avec son rail déplié et les sections PJ, PNJ, lieux et événements toutes ouvertes
// when: le DM clique sur le titre « PJ », puis clique de nouveau sur ce même titre
// then: au premier clic, les éléments PJ sont masqués, le triangle à gauche du titre indique que la section est fermée, et les sections PNJ, lieux et événements restent ouvertes ; au second clic, les éléments PJ réapparaissent et le triangle indique que la section est ouverte
#[test]
fn r96_rail_sections_fold_by_category() {
    rule(
        "R96",
        &[
            "capability:campaign.listCharacters",
            "capability:campaign.listPlaces",
            "capability:campaign.listEvents",
        ],
        |s| {
            // Folding is the page's: tests/rules/journey/R96-*.spec.ts clicks the titles.
            // Here, the four sections the rail groups, each with something to hide.
            let chars = characters(s, CAMPAIGN)?;
            check(
                chars.iter().any(|c| text(c, "kind") == "pj"),
                "the PJ section is empty",
            )?;
            check(
                chars.iter().any(|c| text(c, "kind") == "pnj"),
                "the PNJ section is empty",
            )?;
            check(
                !places(s, CAMPAIGN)?.is_empty(),
                "the places section is empty",
            )?;
            check(
                !events(s, CAMPAIGN)?.is_empty(),
                "the events section is empty",
            )?;

            Ok(())
        },
    );
}
