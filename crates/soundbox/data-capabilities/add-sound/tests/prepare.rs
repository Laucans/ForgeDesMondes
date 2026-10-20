//! What an addition of a sound becomes before it is submitted: the row it
//! writes on its own partition, or why it is refused before it is queued.

use add_sound::{Fresh, Refusal, Row, names, prepare};
use serde_json::{Value, json};

const SOUND: &str = "50000000-0000-4000-8000-000000000001";
const VALOMBRE: &str = "c1000000-0000-4000-8000-000000000001";
const NOW: &str = "2026-10-20T10:00:00.000Z";

fn wire(payload: Value) -> Row {
    let Value::Object(payload) = payload else {
        panic!("a payload is an object");
    };
    payload.into_iter().collect()
}

fn prepared(payload: Value) -> Result<Row, Refusal> {
    let fresh = Fresh {
        sound: SOUND,
        now: NOW,
    };
    prepare(&wire(payload), fresh).map(|prepared| {
        assert_eq!(prepared.target_id, SOUND);
        prepared.payload
    })
}

/// The crowd's murmur of the issue, its campaign as `with` changes it.
fn crowd(with: Value) -> Value {
    let mut payload = json!({
        "campaign": VALOMBRE,
        "name": "Bruit de foule",
        "tags": ["Taverne"],
        "volume": 55,
        "file": "son.ogg",
    });
    for (field, value) in with.as_object().expect("an object") {
        payload[field] = value.clone();
    }
    payload
}

#[test]
fn a_sound_is_written_whole_its_tags_normalised_and_its_volume_a_fraction() {
    let row = prepared(crowd(json!({}))).expect("prepared");
    let expected = wire(json!({
        "id": SOUND,
        "campaignId": VALOMBRE,
        "name": "Bruit de foule",
        "filePath": "son.ogg",
        "tags": ["taverne"],
        "volume": 0.55,
        "createdAt": NOW,
    }));
    assert_eq!(row, expected);
}

#[test]
fn tags_differing_by_case_and_surrounding_whitespace_are_one_tag() {
    let row = prepared(crowd(json!({ "tags": ["Bois", "bois "] }))).expect("prepared");
    assert_eq!(row["tags"], json!(["bois"]));
}

#[test]
fn no_tags_given_are_no_tags() {
    let mut payload = crowd(json!({}));
    payload.as_object_mut().expect("an object").remove("tags");
    assert_eq!(prepared(payload).expect("prepared")["tags"], json!([]));
    let row = prepared(crowd(json!({ "tags": null }))).expect("prepared");
    assert_eq!(row["tags"], json!([]));
}

#[test]
fn tags_that_are_no_list_of_texts_are_kept_for_tags_normalized_to_reject() {
    for tags in [json!("Taverne"), json!(["Taverne", 3])] {
        let row = prepared(crowd(json!({ "tags": tags.clone() }))).expect("prepared");
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
    ] {
        let row = prepared(crowd(json!({ "volume": percent.clone() }))).expect("prepared");
        assert_eq!(row["volume"], json!(fraction), "{percent}");
    }
}

#[test]
fn a_volume_in_range_is_kept_to_the_hundredth_the_table_holds() {
    for (percent, fraction) in [(55.5, 0.56), (0.4, 0.0), (99.6, 1.0)] {
        let row = prepared(crowd(json!({ "volume": percent }))).expect("prepared");
        assert_eq!(row["volume"], json!(fraction), "{percent}");
    }
}

#[test]
fn a_volume_just_out_of_range_is_kept_unrounded() {
    let row = prepared(crowd(json!({ "volume": 100.4 }))).expect("prepared");
    assert_eq!(row["volume"], json!(1.004));
}

#[test]
fn no_volume_given_is_full_volume() {
    let mut payload = crowd(json!({}));
    payload.as_object_mut().expect("an object").remove("volume");
    assert_eq!(prepared(payload).expect("prepared")["volume"], json!(1.0));
    let row = prepared(crowd(json!({ "volume": null }))).expect("prepared");
    assert_eq!(row["volume"], json!(1.0));
}

#[test]
fn a_volume_out_of_range_is_still_submitted_for_sound_volume_range_to_reject() {
    let row = prepared(crowd(json!({ "volume": 120 }))).expect("prepared");
    assert_eq!(row["volume"], json!(1.2));
    let row = prepared(crowd(json!({ "volume": -5 }))).expect("prepared");
    assert_eq!(row["volume"], json!(-0.05));
}

#[test]
fn a_volume_that_is_no_number_is_kept_as_given() {
    for volume in [json!("fort"), json!(true), json!("NaN")] {
        let row = prepared(crowd(json!({ "volume": volume.clone() }))).expect("prepared");
        assert_eq!(row["volume"], volume);
    }
}

#[test]
fn the_name_and_the_file_are_trimmed_and_the_campaign_lowercased() {
    let row = prepared(crowd(json!({
        "campaign": format!(" {} ", VALOMBRE.to_uppercase()),
        "name": "  Bruit de foule ",
        "file": " son.ogg ",
    })))
    .expect("prepared");
    assert_eq!(row["campaignId"], json!(VALOMBRE));
    assert_eq!(row["name"], json!("Bruit de foule"));
    assert_eq!(row["filePath"], json!("son.ogg"));
}

#[test]
fn only_the_sound_s_fields_are_written() {
    let row = prepared(crowd(json!({ "id": "another", "playing": true }))).expect("prepared");
    let fields: Vec<&str> = row.keys().map(String::as_str).collect();
    assert_eq!(
        fields,
        [
            "campaignId",
            "createdAt",
            "filePath",
            "id",
            "name",
            "tags",
            "volume"
        ]
    );
    assert_eq!(row["id"], json!(SOUND));
}

#[test]
fn a_sound_with_no_campaign_no_name_or_no_file_is_refused() {
    for (with, refusal) in [
        (json!({ "campaign": null }), Refusal::NoCampaign),
        (json!({ "campaign": "  " }), Refusal::NoCampaign),
        (json!({ "name": "" }), Refusal::NoName),
        (json!({ "name": "   " }), Refusal::NoName),
        (json!({ "name": 3 }), Refusal::NoName),
        (json!({ "file": "" }), Refusal::NoFile),
        (json!({ "file": null }), Refusal::NoFile),
    ] {
        assert_eq!(prepared(crowd(with.clone())), Err(refusal), "{with}");
    }
}

#[test]
fn each_refusal_says_why_in_french() {
    assert_eq!(
        Refusal::NoCampaign.to_string(),
        "cette campagne n'existe pas"
    );
    assert_eq!(Refusal::NoName.to_string(), "un son a besoin d'un nom");
    assert_eq!(
        Refusal::NoFile.to_string(),
        "un son a besoin de son fichier"
    );
}

#[test]
fn the_data_capability_is_named_with_or_without_its_version() {
    assert!(names("soundbox.addSound"));
    assert!(names("soundbox.addSound@1"));
    assert!(!names("soundbox.updateSound"));
    assert!(!names("soundbox.addSounds"));
}
