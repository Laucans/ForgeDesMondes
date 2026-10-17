//! Capability `campaign.listCharacters`: the PCs and NPCs of a campaign, for
//! the characters tab.
//!
//! It reads, and only reads. The rows of its persisted query
//! (`crates/campaign/queries/list-characters.graphql`, resolved by [`SQL`])
//! go in; the answer, in the shape of
//! `mock/fixtures/campaign.listCharacters.json` — an array of characters —
//! comes out. Each character carries what the store holds of the fixture's
//! keys: its `kind` (`pj` or `pnj`), its `aliases`, its `player` (the id of the
//! player a PC belongs to, `null` for an NPC), its `stats`, `ca` and `pvMax`,
//! its `features` (the sheet's actions) and its token (`color`, `initials`,
//! `portrait`, `crop`). What no column holds yet — the race and class text,
//! the speed, the art and illustration, the NPC's token name — is not
//! invented: the key is absent.
//!
//! `character-is-pj-or-pnj`: only an element of type pj or pnj has a sheet,
//! and only a PC has a player. A place or an event is left out, and so is an
//! NPC's player, whatever the row says. A text the DM left empty is `""`, as
//! in the fixture, never `null`. `initials` is derived at read time
//! (it is the letters of the name, never stored) and the DEX modifier is not
//! in the answer: a list shows no modifier.

use serde_json::{Map, Value, json};

/// The Capability's identifier, as a Micro-UI cites it.
pub const CAPABILITY: &str = "campaign.listCharacters";

/// What the persisted query `list-characters.graphql` selects: the elements
/// of the campaign bound to `$1` (compared as text, so that an id which is not
/// a uuid lists nobody instead of failing) which have a sheet and are a pj or
/// a pnj, PCs first, then in the order they were created. `campaign_id` and
/// `type` are selected too, so that [`answer`] keeps only the campaign's own
/// characters whoever ran the statement.
pub const SQL: &str = r#"SELECT e.id::text AS id,
    e.campaign_id::text AS campaign_id,
    e.type,
    e.name,
    coalesce(
        (SELECT jsonb_agg(a.alias ORDER BY a.alias) FROM entity_alias a WHERE a.entity_id = e.id),
        '[]'::jsonb
    ) AS aliases,
    e.description,
    e.notes,
    c.player_id::text AS player_id,
    c.initial_prompt,
    c.background,
    c.behavior,
    c.strength,
    c.dexterity,
    c.constitution,
    c.intelligence,
    c.wisdom,
    c.charisma,
    c.armor_class,
    c.hp_max,
    c.actions,
    c.portrait_media_id::text AS portrait_media_id,
    c.token_crop,
    c.token_color,
    c.dndbeyond_id,
    to_char(c.dndbeyond_synced_at AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS') AS dndbeyond_synced_at
FROM entity e
JOIN character c ON c.entity_id = e.id
WHERE e.campaign_id::text = $1
  AND e.type IN ('pj', 'pnj')
ORDER BY e.type, e.created_at, e.id"#;

/// Where a D&D Beyond character lives, from its id.
const DNDBEYOND: &str = "https://www.dndbeyond.com/characters/";

/// The six abilities, as the sheet writes them, with the column of each.
const ABILITIES: [(&str, &str); 6] = [
    ("FOR", "strength"),
    ("DEX", "dexterity"),
    ("CON", "constitution"),
    ("INT", "intelligence"),
    ("SAG", "wisdom"),
    ("CHA", "charisma"),
];

/// The characters of `campaign` among `rows` — an array of the objects [`SQL`]
/// selects — as the tab reads them, PCs first, in the order of the rows. An
/// element of another campaign, or of a type that is neither pj nor pnj, is
/// left out; no row is an empty, well-formed answer.
#[must_use]
pub fn answer(rows: &Value, campaign: &str) -> Value {
    let characters: Vec<Value> = rows
        .as_array()
        .map(|rows| {
            rows.iter()
                .filter(|row| row["campaign_id"].as_str() == Some(campaign))
                .filter_map(character)
                .collect()
        })
        .unwrap_or_default();
    Value::Array(characters)
}

/// One row as a character; `None` for an element that is not a pj or a pnj.
fn character(row: &Value) -> Option<Value> {
    let kind = row["type"]
        .as_str()
        .filter(|t| matches!(*t, "pj" | "pnj"))?;
    let name = row["name"].as_str().unwrap_or_default();
    let mut stats = Map::new();
    for (label, column) in ABILITIES {
        stats.insert(label.into(), row[column].clone());
    }
    let ddb = row["dndbeyond_id"]
        .as_str()
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .map(|id| {
            json!({
                "url": format!("{DNDBEYOND}{id}"),
                "syncedAt": row["dndbeyond_synced_at"],
            })
        });
    Some(json!({
        "id": row["id"],
        "name": row["name"],
        "aliases": row["aliases"].as_array().map_or_else(|| json!([]), |a| json!(a)),
        "kind": kind,
        "player": if kind == "pj" { row["player_id"].clone() } else { Value::Null },
        "description": text(&row["description"]),
        "prompt": text(&row["initial_prompt"]),
        "background": text(&row["background"]),
        "behavior": text(&row["behavior"]),
        "stats": stats,
        "ca": row["armor_class"],
        "pvMax": row["hp_max"],
        "features": row["actions"].as_array().map_or_else(|| json!([]), |a| json!(a)),
        "color": row["token_color"],
        "initials": initials(name),
        "portrait": !row["portrait_media_id"].is_null(),
        "crop": row["token_crop"],
        "notes": text(&row["notes"]),
        "ddb": ddb,
    }))
}

/// A text of the sheet: what the DM wrote, or `""` when nothing is, as the
/// fixture writes it.
fn text(value: &Value) -> &str {
    value.as_str().unwrap_or_default()
}

/// The letters a token without an image shows: the first letters of the first
/// two words of the name, or the first two letters of a one-word name, in
/// capitals.
fn initials(name: &str) -> String {
    let words: Vec<&str> = name.split_whitespace().collect();
    let letters: Vec<char> = match words.as_slice() {
        [] => Vec::new(),
        [only] => only.chars().take(2).collect(),
        [first, second, ..] => first
            .chars()
            .take(1)
            .chain(second.chars().take(1))
            .collect(),
    };
    letters
        .into_iter()
        .flat_map(char::to_uppercase)
        .take(2)
        .collect()
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    const CENDRES: &str = "c1000000-0000-4000-8000-000000000001";
    const OTHER: &str = "c1000000-0000-4000-8000-000000000002";

    /// A sheet row as [`SQL`] selects it: stats unset, no portrait, no D&D
    /// Beyond link.
    fn row(id: &str, campaign: &str, kind: &str, name: &str) -> Value {
        json!({
            "id": id, "campaign_id": campaign, "type": kind, "name": name,
            "aliases": [], "description": null, "notes": null, "player_id": null,
            "initial_prompt": null, "background": null, "behavior": null,
            "strength": null, "dexterity": null, "constitution": null,
            "intelligence": null, "wisdom": null, "charisma": null,
            "armor_class": null, "hp_max": null, "actions": [],
            "portrait_media_id": null, "token_crop": null, "token_color": "success",
            "dndbeyond_id": null, "dndbeyond_synced_at": null,
        })
    }

    fn elowen() -> Value {
        let mut r = row("elowen", CENDRES, "pj", "Elowen Ventargent");
        r["aliases"] = json!(["Elowen", "la rôdeuse"]);
        r["player_id"] = json!("jou-julie");
        r["description"] = json!(
            "Rôdeuse elfe aux cheveux cendrés, arc long en if noir. Traque les cendreux depuis la chute de son clan."
        );
        r["strength"] = json!(12);
        r["dexterity"] = json!(18);
        r["constitution"] = json!(14);
        r["intelligence"] = json!(10);
        r["wisdom"] = json!(15);
        r["charisma"] = json!(9);
        r["armor_class"] = json!(16);
        r["hp_max"] = json!(44);
        r["actions"] = json!([{ "name": "Attaque supplémentaire", "text": "Deux attaques." }]);
        r["portrait_media_id"] = json!("m1");
        r["token_crop"] = json!({ "zoom": 140, "x": 50, "y": 32 });
        r["dndbeyond_id"] = json!("112847390");
        r["dndbeyond_synced_at"] = json!("2026-09-26T18:12:00");
        r
    }

    fn morlox() -> Value {
        row("morlox", CENDRES, "pnj", "Morlox")
    }

    fn fixture() -> Value {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../../mock/fixtures/campaign.listCharacters.json");
        serde_json::from_str(&std::fs::read_to_string(path).expect("the fixture")).expect("JSON")
    }

    fn by_id<'a>(listed: &'a Value, id: &str) -> &'a Value {
        listed
            .as_array()
            .expect("an array")
            .iter()
            .find(|c| c["id"] == id)
            .unwrap_or_else(|| panic!("{id} is not listed in {listed}"))
    }

    #[test]
    fn a_pc_has_kind_pj_and_a_player_and_an_npc_has_kind_pnj_and_none() {
        let listed = answer(&json!([elowen(), morlox()]), CENDRES);
        assert_eq!(by_id(&listed, "elowen")["kind"], "pj");
        assert_eq!(by_id(&listed, "elowen")["player"], "jou-julie");
        assert_eq!(by_id(&listed, "morlox")["kind"], "pnj");
        assert_eq!(by_id(&listed, "morlox")["player"], Value::Null);
    }

    #[test]
    fn an_npc_never_shows_a_player_whatever_the_row_says() {
        let mut npc = morlox();
        npc["player_id"] = json!("jou-julie");
        assert_eq!(answer(&json!([npc]), CENDRES)[0]["player"], Value::Null);
    }

    #[test]
    fn a_place_or_an_event_does_not_appear() {
        let rows = json!([
            elowen(),
            row("tour", CENDRES, "lieu", "La Tour d’Ébène"),
            row("incendie", CENDRES, "evenement", "L’incendie du moulin"),
        ]);
        let listed = answer(&rows, CENDRES);
        assert_eq!(listed.as_array().expect("an array").len(), 1);
        assert!(!listed.to_string().contains("tour"), "{listed}");
        assert!(!listed.to_string().contains("incendie"), "{listed}");
    }

    #[test]
    fn a_character_of_another_campaign_does_not_appear() {
        let rows = json!([elowen(), row("intrus", OTHER, "pnj", "Intrus")]);
        let listed = answer(&rows, CENDRES);
        assert_eq!(listed.as_array().expect("an array").len(), 1);
        assert_eq!(answer(&rows, OTHER)[0]["id"], "intrus");
    }

    #[test]
    fn a_pc_has_the_fixtures_values_for_what_the_store_holds() {
        let listed = answer(&json!([elowen()]), CENDRES);
        let got = &listed[0];
        let expected = by_id(&fixture(), "elowen").clone();
        for key in [
            "id",
            "name",
            "aliases",
            "kind",
            "player",
            "description",
            "stats",
            "ca",
            "pvMax",
            "color",
            "initials",
            "portrait",
            "crop",
            "ddb",
        ] {
            assert_eq!(got[key], expected[key], "{key}");
        }
        assert_eq!(got["features"][0]["name"], "Attaque supplémentaire");
    }

    #[test]
    fn an_npc_has_the_fixtures_values_and_no_link_to_d_and_d_beyond() {
        let mut npc = morlox();
        npc["token_color"] = json!("secondary");
        npc["portrait_media_id"] = json!("m2");
        let got = &answer(&json!([npc]), CENDRES)[0];
        let fixture = fixture();
        let expected = by_id(&fixture, "morlox");
        for key in [
            "id", "name", "kind", "player", "color", "initials", "portrait",
        ] {
            assert_eq!(got[key], expected[key], "{key}");
        }
        assert_eq!(got["ddb"], Value::Null);
    }

    #[test]
    fn every_key_of_the_answer_is_a_key_of_the_fixture() {
        let listed = answer(&json!([elowen(), morlox()]), CENDRES);
        let known: Vec<String> = fixture()[0]
            .as_object()
            .expect("an object")
            .keys()
            .cloned()
            .collect();
        for character in listed.as_array().expect("an array") {
            for key in character.as_object().expect("an object").keys() {
                assert!(known.contains(key), "{key} is not in the fixture");
            }
        }
    }

    #[test]
    fn the_initials_are_two_capitals() {
        // Two words: their first letters; one word: its first two.
        assert_eq!(initials("Elowen Ventargent"), "EV");
        assert_eq!(initials("Morlox"), "MO");
        assert_eq!(initials("Bandit"), "BA");
        assert_eq!(initials("élodie du lac"), "ÉD");
        assert_eq!(initials("  Kael   Dorn "), "KD");
        assert_eq!(initials("X"), "X");
        assert_eq!(initials("ßa"), "SS");
        assert_eq!(initials(""), "");
    }

    #[test]
    fn a_text_left_empty_is_an_empty_string_as_in_the_fixture() {
        let listed = answer(&json!([morlox()]), CENDRES);
        for key in ["description", "prompt", "background", "behavior", "notes"] {
            assert_eq!(listed[0][key], "", "{key}");
        }
        let expected = fixture();
        for key in ["description", "prompt", "background", "behavior", "notes"] {
            assert!(
                expected[0][key].is_string(),
                "{key} is a string in the fixture"
            );
        }
    }

    #[test]
    fn a_blank_dndbeyond_id_is_no_link() {
        for blank in ["", "  "] {
            let mut r = elowen();
            r["dndbeyond_id"] = json!(blank);
            assert_eq!(
                answer(&json!([r]), CENDRES)[0]["ddb"],
                Value::Null,
                "{blank:?}"
            );
        }
        let mut padded = elowen();
        padded["dndbeyond_id"] = json!(" 112847390 ");
        assert_eq!(
            answer(&json!([padded]), CENDRES)[0]["ddb"]["url"],
            "https://www.dndbeyond.com/characters/112847390"
        );
    }

    #[test]
    fn a_missing_alias_list_or_action_list_is_an_empty_one() {
        let mut r = morlox();
        r["aliases"] = Value::Null;
        r["actions"] = Value::Null;
        let listed = answer(&json!([r]), CENDRES);
        assert_eq!(listed[0]["aliases"], json!([]));
        assert_eq!(listed[0]["features"], json!([]));
    }

    #[test]
    fn no_row_is_an_empty_well_formed_answer() {
        assert_eq!(answer(&json!([]), CENDRES), json!([]));
        assert_eq!(answer(&Value::Null, CENDRES), json!([]));
    }

    #[test]
    fn the_sql_is_one_select_statement_bound_to_one_campaign() {
        assert!(
            SQL.trim_start().to_ascii_uppercase().starts_with("SELECT"),
            "{SQL}"
        );
        assert!(!SQL.contains(';'), "a second statement in {SQL}");
        assert!(SQL.contains("WHERE e.campaign_id::text = $1"), "{SQL}");
        assert!(SQL.contains("e.type IN ('pj', 'pnj')"), "{SQL}");
    }
}
