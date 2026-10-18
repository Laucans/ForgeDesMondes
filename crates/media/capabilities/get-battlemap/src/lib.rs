//! Capability `media.getBattlemap`: one battlemap with its hex grid, as the
//! grid settings screen and the battlemap itself read it.
//!
//! It reads, and only reads. The rows of its persisted query
//! (`crates/media/queries/getBattlemap.graphql`, resolved by [`SQL`]) go in,
//! with the id the caller names; the answer, in the shape of
//! `mock/fixtures/media.getBattlemap.json`, comes out:
//!
//! - `id`, `title`, `file`, `uploadedAt`, `width`, `height`, `place`: the
//!   media as stored — `file` is the path the DM chose, `place` the id of its
//!   lieu or `null`;
//! - `grid`: the settings as last saved, in the words of `HexGrid@1` — `size`,
//!   `ox`, `oy`, `orient` (`pointy` or `flat`), `color`, and `opacity` as a
//!   fraction of 1, not a percentage — and `feetPerHex`;
//! - `size` (the file's weight) and `terrain` (the mock-up's drawn map): no
//!   store holds them, they come back `null` and empty.
//!
//! An illustration, an unknown id, or no row at all is `null`: there is no
//! battlemap, and no grid is made up for it.
//!
//! [`hex_grid`] is this Capability's own reading of `HexGrid@1`, held to the
//! Concept's fixtures; the `feetPerHex` of an answer comes from it, so a hex
//! is 5 feet whatever the store holds.

use serde_json::{Value, json};

/// The Capability's identifier, as a Micro-UI cites it.
pub const CAPABILITY: &str = "media.getBattlemap";

/// What the persisted query `getBattlemap.graphql` selects: every battlemap
/// with the grid it was uploaded with and the settings last saved.
pub const SQL: &str = r"SELECT m.id::text AS id,
    m.title,
    m.file_path,
    m.width_px,
    m.height_px,
    m.place_id::text AS place_id,
    to_char(m.uploaded_at AT TIME ZONE 'UTC', 'YYYY-MM-DD') AS uploaded_at,
    g.hex_size_px,
    g.offset_x,
    g.offset_y,
    g.orientation,
    g.color,
    g.opacity
FROM media m
JOIN battlemap_grid g ON g.media_id = m.id
WHERE m.kind = 'battlemap'
ORDER BY m.id";

/// One hex is this many feet, whatever its size, offset or orientation.
const FEET_PER_HEX: i64 = 5;

/// The battlemap `id` among `rows` — an array of the objects [`SQL`] selects
/// — as the screens read it; `null` when no row has that id.
#[must_use]
pub fn answer(rows: &Value, id: &str) -> Value {
    let Some(row) = rows
        .as_array()
        .and_then(|rows| rows.iter().find(|row| row["id"] == id))
    else {
        return Value::Null;
    };
    let mut grid = json!({
        "size": tidy(&row["hex_size_px"]),
        "ox": tidy(&row["offset_x"]),
        "oy": tidy(&row["offset_y"]),
        "orient": orient(&row["orientation"]),
        "color": row["color"],
        "opacity": tidy(&row["opacity"]),
    });
    grid["feetPerHex"] = hex_grid(grid.clone())["feetPerHex"].clone();
    json!({
        "id": row["id"],
        "title": row["title"],
        "file": row["file_path"],
        "size": Value::Null,
        "uploadedAt": row["uploaded_at"],
        "width": row["width_px"],
        "height": row["height_px"],
        "grid": grid,
        "terrain": [],
        "place": row["place_id"],
    })
}

/// The orientation in `HexGrid@1`'s words: the French ones a store may hold
/// become `pointy` and `flat`; anything else is kept, so that the Concept
/// reports it rather than this Capability hiding it.
fn orient(stored: &Value) -> Value {
    match stored.as_str() {
        Some("pointe") => json!("pointy"),
        Some("plat") => json!("flat"),
        _ => stored.clone(),
    }
}

/// A number the database wrote as `40.00` as the whole number it is, so that
/// the settings come back exactly as they were entered; anything else as is.
fn tidy(value: &Value) -> Value {
    match value.as_f64() {
        #[allow(clippy::cast_possible_truncation)]
        Some(n) if n.fract() == 0.0 && n.abs() < 1e15 => json!(n as i64),
        _ => value.clone(),
    }
}

/// `HexGrid@1`, implemented locally: whether the grid in `input` (`size`,
/// `ox`, `oy`, `orient`, `color`, `opacity`) is valid and which fields are
/// not, the 5 feet of a hex, and the feet of a distance of `input.hexes`
/// hexes.
///
/// `size` holds above 0, `ox` and `oy` as numbers, `orient` as `pointy` or
/// `flat`, `color` as a non-empty string, `opacity` from 0 to 1. A field
/// missing or of another type does not hold. A hex is 5 feet, the grid valid
/// or not; `feet` is `hexes` times 5 for a whole `hexes` from 0 up, else
/// `null`.
#[must_use]
pub fn hex_grid(input: Value) -> Value {
    let number = |field: &str| input[field].as_f64();
    let holds = [
        ("size", number("size").is_some_and(|n| n > 0.0)),
        ("ox", number("ox").is_some()),
        ("oy", number("oy").is_some()),
        (
            "orient",
            matches!(input["orient"].as_str(), Some("pointy" | "flat")),
        ),
        (
            "color",
            input["color"].as_str().is_some_and(|c| !c.is_empty()),
        ),
        (
            "opacity",
            number("opacity").is_some_and(|n| (0.0..=1.0).contains(&n)),
        ),
    ];
    let problems: Vec<&str> = holds
        .iter()
        .filter(|(_, holds)| !holds)
        .map(|(field, _)| *field)
        .collect();
    json!({
        "valid": problems.is_empty(),
        "problems": problems,
        "feetPerHex": FEET_PER_HEX,
        "feet": feet(&input["hexes"]),
    })
}

/// The feet of `hexes` hexes: `null` unless it is a whole number from 0 up.
fn feet(hexes: &Value) -> Value {
    match hexes.as_f64() {
        #[allow(clippy::cast_possible_truncation)]
        Some(n) if n >= 0.0 && n.fract() == 0.0 && n < 1e15 => json!(n as i64 * FEET_PER_HEX),
        _ => Value::Null,
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    const GUE: &str = "b1000000-0000-4000-8000-000000000001";
    const MOULIN: &str = "b1000000-0000-4000-8000-000000000002";
    const LIEU: &str = "e1000000-0000-4000-8000-000000000009";

    /// A battlemap row as [`SQL`] selects it, as the database writes numbers:
    /// `numeric` columns come back with their decimals.
    fn row(id: &str, grid: &Value) -> Value {
        let mut row = json!({
            "id": id, "title": "Le Gué des Corbeaux", "file_path": "gue-des-corbeaux.jpg",
            "width_px": 960, "height_px": 640, "place_id": null, "uploaded_at": "2026-10-14",
            "hex_size_px": 30.00, "offset_x": 12.00, "offset_y": 8.00,
            "orientation": "pointy", "color": "text", "opacity": 0.40,
        });
        for (key, value) in grid.as_object().expect("an object") {
            row[key] = value.clone();
        }
        row
    }

    fn rows() -> Value {
        json!([
            row(GUE, &json!({})),
            row(
                MOULIN,
                &json!({ "title": "Le moulin", "place_id": LIEU, "orientation": "flat",
                    "hex_size_px": 64.5, "offset_x": -3.25, "color": "accent", "opacity": 0.75 })
            ),
        ])
    }

    fn fixture() -> Value {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../../mock/fixtures/media.getBattlemap.json");
        serde_json::from_str(&std::fs::read_to_string(path).expect("the fixture")).expect("JSON")
    }

    #[test]
    fn conformance() {
        platform_conformance::check("HexGrid@1", hex_grid);
    }

    #[test]
    fn the_answer_has_the_keys_of_the_fixture() {
        let answer = answer(&rows(), GUE);
        for key in fixture().as_object().expect("an object").keys() {
            assert!(answer.get(key).is_some(), "{key} is missing from {answer}");
        }
        for key in fixture()["grid"].as_object().expect("an object").keys() {
            assert!(answer["grid"].get(key).is_some(), "grid.{key} is missing");
        }
    }

    #[test]
    fn the_grid_comes_back_exactly_as_saved_with_five_feet_a_hex() {
        let answer = answer(&rows(), MOULIN);
        assert_eq!(
            answer["grid"],
            json!({ "size": 64.5, "ox": -3.25, "oy": 8, "orient": "flat", "color": "accent",
                "opacity": 0.75, "feetPerHex": 5 })
        );
        assert_eq!(answer["place"], LIEU);
        assert_eq!(answer["title"], "Le moulin");
    }

    #[test]
    fn whole_numbers_the_database_pads_with_decimals_are_whole_numbers() {
        let grid = &answer(&rows(), GUE)["grid"];
        // 30.00 and 12.00 read as 30 and 12, so that 30 saved is 30 read.
        assert_eq!(grid["size"], json!(30));
        assert_eq!(grid["ox"], json!(12));
        assert_eq!(grid["opacity"], json!(0.4));
        assert_eq!(tidy(&json!(1.0)), json!(1));
        assert_eq!(tidy(&json!(0.0)), json!(0));
        assert_eq!(tidy(&json!(-37.0)), json!(-37));
        assert_eq!(tidy(&json!(0.4)), json!(0.4));
        assert_eq!(tidy(&Value::Null), Value::Null);
    }

    #[test]
    fn the_media_is_the_one_stored() {
        let answer = answer(&rows(), GUE);
        assert_eq!(answer["id"], GUE);
        assert_eq!(answer["file"], "gue-des-corbeaux.jpg");
        assert_eq!(answer["uploadedAt"], "2026-10-14");
        assert_eq!(answer["width"], 960);
        assert_eq!(answer["height"], 640);
        assert_eq!(answer["place"], Value::Null);
        // No store holds the file's weight or the mock-up's drawn terrain.
        assert_eq!(answer["size"], Value::Null);
        assert_eq!(answer["terrain"], json!([]));
    }

    #[test]
    fn an_unknown_id_an_illustration_or_no_row_is_no_battlemap() {
        // An illustration is not selected: its id is among no row.
        for id in ["b1000000-0000-4000-8000-0000000000ff", "ill-cover", ""] {
            assert_eq!(answer(&rows(), id), Value::Null, "{id}");
        }
        assert_eq!(answer(&json!([]), GUE), Value::Null);
        assert_eq!(answer(&Value::Null, GUE), Value::Null);
        assert_eq!(answer(&json!({ "id": GUE }), GUE), Value::Null);
    }

    #[test]
    fn a_hex_is_five_feet_whatever_the_settings() {
        let odd = [
            json!({ "hex_size_px": 0.5 }),
            json!({ "hex_size_px": 480.5, "offset_x": -37, "offset_y": -12.5 }),
            json!({ "orientation": "plat" }),
            json!({ "opacity": 0 }),
            json!({ "opacity": 1 }),
            // Out of range or foreign values the guard would have refused:
            // the hex is still 5 feet, and the settings are not corrected.
            json!({ "hex_size_px": 0 }),
            json!({ "opacity": 40 }),
            json!({ "orientation": "square" }),
        ];
        for settings in odd {
            let rows = json!([row(GUE, &settings)]);
            assert_eq!(answer(&rows, GUE)["grid"]["feetPerHex"], 5, "{settings}");
        }
        let refused = answer(&json!([row(GUE, &json!({ "opacity": 40 }))]), GUE);
        assert_eq!(refused["grid"]["opacity"], json!(40));
    }

    #[test]
    fn the_stored_french_orientations_read_pointy_and_flat() {
        assert_eq!(orient(&json!("pointe")), json!("pointy"));
        assert_eq!(orient(&json!("plat")), json!("flat"));
        assert_eq!(orient(&json!("pointy")), json!("pointy"));
        assert_eq!(orient(&json!("flat")), json!("flat"));
        assert_eq!(orient(&json!("square")), json!("square"));
        assert_eq!(orient(&Value::Null), Value::Null);
    }

    #[test]
    fn the_grid_of_an_answer_is_valid_per_the_concept() {
        for id in [GUE, MOULIN] {
            let grid = answer(&rows(), id)["grid"].clone();
            let checked = hex_grid(grid);
            assert_eq!(checked["valid"], true, "{checked}");
            assert_eq!(checked["problems"], json!([]));
        }
    }

    #[test]
    fn a_distance_is_five_feet_a_hex_and_only_for_whole_hexes() {
        let feet = |hexes: Value| hex_grid(json!({ "hexes": hexes }))["feet"].clone();
        assert_eq!(feet(json!(6)), json!(30));
        assert_eq!(feet(json!(0)), json!(0));
        assert_eq!(feet(json!(2.0)), json!(10));
        assert_eq!(feet(json!(-1)), Value::Null);
        assert_eq!(feet(json!(2.5)), Value::Null);
        assert_eq!(feet(json!("6")), Value::Null);
        assert_eq!(feet(Value::Null), Value::Null);
        assert_eq!(feet(json!(1e300)), Value::Null);
    }

    /// Whether [`SQL`] writes `name` as a whole column or alias, not as the
    /// start of a longer one.
    fn selects(name: &str) -> bool {
        SQL.match_indices(name).any(|(at, _)| {
            SQL[at + name.len()..]
                .chars()
                .next()
                .is_none_or(|c| !(c.is_ascii_alphanumeric() || c == '_'))
        })
    }

    #[test]
    fn the_query_file_selects_what_the_sql_reads() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../queries/getBattlemap.graphql");
        let query = std::fs::read_to_string(path).expect("the persisted query");
        let fields: Vec<&str> = query
            .lines()
            .skip_while(|l| !l.starts_with("query "))
            .skip(1)
            .map(str::trim)
            .filter(|l| !l.is_empty() && l.chars().all(|c| c.is_ascii_alphanumeric()))
            .filter(|l| *l != "battlemaps")
            .collect();
        assert!(!fields.is_empty(), "the query selects nothing");
        for field in fields {
            let column: String = field
                .chars()
                .flat_map(|c| {
                    if c.is_ascii_uppercase() {
                        vec!['_', c.to_ascii_lowercase()]
                    } else {
                        vec![c]
                    }
                })
                .collect();
            let selected = [
                format!("m.{column}"),
                format!("g.{column}"),
                format!("AS {column}"),
            ]
            .iter()
            .any(|name| selects(name));
            assert!(
                selected,
                "the query names {field}, the SQL does not select it"
            );
        }
    }

    #[test]
    fn a_column_is_selected_whole_not_as_the_start_of_a_longer_one() {
        assert!(selects("m.id"));
        assert!(selects("g.hex_size_px"));
        assert!(selects("AS uploaded_at"));
        assert!(!selects("g.hex_size"));
        assert!(!selects("m.file"));
        assert!(!selects("m.place"));
    }

    #[test]
    fn the_sql_reads_and_never_writes() {
        let sql = SQL.to_ascii_uppercase();
        assert!(sql.starts_with("SELECT"));
        for verb in ["INSERT", "UPDATE", "DELETE", "DROP", "ALTER", "TRUNC"] {
            assert!(!sql.contains(verb), "{verb} in {SQL}");
        }
        // A battlemap alone: an illustration has no grid.
        assert!(SQL.contains("m.kind = 'battlemap'"));
    }
}
