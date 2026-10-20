//! soundbox.setMasterVolume's preparation: the slider's percent made the
//! fraction the command writes on its campaign — the one field it writes.

use serde_json::{Value, json};
use set_master_volume::{MASTER_VOLUME, Refusal, Row, names, prepare};

const VALOMBRE: &str = "c1000000-0000-4000-8000-000000000001";

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
        (json!(12.5), json!(0.125)),
    ] {
        let prepared = prepare(
            &wire(json!({ "campaign": VALOMBRE, "volume": volume })),
            None,
        )
        .expect("prepared");
        assert_eq!(prepared.target_id, VALOMBRE);
        assert_eq!(
            prepared.payload,
            wire(json!({ MASTER_VOLUME: fraction })),
            "{volume}"
        );
    }
}

#[test]
fn a_volume_out_of_range_is_never_clamped_and_one_that_is_no_number_is_null() {
    // master-volume-range refuses them once the command is processed.
    for (volume, written) in [
        (json!(150), json!(1.5)),
        (json!(-5), json!(-0.05)),
        (json!("40"), Value::Null),
        (Value::Null, Value::Null),
        (json!([40]), Value::Null),
    ] {
        let prepared = prepare(
            &wire(json!({ "campaign": VALOMBRE, "volume": volume })),
            None,
        )
        .expect("prepared");
        assert_eq!(prepared.payload[MASTER_VOLUME], written, "{volume}");
    }
    let prepared = prepare(&wire(json!({ "campaign": VALOMBRE })), None).expect("prepared");
    assert_eq!(prepared.payload, wire(json!({ MASTER_VOLUME: null })));
}

#[test]
fn the_campaign_is_the_command_s_target_else_its_payload_s_lowercase() {
    let payload =
        wire(json!({ "campaign": " C1000000-0000-4000-8000-0000000000FF ", "volume": 40 }));
    assert_eq!(
        prepare(&payload, Some(&VALOMBRE.to_uppercase()))
            .expect("prepared")
            .target_id,
        VALOMBRE
    );
    assert_eq!(
        prepare(&payload, None).expect("prepared").target_id,
        "c1000000-0000-4000-8000-0000000000ff"
    );
    // A blank target names nothing: the payload's campaign is the target.
    assert_eq!(
        prepare(&payload, Some(" ")).expect("prepared").target_id,
        "c1000000-0000-4000-8000-0000000000ff"
    );
    // The campaign is the target alone: the payload never carries it.
    assert_eq!(prepare(&payload, None).expect("prepared").payload.len(), 1);
}

#[test]
fn a_request_naming_no_campaign_is_refused() {
    for (payload, target) in [
        (json!({ "volume": 40 }), None),
        (json!({ "campaign": "  ", "volume": 40 }), None),
        (json!({ "campaign": 7, "volume": 40 }), None),
        (json!({ "volume": 40 }), Some(" ")),
    ] {
        assert_eq!(
            prepare(&wire(payload.clone()), target),
            Err(Refusal::NoCampaign),
            "{payload}"
        );
    }
    assert_eq!(
        Refusal::NoCampaign.to_string(),
        "cette campagne n'existe pas"
    );
}

#[test]
fn only_this_data_capability_is_named_with_or_without_its_version() {
    assert!(names("soundbox.setMasterVolume"));
    assert!(names("soundbox.setMasterVolume@1"));
    assert!(!names("soundbox.setSoundVolume@1"));
    assert!(!names("soundbox.setMasterVolumes"));
}
