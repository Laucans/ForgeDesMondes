//! Capability `live.getDiffusion`: what the live session's player view shows.
//!
//! It reads, and only reads. The rows of its persisted query
//! (`crates/live/queries/getDiffusion.graphql`, resolved by [`SQL`]) go in;
//! the answer, in the shape of `mock/fixtures/live.getDiffusion.json`, comes
//! out: `base`, `window` and `view`.
//!
//! Both player views — the projected one and the DM's piloted tab — read the
//! one row of the session (single-diffusion-source), so the answer does not
//! depend on the view asked for: view `dm` and view `player` get the same
//! position, size, zoom and pan. The player view never gets more than the DM
//! does, and the answer holds nothing a player must not see: the query
//! selects no token, no hit point figure, no note (players-see-band-only).
//!
//! The window's height is derived, never stored: the query computes
//! `window_width × height_px / width_px` from the media's proportions
//! (window-proportional), so a UI never works the ratio out on its own. The
//! window's x, y, w and h are fractions of the screen's width.

use std::collections::BTreeMap;

use serde_json::{Value, json};

/// The Capability's identifier, as a Micro-UI cites it.
pub const CAPABILITY: &str = "live.getDiffusion";

/// What the persisted query `getDiffusion.graphql` selects: the live session
/// of the campaign bound to `$1` — of two, the last one planned, as the
/// DataCapabilities that write the row find it; the last one started, among
/// the campaigns', when `$1` is the empty text or NULL (the engine binds a
/// variable the request leaves out as NULL) — with its diffusion row, if
/// it has one, and the proportions of the media of its window. A background
/// whose media or instance was deleted (onDelete nullify) shows the cover. The campaign is
/// compared as text, so that an id which is not a uuid finds nothing instead
/// of failing.
pub const SQL: &str = r#"SELECT
    CASE
        WHEN d.base_kind = 'illustration' AND d.base_media_id IS NOT NULL THEN 'illustration'
        WHEN d.base_kind = 'battlemap' AND d.base_instance_id IS NOT NULL THEN 'battlemap'
        ELSE 'cover'
    END AS base_kind,
    CASE
        WHEN d.base_kind = 'illustration' AND d.base_media_id IS NOT NULL THEN d.base_media_id::text
        WHEN d.base_kind = 'battlemap' AND d.base_instance_id IS NOT NULL THEN d.base_instance_id::text
        ELSE c.cover_media_id::text
    END AS base_id,
    d.window_media_id::text AS window_media_id,
    d.window_x::float8 AS window_x,
    d.window_y::float8 AS window_y,
    d.window_width::float8 AS window_width,
    round(d.window_width * m.height_px / nullif(m.width_px, 0), 4)::float8 AS window_height,
    d.view_zoom::float8 AS view_zoom,
    d.view_pan_x::float8 AS view_pan_x,
    d.view_pan_y::float8 AS view_pan_y,
    d.session_id IS NOT NULL AS shown
FROM session s
JOIN campaign c ON c.id = s.campaign_id
LEFT JOIN diffusion d ON d.session_id = s.id
LEFT JOIN media m ON m.id = d.window_media_id
WHERE s.status = 'live' AND (coalesce($1, '') = '' OR s.campaign_id::text = $1)
ORDER BY CASE WHEN coalesce($1, '') = '' THEN s.started_at END DESC NULLS LAST, s.position DESC, s.id
LIMIT 1"#;

/// What the session shows among `rows` — an array of the objects [`SQL`]
/// selects, at most one — as `{ base, window, view }`.
///
/// No row (no live session, or none for the campaign) and a session with no
/// diffusion row both show the cover and no window: the player view has
/// something to display before the DM sends anything. The window is `null`
/// while no media is shown in one; the framing is `null` while the session
/// has no diffusion row.
#[must_use]
pub fn answer_rows(rows: &Value) -> Value {
    let Some(row) = rows.as_array().and_then(|rows| rows.first()) else {
        return json!({ "base": { "kind": "cover", "id": "" }, "window": null, "view": null });
    };
    let window = row["window_media_id"].as_str().map(|media| {
        json!({
            "media": media,
            "x": row["window_x"],
            "y": row["window_y"],
            "w": row["window_width"],
            "h": row["window_height"],
        })
    });
    let view = (row["shown"] == json!(true)).then(|| {
        json!({
            "zoom": row["view_zoom"],
            "panX": row["view_pan_x"],
            "panY": row["view_pan_y"],
        })
    });
    json!({
        "base": {
            "kind": row["base_kind"].as_str().unwrap_or("cover"),
            "id": row["base_id"].as_str().unwrap_or_default(),
        },
        "window": window,
        "view": view,
    })
}

/// The request's variables [`SQL`] binds, in the order of its `$1`: the
/// campaign. `view` is not one: both views read the same row.
pub const VARIABLES: &[&str] = &["campaign"];

/// The answer to a read, as the registry serves it: the rows of [`SQL`]; the
/// variables bind the query and do not shape the answer — `view` 'dm' and
/// 'player' get the same values.
#[must_use]
pub fn answer(rows: &Value, _variables: &BTreeMap<String, Value>) -> Value {
    answer_rows(rows)
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    const MEDIA: &str = "e0000000-0000-4000-8000-000000000001";
    const INSTANCE: &str = "b0000000-0000-4000-8000-000000000001";

    fn cover_row() -> Value {
        json!({
            "base_kind": "cover", "base_id": "ill-cover",
            "window_media_id": null, "window_x": null, "window_y": null,
            "window_width": null, "window_height": null,
            "view_zoom": 1, "view_pan_x": 0, "view_pan_y": 0, "shown": true,
        })
    }

    fn windowed_row() -> Value {
        // An 800 × 1200 illustration, 0.4 of the screen's width wide.
        json!({
            "base_kind": "battlemap", "base_id": INSTANCE,
            "window_media_id": MEDIA, "window_x": 0.1, "window_y": 0.2,
            "window_width": 0.4, "window_height": 0.6,
            "view_zoom": 1.5, "view_pan_x": -40, "view_pan_y": 12.5, "shown": true,
        })
    }

    fn fixture() -> Value {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../../mock/fixtures/live.getDiffusion.json");
        serde_json::from_str(&std::fs::read_to_string(path).expect("the fixture")).expect("JSON")
    }

    #[test]
    fn a_cover_row_answers_as_the_fixture_does() {
        let mut row = cover_row();
        row["shown"] = json!(false);
        assert_eq!(answer_rows(&json!([row])), fixture());
    }

    #[test]
    fn no_row_shows_the_cover_and_no_window() {
        for rows in [json!([]), Value::Null, json!({})] {
            let got = answer_rows(&rows);
            assert_eq!(got["base"]["kind"], "cover", "{rows}");
            assert_eq!(got["window"], Value::Null, "{rows}");
            assert_eq!(got["view"], Value::Null, "{rows}");
        }
    }

    #[test]
    fn a_window_carries_its_media_position_width_and_derived_height() {
        let got = answer_rows(&json!([windowed_row()]));
        assert_eq!(
            got["window"],
            json!({ "media": MEDIA, "x": 0.1, "y": 0.2, "w": 0.4, "h": 0.6 })
        );
        let (w, h) = (got["window"]["w"].as_f64(), got["window"]["h"].as_f64());
        assert!((w.unwrap() / h.unwrap() - 2.0 / 3.0).abs() < 1e-9, "2/3");
    }

    #[test]
    fn the_base_names_the_instance_of_a_battlemap_and_the_framing_is_the_rows() {
        let got = answer_rows(&json!([windowed_row()]));
        assert_eq!(got["base"], json!({ "kind": "battlemap", "id": INSTANCE }));
        assert_eq!(
            got["view"],
            json!({ "zoom": 1.5, "panX": -40, "panY": 12.5 })
        );
    }

    #[test]
    fn a_cover_with_no_media_has_an_empty_id() {
        let mut row = cover_row();
        row["base_id"] = Value::Null;
        assert_eq!(
            answer_rows(&json!([row]))["base"],
            json!({ "kind": "cover", "id": "" })
        );
    }

    #[test]
    fn a_closed_window_is_null_whatever_the_base() {
        assert_eq!(answer_rows(&json!([cover_row()]))["window"], Value::Null);
    }

    #[test]
    fn view_dm_and_view_player_answer_the_same_values() {
        let rows = json!([windowed_row()]);
        let ask = |view: &str| {
            let variables = BTreeMap::from([("view".to_string(), json!(view))]);
            answer(&rows, &variables)
        };
        assert_eq!(ask("dm"), ask("player"));
        assert_eq!(ask("dm"), answer_rows(&rows));
    }

    #[test]
    fn the_campaign_is_the_one_variable_the_sql_binds() {
        assert_eq!(VARIABLES, ["campaign"]);
    }

    #[test]
    fn the_answer_holds_only_base_window_and_view() {
        let got = answer_rows(&json!([windowed_row()]));
        let mut keys: Vec<_> = got.as_object().expect("an object").keys().collect();
        keys.sort();
        assert_eq!(keys, ["base", "view", "window"]);
        let text = got.to_string();
        for forbidden in ["hp", "notes", "token", "hidden"] {
            assert!(!text.contains(forbidden), "{forbidden} in {text}");
        }
    }

    #[test]
    fn the_sql_is_one_select_statement_bound_to_one_campaign_or_the_live_session() {
        assert!(
            SQL.trim_start().to_ascii_uppercase().starts_with("SELECT"),
            "{SQL}"
        );
        assert!(!SQL.contains(';'), "a second statement in {SQL}");
        assert!(
            SQL.contains("(coalesce($1, '') = '' OR s.campaign_id::text = $1)"),
            "{SQL}"
        );
        assert!(SQL.contains("s.status = 'live'"), "{SQL}");
        assert!(SQL.contains("ORDER BY CASE WHEN coalesce($1, '') = '' THEN s.started_at END DESC NULLS LAST, s.position DESC, s.id"), "{SQL}");
        assert!(SQL.contains("LIMIT 1"), "{SQL}");
        assert!(SQL.contains("d.window_width * m.height_px"), "{SQL}");
    }

    #[test]
    fn the_sql_selects_no_token_hit_point_or_note() {
        for forbidden in ["token", "hp", "note", "hit_point", "visible"] {
            assert!(
                !SQL.to_lowercase().contains(forbidden),
                "{forbidden} in {SQL}"
            );
        }
    }

    #[test]
    fn the_query_file_selects_what_the_sql_reads() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../queries/getDiffusion.graphql");
        let query = std::fs::read_to_string(path).expect("the persisted query");
        let fields = query
            .lines()
            .skip_while(|l| !l.starts_with("query "))
            .skip(2)
            .map(str::trim)
            .take_while(|l| *l != "}")
            .collect::<Vec<_>>();
        assert_eq!(
            fields,
            [
                "baseKind",
                "baseId",
                "windowMediaId",
                "windowX",
                "windowY",
                "windowWidth",
                "windowHeight",
                "viewZoom",
                "viewPanX",
                "viewPanY",
                "shown"
            ]
        );
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
            assert!(
                SQL.contains(&format!("AS {column}")),
                "the query names {field}, the SQL does not select {column}"
            );
        }
    }
}
