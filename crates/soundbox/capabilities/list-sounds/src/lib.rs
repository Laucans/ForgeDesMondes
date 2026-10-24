//! Capability `soundbox.listSounds`: a campaign's sounds, for the library and
//! the live soundbox alike.
//!
//! It reads, and only reads. The rows of its persisted query
//! (`crates/soundbox/queries/listSounds.graphql`, resolved by [`SQL`]) go in;
//! the answer, in the shape of `mock/fixtures/soundbox.listSounds.json` plus
//! what the store derives — `{ masterVolume, sounds: [{ id, name, tags,
//! volume, file, gain, playing }] }` — comes out. Volumes are percents on the
//! wire (the store keeps fractions; the query hands them over as percents).
//! `duration` is in the fixture but in no column yet: the key is absent, not
//! invented.
//!
//! It implements `EffectiveVolume@1` (`concepts/EffectiveVolume/v1`): the
//! `gain` of a sound is never stored, it is its own volume times the
//! campaign's master volume, computed here at read time by
//! [`effective_volume`], so that lowering the master lowers every sound by
//! the same factor. The tag filter reads the tags as `SoundTag@1`
//! (`concepts/SoundTag/v1`) defines them — [`normalise`] and [`matches`] — and
//! the name search reads the name alone (`tags-normalized`). `playing` is the
//! DM's view: a sound is playing while one of its playbacks has no
//! `stopped_at` in a session that is not closed (`dm-side-only`). The name
//! search is case-insensitive and keeps accents as typed, as `SoundTag@1`
//! reads text.

use std::collections::BTreeMap;

use serde_json::{Value, json};

/// The Capability's identifier, as a Micro-UI cites it.
pub const CAPABILITY: &str = "soundbox.listSounds";

/// What the persisted query `listSounds.graphql` selects: the campaign bound
/// to `$1` (compared as text, so that an id which is not a uuid finds nothing
/// instead of failing) with one row per sound, or a single row without sound
/// columns when its library is empty. The tags are handed over as a JSON array
/// (the engine's reads have no mapping for a `text[]`). Volumes are handed
/// over as whole percents (a stored volume has two decimals); `playing` is
/// whether a playback of the sound has not stopped in a session that is not
/// closed (a session that ended leaves nothing playing).
pub const SQL: &str = r"SELECT c.id::text AS campaign_id,
    round(c.sound_master_volume * 100)::int4 AS master_volume,
    s.id::text AS id,
    s.name,
    s.file_path,
    to_jsonb(s.tags) AS tags,
    round(s.volume * 100)::int4 AS volume,
    EXISTS (
        SELECT 1 FROM sound_playback p
        JOIN session se ON se.id = p.session_id
        WHERE p.sound_id = s.id AND p.stopped_at IS NULL AND se.status <> 'closed'
    ) AS playing
FROM campaign c
LEFT JOIN sound s ON s.campaign_id = c.id
WHERE c.id::text = $1
ORDER BY s.created_at, s.id";

/// The Concept's message for an own volume outside 0 to 100 %.
const VOLUME_OUT_OF_RANGE: &str = "volume propre hors de 0 à 100 %";

/// The Concept's message for a master volume outside 0 to 100 %.
const MASTER_OUT_OF_RANGE: &str = "volume global hors de 0 à 100 %";

/// `EffectiveVolume@1`: the own `volume` and the `masterVolume`, both
/// percents, as the Concept answers them — `gain` (volume × master ÷ 10000),
/// the two fractions, `valid` and `message`. A volume that is absent, not a
/// number or outside 0 to 100 is refused, never clamped, the own volume
/// first.
#[must_use]
fn effective_volume(volume: &Value, master_volume: &Value) -> Value {
    let percent = |value: &Value| value.as_f64().filter(|p| (0.0..=100.0).contains(p));
    match (percent(volume), percent(master_volume)) {
        (Some(volume), Some(master)) => json!({
            "gain": volume * master / 10_000.0,
            "volumeFraction": volume / 100.0,
            "masterFraction": master / 100.0,
            "valid": true,
            "message": null,
        }),
        (volume, _) => json!({
            "gain": null,
            "volumeFraction": null,
            "masterFraction": null,
            "valid": false,
            "message": if volume.is_none() { VOLUME_OUT_OF_RANGE } else { MASTER_OUT_OF_RANGE },
        }),
    }
}

/// `SoundTag@1`: a tag trimmed and lowercased character by character (accents
/// kept), `None` when nothing is left of it.
fn normal_tag(text: &str) -> Option<String> {
    let tag: String = text.trim().chars().flat_map(char::to_lowercase).collect();
    (!tag.is_empty()).then_some(tag)
}

/// `SoundTag@1`: a list of tags normalised tag by tag; a tag already seen is
/// dropped, the first occurrence stays and the order is kept.
#[must_use]
fn normalise<'a>(tags: impl IntoIterator<Item = &'a str>) -> Vec<String> {
    let mut kept: Vec<String> = Vec::new();
    for tag in tags.into_iter().filter_map(normal_tag) {
        if !kept.contains(&tag) {
            kept.push(tag);
        }
    }
    kept
}

/// `SoundTag@1`: whether the tag filter `filter` keeps a sound carrying
/// `tags` — always when the filter is empty once normalised, else when one of
/// its tags is among those of the sound.
#[must_use]
fn matches<'a>(tags: &[String], filter: impl IntoIterator<Item = &'a str>) -> bool {
    let filter = normalise(filter);
    filter.is_empty() || filter.iter().any(|wanted| tags.contains(wanted))
}

/// The sounds of `campaign` among `rows` — an array of the objects [`SQL`]
/// selects — as the DM reads them, in the order of the rows. An empty
/// `search` keeps every name, otherwise a sound is kept when its name holds
/// the search, whatever the case; an empty `tag` keeps every sound, otherwise
/// a sound is kept when it carries the tag (`SoundTag@1`). `null` when no row
/// is that campaign, which is an unknown campaign, not an empty library.
#[must_use]
pub fn answer_rows(rows: &Value, campaign: &str, search: &str, tag: &str) -> Value {
    let rows: Vec<&Value> = rows
        .as_array()
        .map(|rows| {
            rows.iter()
                .filter(|row| row["campaign_id"].as_str() == Some(campaign))
                .collect()
        })
        .unwrap_or_default();
    let Some(first) = rows.first() else {
        return Value::Null;
    };
    let master = &first["master_volume"];
    let search = lower(search.trim());
    let sounds: Vec<Value> = rows
        .iter()
        .filter(|row| row["id"].is_string())
        .filter_map(|row| {
            let tags = normalise(
                row["tags"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str),
            );
            let named = lower(row["name"].as_str().unwrap_or_default()).contains(&search);
            (named && matches(&tags, [tag])).then(|| {
                json!({
                    "id": row["id"],
                    "name": row["name"],
                    "tags": tags,
                    "volume": row["volume"],
                    "file": row["file_path"],
                    "gain": effective_volume(&row["volume"], master)["gain"],
                    "playing": row["playing"].as_bool().unwrap_or(false),
                })
            })
        })
        .collect();
    json!({ "masterVolume": master, "sounds": sounds })
}

/// The request's variables [`SQL`] binds, in the order of its `$1`, `$2`…:
/// what the registry hands the query. `search` and `tag` are not bound: they
/// filter the rows [`answer`] reads, they do not change what is selected.
pub const VARIABLES: &[&str] = &["campaign"];

/// The answer to a read, as the registry serves it: the rows of [`SQL`] and
/// the request's variables. It reads `campaign`, as [`SQL`] binds it, and the
/// optional `search` and `tag`; a variable absent or not a text is the empty
/// text, which names no campaign and filters nothing.
#[must_use]
pub fn answer(rows: &Value, variables: &BTreeMap<String, Value>) -> Value {
    let text = |name: &str| {
        variables
            .get(name)
            .and_then(Value::as_str)
            .unwrap_or_default()
    };
    answer_rows(rows, text("campaign"), text("search"), text("tag"))
}

/// Lowercase character by character, accents kept, as `SoundTag@1` reads text.
fn lower(text: &str) -> String {
    text.chars().flat_map(char::to_lowercase).collect()
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    const CAMPAIGN: &str = "c1";

    /// A row as [`SQL`] selects it for a sound of `CAMPAIGN`, master at 80.
    fn row(id: &str, name: &str, tags: &[&str], volume: u32, playing: bool) -> Value {
        json!({
            "campaign_id": CAMPAIGN, "master_volume": 80, "id": id, "name": name,
            "file_path": format!("{id}.ogg"), "tags": tags, "volume": volume, "playing": playing,
        })
    }

    /// The row of a campaign whose library is empty.
    fn empty_library(master: u32) -> Value {
        json!({
            "campaign_id": CAMPAIGN, "master_volume": master, "id": null, "name": null,
            "file_path": null, "tags": null, "volume": null, "playing": false,
        })
    }

    fn library() -> Value {
        json!([
            row(
                "snd-foule",
                "Bruit de foule en taverne",
                &["foule", "taverne"],
                55,
                false
            ),
            row(
                "snd-porte",
                "Porte qui grince",
                &["porte", "bois"],
                80,
                false
            ),
            row(
                "snd-chope",
                "Chope qui se brise",
                &["taverne", "verre"],
                70,
                false
            ),
        ])
    }

    fn ids(answer: &Value) -> Vec<&str> {
        answer["sounds"]
            .as_array()
            .expect("sounds")
            .iter()
            .map(|s| s["id"].as_str().expect("an id"))
            .collect()
    }

    fn fixture() -> Value {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../../mock/fixtures/soundbox.listSounds.json");
        serde_json::from_str(&std::fs::read_to_string(path).expect("the fixture")).expect("JSON")
    }

    #[test]
    fn effective_volume_conformance() {
        platform_conformance::check("EffectiveVolume@1", |input| {
            effective_volume(&input["volume"], &input["masterVolume"])
        });
    }

    #[test]
    fn sound_tag_conformance() {
        platform_conformance::check("SoundTag@1", |input| {
            let sound = input["tags"]
                .as_array()
                .expect("tags")
                .iter()
                .filter_map(Value::as_str);
            let tags = normalise(sound);
            let filter = input["filter"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str);
            json!({ "matches": matches(&tags, filter), "tags": tags })
        });
    }

    #[test]
    fn sounds_at_55_and_80_under_a_master_of_80_play_at_044_and_064() {
        let got = answer_rows(&library(), CAMPAIGN, "", "");
        assert_eq!(got["masterVolume"], 80);
        let gains: Vec<&Value> = got["sounds"]
            .as_array()
            .expect("sounds")
            .iter()
            .map(|s| &s["gain"])
            .collect();
        assert_eq!(gains[0], &json!(0.44));
        assert_eq!(gains[1], &json!(0.64));
    }

    #[test]
    fn a_sound_with_a_running_playback_is_playing_and_the_others_are_not() {
        let mut rows = library();
        rows[1]["playing"] = json!(true);
        let got = answer_rows(&rows, CAMPAIGN, "", "");
        let playing: Vec<bool> = got["sounds"]
            .as_array()
            .expect("sounds")
            .iter()
            .map(|s| s["playing"].as_bool().expect("a flag"))
            .collect();
        assert_eq!(playing, [false, true, false]);
    }

    #[test]
    fn a_tag_filter_keeps_the_sounds_with_that_tag_whatever_its_case() {
        for filter in ["Taverne", "taverne", "  TAVERNE "] {
            let got = answer_rows(&library(), CAMPAIGN, "", filter);
            assert_eq!(ids(&got), ["snd-foule", "snd-chope"], "{filter}");
        }
    }

    #[test]
    fn a_tag_filter_reads_accents_as_they_are() {
        let rows = json!([row("a", "Écho", &["Intérieur"], 50, false)]);
        assert_eq!(ids(&answer_rows(&rows, CAMPAIGN, "", "intérieur")), ["a"]);
        assert!(ids(&answer_rows(&rows, CAMPAIGN, "", "interieur")).is_empty());
    }

    #[test]
    fn a_search_keeps_the_sounds_whose_name_holds_it() {
        for search in ["porte", "PORTE", " Porte "] {
            let got = answer_rows(&library(), CAMPAIGN, search, "");
            assert_eq!(ids(&got), ["snd-porte"], "{search}");
        }
        assert_eq!(ids(&answer_rows(&library(), CAMPAIGN, "qui", "")).len(), 2);
    }

    #[test]
    fn a_search_reads_the_name_alone_not_the_tags() {
        // « bois » is a tag of the door, in no name.
        assert!(ids(&answer_rows(&library(), CAMPAIGN, "bois", "")).is_empty());
    }

    #[test]
    fn the_search_and_the_tag_filter_both_apply() {
        let got = answer_rows(&library(), CAMPAIGN, "chope", "taverne");
        assert_eq!(ids(&got), ["snd-chope"]);
        assert!(ids(&answer_rows(&library(), CAMPAIGN, "porte", "taverne")).is_empty());
    }

    #[test]
    fn a_filter_that_matches_nothing_is_an_empty_list_with_the_master() {
        let got = answer_rows(&library(), CAMPAIGN, "zzz", "");
        assert_eq!(got, json!({ "masterVolume": 80, "sounds": [] }));
    }

    #[test]
    fn an_empty_library_is_no_sound_under_its_master() {
        let got = answer_rows(&json!([empty_library(60)]), CAMPAIGN, "", "");
        assert_eq!(got, json!({ "masterVolume": 60, "sounds": [] }));
    }

    #[test]
    fn an_unknown_campaign_has_no_answer() {
        assert_eq!(answer_rows(&library(), "nobody", "", ""), Value::Null);
        assert_eq!(answer_rows(&json!([]), CAMPAIGN, "", ""), Value::Null);
        assert_eq!(answer_rows(&Value::Null, CAMPAIGN, "", ""), Value::Null);
    }

    #[test]
    fn only_the_asked_campaigns_sounds_are_answered() {
        let mut rows = library();
        let other = &mut rows.as_array_mut().expect("rows");
        other.push(json!({
            "campaign_id": "c2", "master_volume": 10, "id": "snd-x", "name": "Autre",
            "file_path": "x.ogg", "tags": [], "volume": 10, "playing": true,
        }));
        let got = answer_rows(&rows, CAMPAIGN, "", "");
        assert_eq!(got["masterVolume"], 80);
        assert_eq!(ids(&got), ["snd-foule", "snd-porte", "snd-chope"]);
    }

    #[test]
    fn the_rows_order_is_kept() {
        let got = answer_rows(&library(), CAMPAIGN, "", "");
        assert_eq!(ids(&got), ["snd-foule", "snd-porte", "snd-chope"]);
    }

    #[test]
    fn the_tags_are_answered_normalised() {
        let rows = json!([row(
            "a",
            "A",
            &["Taverne", "taverne ", "", "Foule"],
            50,
            false
        )]);
        let got = answer_rows(&rows, CAMPAIGN, "", "");
        assert_eq!(got["sounds"][0]["tags"], json!(["taverne", "foule"]));
    }

    #[test]
    fn a_volume_outside_the_range_has_no_gain_and_is_not_clamped() {
        let rows = json!([row("a", "A", &[], 120, false)]);
        let got = answer_rows(&rows, CAMPAIGN, "", "");
        assert_eq!(got["sounds"][0]["gain"], Value::Null);
        assert_eq!(got["sounds"][0]["volume"], 120);
    }

    #[test]
    fn the_master_down_lowers_every_gain_by_the_same_factor() {
        let mut rows = library();
        for row in rows.as_array_mut().expect("rows") {
            row["master_volume"] = json!(40);
        }
        let got = answer_rows(&rows, CAMPAIGN, "", "");
        assert_eq!(got["sounds"][0]["gain"], json!(0.22));
        assert_eq!(got["sounds"][1]["gain"], json!(0.32));
    }

    #[test]
    fn a_sound_has_the_fixtures_keys_but_the_duration_the_store_lacks() {
        let fixture = fixture();
        let known = fixture["sounds"][0].as_object().expect("a sound");
        let got = answer_rows(&library(), CAMPAIGN, "", "");
        let sound = got["sounds"][0].as_object().expect("a sound");
        for key in known.keys().filter(|key| *key != "duration") {
            assert!(sound.contains_key(key), "{key} is missing from {sound:?}");
        }
        for key in sound.keys() {
            assert!(
                known.contains_key(key) || matches!(key.as_str(), "gain" | "playing"),
                "{key} is neither in the fixture nor derived"
            );
        }
        assert!(!sound.contains_key("duration"));
    }

    #[test]
    fn the_fixtures_sounds_come_back_as_they_are_with_a_gain_and_no_playback() {
        let fixture = fixture();
        let master = fixture["masterVolume"].clone();
        let rows: Vec<Value> = fixture["sounds"]
            .as_array()
            .expect("sounds")
            .iter()
            .map(|s| {
                json!({
                    "campaign_id": CAMPAIGN, "master_volume": master, "id": s["id"],
                    "name": s["name"], "file_path": s["file"], "tags": s["tags"],
                    "volume": s["volume"], "playing": false,
                })
            })
            .collect();
        let got = answer_rows(&json!(rows), CAMPAIGN, "", "");
        assert_eq!(got["masterVolume"], fixture["masterVolume"]);
        for (got, known) in got["sounds"]
            .as_array()
            .expect("sounds")
            .iter()
            .zip(fixture["sounds"].as_array().expect("sounds"))
        {
            for key in ["id", "name", "tags", "volume", "file"] {
                assert_eq!(got[key], known[key], "{key} of {}", known["id"]);
            }
            assert_eq!(got["playing"], false);
            assert!(got["gain"].is_number());
        }
    }

    #[test]
    fn the_registrys_answer_reads_the_campaign_the_search_and_the_tag() {
        let variables = |pairs: &[(&str, &str)]| -> BTreeMap<String, Value> {
            pairs
                .iter()
                .map(|(name, value)| ((*name).to_owned(), json!(value)))
                .collect()
        };
        let all = answer(&library(), &variables(&[("campaign", CAMPAIGN)]));
        assert_eq!(ids(&all), ["snd-foule", "snd-porte", "snd-chope"]);
        let searched = answer(
            &library(),
            &variables(&[("campaign", CAMPAIGN), ("search", "porte")]),
        );
        assert_eq!(ids(&searched), ["snd-porte"]);
        let tagged = answer(
            &library(),
            &variables(&[("campaign", CAMPAIGN), ("tag", "Taverne")]),
        );
        assert_eq!(ids(&tagged), ["snd-foule", "snd-chope"]);
        assert_eq!(answer(&library(), &BTreeMap::new()), Value::Null);
        assert_eq!(VARIABLES, ["campaign"]);
    }

    #[test]
    fn the_sql_is_one_read_statement_bound_to_one_campaign() {
        assert!(
            SQL.trim_start().to_ascii_uppercase().starts_with("SELECT"),
            "{SQL}"
        );
        assert!(!SQL.contains(';'), "a second statement in {SQL}");
        assert!(SQL.contains("WHERE c.id::text = $1"), "{SQL}");
        assert!(SQL.contains("LEFT JOIN sound s"), "{SQL}");
        assert!(SQL.contains("to_jsonb(s.tags) AS tags"), "{SQL}");
        assert!(SQL.contains("p.stopped_at IS NULL"), "{SQL}");
        assert!(SQL.contains("se.status <> 'closed'"), "{SQL}");
        assert!(!SQL.to_ascii_lowercase().contains("gain"), "{SQL}");
    }
}
