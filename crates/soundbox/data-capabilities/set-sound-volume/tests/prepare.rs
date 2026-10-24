//! `prepare`: what the LiveSoundbox's slider sends made the command's
//! payload and target — the volume alone, the percent divided by 100 to the
//! whole percent, out of range never clamped, no number written null; the
//! sound named by the target, else the payload, trimmed and lowercase.

use serde_json::{Value, json};
use set_sound_volume::{DATA_CAPABILITY, Prepared, Refusal, Row, prepare};

const PORTE: &str = "50000000-0000-4000-8000-000000000002";

fn wire(payload: Value) -> Row {
    let Value::Object(fields) = payload else {
        panic!("an object")
    };
    fields.into_iter().collect()
}

fn volume(given: Value) -> Value {
    prepare(&wire(json!({ "volume": given })), Some(PORTE))
        .expect("prepared")
        .payload["volume"]
        .clone()
}

#[test]
fn the_id_is_the_manifest_s() {
    let manifest: Value =
        serde_json::from_str(include_str!("../data-capability.json")).expect("contract F");
    assert_eq!(manifest["dataCapability"], DATA_CAPABILITY);
}

#[test]
fn forty_percent_is_stored_as_0_4_and_nothing_else() {
    assert_eq!(
        prepare(&wire(json!({ "volume": 40, "name": "Porte" })), Some(PORTE)),
        Ok(Prepared {
            payload: wire(json!({ "volume": 0.4 })),
            target_id: PORTE.into(),
        })
    );
}

#[test]
fn a_percent_in_range_is_kept_to_the_whole_percent() {
    assert_eq!(volume(json!(0)), json!(0.0));
    assert_eq!(volume(json!(100)), json!(1.0));
    assert_eq!(volume(json!(40.4)), json!(0.4));
    assert_eq!(volume(json!(40.6)), json!(0.41));
    assert_eq!(volume(json!(99.5)), json!(1.0));
}

#[test]
fn a_negative_zero_is_stored_as_the_zero_the_table_holds() {
    assert_eq!(volume(json!(-0.0)).to_string(), "0.0");
    assert_eq!(volume(json!(-0.4)), json!(-0.004));
}

#[test]
fn a_percent_out_of_range_is_never_clamped_nor_rounded() {
    assert_eq!(volume(json!(101)), json!(1.01));
    assert_eq!(volume(json!(100.4)), json!(1.004));
    assert_eq!(volume(json!(-0.4)), json!(-0.004));
    assert_eq!(volume(json!(-5)), json!(-0.05));
}

#[test]
fn a_volume_that_is_no_number_is_written_null() {
    for given in [json!("40"), json!(null), json!(true), json!([40])] {
        assert_eq!(volume(given.clone()), Value::Null, "{given}");
    }
    let prepared = prepare(&Row::new(), Some(PORTE)).expect("prepared");
    assert_eq!(prepared.payload, wire(json!({ "volume": null })));
}

#[test]
fn the_target_is_trimmed_and_lowercase() {
    let prepared = prepare(
        &wire(json!({ "volume": 40 })),
        Some(&format!("  {}  ", PORTE.to_uppercase())),
    )
    .expect("prepared");
    assert_eq!(prepared.target_id, PORTE);
}

#[test]
fn the_payload_s_sound_names_it_when_the_command_names_no_target() {
    let named = wire(json!({ "sound": format!(" {} ", PORTE.to_uppercase()), "volume": 40 }));
    assert_eq!(prepare(&named, None).expect("prepared").target_id, PORTE);
    assert_eq!(
        prepare(&named, Some("  ")).expect("prepared").target_id,
        PORTE
    );
    let other = "50000000-0000-4000-8000-000000000003";
    assert_eq!(
        prepare(&named, Some(other)).expect("prepared").target_id,
        other
    );
}

#[test]
fn a_change_naming_no_sound_is_refused() {
    for (payload, target) in [
        (json!({ "volume": 40 }), None),
        (json!({ "volume": 40 }), Some(" ")),
        (json!({ "sound": "  ", "volume": 40 }), None),
        (json!({ "sound": 3, "volume": 40 }), None),
    ] {
        assert_eq!(
            prepare(&wire(payload.clone()), target),
            Err(Refusal::NoSound),
            "{payload}"
        );
    }
    assert_eq!(Refusal::NoSound.to_string(), "ce son n'existe pas");
}
