//! Rule R20: Dépôt de personnages en une ou plusieurs instances.
mod common;

use common::*;

// given: un monstre glissé « × 3 » sur la battlemap
// when: il est déposé
// then: trois instances numérotées 1 à 3 apparaissent, chacune renommable sans toucher aux autres.
#[test]
fn r20_dropping_one_or_several_instances() {
    rule(
        "R20",
        &[
            "data-capability:table.placeTokens",
            "data-capability:table.renameToken",
            "capability:table.getBattlemapInstance",
            "capability:campaign.listCharacters",
            "capability:session.listScenes",
            "capability:session.listSessions",
            "data-capability:session.addScene",
            "data-capability:session.createSession",
            "data-capability:table.createBattlemapInstance",
        ],
        |s| {
            let map = fresh_instance(s, &[])?;
            place(s, &map, "bandit", (5, 5), 3)?;
            let placed = tokens(s, &map, "dm")?;
            check(placed.len() == 3, "× 3 must drop three instances")?;
            let mut names: Vec<String> =
                placed.iter().map(|t| text(t, "name").to_string()).collect();
            names.sort();
            check(
                names == ["Bandit 1", "Bandit 2", "Bandit 3"],
                "the instances are not numbered 1 to 3",
            )?;
            let mut numbers: Vec<u64> = placed.iter().filter_map(|t| t["num"].as_u64()).collect();
            numbers.sort_unstable();
            check(numbers == [1, 2, 3], "the instance numbers are not 1 to 3")?;

            // Renaming one leaves the other two as they were.
            let second = token_named(&placed, "Bandit 2")?;
            apply(
                s,
                "table.renameToken",
                json!({ "token": second["id"], "name": "Bandit borgne" }),
                Some(&map),
            )?;
            let renamed = tokens(s, &map, "dm")?;
            check(renamed.len() == 3, "renaming changed the count")?;
            check(
                by_id(&renamed, text(&second, "id"))
                    .is_some_and(|t| text(t, "name") == "Bandit borgne"),
                "the rename was lost",
            )?;
            for name in ["Bandit 1", "Bandit 3"] {
                let was = token_named(&placed, name)?;
                let is = by_id(&renamed, text(&was, "id")).ok_or("a token vanished")?;
                check(
                    *is == was,
                    &format!("{name} changed when another was renamed"),
                )?;
            }

            Ok(())
        },
    );
}
