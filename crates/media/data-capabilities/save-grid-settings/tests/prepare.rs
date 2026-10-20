//! `prepare`: the wire payload of a save, in `HexGrid@1`'s words, made the
//! six fields written on the battlemap's `Media` partition — and what is
//! refused before it is queued.

use save_grid_settings::{Prepared, Refusal, Row, names, prepare};
use serde_json::{Value, json};

const CARTE: &str = "d1000000-0000-4000-8000-00000000000a";

fn wire(payload: Value) -> Row {
    let Value::Object(payload) = payload else {
        panic!("a payload is an object");
    };
    payload.into_iter().collect()
}

/// `payload` saved on the tavern's battlemap: the fields it writes.
fn saved(payload: Value) -> Result<Row, Refusal> {
    prepare(&wire(payload), Some(CARTE)).map(|prepared| prepared.payload)
}

/// The save of the acceptance criteria: hex size 48, offsets 30/25, flat,
/// a colour, opacity 0.2.
fn adjusted() -> Value {
    json!({ "size": 48, "ox": 30, "oy": 25, "orient": "flat", "color": "accent", "opacity": 0.2 })
}

#[test]
fn the_six_settings_are_the_aggregate_s_fields_and_nothing_else() {
    let expected = wire(json!({
        "hexSizePx": 48,
        "offsetX": 30,
        "offsetY": 25,
        "orientation": "plat",
        "color": "accent",
        "opacity": 0.2,
    }));
    assert_eq!(saved(adjusted()), Ok(expected));
}

#[test]
fn pointy_is_stored_pointe_and_the_stored_words_are_kept() {
    for (given, stored) in [
        ("pointy", "pointe"),
        ("flat", "plat"),
        ("pointe", "pointe"),
        ("plat", "plat"),
    ] {
        let mut save = adjusted();
        save["orient"] = json!(given);
        let payload = saved(save).expect("a save");
        assert_eq!(payload["orientation"], stored, "{given}");
    }
}

#[test]
fn the_feet_per_hex_or_any_field_outside_the_six_is_refused() {
    for field in ["feetPerHex", "feet_per_hex", "mediaId", "kind", "hexSizePx"] {
        let mut save = adjusted();
        save[field] = json!(5);
        assert_eq!(
            saved(save),
            Err(Refusal::Unknown(field.to_string())),
            "{field}"
        );
    }
}

#[test]
fn a_missing_setting_is_refused_naming_it() {
    for field in ["size", "ox", "oy", "orient", "color", "opacity"] {
        let mut save = adjusted();
        save.as_object_mut().expect("an object").remove(field);
        assert_eq!(saved(save.clone()), Err(Refusal::Missing(field)), "{field}");
        save[field] = Value::Null;
        assert_eq!(saved(save), Err(Refusal::Missing(field)), "{field} null");
    }
}

#[test]
fn a_value_no_grid_can_hold_is_refused_naming_it() {
    for (field, value) in [
        ("size", json!("48")),
        ("size", json!(100_000)),
        ("ox", json!(-100_000)),
        ("oy", json!(true)),
        ("orient", json!("diagonal")),
        ("orient", json!(1)),
        ("color", json!("")),
        ("color", json!(3)),
        ("opacity", json!("0.2")),
    ] {
        let mut save = adjusted();
        save[field] = value.clone();
        assert_eq!(
            saved(save),
            Err(Refusal::Invalid(field)),
            "{field}: {value}"
        );
    }
}

#[test]
fn a_size_of_zero_or_an_opacity_out_of_range_is_left_to_grid_values_range() {
    // grid-values-range rejects them once queued, naming itself.
    for (field, value) in [
        ("size", json!(0)),
        ("size", json!(-4)),
        ("opacity", json!(1.5)),
        ("opacity", json!(-0.1)),
    ] {
        let mut save = adjusted();
        save[field] = value.clone();
        let payload = saved(save).expect("submitted");
        let stored = if field == "size" {
            "hexSizePx"
        } else {
            "opacity"
        };
        assert_eq!(payload[stored], value, "{field}");
    }
}

#[test]
fn negative_offsets_and_the_largest_the_table_holds_are_kept() {
    let mut save = adjusted();
    save["ox"] = json!(-12.5);
    save["oy"] = json!(99_999.99);
    let payload = saved(save).expect("a save");
    assert_eq!(payload["offsetX"], json!(-12.5));
    assert_eq!(payload["offsetY"], json!(99_999.99));
}

#[test]
fn the_battlemap_is_the_target_or_the_payload_s_id_lowercase_and_the_id_is_never_written() {
    let upper = CARTE.to_uppercase();
    let mut save = adjusted();
    let by_target = prepare(&wire(save.clone()), Some(&upper)).expect("a save");
    save["id"] = json!(upper);
    let by_id = prepare(&wire(save.clone()), None).expect("a save");
    assert_eq!(by_target.target_id, CARTE);
    assert_eq!(by_id, by_target);
    // The command's target wins over the payload's id.
    let other = "d1000000-0000-4000-8000-00000000000b";
    let Prepared { target_id, payload } = prepare(&wire(save), Some(other)).expect("a save");
    assert_eq!(target_id, other);
    assert!(!payload.contains_key("id"));
}

#[test]
fn a_save_naming_no_battlemap_is_refused() {
    assert_eq!(prepare(&wire(adjusted()), None), Err(Refusal::NoBattlemap));
    assert_eq!(
        prepare(&wire(adjusted()), Some(" ")),
        Err(Refusal::NoBattlemap)
    );
    let mut save = adjusted();
    save["id"] = json!(7);
    assert_eq!(prepare(&wire(save), None), Err(Refusal::NoBattlemap));
}

#[test]
fn each_number_is_kept_to_the_hundredth_the_table_keeps() {
    let mut save = adjusted();
    save["size"] = json!(48.126);
    save["ox"] = json!(-0.004);
    save["opacity"] = json!(0.333);
    let payload = saved(save).expect("a save");
    assert_eq!(payload["hexSizePx"], json!(48.13));
    assert_eq!(payload["offsetX"].as_f64(), Some(0.0));
    assert_eq!(payload["opacity"], json!(0.33));
    // A size the table would round to 0 is the 0 grid-values-range rejects.
    let mut save = adjusted();
    save["size"] = json!(0.004);
    assert_eq!(saved(save).expect("submitted")["hexSizePx"], json!(0.0));
}

#[test]
fn the_data_capability_is_named_with_or_without_its_version() {
    assert!(names("media.saveGridSettings"));
    assert!(names("media.saveGridSettings@1"));
    assert!(!names("media.uploadMedia@1"));
    assert!(!names("media.saveGridSettingsX"));
}

#[test]
fn a_refusal_names_its_field() {
    assert!(
        Refusal::Unknown("feetPerHex".into())
            .to_string()
            .starts_with("feetPerHex")
    );
    assert!(Refusal::Missing("size").to_string().starts_with("size"));
    assert!(Refusal::Invalid("orient").to_string().starts_with("orient"));
    assert!(!Refusal::NoBattlemap.to_string().is_empty());
}
