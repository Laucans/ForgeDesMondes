//! Rule R96: Sections du rail repliables par catégorie.
mod common;

use common::rule;

// given: la fiche d'entité ouverte avec son rail déplié et les sections PJ, PNJ, lieux et événements toutes ouvertes
// when: le DM clique sur le titre « PJ », puis clique de nouveau sur ce même titre
// then: au premier clic, les éléments PJ sont masqués, le triangle à gauche du titre indique que la section est fermée, et les sections PNJ, lieux et événements restent ouvertes ; au second clic, les éléments PJ réapparaissent et le triangle indique que la section est ouverte
#[test]
fn r96_rail_sections_fold_by_category() {
    rule(
        "R96",
        &["micro-ui:CampaignElements", "screen:fiche-entite"],
        |_s| {
            // Nothing of this criterion is kept by the product over the wire: the sections and their triangles live
            // in the page alone, and tests/rules/journey/R96-rail-sections-fold-by-category.spec.ts performs it.
            // Passing here on anything else would assert what the criterion does not say, so the test
            // stays red once the rail is built, until the rules' owner settles this rule's family.
            Err("R96 is a page behaviour with no product-side part: its family is to be settled by the rules' owner".to_string())
        },
    );
}
