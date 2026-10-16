//! Rule R114: Synchronisation des onglets sous une seconde.
mod common;

use common::*;

// given: la DM view et la player view ouvertes dans deux onglets de la même machine
// when: le DM dépose un token sur un hex de la DM view
// then: le token apparaît sur le même hex de la player view en moins d'une seconde
#[test]
fn r114_tabs_in_sync_under_a_second() {
    rule(
        "R114",
        &[
            "data-capability:table.placeTokens",
            "data-capability:table.moveToken",
            "capability:table.getBattlemapInstance",
        ],
        |s| {
            let map = fresh_instance(s, &["elowen"])?;
            place(s, &map, "elowen", (1, 1), 1)?;
            let token = of_character(&tokens(s, &map, "dm")?, "elowen")[0].clone();

            // The DM drops the token on a hex; the player view must show it there within a second.
            let started = std::time::Instant::now();
            apply(
                s,
                "table.moveToken",
                json!({ "token": token["id"], "q": 4, "r": 3 }),
                Some(&map),
            )?;
            let mut seen = false;
            while started.elapsed() < std::time::Duration::from_secs(1) && !seen {
                let shown = tokens(s, &map, "player")?;
                seen =
                    by_id(&shown, text(&token, "id")).is_some_and(|t| t["q"] == 4 && t["r"] == 3);
                if !seen {
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
            }
            check(
                seen,
                "the token did not reach the player view on the same hex within a second",
            )?;

            Ok(())
        },
    );
}
