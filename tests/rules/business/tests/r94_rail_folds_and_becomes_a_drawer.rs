//! Rule R94: Rail repliable et tiroir sur petit écran.
mod common;

use common::rule;

// given: la fiche d'entité ouverte avec son rail déplié
// when: le DM replie le rail et recharge la page, puis réduit la fenêtre sous 960 px, ouvre le tiroir et le referme
// then: le rail est toujours replié après le rechargement, et sous 960 px le tiroir s'ouvre au-dessus de la fiche puis se referme sans la masquer
#[test]
fn r94_rail_folds_and_becomes_a_drawer() {
    rule(
        "R94",
        &["micro-ui:CampaignElements", "screen:fiche-entite"],
        |_s| {
            // Nothing of this criterion is kept by the product over the wire: the fold memory and the drawer live
            // in the page alone, and tests/rules/journey/R94-rail-folds-and-becomes-a-drawer.spec.ts performs it.
            // Passing here on anything else would assert what the criterion does not say, so the test
            // stays red once the rail is built, until the rules' owner settles this rule's family.
            Err("R94 is a page behaviour with no product-side part: its family is to be settled by the rules' owner".to_string())
        },
    );
}
