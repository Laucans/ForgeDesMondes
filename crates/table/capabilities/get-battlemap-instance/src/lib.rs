//! Capability `table.getBattlemapInstance`: one battlemap instance, as the DM
//! or the players see it.
//!
//! It reads, and only reads. The row of its persisted query
//! (`crates/table/queries/get-battlemap-instance.graphql`, resolved by
//! [`SQL`]) goes in, with the view the caller names; the answer, in the shape
//! of `mock/fixtures/table.getBattlemapInstance.json`, comes out:
//!
//! - `id`, `battlemap`, `session`, `scene`: the instance as stored, and
//!   `roster`, the characters that have a token on it;
//! - `state.tokens`: each token with its character (`char`), `kind` (`pj` or
//!   `pnj`), `name`, `num`, hex (`q`, `r`), `visible`, `states` (its
//!   conditions), `status` (its health band), `dead`, and its disc — `color`,
//!   `initials`, `portrait` and `crop`: a coloured disc with the initial of
//!   the name until the character has a portrait;
//! - `state.combat`: the combat running, with its `round`, `current` (the
//!   token whose turn it is) and `order`, the carousel — each participant at
//!   its rank, dead ones skipped; `null` when no combat runs.
//!
//! The DM view shows everything: the hit points (`hp`, `hpMax`) and the hidden
//! tokens, which keep their rank in the carousel and are flagged `hidden`
//! (`hidden-keeps-rank`). The player view receives the band alone
//! (`players-see-band-only`): no hit point figure, no hidden token, hidden
//! participant or turn, and no notes — they are not even read. A view that is
//! not `dm` is the players': what is not asked for is not shown.
//!
//! The hit points of a PC are never shown nor tracked (`pj-hp-not-tracked`):
//! a PC has no band and is never dead. An NPC is dead if and only if its
//! current hit points are 0 or less (`dead-derived`).
//!
//! [`health_band`] is this Capability's own reading of `HealthBand@1`, held to
//! the Concept's fixtures; the `status` of every token comes from it.
//!
//! An unknown instance, or no row at all, is `null`.

use std::collections::BTreeMap;

use serde_json::{Map, Value, json};

/// The Capability's identifier, as a Micro-UI cites it.
pub const CAPABILITY: &str = "table.getBattlemapInstance";

/// What the persisted query `get-battlemap-instance.graphql` selects: the
/// instance bound to `$1` (compared as text, so that an id which is not a
/// uuid finds nothing instead of failing) with its tokens — each joined to
/// the sheet it stands for — and the combat running, with its initiative
/// entries in the order of `initiative_order_view`. The tables' generated
/// columns (`dead`, `health_band`, `label_initial`) are not read: they are
/// derived here.
pub const SQL: &str = r#"SELECT i.id::text AS id,
    i.battlemap_media_id::text AS battlemap,
    i.session_id::text AS session,
    i.scene_id::text AS scene,
    coalesce((
        SELECT jsonb_agg(jsonb_build_object(
            'id', t.id::text,
            'character_id', t.character_id::text,
            'kind', e.type,
            'number', t.number,
            'name', t.name,
            'q', t.q,
            'r', t.r,
            'visible', t.visible,
            'hp_current', t.hp_current,
            'hp_max', t.hp_max,
            'conditions', to_jsonb(t.conditions),
            'token_color', c.token_color,
            'portrait_media_id', c.portrait_media_id::text,
            'token_crop', c.token_crop
        ) ORDER BY (e.type <> 'pj'), e.name, t.number NULLS FIRST, t.id)
        FROM token t
        JOIN character c ON c.entity_id = t.character_id
        JOIN entity e ON e.id = c.entity_id
        WHERE t.instance_id = i.id
    ), '[]'::jsonb) AS tokens,
    (
        SELECT jsonb_build_object(
            'round', k.round,
            'current_token_id', cur.token_id::text,
            'entries', coalesce((
                SELECT jsonb_agg(jsonb_build_object(
                    'token_id', en.token_id::text,
                    'name', tk.name,
                    'initiative', en.initiative,
                    'shown_to_players', o.shown_to_players,
                    'skipped', o.skipped
                ) ORDER BY o.order_index)
                FROM initiative_entry en
                JOIN initiative_order_view o ON o.entry_id = en.id
                JOIN token tk ON tk.id = en.token_id
                WHERE en.combat_id = k.id
            ), '[]'::jsonb)
        )
        FROM combat k
        LEFT JOIN initiative_entry cur ON cur.id = k.current_entry_id
        WHERE k.instance_id = i.id AND k.ended_at IS NULL
        ORDER BY k.started_at DESC
        LIMIT 1
    ) AS combat
FROM battlemap_instance i
WHERE i.id::text = lower($1)"#;

/// `HealthBand@1`, implemented locally: the band of a creature from its
/// `hp_current` and `hp_max` and from nothing else.
///
/// A band needs both figures, whole numbers, the maximum at least 1; a
/// current figure outside 0 to the maximum is read as the nearest bound. It
/// is `mort` at 0, then `indemne` at the maximum, `blessé` above half of it
/// (twice the current is more than the maximum) and `en sang` at half or
/// less. A figure absent, null, or not a whole number gives a null band: the
/// output is the band alone, never a hit point.
#[must_use]
pub fn health_band(input: &Value) -> Value {
    json!({ "band": band(&input["hp_current"], &input["hp_max"]) })
}

/// The band of `current` hit points out of `max`, or `null`.
fn band(current: &Value, max: &Value) -> Value {
    let (Some(current), Some(max)) = (current.as_i64(), max.as_i64()) else {
        return Value::Null;
    };
    if max < 1 {
        return Value::Null;
    }
    let current = current.clamp(0, max);
    let band = if current == 0 {
        "mort"
    } else if current == max {
        "indemne"
    } else if current * 2 > max {
        "blessé"
    } else {
        "en sang"
    };
    json!(band)
}

/// The initial of a name, upper case: what a disc carries until its
/// character has a portrait.
fn initial(name: &str) -> String {
    name.trim()
        .chars()
        .next()
        .map_or_else(String::new, |c| c.to_uppercase().collect())
}

/// One token as `view` may show it. A PC's hit points are never followed:
/// whatever the row holds, a PC has none, no band and is never dead.
fn token(row: &Value, dm: bool) -> Value {
    let pc = row["kind"] == "pj";
    let (hp, hp_max) = if pc {
        (Value::Null, Value::Null)
    } else {
        (row["hp_current"].clone(), row["hp_max"].clone())
    };
    let mut token = Map::new();
    token.insert("id".into(), row["id"].clone());
    token.insert("char".into(), row["character_id"].clone());
    token.insert("kind".into(), row["kind"].clone());
    token.insert("name".into(), row["name"].clone());
    token.insert("num".into(), row["number"].clone());
    token.insert("q".into(), row["q"].clone());
    token.insert("r".into(), row["r"].clone());
    token.insert("visible".into(), row["visible"].clone());
    token.insert(
        "states".into(),
        if row["conditions"].is_array() {
            row["conditions"].clone()
        } else {
            json!([])
        },
    );
    token.insert("status".into(), band(&hp, &hp_max));
    token.insert("dead".into(), json!(hp.as_i64().is_some_and(|n| n <= 0)));
    token.insert(
        "initials".into(),
        json!(initial(row["name"].as_str().unwrap_or_default())),
    );
    token.insert("color".into(), row["token_color"].clone());
    token.insert(
        "portrait".into(),
        json!(!row["portrait_media_id"].is_null()),
    );
    token.insert("crop".into(), row["token_crop"].clone());
    if dm {
        token.insert("hp".into(), hp);
        token.insert("hpMax".into(), hp_max);
    }
    Value::Object(token)
}

/// The combat of the row as `view` may show it: `null` when none runs.
fn combat(row: &Value, dm: bool) -> Value {
    let combat = &row["combat"];
    if combat.is_null() {
        return Value::Null;
    }
    let entries = combat["entries"].as_array().map_or(&[][..], Vec::as_slice);
    let order: Vec<Value> = entries
        .iter()
        .filter(|entry| entry["skipped"] != true && (dm || entry["shown_to_players"] == true))
        .map(|entry| {
            let mut seat = json!({
                "id": entry["token_id"],
                "name": entry["name"],
                "initiative": entry["initiative"],
            });
            if dm {
                seat["hidden"] = json!(entry["shown_to_players"] != true);
            }
            seat
        })
        .collect();
    // The turn of a participant the players do not see is not theirs to know.
    let current = &combat["current_token_id"];
    let hidden_turn = !dm
        && entries
            .iter()
            .any(|entry| entry["token_id"] == *current && entry["shown_to_players"] != true);
    json!({
        "round": combat["round"],
        "current": if hidden_turn { &Value::Null } else { current },
        "order": order,
    })
}

/// The instance in `rows` — an array of the objects [`SQL`] selects — as
/// `view` (`dm`, anything else being the players') shows it; `null` when
/// there is no row.
#[must_use]
pub fn answer_rows(rows: &Value, view: &str) -> Value {
    let Some(row) = rows.as_array().and_then(|rows| rows.first()) else {
        return Value::Null;
    };
    let dm = view == "dm";
    let tokens: Vec<Value> = row["tokens"]
        .as_array()
        .map_or(&[][..], Vec::as_slice)
        .iter()
        .filter(|token| dm || token["visible"] == true)
        .map(|row| token(row, dm))
        .collect();
    let mut roster: Vec<&Value> = Vec::new();
    for token in &tokens {
        if !roster.contains(&&token["char"]) {
            roster.push(&token["char"]);
        }
    }
    json!({
        "id": row["id"],
        "battlemap": row["battlemap"],
        "session": row["session"],
        "scene": row["scene"],
        "roster": roster,
        "state": { "tokens": tokens, "combat": combat(row, dm) },
    })
}

/// The request's variables [`SQL`] binds, in the order of its `$1`, `$2`…:
/// what the registry hands the query. `view` is not bound: it shapes the
/// answer, not the read.
pub const VARIABLES: &[&str] = &["instance"];

/// The answer to a read, as the registry serves it: the rows of [`SQL`] and
/// the request's variables. It reads `view`; a view absent is the players'.
#[must_use]
pub fn answer(rows: &Value, variables: &BTreeMap<String, Value>) -> Value {
    answer_rows(
        rows,
        variables
            .get("view")
            .and_then(Value::as_str)
            .unwrap_or_default(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const INSTANCE: &str = "b2000000-0000-4000-8000-000000000001";

    /// A token as [`SQL`] selects it.
    fn row(id: &str, kind: &str, name: &str, number: Option<u32>, hp: (i64, i64)) -> Value {
        let (current, max) = if kind == "pj" {
            (Value::Null, Value::Null)
        } else {
            (json!(hp.0), json!(hp.1))
        };
        json!({
            "id": id, "character_id": format!("c-{}", name.to_lowercase()), "kind": kind,
            "number": number, "name": name, "q": 5, "r": 6, "visible": true,
            "hp_current": current, "hp_max": max, "conditions": [],
            "token_color": "danger", "portrait_media_id": null, "token_crop": null,
        })
    }

    fn with(mut row: Value, field: &str, value: Value) -> Value {
        row[field] = value;
        row
    }

    /// A seat of the combat as [`SQL`] selects it.
    fn seat(token: &str, name: &str, initiative: i64, shown: bool, skipped: bool) -> Value {
        json!({
            "token_id": token, "name": name, "initiative": initiative,
            "shown_to_players": shown, "skipped": skipped,
        })
    }

    fn rows(tokens: Vec<Value>, combat: Value) -> Value {
        json!([{
            "id": INSTANCE, "battlemap": "bm-gue", "session": "s4", "scene": "sc3",
            "tokens": tokens, "combat": combat,
        }])
    }

    /// Elowen (PC), a hurt visible Bandit 1, a hidden Chef bandit 1 who has
    /// the second initiative, and Bandit 2 at 0.
    fn board() -> Value {
        let tokens = vec![
            row("t-elowen", "pj", "Elowen", None, (0, 0)),
            row("t-bandit-1", "pnj", "Bandit", Some(1), (7, 11)),
            with(
                row("t-chef", "pnj", "Chef bandit", Some(1), (30, 30)),
                "visible",
                json!(false),
            ),
            row("t-bandit-2", "pnj", "Bandit", Some(2), (0, 11)),
        ];
        let combat = json!({
            "round": 2, "current_token_id": "t-bandit-1",
            "entries": [
                seat("t-elowen", "Elowen", 18, true, false),
                seat("t-chef", "Chef bandit 1", 15, false, false),
                seat("t-bandit-1", "Bandit 1", 12, true, false),
                seat("t-bandit-2", "Bandit 2", 9, true, true),
            ],
        });
        rows(tokens, combat)
    }

    fn ids(list: &Value) -> Vec<&str> {
        list.as_array()
            .expect("an array")
            .iter()
            .map(|v| v["id"].as_str().expect("an id"))
            .collect()
    }

    fn token_of<'a>(answer: &'a Value, id: &str) -> &'a Value {
        answer["state"]["tokens"]
            .as_array()
            .expect("tokens")
            .iter()
            .find(|t| t["id"] == id)
            .expect("the token")
    }

    #[test]
    fn conformance() {
        platform_conformance::check("HealthBand@1", |input| health_band(&input));
    }

    #[test]
    fn the_band_is_told_from_the_hit_points_alone() {
        let band = |current: Value, max: Value| {
            health_band(&json!({ "hp_current": current, "hp_max": max }))["band"].clone()
        };
        assert_eq!(band(json!(20), json!(20)), "indemne");
        assert_eq!(band(json!(13), json!(20)), "blessé");
        assert_eq!(band(json!(10), json!(20)), "en sang");
        assert_eq!(band(json!(0), json!(20)), "mort");
        // Outside 0 to the maximum, the nearest bound; one figure alone, or a maximum under 1, none.
        assert_eq!(band(json!(-3), json!(20)), "mort");
        assert_eq!(band(json!(25), json!(20)), "indemne");
        assert_eq!(band(json!(5), Value::Null), Value::Null);
        assert_eq!(band(Value::Null, json!(5)), Value::Null);
        assert_eq!(band(json!(0), json!(0)), Value::Null);
        assert_eq!(band(json!(1.5), json!(20)), Value::Null);
    }

    #[test]
    fn the_answer_has_the_keys_of_the_fixture() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../../mock/fixtures/table.getBattlemapInstance.json");
        let fixture: Value =
            serde_json::from_str(&std::fs::read_to_string(path).expect("the fixture"))
                .expect("JSON");
        let answer = answer_rows(&board(), "dm");
        for key in ["id", "battlemap", "session", "scene", "roster", "state"] {
            assert!(fixture.get(key).is_some(), "{key} left the fixture");
            assert!(answer.get(key).is_some(), "{key} is missing from {answer}");
        }
        let expected = fixture["state"]["tokens"][4].as_object().expect("a token");
        let token = token_of(&answer, "t-bandit-1");
        for key in expected.keys().filter(|key| *key != "id") {
            // The fixture's `kind` is `pnj`, its `hp` the figure: both are ours.
            assert!(
                token.get(key).is_some(),
                "token.{key} is missing from {token}"
            );
        }
    }

    #[test]
    fn the_dm_sees_everything() {
        let answer = answer_rows(&board(), "dm");
        let tokens = &answer["state"]["tokens"];
        assert_eq!(
            ids(tokens),
            ["t-elowen", "t-bandit-1", "t-chef", "t-bandit-2"]
        );
        let bandit = token_of(&answer, "t-bandit-1");
        assert_eq!((&bandit["hp"], &bandit["hpMax"]), (&json!(7), &json!(11)));
        assert_eq!(bandit["status"], "blessé");
        assert_eq!(token_of(&answer, "t-chef")["visible"], false);
        let combat = &answer["state"]["combat"];
        assert_eq!(combat["round"], 2);
        assert_eq!(combat["current"], "t-bandit-1");
    }

    #[test]
    fn the_players_see_the_band_and_no_figure_no_hidden_token() {
        let answer = answer_rows(&board(), "player");
        let tokens = &answer["state"]["tokens"];
        assert_eq!(ids(tokens), ["t-elowen", "t-bandit-1", "t-bandit-2"]);
        for token in tokens.as_array().expect("tokens") {
            assert!(token.get("hp").is_none(), "hp reaches the players: {token}");
            assert!(
                token.get("hpMax").is_none(),
                "hpMax reaches the players: {token}"
            );
        }
        let said = answer.to_string();
        assert!(
            !said.contains("Chef"),
            "a hidden token reaches the players: {said}"
        );
        assert!(
            !said.contains("t-chef"),
            "a hidden token reaches the players: {said}"
        );
        assert!(!said.contains("notes"), "notes reach the players: {said}");
        assert_eq!(token_of(&answer, "t-bandit-1")["status"], "blessé");
        assert_eq!(answer["roster"], json!(["c-elowen", "c-bandit"]));
    }

    #[test]
    fn a_view_not_given_is_the_players() {
        let rows = board();
        let by_default = answer(&rows, &BTreeMap::new());
        assert_eq!(by_default, answer_rows(&rows, "player"));
        assert_ne!(by_default, answer_rows(&rows, "dm"));
        let unknown = BTreeMap::from([("view".to_string(), json!("admin"))]);
        assert_eq!(answer(&rows, &unknown), by_default);
        let dm = BTreeMap::from([("view".to_string(), json!("dm"))]);
        assert_eq!(answer(&rows, &dm), answer_rows(&rows, "dm"));
        assert_eq!(VARIABLES, ["instance"]);
        assert_eq!(CAPABILITY, "table.getBattlemapInstance");
    }

    #[test]
    fn an_npc_is_dead_at_zero_and_a_pc_never_is() {
        let answer = answer_rows(&board(), "player");
        let dead = token_of(&answer, "t-bandit-2");
        assert_eq!(dead["dead"], true);
        assert_eq!(dead["status"], "mort");
        let alive = token_of(&answer, "t-bandit-1");
        assert_eq!(alive["dead"], false);
        let pc = token_of(&answer, "t-elowen");
        assert_eq!((&pc["dead"], &pc["status"]), (&json!(false), &Value::Null));
        // Even a PC whose row holds hit points at 0 is neither dead nor followed.
        let strayed = with(
            with(
                row("t-pc", "pj", "Kael", None, (0, 0)),
                "hp_current",
                json!(0),
            ),
            "hp_max",
            json!(30),
        );
        let answer = answer_rows(&rows(vec![strayed], Value::Null), "dm");
        let pc = token_of(&answer, "t-pc");
        assert_eq!(pc["dead"], false);
        assert_eq!(pc["status"], Value::Null);
        assert_eq!(pc["hp"], Value::Null);
        assert_eq!(pc["hpMax"], Value::Null);
        // An NPC at 0 whose maximum is unknown is dead all the same (dead iff 0 HP).
        let npc = with(
            row("t-n", "pnj", "Rat", Some(1), (0, 0)),
            "hp_max",
            Value::Null,
        );
        let answer = answer_rows(&rows(vec![npc], Value::Null), "player");
        assert_eq!(token_of(&answer, "t-n")["dead"], true);
        assert_eq!(token_of(&answer, "t-n")["status"], Value::Null);
    }

    #[test]
    fn a_hidden_participant_keeps_its_rank_on_the_dm_side_and_takes_it_once_revealed() {
        let dm = answer_rows(&board(), "dm");
        let order = &dm["state"]["combat"]["order"];
        // Dead Bandit 2 is skipped; the hidden Chef is second, flagged.
        assert_eq!(ids(order), ["t-elowen", "t-chef", "t-bandit-1"]);
        assert_eq!(order[1]["hidden"], true);
        assert_eq!(order[0]["hidden"], false);
        let players = answer_rows(&board(), "player");
        let before = &players["state"]["combat"]["order"];
        assert_eq!(ids(before), ["t-elowen", "t-bandit-1"]);
        assert!(before[0].get("hidden").is_none());

        // The Chef is revealed: second on the players' carousel, the others unmoved.
        let mut revealed = board();
        revealed[0]["tokens"][2]["visible"] = json!(true);
        revealed[0]["combat"]["entries"][1]["shown_to_players"] = json!(true);
        let after = answer_rows(&revealed, "player");
        let after = &after["state"]["combat"]["order"];
        assert_eq!(ids(after), ["t-elowen", "t-chef", "t-bandit-1"]);
        assert_eq!(
            ids(after),
            ids(&answer_rows(&revealed, "dm")["state"]["combat"]["order"])
        );
        assert_eq!(after[1]["initiative"], 15);
        assert_eq!(after[1]["name"], "Chef bandit 1");
    }

    #[test]
    fn the_turn_of_a_hidden_participant_is_the_dms_alone() {
        let mut ambush = board();
        ambush[0]["combat"]["current_token_id"] = json!("t-chef");
        assert_eq!(
            answer_rows(&ambush, "dm")["state"]["combat"]["current"],
            "t-chef"
        );
        assert_eq!(
            answer_rows(&ambush, "player")["state"]["combat"]["current"],
            Value::Null
        );
        // A visible one is told to both.
        assert_eq!(
            answer_rows(&board(), "player")["state"]["combat"]["current"],
            "t-bandit-1"
        );
    }

    #[test]
    fn no_combat_running_is_null_and_the_tokens_stay() {
        let answer = answer_rows(
            &rows(
                vec![row("t-b", "pnj", "Bandit", Some(1), (11, 11))],
                Value::Null,
            ),
            "dm",
        );
        assert_eq!(answer["state"]["combat"], Value::Null);
        assert_eq!(ids(&answer["state"]["tokens"]), ["t-b"]);
        let empty = answer_rows(&rows(vec![], Value::Null), "dm");
        assert_eq!(empty["state"], json!({ "tokens": [], "combat": null }));
        assert_eq!(empty["roster"], json!([]));
    }

    #[test]
    fn three_bandits_are_discs_with_their_initial_and_number_until_a_portrait_is_cropped() {
        let bandits: Vec<Value> = (1..=3)
            .map(|n| {
                with(
                    row(
                        &format!("t-{n}"),
                        "pnj",
                        &format!("Bandit {n}"),
                        Some(n),
                        (11, 11),
                    ),
                    "token_color",
                    json!("danger"),
                )
            })
            .collect();
        for view in ["dm", "player"] {
            let answer = answer_rows(&rows(bandits.clone(), Value::Null), view);
            for (n, token) in answer["state"]["tokens"]
                .as_array()
                .expect("tokens")
                .iter()
                .enumerate()
            {
                assert_eq!(token["initials"], "B", "{view}");
                assert_eq!(token["num"], n + 1, "{view}");
                assert_eq!(token["color"], "danger", "{view}");
                assert_eq!(token["portrait"], false, "{view}");
                assert_eq!(token["crop"], Value::Null, "{view}");
            }
        }
        // A portrait is added and cropped: every token shows the crop.
        let crop = json!({ "zoom": 150, "x": 40, "y": 30 });
        let cropped: Vec<Value> = bandits
            .into_iter()
            .map(|b| {
                with(
                    with(b, "portrait_media_id", json!("m1")),
                    "token_crop",
                    crop.clone(),
                )
            })
            .collect();
        for view in ["dm", "player"] {
            let answer = answer_rows(&rows(cropped.clone(), Value::Null), view);
            for token in answer["state"]["tokens"].as_array().expect("tokens") {
                assert_eq!(
                    (&token["portrait"], &token["crop"]),
                    (&json!(true), &crop),
                    "{view}"
                );
            }
        }
    }

    #[test]
    fn the_initial_is_the_names_first_letter_in_capital() {
        assert_eq!(initial("bandit 1"), "B");
        assert_eq!(initial("  élan"), "É");
        assert_eq!(initial(""), "");
        let renamed = answer_rows(
            &rows(vec![row("t", "pnj", "chef", Some(1), (1, 1))], Value::Null),
            "dm",
        );
        assert_eq!(token_of(&renamed, "t")["initials"], "C");
    }

    #[test]
    fn the_conditions_come_back_as_the_states() {
        let scared = with(
            row("t", "pnj", "Bandit", Some(1), (11, 11)),
            "conditions",
            json!(["Effrayé"]),
        );
        let answer = answer_rows(&rows(vec![scared], Value::Null), "player");
        assert_eq!(token_of(&answer, "t")["states"], json!(["Effrayé"]));
        let none = with(
            row("u", "pnj", "Bandit", Some(2), (11, 11)),
            "conditions",
            Value::Null,
        );
        let answer = answer_rows(&rows(vec![none], Value::Null), "player");
        assert_eq!(token_of(&answer, "u")["states"], json!([]));
    }

    #[test]
    fn an_unknown_instance_has_no_answer() {
        assert_eq!(answer_rows(&json!([]), "dm"), Value::Null);
        assert_eq!(answer_rows(&Value::Null, "dm"), Value::Null);
    }
}
