//! What an addition of a sound becomes before it is submitted
//! (`add_sound::prepare`): the row it writes on its new partition — its
//! tags normalised, its volume a fraction of 1 — or why it is refused.

use add_sound::{Fresh, Refusal, Row, campaign_id, prepare, stamp};
use serde_json::{Value, json};

const VALOMBRE: &str = "c1000000-0000-4000-8000-000000000001";
const SOUND: &str = "50000000-0000-4000-8000-000000000001";
const NOW: &str = "2026-10-22T20:00:00.000Z";

fn wire(value: Value) -> Row {
    serde_json::from_value(value).expect("a JSON object")
}

fn fresh() -> Fresh<'static> {
    Fresh {
        sound: SOUND,
        now: NOW,
    }
}

/// The row `payload` is prepared into.
fn prepared(payload: Value) -> Row {
    prepare(&wire(payload), fresh()).expect("prepared")
}

/// A crowd in a tavern, with `field` set to `value`.
fn crowd_with(field: &str, value: Value) -> Value {
    let mut payload = json!({
        "campaign": VALOMBRE,
        "name": "Bruit de foule",
        "tags": ["Taverne"],
        "volume": 55,
        "file": "son.ogg",
    });
    payload[field] = value;
    payload
}

#[test]
fn a_sound_is_written_whole_on_its_own_partition() {
    let row = prepared(crowd_with("name", json!("Bruit de foule")));
    assert_eq!(
        Value::Object(row.into_iter().collect()),
        json!({
            "id": SOUND,
            "campaignId": VALOMBRE,
            "name": "Bruit de foule",
            "filePath": "son.ogg",
            "tags": ["taverne"],
            "volume": 0.55,
            "createdAt": NOW,
        })
    );
}

#[test]
fn tags_normalized_one_tag_whatever_its_case_and_spaces() {
    let row = prepared(crowd_with("tags", json!(["Bois", "bois "])));
    assert_eq!(row["tags"], json!(["bois"]));
    let row = prepared(crowd_with("tags", json!(["  Foule", "", "PORTE", "foule"])));
    assert_eq!(row["tags"], json!(["foule", "porte"]));
}

#[test]
fn no_tags_given_is_no_tag() {
    let mut payload = crowd_with("tags", Value::Null);
    assert_eq!(prepared(payload.clone())["tags"], json!([]));
    payload.as_object_mut().expect("an object").remove("tags");
    assert_eq!(prepared(payload)["tags"], json!([]));
}

#[test]
fn tags_that_are_no_list_of_texts_are_kept_for_the_invariant_to_reject() {
    for tags in [json!("Taverne"), json!(["Taverne", 3]), json!({"a": 1})] {
        assert_eq!(prepared(crowd_with("tags", tags.clone()))["tags"], tags);
    }
}

#[test]
fn sound_volume_range_a_percent_becomes_a_fraction_of_one() {
    for (percent, fraction) in [
        (json!(55), json!(0.55)),
        (json!(0), json!(0.0)),
        (json!(100), json!(1.0)),
        (json!("80"), json!(0.8)),
        (json!(" 30 "), json!(0.3)),
        // Kept to the whole percent, the hundredth `sound.volume` holds.
        (json!(55.4), json!(0.55)),
        (json!(55.6), json!(0.56)),
    ] {
        assert_eq!(
            prepared(crowd_with("volume", percent.clone()))["volume"],
            fraction,
            "{percent}"
        );
    }
}

#[test]
fn no_volume_given_is_full_volume() {
    let mut payload = crowd_with("volume", Value::Null);
    assert_eq!(prepared(payload.clone())["volume"], json!(1.0));
    payload.as_object_mut().expect("an object").remove("volume");
    assert_eq!(prepared(payload)["volume"], json!(1.0));
}

#[test]
fn sound_volume_range_a_volume_out_of_range_is_kept_unrounded_for_the_invariant() {
    for (percent, kept) in [
        (json!(120), json!(1.2)),
        (json!(100.4), json!(1.004)),
        (json!(-0.4), json!(-0.004)),
        (json!(-5), json!(-0.05)),
        (json!("fort"), json!("fort")),
        (json!("inf"), json!("inf")),
        (json!(true), json!(true)),
    ] {
        assert_eq!(
            prepared(crowd_with("volume", percent.clone()))["volume"],
            kept,
            "{percent}"
        );
    }
}

#[test]
fn the_name_and_the_file_are_trimmed() {
    let row = prepared(json!({
        "campaign": VALOMBRE, "name": "  Pluie ", "file": " sons/pluie.ogg\t",
    }));
    assert_eq!(row["name"], json!("Pluie"));
    assert_eq!(row["filePath"], json!("sons/pluie.ogg"));
}

#[test]
fn the_campaign_is_named_as_the_table_writes_its_ids() {
    let upper = VALOMBRE.to_uppercase();
    assert_eq!(
        campaign_id(&wire(json!({ "campaign": format!(" {upper} ") }))),
        Some(VALOMBRE.to_owned())
    );
    for none in [json!({}), json!({"campaign": "  "}), json!({"campaign": 7})] {
        assert_eq!(campaign_id(&wire(none.clone())), None, "{none}");
    }
}

#[test]
fn what_the_table_could_never_hold_is_refused() {
    for (payload, refusal) in [
        (crowd_with("campaign", Value::Null), Refusal::NoCampaign),
        (crowd_with("campaign", json!(" ")), Refusal::NoCampaign),
        (crowd_with("name", json!("  ")), Refusal::NoName),
        (crowd_with("name", json!(3)), Refusal::NoName),
        (crowd_with("file", json!("")), Refusal::NoFile),
        (crowd_with("file", Value::Null), Refusal::NoFile),
        (
            crowd_with("campaign", json!("c1\0")),
            Refusal::Nul("campaign"),
        ),
        (crowd_with("name", json!("Pl\0uie")), Refusal::Nul("name")),
        (crowd_with("file", json!("son\0.ogg")), Refusal::Nul("file")),
        (
            crowd_with("tags", json!(["ok", "ta\0verne"])),
            Refusal::Nul("tags"),
        ),
    ] {
        assert_eq!(
            prepare(&wire(payload.clone()), fresh()),
            Err(refusal),
            "{payload}"
        );
    }
}

#[test]
fn a_refusal_says_why_in_the_dm_s_words() {
    for (refusal, says) in [
        (Refusal::NoCampaign, "un son nomme sa campagne"),
        (Refusal::UnknownCampaign, "cette campagne n'existe pas"),
        (Refusal::NoName, "un son a besoin d'un nom"),
        (Refusal::NoFile, "un son a besoin de son fichier"),
        (
            Refusal::Nul("name"),
            "le champ name contient un caractère nul",
        ),
    ] {
        assert_eq!(refusal.to_string(), says);
    }
}

#[test]
fn now_is_rfc_3339_in_utc_to_the_millisecond() {
    for (millis, text) in [
        (0, "1970-01-01T00:00:00.000Z"),
        (951_782_400_001, "2000-02-29T00:00:00.001Z"),
        (1_792_699_200_123, "2026-10-22T20:00:00.123Z"),
        (1_798_761_599_999, "2026-12-31T23:59:59.999Z"),
        (4_102_444_799_999, "2099-12-31T23:59:59.999Z"),
        // A century that is no leap year, a fourth one that is.
        (4_107_542_399_000, "2100-02-28T23:59:59.000Z"),
        (4_107_542_400_000, "2100-03-01T00:00:00.000Z"),
        (951_868_799_000, "2000-02-29T23:59:59.000Z"),
        (13_574_563_200_000, "2400-02-29T00:00:00.000Z"),
    ] {
        assert_eq!(stamp(millis), text, "{millis}");
    }
}
