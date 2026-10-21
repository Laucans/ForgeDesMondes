//! Capability `media.listMedia`: a campaign's media library, as a screen
//! that shows its media reads it.
//!
//! It reads, and only reads. The rows of its persisted query
//! (`crates/media/queries/listMedia.graphql`, resolved by [`SQL`]) go in with
//! the campaign asked for; the answer comes out:
//!
//! - `cover`: the id of the campaign's cover, or null;
//! - `battlemaps`: each with its title, file, upload date, dimensions, `place`
//!   and its `grid` — hex `size`, offsets `ox` and `oy`, `orient` (`pointy` or
//!   `flat`), `color`, `opacity` (a fraction of 1, as it is stored) and
//!   `feetPerHex`, which is 5 whatever the rest;
//! - `illustrations`: each with its title, file, upload date, `ratio` (width
//!   over height) and `place`.
//!
//! A media without a place is listed, its `place` null; a media of another
//! campaign is not. The mock-up's drawing of a map (`terrain`), the palette of
//! an illustration and the weight of a file are in no row and in no answer.

use std::collections::BTreeMap;

use serde_json::{Map, Value, json};

/// The Capability's identifier, as a Micro-UI cites it.
pub const CAPABILITY: &str = "media.listMedia";

/// What the persisted query `listMedia.graphql` selects: every media, newest
/// first, with the grid of a battlemap and whether it is its campaign's cover.
/// The campaign is kept by [`answer_rows`], not by the query.
pub const SQL: &str = r#"SELECT m.id::text AS id,
    m.campaign_id::text AS campaign_id,
    m.kind,
    m.title,
    m.file_path,
    m.width_px,
    m.height_px,
    m.place_id::text AS place_id,
    to_char(m.uploaded_at AT TIME ZONE 'UTC', 'YYYY-MM-DD') AS uploaded_at,
    coalesce(c.cover_media_id = m.id, false) AS is_cover,
    g.hex_size_px,
    g.offset_x,
    g.offset_y,
    g.orientation,
    g.color,
    g.opacity
FROM media m
JOIN campaign c ON c.id = m.campaign_id
LEFT JOIN battlemap_grid g ON g.media_id = m.id
ORDER BY m.uploaded_at DESC, m.title, m.id"#;

/// One hex is always 5 feet: whatever the grid stores, this is what is told.
const FEET_PER_HEX: u8 = 5;

/// The media of `campaign_id` among `rows` — an array of the objects [`SQL`]
/// selects — as the library shows them. No row is an empty, well-formed
/// answer.
#[must_use]
pub fn answer_rows(rows: &Value, campaign_id: &str) -> Value {
    let mut cover = Value::Null;
    let mut battlemaps = Vec::new();
    let mut illustrations = Vec::new();
    for row in rows.as_array().into_iter().flatten() {
        if row["campaign_id"].as_str() != Some(campaign_id) {
            continue;
        }
        match row["kind"].as_str() {
            Some("battlemap") => battlemaps.push(battlemap(row)),
            Some("illustration") => illustrations.push(illustration(row)),
            _ => continue,
        }
        if row["is_cover"] == true {
            cover = row["id"].clone();
        }
    }
    // Newest first, as the query asks; the database's wrapper does not promise
    // to keep an `ORDER BY`, so the order is also this Capability's.
    for list in [&mut battlemaps, &mut illustrations] {
        list.sort_by(|a, b| {
            let key = |m: &Value| {
                (
                    m["uploadedAt"].as_str().unwrap_or_default().to_owned(),
                    m["title"].as_str().unwrap_or_default().to_owned(),
                    m["id"].as_str().unwrap_or_default().to_owned(),
                )
            };
            let (a, b) = (key(a), key(b));
            (b.0, a.1, a.2).cmp(&(a.0, b.1, b.2))
        });
    }
    json!({
        "cover": cover,
        "battlemaps": battlemaps,
        "illustrations": illustrations,
    })
}

/// The fields every media shows.
fn common(row: &Value) -> Map<String, Value> {
    let mut media = Map::new();
    media.insert("id".into(), row["id"].clone());
    media.insert("title".into(), row["title"].clone());
    media.insert("file".into(), file_name(row["file_path"].as_str()));
    media.insert("uploadedAt".into(), row["uploaded_at"].clone());
    media.insert("width".into(), row["width_px"].clone());
    media.insert("height".into(), row["height_px"].clone());
    media.insert("place".into(), row["place_id"].clone());
    media
}

fn battlemap(row: &Value) -> Value {
    let mut media = common(row);
    media.insert("grid".into(), grid(row));
    Value::Object(media)
}

fn illustration(row: &Value) -> Value {
    let mut media = common(row);
    media.remove("width");
    media.remove("height");
    media.insert("ratio".into(), ratio(&row["width_px"], &row["height_px"]));
    Value::Object(media)
}

/// The grid of a battlemap row; null for a battlemap that holds none.
fn grid(row: &Value) -> Value {
    if row["hex_size_px"].is_null() {
        return Value::Null;
    }
    json!({
        "size": row["hex_size_px"],
        "ox": row["offset_x"],
        "oy": row["offset_y"],
        "orient": orient(&row["orientation"]),
        "color": row["color"],
        "opacity": row["opacity"],
        "feetPerHex": FEET_PER_HEX,
    })
}

/// The stored orientation (`pointe`, `plat`) as the grid is told (`pointy`,
/// `flat`); any other text is left as it is.
fn orient(stored: &Value) -> Value {
    match stored.as_str() {
        Some("pointe") => json!("pointy"),
        Some("plat") => json!("flat"),
        _ => stored.clone(),
    }
}

/// The last segment of a local path; null when there is no path.
fn file_name(path: Option<&str>) -> Value {
    path.map_or(Value::Null, |p| {
        json!(p.rsplit(['/', '\\']).next().unwrap_or(p))
    })
}

/// Width over height, to two decimals; null when either is missing or the
/// height is not above 0.
fn ratio(width: &Value, height: &Value) -> Value {
    match (width.as_f64(), height.as_f64()) {
        (Some(w), Some(h)) if h > 0.0 => json!((w / h * 100.0).round() / 100.0),
        _ => Value::Null,
    }
}

/// The request's variables [`SQL`] binds, in the order of its `$1`, `$2`…:
/// what the registry hands the query.
pub const VARIABLES: &[&str] = &[];

/// The answer to a read, as the registry serves it: the rows of [`SQL`] and
/// the request's variables. It reads `campaignId` from the request: [`SQL`]
/// selects every row, the answer picks; a variable absent is the empty text,
/// which names nothing.
#[must_use]
pub fn answer(rows: &Value, variables: &BTreeMap<String, Value>) -> Value {
    answer_rows(
        rows,
        variables
            .get("campaignId")
            .and_then(Value::as_str)
            .unwrap_or_default(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const CAMPAIGN: &str = "c-valombre";

    fn media(id: &str, campaign: &str, kind: &str, place: Option<&str>) -> Value {
        json!({
            "id": id, "campaign_id": campaign, "kind": kind, "title": format!("Titre {id}"),
            "file_path": format!("/data/media/{id}.jpg"), "width_px": 960, "height_px": 640,
            "place_id": place, "uploaded_at": "2026-10-14", "is_cover": false,
            "hex_size_px": null, "offset_x": null, "offset_y": null,
            "orientation": null, "color": null, "opacity": null,
        })
    }

    fn battlemap_row(id: &str, campaign: &str, place: Option<&str>) -> Value {
        let mut row = media(id, campaign, "battlemap", place);
        row["hex_size_px"] = json!(30);
        row["offset_x"] = json!(12);
        row["offset_y"] = json!(-8);
        row["orientation"] = json!("pointe");
        row["color"] = json!("text");
        row["opacity"] = json!(0.4);
        row
    }

    fn ids(list: &Value) -> Vec<&str> {
        list.as_array()
            .expect("a list")
            .iter()
            .filter_map(|m| m["id"].as_str())
            .collect()
    }

    #[test]
    fn two_battlemaps_and_an_illustration_come_back_with_their_kind_and_grid() {
        let rows = json!([
            battlemap_row("bm-1", CAMPAIGN, None),
            battlemap_row("bm-2", CAMPAIGN, Some("moulin")),
            media("ill-1", CAMPAIGN, "illustration", None),
        ]);
        let out = answer_rows(&rows, CAMPAIGN);
        assert_eq!(ids(&out["battlemaps"]), ["bm-1", "bm-2"]);
        assert_eq!(ids(&out["illustrations"]), ["ill-1"]);
        for map in out["battlemaps"].as_array().expect("battlemaps") {
            assert_eq!(map["grid"]["size"], json!(30));
            assert_eq!(map["grid"]["ox"], json!(12));
            assert_eq!(map["grid"]["oy"], json!(-8));
            assert_eq!(map["grid"]["color"], json!("text"));
            assert_eq!(map["grid"]["opacity"], json!(0.4));
        }
        assert!(out["illustrations"][0].get("grid").is_none());
    }

    #[test]
    fn a_media_of_another_campaign_is_absent() {
        let rows = json!([
            battlemap_row("bm-1", CAMPAIGN, None),
            battlemap_row("bm-other", "c-autre", None),
            media("ill-other", "c-autre", "illustration", None),
        ]);
        let out = answer_rows(&rows, CAMPAIGN);
        assert_eq!(ids(&out["battlemaps"]), ["bm-1"]);
        assert_eq!(ids(&out["illustrations"]), Vec::<&str>::new());
    }

    #[test]
    fn a_media_without_a_place_is_listed_with_a_null_place() {
        let rows = json!([
            battlemap_row("bm-1", CAMPAIGN, None),
            media("ill-1", CAMPAIGN, "illustration", None),
            media("ill-2", CAMPAIGN, "illustration", Some("valombre")),
        ]);
        let out = answer_rows(&rows, CAMPAIGN);
        assert_eq!(out["battlemaps"][0]["place"], Value::Null);
        assert_eq!(out["illustrations"][0]["place"], Value::Null);
        assert_eq!(out["illustrations"][1]["place"], json!("valombre"));
    }

    #[test]
    fn a_hex_is_five_feet_whatever_the_grid() {
        let mut flat = battlemap_row("bm-1", CAMPAIGN, None);
        flat["orientation"] = json!("plat");
        flat["hex_size_px"] = json!(480.5);
        let mut stored = battlemap_row("bm-2", CAMPAIGN, None);
        stored["feet_per_hex"] = json!(7);
        let out = answer_rows(&json!([flat, stored]), CAMPAIGN);
        for map in out["battlemaps"].as_array().expect("battlemaps") {
            assert_eq!(map["grid"]["feetPerHex"], json!(5));
        }
    }

    #[test]
    fn the_stored_orientation_is_told_pointy_or_flat() {
        let mut flat = battlemap_row("bm-flat", CAMPAIGN, None);
        flat["orientation"] = json!("plat");
        let rows = json!([battlemap_row("bm-pointy", CAMPAIGN, None), flat]);
        let out = answer_rows(&rows, CAMPAIGN);
        let orient = |id: &str| {
            let maps = out["battlemaps"].as_array().expect("battlemaps");
            maps.iter().find(|m| m["id"] == id).expect("listed")["grid"]["orient"].clone()
        };
        assert_eq!(orient("bm-pointy"), json!("pointy"));
        assert_eq!(orient("bm-flat"), json!("flat"));
    }

    #[test]
    fn the_cover_is_the_campaigns_own_and_null_without_one() {
        let mut cover = media("ill-cover", CAMPAIGN, "illustration", None);
        cover["is_cover"] = json!(true);
        let mut other = media("ill-other", "c-autre", "illustration", None);
        other["is_cover"] = json!(true);
        let out = answer_rows(&json!([other.clone(), cover]), CAMPAIGN);
        assert_eq!(out["cover"], json!("ill-cover"));
        let out = answer_rows(&json!([other]), CAMPAIGN);
        assert_eq!(out["cover"], Value::Null);
    }

    #[test]
    fn a_battlemap_cover_is_still_the_cover() {
        let mut map = battlemap_row("bm-1", CAMPAIGN, None);
        map["is_cover"] = json!(true);
        assert_eq!(answer_rows(&json!([map]), CAMPAIGN)["cover"], json!("bm-1"));
    }

    #[test]
    fn the_file_is_its_name_and_the_ratio_is_width_over_height() {
        let mut ill = media("ill-1", CAMPAIGN, "illustration", None);
        ill["file_path"] = json!("C:\\medias\\valombre-couverture.jpg");
        ill["width_px"] = json!(1600);
        ill["height_px"] = json!(900);
        let out = answer_rows(&json!([ill]), CAMPAIGN);
        assert_eq!(
            out["illustrations"][0]["file"],
            json!("valombre-couverture.jpg")
        );
        assert_eq!(out["illustrations"][0]["ratio"], json!(1.78));
        assert!(out["illustrations"][0].get("width").is_none());
        assert_eq!(out["illustrations"][0]["uploadedAt"], json!("2026-10-14"));
    }

    #[test]
    fn a_zero_height_has_no_ratio_and_a_battlemap_without_grid_has_none() {
        let mut ill = media("ill-1", CAMPAIGN, "illustration", None);
        ill["height_px"] = json!(0);
        let map = media("bm-1", CAMPAIGN, "battlemap", None);
        let out = answer_rows(&json!([ill, map]), CAMPAIGN);
        assert_eq!(out["illustrations"][0]["ratio"], Value::Null);
        assert_eq!(out["battlemaps"][0]["grid"], Value::Null);
    }

    #[test]
    fn the_media_are_listed_newest_first_and_an_unknown_kind_is_skipped() {
        let mut old = media("ill-old", CAMPAIGN, "illustration", None);
        old["uploaded_at"] = json!("2026-08-20");
        let mut same_b = media("ill-b", CAMPAIGN, "illustration", None);
        same_b["title"] = json!("B");
        let mut same_a = media("ill-a", CAMPAIGN, "illustration", None);
        same_a["title"] = json!("A");
        let rows = json!([old, media("sound-1", CAMPAIGN, "son", None), same_b, same_a]);
        let out = answer_rows(&rows, CAMPAIGN);
        assert_eq!(ids(&out["illustrations"]), ["ill-a", "ill-b", "ill-old"]);
        assert_eq!(out["battlemaps"], json!([]));
    }

    #[test]
    fn no_row_is_an_empty_well_formed_answer() {
        let expected = json!({ "cover": null, "battlemaps": [], "illustrations": [] });
        assert_eq!(answer_rows(&json!([]), CAMPAIGN), expected);
        assert_eq!(answer_rows(&Value::Null, CAMPAIGN), expected);
    }

    #[test]
    fn the_answer_only_names_what_the_mock_ups_fixture_names() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../../mock/fixtures/media.listMedia.json");
        let fixture: Value =
            serde_json::from_str(&std::fs::read_to_string(path).expect("the fixture"))
                .expect("the fixture is JSON");
        let rows = json!([
            battlemap_row("bm-1", CAMPAIGN, None),
            media("ill-1", CAMPAIGN, "illustration", None),
        ]);
        let out = answer_rows(&rows, CAMPAIGN);
        let keys = |v: &Value| -> Vec<String> {
            v.as_object().expect("an object").keys().cloned().collect()
        };
        for key in keys(&out) {
            assert!(fixture.get(&key).is_some(), "{key} in the fixture");
        }
        for kind in ["battlemaps", "illustrations"] {
            for key in keys(&out[kind][0]) {
                assert!(
                    fixture[kind][0].get(&key).is_some(),
                    "{kind}.{key} in the fixture"
                );
            }
        }
    }

    #[test]
    fn the_registry_s_answer_reads_its_variable_from_the_request() {
        let rows = json!([battlemap_row("bm-1", CAMPAIGN, None)]);
        let variables = BTreeMap::from([("campaignId".to_string(), json!(CAMPAIGN))]);
        let answered = answer(&rows, &variables);
        assert_ne!(answered, Value::Null);
        assert_eq!(answered, answer_rows(&rows, CAMPAIGN));
        assert_ne!(answer(&rows, &BTreeMap::new()), answered);
    }
}
