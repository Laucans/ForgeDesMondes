//! What an edit of a sound becomes before it is submitted: the fields it
//! writes on the sound's partition, or why it is refused before it is
//! queued.

use harness_dataguard::plugins::{self, RefusalKind};
use serde_json::{Value, json};
use update_sound::{Refusal, Row, prepare};

const DOOR: &str = "50000000-0000-4000-8000-000000000001";

fn wire(payload: Value) -> Row {
    let Value::Object(payload) = payload else {
        panic!("a payload is an object");
    };
    payload.into_iter().collect()
}

/// The fields an edit of the door writes.
fn prepared(payload: Value) -> Result<Row, Refusal> {
    prepare(&wire(payload), Some(DOOR)).map(|prepared| {
        assert_eq!(prepared.target_id, DOOR);
        prepared.payload
    })
}

#[test]
fn the_name_and_the_tags_given_are_written_the_tags_normalised() {
    let row = prepared(json!({ "name": "Porte qui grince", "tags": ["Porte", "porte"] }))
        .expect("prepared");
    assert_eq!(
        row,
        wire(json!({ "name": "Porte qui grince", "tags": ["porte"] }))
    );
}

#[test]
fn every_field_given_is_written_under_the_aggregate_s_name() {
    let row = prepared(json!({
        "name": "  Porte qui grince ",
        "file": " sons/porte.ogg ",
        "tags": [" Bois", "Intérieur "],
        "volume": 80,
    }))
    .expect("prepared");
    assert_eq!(
        row,
        wire(json!({
            "name": "Porte qui grince",
            "filePath": "sons/porte.ogg",
            "tags": ["bois", "intérieur"],
            "volume": 0.8,
        }))
    );
}

#[test]
fn a_field_left_out_is_not_written_and_one_the_sound_has_not_is_dropped() {
    let row =
        prepared(json!({ "volume": 30, "id": "another", "playing": true })).expect("prepared");
    assert_eq!(row, wire(json!({ "volume": 0.3 })));
}

#[test]
fn null_tags_are_no_tags() {
    let row = prepared(json!({ "tags": null })).expect("prepared");
    assert_eq!(row["tags"], json!([]));
}

#[test]
fn tags_that_are_no_list_of_texts_are_kept_for_tags_normalized_to_reject() {
    for tags in [json!("Porte"), json!(["Porte", 3]), json!(3)] {
        let row = prepared(json!({ "tags": tags.clone() })).expect("prepared");
        assert_eq!(row["tags"], tags);
    }
}

#[test]
fn a_volume_in_percent_is_stored_as_a_fraction_of_1() {
    for (percent, fraction) in [
        (json!(0), 0.0),
        (json!(100), 1.0),
        (json!(80), 0.8),
        (json!("30"), 0.3),
        (json!(" 45 "), 0.45),
    ] {
        let row = prepared(json!({ "volume": percent.clone() })).expect("prepared");
        assert_eq!(row["volume"], json!(fraction), "{percent}");
    }
}

#[test]
fn a_volume_in_range_is_kept_to_the_hundredth_the_table_holds() {
    for (percent, fraction) in [(55.5, 0.56), (0.4, 0.0), (99.6, 1.0)] {
        let row = prepared(json!({ "volume": percent })).expect("prepared");
        assert_eq!(row["volume"], json!(fraction), "{percent}");
    }
}

#[test]
fn a_volume_out_of_range_is_kept_unrounded_for_sound_volume_range_to_reject() {
    for (percent, fraction) in [(-5.0, -0.05), (120.0, 1.2), (100.4, 1.004), (-0.4, -0.004)] {
        let row = prepared(json!({ "volume": percent })).expect("prepared");
        assert_eq!(row["volume"], json!(fraction), "{percent}");
    }
}

#[test]
fn a_volume_that_is_no_number_is_kept_as_given() {
    for volume in [
        json!("fort"),
        json!(true),
        json!(null),
        json!("NaN"),
        json!("inf"),
    ] {
        let row = prepared(json!({ "volume": volume.clone() })).expect("prepared");
        assert_eq!(row["volume"], volume);
    }
}

#[test]
fn the_sound_is_its_target_trimmed_and_lowercased() {
    let given = format!(" {} ", DOOR.to_uppercase());
    let prepared = prepare(&wire(json!({ "volume": 50 })), Some(&given)).expect("prepared");
    assert_eq!(prepared.target_id, DOOR);
}

#[test]
fn an_edit_naming_no_sound_is_refused() {
    for target in [None, Some(""), Some("   ")] {
        assert_eq!(
            prepare(&wire(json!({ "name": "Porte" })), target),
            Err(Refusal::NoSound),
            "{target:?}"
        );
    }
}

#[test]
fn a_blank_name_or_file_given_is_refused() {
    for (payload, refusal) in [
        (json!({ "name": "" }), Refusal::NoName),
        (json!({ "name": "   " }), Refusal::NoName),
        (json!({ "name": null }), Refusal::NoName),
        (json!({ "name": 3 }), Refusal::NoName),
        (json!({ "volume": 50, "file": "" }), Refusal::NoFile),
        (json!({ "file": null }), Refusal::NoFile),
    ] {
        assert_eq!(prepared(payload.clone()), Err(refusal), "{payload}");
    }
}

#[test]
fn an_edit_changing_nothing_is_refused() {
    for payload in [json!({}), json!({ "playing": true, "campaign": "x" })] {
        assert_eq!(
            prepared(payload.clone()),
            Err(Refusal::Nothing),
            "{payload}"
        );
    }
}

#[test]
fn each_refusal_says_why_in_french() {
    assert_eq!(Refusal::NoSound.to_string(), "ce son n'existe pas");
    assert_eq!(Refusal::NoName.to_string(), "un son a besoin d'un nom");
    assert_eq!(
        Refusal::NoFile.to_string(),
        "un son a besoin de son fichier"
    );
    assert_eq!(Refusal::Nothing.to_string(), "rien à modifier sur ce son");
}

#[test]
fn with_no_target_the_sound_is_the_payload_s_id_trimmed_and_lowercased() {
    let given = format!(" {} ", DOOR.to_uppercase());
    let prepared = prepare(&wire(json!({ "id": given, "volume": 50 })), None).expect("prepared");
    assert_eq!(prepared.target_id, DOOR);
    assert_eq!(prepared.payload, wire(json!({ "volume": 0.5 })));
}

#[test]
fn no_sound_is_a_404_and_any_other_refusal_a_422() {
    let kind = |refusal: Refusal| plugins::Refusal::from(refusal).kind;
    assert_eq!(kind(Refusal::NoSound), RefusalKind::NotFound);
    for refusal in [Refusal::NoName, Refusal::NoFile, Refusal::Nothing] {
        assert_eq!(kind(refusal), RefusalKind::Unprocessable, "{refusal:?}");
    }
    assert_eq!(
        plugins::Refusal::from(Refusal::NoName).message,
        "un son a besoin d'un nom"
    );
}
