//! soundbox.setSoundVolume's preparation: the slider's percent made the
//! fraction the command writes on its sound — the one field it writes.

use serde_json::{Value, json};
use set_sound_volume::{Refusal, Row, VOLUME, names, prepare};

const DOOR: &str = "50000000-0000-4000-8000-000000000001";

fn wire(payload: Value) -> Row {
    let Value::Object(payload) = payload else {
        panic!("a payload is an object");
    };
    payload.into_iter().collect()
}

#[test]
fn a_percent_is_written_as_its_fraction_and_nothing_else() {
    for (volume, fraction) in [
        (json!(40), json!(0.4)),
        (json!(0), json!(0.0)),
        (json!(100), json!(1.0)),
        (json!(55), json!(0.55)),
        (json!(7), json!(0.07)),
        // To the whole percent: the column holds hundredths.
        (json!(12.5), json!(0.13)),
        (json!(99.6), json!(1.0)),
        (json!(0.4), json!(0.0)),
    ] {
        let prepared = prepare(&wire(json!({ "volume": volume })), Some(DOOR)).expect("prepared");
        assert_eq!(prepared.target_id, DOOR);
        assert_eq!(
            prepared.payload,
            wire(json!({ VOLUME: fraction })),
            "{volume}"
        );
    }
}

#[test]
fn a_field_other_than_the_volume_is_never_written() {
    let given = wire(json!({ "volume": 30, "name": "Porte", "tags": ["bois"], "file": "x.ogg" }));
    let prepared = prepare(&given, Some(DOOR)).expect("prepared");
    assert_eq!(prepared.payload, wire(json!({ VOLUME: 0.3 })));
}

#[test]
fn a_volume_out_of_range_is_never_clamped_and_one_that_is_no_number_is_null() {
    // sound-volume-range refuses them once the command is processed.
    for (volume, written) in [
        (json!(101), json!(1.01)),
        (json!(-5), json!(-0.05)),
        // Out of range is never rounded into it.
        (json!(100.4), json!(1.004)),
        (json!(-0.4), json!(-0.004)),
        (json!("40"), Value::Null),
        (Value::Null, Value::Null),
        (json!([40]), Value::Null),
    ] {
        let prepared = prepare(&wire(json!({ "volume": volume })), Some(DOOR)).expect("prepared");
        assert_eq!(prepared.payload[VOLUME], written, "{volume}");
    }
    let prepared = prepare(&wire(json!({})), Some(DOOR)).expect("prepared");
    assert_eq!(prepared.payload, wire(json!({ VOLUME: null })));
}

#[test]
fn the_sound_is_its_target_trimmed_and_lowercased() {
    let given = format!(" {} ", DOOR.to_uppercase());
    let prepared = prepare(&wire(json!({ "volume": 50 })), Some(&given)).expect("prepared");
    assert_eq!(prepared.target_id, DOOR);
}

#[test]
fn a_request_naming_no_sound_is_refused() {
    for target in [None, Some(""), Some("   ")] {
        assert_eq!(
            prepare(&wire(json!({ "volume": 40 })), target),
            Err(Refusal::NoSound),
            "{target:?}"
        );
    }
    assert_eq!(Refusal::NoSound.to_string(), "ce son n'existe pas");
}

#[test]
fn only_this_data_capability_is_named_with_or_without_its_version() {
    assert!(names("soundbox.setSoundVolume"));
    assert!(names("soundbox.setSoundVolume@1"));
    assert!(!names("soundbox.setMasterVolume@1"));
    assert!(!names("soundbox.setSoundVolumes"));
}
