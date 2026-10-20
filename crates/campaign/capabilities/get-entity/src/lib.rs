//! Capability `campaign.getEntity`: one element's full sheet.
//!
//! It reads, and only reads. The row of its persisted query
//! (`crates/campaign/queries/get-entity.graphql`, resolved by [`SQL`]) goes
//! in; the answer, in the shape of one entry of `mock/fixtures/entities.json`,
//! comes out. A PC or an NPC carries its `kind` (`pj` or `pnj`), its
//! `aliases`, its `player` (the id of the player a PC belongs to, `null` for
//! an NPC), its `stats` and their `modifiers`, `ca` and `pvMax`, its
//! `features` (the sheet's actions, then its spells) and its token (`color`,
//! `initials`, `portrait`, `tokenImage`, `crop`). A place or an event has its
//! `kind` (`lieu` or `evenement`), `aliases`, `description` and `notes` alone:
//! it has no sheet (`character-is-pj-or-pnj`). What no column holds yet — the
//! race and class text, the speed, the art and illustration — is not
//! invented: the key is absent.
//!
//! It implements `AbilityModifier@1` (`concepts/AbilityModifier/v1`): the
//! modifier of a score is never stored apart from it, it is computed here, at
//! read time, by [`modifier`] (`dex-modifier-derived`). The same goes for the
//! initials of the token (`default-token`): with neither a portrait nor a
//! token image, the token is the initials of the name on `color`.

use serde_json::{Map, Value, json};

/// The Capability's identifier, as a Micro-UI cites it.
pub const CAPABILITY: &str = "campaign.getEntity";

/// What the persisted query `get-entity.graphql` selects: the element bound
/// to `$1` (compared as text, so that an id which is not a uuid finds nothing
/// instead of failing) with its aliases and, when it is a PC or an NPC, its
/// sheet — the `character` columns are null for a place or an event.
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
    c.spells,
    c.actions,
    c.portrait_media_id::text AS portrait_media_id,
    c.token_media_id::text AS token_media_id,
    c.token_crop,
    c.token_color,
    c.dndbeyond_id,
    to_char(c.dndbeyond_synced_at AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS') AS dndbeyond_synced_at
FROM entity e
LEFT JOIN character c ON c.entity_id = e.id
WHERE e.id::text = $1"#;

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

/// The modifier of an ability score, `AbilityModifier@1`: `floor((score -
/// 10) / 2)`, rounded toward negative infinity, so that 9 gives -1 and 18
/// gives +4.
#[must_use]
pub const fn modifier(score: i64) -> i64 {
    (score - 10).div_euclid(2)
}

/// The element `id` among `rows` — an array of the objects [`SQL`] selects —
/// as the DM reads it; `null` when no row is that element, which is an
/// unknown element, not an empty sheet.
#[must_use]
pub fn answer(rows: &Value, id: &str) -> Value {
    rows.as_array()
        .and_then(|rows| rows.iter().find(|row| row["id"].as_str() == Some(id)))
        .and_then(entity)
        .unwrap_or(Value::Null)
}

/// One row as an element; `None` for a row with no known type.
fn entity(row: &Value) -> Option<Value> {
    let kind = row["type"]
        .as_str()
        .filter(|t| matches!(*t, "pj" | "pnj" | "lieu" | "evenement"))?;
    let mut entity = Map::new();
    entity.insert("id".into(), row["id"].clone());
    entity.insert("name".into(), row["name"].clone());
    entity.insert(
        "aliases".into(),
        row["aliases"]
            .as_array()
            .map_or_else(|| json!([]), |a| json!(a)),
    );
    entity.insert("kind".into(), json!(kind));
    entity.insert("description".into(), json!(text(&row["description"])));
    entity.insert("notes".into(), json!(text(&row["notes"])));
    if matches!(kind, "pj" | "pnj") {
        entity.extend(sheet(row, kind));
    }
    Some(Value::Object(entity))
}

/// What only a PC or an NPC has: its sheet and its token.
fn sheet(row: &Value, kind: &str) -> Map<String, Value> {
    let name = row["name"].as_str().unwrap_or_default();
    let mut stats = Map::new();
    let mut modifiers = Map::new();
    for (label, column) in ABILITIES {
        stats.insert(label.into(), row[column].clone());
        modifiers.insert(
            label.into(),
            row[column]
                .as_i64()
                .map_or(Value::Null, |s| json!(modifier(s))),
        );
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
    let features: Vec<&Value> = [&row["actions"], &row["spells"]]
        .into_iter()
        .filter_map(|list| list.as_array())
        .flatten()
        .collect();
    let mut sheet = Map::new();
    sheet.insert(
        "player".into(),
        if kind == "pj" {
            row["player_id"].clone()
        } else {
            Value::Null
        },
    );
    sheet.insert("prompt".into(), json!(text(&row["initial_prompt"])));
    sheet.insert("background".into(), json!(text(&row["background"])));
    sheet.insert("behavior".into(), json!(text(&row["behavior"])));
    sheet.insert("stats".into(), Value::Object(stats));
    sheet.insert("modifiers".into(), Value::Object(modifiers));
    sheet.insert("ca".into(), row["armor_class"].clone());
    sheet.insert("pvMax".into(), row["hp_max"].clone());
    sheet.insert("features".into(), json!(features));
    sheet.insert("color".into(), row["token_color"].clone());
    sheet.insert("initials".into(), json!(initials(name)));
    sheet.insert(
        "portrait".into(),
        json!(!row["portrait_media_id"].is_null()),
    );
    sheet.insert("tokenImage".into(), json!(!row["token_media_id"].is_null()));
    sheet.insert("crop".into(), row["token_crop"].clone());
    sheet.insert("ddb".into(), ddb.unwrap_or(Value::Null));
    sheet
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

    /// A row as [`SQL`] selects it for a PC or an NPC: stats unset, no
    /// portrait, no token image, no D&D Beyond link.
    fn row(id: &str, kind: &str, name: &str) -> Value {
        json!({
            "id": id, "campaign_id": "c1", "type": kind, "name": name,
            "aliases": [], "description": null, "notes": null, "player_id": null,
            "initial_prompt": null, "background": null, "behavior": null,
            "strength": null, "dexterity": null, "constitution": null,
            "intelligence": null, "wisdom": null, "charisma": null,
            "armor_class": null, "hp_max": null, "spells": [], "actions": [],
            "portrait_media_id": null, "token_media_id": null, "token_crop": null,
            "token_color": "success", "dndbeyond_id": null, "dndbeyond_synced_at": null,
        })
    }

    /// A place's row: the `character` columns are null.
    fn place(id: &str, kind: &str, name: &str) -> Value {
        json!({
            "id": id, "campaign_id": "c1", "type": kind, "name": name,
            "aliases": ["la taverne"], "description": "Grande salle basse.", "notes": null,
            "player_id": null, "initial_prompt": null, "background": null,
            "behavior": null, "strength": null, "dexterity": null, "constitution": null,
            "intelligence": null, "wisdom": null, "charisma": null, "armor_class": null,
            "hp_max": null, "spells": null, "actions": null, "portrait_media_id": null,
            "token_media_id": null, "token_crop": null, "token_color": null,
            "dndbeyond_id": null, "dndbeyond_synced_at": null,
        })
    }

    fn elowen() -> Value {
        let mut r = row("elowen", "pj", "Elowen Ventargent");
        r["aliases"] = json!(["Elowen", "la rôdeuse"]);
        r["player_id"] = json!("jou-julie");
        r["description"] = json!(
            "Rôdeuse elfe aux cheveux cendrés, arc long en if noir. Traque les cendreux depuis la chute de son clan."
        );
        r["notes"] =
            json!("Cherche son frère Aerin, disparu près du Gué des Corbeaux depuis l’hiver.");
        r["strength"] = json!(12);
        r["dexterity"] = json!(18);
        r["constitution"] = json!(14);
        r["intelligence"] = json!(10);
        r["wisdom"] = json!(15);
        r["charisma"] = json!(9);
        r["armor_class"] = json!(16);
        r["hp_max"] = json!(44);
        r["actions"] = json!([{ "name": "Attaque supplémentaire", "text": "Deux attaques." }]);
        r["spells"] = json!([{ "name": "Marque du chasseur", "text": "Sort de niveau 1." }]);
        r["portrait_media_id"] = json!("m1");
        r["token_crop"] = json!({ "zoom": 140, "x": 50, "y": 32 });
        r["dndbeyond_id"] = json!("112847390");
        r["dndbeyond_synced_at"] = json!("2026-09-26T18:12:00");
        r
    }

    fn morlox() -> Value {
        let mut r = row("morlox", "pnj", "Morlox");
        r["token_color"] = json!("secondary");
        r["actions"] = json!([{ "name": "Dague", "text": "Attaque au corps à corps." }]);
        r
    }

    fn fixture() -> Value {
        let path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../mock/fixtures/entities.json");
        serde_json::from_str(&std::fs::read_to_string(path).expect("the fixture")).expect("JSON")
    }

    fn known(id: &str) -> Value {
        let fixture = fixture();
        ["characters", "places", "events"]
            .iter()
            .flat_map(|list| fixture[*list].as_array().expect("a list").clone())
            .find(|e| e["id"] == id)
            .unwrap_or_else(|| panic!("{id} is not in the fixture"))
    }

    #[test]
    fn conformance() {
        platform_conformance::check("AbilityModifier@1", |input| {
            let score = input["score"].as_i64().expect("an integer score");
            json!({ "modifier": modifier(score) })
        });
    }

    #[test]
    fn elowen_with_a_dex_of_18_has_a_dex_modifier_of_plus_4() {
        let got = answer(&json!([elowen()]), "elowen");
        assert_eq!(got["stats"]["DEX"], 18);
        assert_eq!(got["modifiers"]["DEX"], 4);
    }

    #[test]
    fn every_ability_has_its_modifier_floored_from_its_score() {
        let got = answer(&json!([elowen()]), "elowen");
        for (label, modifier) in [
            ("FOR", 1),
            ("DEX", 4),
            ("CON", 2),
            ("INT", 0),
            ("SAG", 2),
            ("CHA", -1),
        ] {
            assert_eq!(got["modifiers"][label], modifier, "{label}");
        }
    }

    #[test]
    fn a_score_left_unset_has_no_modifier() {
        let got = answer(&json!([morlox()]), "morlox");
        assert_eq!(got["stats"]["DEX"], Value::Null);
        assert_eq!(got["modifiers"]["DEX"], Value::Null);
    }

    #[test]
    fn a_pc_has_kind_pj_and_a_player_and_an_npc_has_kind_pnj_and_none() {
        let rows = json!([elowen(), morlox()]);
        assert_eq!(answer(&rows, "elowen")["kind"], "pj");
        assert_eq!(answer(&rows, "elowen")["player"], "jou-julie");
        assert_eq!(answer(&rows, "morlox")["kind"], "pnj");
        assert_eq!(answer(&rows, "morlox")["player"], Value::Null);
    }

    #[test]
    fn an_npc_never_shows_a_player_whatever_the_row_says() {
        let mut npc = morlox();
        npc["player_id"] = json!("jou-julie");
        assert_eq!(answer(&json!([npc]), "morlox")["player"], Value::Null);
    }

    #[test]
    fn a_sheet_has_its_stats_ac_hp_features_token_and_notes() {
        let got = answer(&json!([elowen()]), "elowen");
        for key in ["stats", "ca", "pvMax", "features", "initials", "notes"] {
            assert!(!got[key].is_null(), "{key} is missing from {got}");
        }
        assert_eq!(got["ca"], 16);
        assert_eq!(got["pvMax"], 44);
        assert_eq!(got["initials"], "EV");
        assert!(got["notes"].as_str().expect("notes").contains("Aerin"));
    }

    #[test]
    fn the_features_are_the_actions_then_the_spells() {
        let got = answer(&json!([elowen()]), "elowen");
        let names: Vec<&str> = got["features"]
            .as_array()
            .expect("an array")
            .iter()
            .map(|f| f["name"].as_str().expect("a name"))
            .collect();
        assert_eq!(names, ["Attaque supplémentaire", "Marque du chasseur"]);
    }

    #[test]
    fn a_missing_alias_list_action_list_or_spell_list_is_an_empty_one() {
        let mut r = morlox();
        r["aliases"] = Value::Null;
        r["actions"] = Value::Null;
        r["spells"] = Value::Null;
        let got = answer(&json!([r]), "morlox");
        assert_eq!(got["aliases"], json!([]));
        assert_eq!(got["features"], json!([]));
    }

    #[test]
    fn with_no_token_image_and_no_portrait_the_token_is_the_initial_on_its_colour() {
        let mut bandit = row("bandit", "pnj", "Bandit");
        bandit["token_color"] = json!("danger");
        let got = answer(&json!([bandit]), "bandit");
        assert_eq!(got["portrait"], false);
        assert_eq!(got["tokenImage"], false);
        assert_eq!(got["crop"], Value::Null);
        assert_eq!(got["color"], "danger");
        assert_eq!(got["initials"], "BA");
    }

    #[test]
    fn a_portrait_or_a_token_image_is_told_from_none() {
        let got = answer(&json!([elowen()]), "elowen");
        assert_eq!(got["portrait"], true);
        assert_eq!(got["crop"], json!({ "zoom": 140, "x": 50, "y": 32 }));
        let mut npc = morlox();
        npc["token_media_id"] = json!("t1");
        let got = answer(&json!([npc]), "morlox");
        assert_eq!(got["tokenImage"], true);
        assert_eq!(got["portrait"], false);
    }

    #[test]
    fn the_initials_are_two_capitals() {
        assert_eq!(initials("Elowen Ventargent"), "EV");
        assert_eq!(initials("Morlox"), "MO");
        assert_eq!(initials("la taverne du sanglier"), "LT");
        assert_eq!(initials("Ébène"), "ÉB");
        assert_eq!(initials("  "), "");
    }

    #[test]
    fn a_place_or_an_event_is_answered_without_a_sheet() {
        let rows = json!([
            place("taverne-sanglier", "lieu", "La Taverne du Sanglier"),
            place("incendie-moulin", "evenement", "L’incendie du moulin"),
        ]);
        let tavern = answer(&rows, "taverne-sanglier");
        assert_eq!(tavern["kind"], "lieu");
        assert_eq!(tavern["aliases"], json!(["la taverne"]));
        assert_eq!(tavern["description"], "Grande salle basse.");
        assert_eq!(tavern["notes"], "");
        assert_eq!(answer(&rows, "incendie-moulin")["kind"], "evenement");
        for key in [
            "stats",
            "modifiers",
            "ca",
            "pvMax",
            "features",
            "player",
            "color",
        ] {
            assert!(tavern.get(key).is_none(), "{key} on a place: {tavern}");
        }
    }

    #[test]
    fn an_unknown_element_has_no_answer() {
        assert_eq!(answer(&json!([elowen()]), "nobody"), Value::Null);
        assert_eq!(answer(&json!([]), "elowen"), Value::Null);
        assert_eq!(answer(&Value::Null, "elowen"), Value::Null);
    }

    #[test]
    fn a_row_of_an_unknown_type_has_no_answer() {
        assert_eq!(
            answer(&json!([row("x", "objet", "Épée")]), "x"),
            Value::Null
        );
    }

    #[test]
    fn only_the_asked_element_is_answered() {
        let rows = json!([morlox(), elowen()]);
        assert_eq!(answer(&rows, "elowen")["name"], "Elowen Ventargent");
    }

    #[test]
    fn a_text_left_empty_is_an_empty_string_as_in_the_fixture() {
        let got = answer(&json!([morlox()]), "morlox");
        for key in ["description", "notes", "prompt", "background", "behavior"] {
            assert_eq!(got[key], "", "{key}");
        }
    }

    #[test]
    fn a_blank_dndbeyond_id_is_no_link() {
        for blank in [json!(null), json!(""), json!("   ")] {
            let mut r = elowen();
            r["dndbeyond_id"] = blank;
            assert_eq!(answer(&json!([r]), "elowen")["ddb"], Value::Null);
        }
        let got = answer(&json!([elowen()]), "elowen");
        assert_eq!(
            got["ddb"],
            json!({
                "url": "https://www.dndbeyond.com/characters/112847390",
                "syncedAt": "2026-09-26T18:12:00",
            })
        );
    }

    #[test]
    fn a_pc_has_the_fixtures_values_for_what_the_store_holds() {
        let got = answer(&json!([elowen()]), "elowen");
        let expected = known("elowen");
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
            "notes",
            "ddb",
        ] {
            assert_eq!(got[key], expected[key], "{key}");
        }
    }

    #[test]
    fn a_place_and_an_event_have_the_fixtures_values() {
        let rows = json!([place("taverne-sanglier", "lieu", "La Taverne du Sanglier")]);
        let got = answer(&rows, "taverne-sanglier");
        let expected = known("taverne-sanglier");
        assert_eq!(got["name"], expected["name"]);
        assert_eq!(got["id"], expected["id"]);
    }

    #[test]
    fn every_key_of_a_sheet_is_a_key_of_the_fixture_but_the_derived_ones() {
        let known = known("elowen");
        let known = known.as_object().expect("an object");
        let got = answer(&json!([elowen()]), "elowen");
        for key in got.as_object().expect("an object").keys() {
            if matches!(key.as_str(), "modifiers" | "tokenImage") {
                continue;
            }
            assert!(known.contains_key(key), "{key} is not in the fixture");
        }
    }

    #[test]
    fn the_sql_is_one_select_statement_bound_to_one_element() {
        assert!(
            SQL.trim_start().to_ascii_uppercase().starts_with("SELECT"),
            "{SQL}"
        );
        assert!(!SQL.contains(';'), "a second statement in {SQL}");
        assert!(SQL.contains("WHERE e.id::text = $1"), "{SQL}");
        assert!(SQL.contains("LEFT JOIN character"), "{SQL}");
        assert!(!SQL.to_ascii_lowercase().contains("modifier"), "{SQL}");
    }
}
