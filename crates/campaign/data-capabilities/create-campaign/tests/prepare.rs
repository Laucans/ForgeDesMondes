//! campaign.createCampaign's preparation: the wire's `{name, type, pitch?,
//! art?, glyph?}` made the campaign the command writes — and a oneshot's
//! single session.

use create_campaign::{
    CAMPAGNE, Fresh, MASTER_VOLUME, MICROPHONE_ON, ONESHOT, Refusal, Row, SESSION_POSITION,
    SESSION_STATUS, prepare, timestamp,
};
use serde_json::{Value, json};

const CAMPAIGN: &str = "C1000000-0000-4000-8000-000000000001";
const SESSION: &str = "D1000000-0000-4000-8000-000000000001";
const NOW: &str = "2026-10-20T09:30:00.123Z";

const FRESH: Fresh<'static> = Fresh {
    campaign: CAMPAIGN,
    session: SESSION,
    now: NOW,
};

fn wire(payload: Value) -> Row {
    let Value::Object(payload) = payload else {
        panic!("a payload is an object");
    };
    payload.into_iter().collect()
}

#[test]
fn a_campagne_is_written_whole_under_its_new_id_with_no_session() {
    let prepared = prepare(
        &wire(json!({
            "name": "  Les Brumes de Corbière ",
            "type": "campagne",
            "pitch": "Un port noyé de brume.",
            "art": "forest",
            "glyph": "B",
        })),
        FRESH,
    )
    .expect("prepared");
    let id = CAMPAIGN.to_ascii_lowercase();
    assert_eq!(prepared.target_id, id);
    assert_eq!(
        prepared.payload,
        wire(json!({
            "id": id,
            "name": "Les Brumes de Corbière",
            "kind": CAMPAGNE,
            "pitch": "Un port noyé de brume.",
            "art": "forest",
            "glyph": "B",
            "createdAt": NOW,
            "soundMasterVolume": MASTER_VOLUME,
        }))
    );
}

#[test]
fn oneshot_single_session_a_oneshot_carries_its_one_session_in_prep_at_position_1_microphone_off() {
    let prepared =
        prepare(&wire(json!({ "name": "Le Bal", "type": "oneshot" })), FRESH).expect("prepared");
    let id = CAMPAIGN.to_ascii_lowercase();
    assert_eq!(
        prepared.payload,
        wire(json!({
            "id": id,
            "name": "Le Bal",
            "kind": ONESHOT,
            "pitch": null,
            "art": null,
            "glyph": null,
            "createdAt": NOW,
            "soundMasterVolume": MASTER_VOLUME,
            "sessionId": SESSION.to_ascii_lowercase(),
            "sessionCampaignId": id,
            "sessionPosition": 1,
            "sessionTitle": "Le Bal",
            "sessionStatus": "prep",
            "microphoneOn": false,
        }))
    );
    assert_eq!(
        (SESSION_POSITION, SESSION_STATUS, MICROPHONE_ON),
        (1, "prep", false)
    );
}

#[test]
fn master_volume_range_a_new_campaign_starts_at_full_volume_inside_0_to_1() {
    assert!((0.0..=1.0).contains(&MASTER_VOLUME));
    assert!((MASTER_VOLUME - 1.0).abs() < f64::EPSILON);
    for kind in [CAMPAGNE, ONESHOT] {
        let prepared =
            prepare(&wire(json!({ "name": "Le Bal", "type": kind })), FRESH).expect("prepared");
        assert_eq!(prepared.payload["soundMasterVolume"], json!(1.0), "{kind}");
    }
}

#[test]
fn a_volume_the_wire_names_is_not_the_dm_s_to_set_at_creation() {
    let prepared = prepare(
        &wire(json!({ "name": "Le Bal", "type": "campagne", "soundMasterVolume": 7 })),
        FRESH,
    )
    .expect("prepared");
    assert_eq!(prepared.payload["soundMasterVolume"], json!(1.0));
}

#[test]
fn a_pitch_art_or_glyph_that_is_no_text_is_null() {
    let prepared = prepare(
        &wire(
            json!({ "name": "Le Bal", "type": "campagne", "pitch": 3, "art": {}, "glyph": null }),
        ),
        FRESH,
    )
    .expect("prepared");
    for field in ["pitch", "art", "glyph"] {
        assert_eq!(prepared.payload[field], Value::Null, "{field}");
    }
}

#[test]
fn a_campaign_with_no_name_is_refused() {
    for payload in [
        json!({ "type": "campagne" }),
        json!({ "name": "   ", "type": "campagne" }),
        json!({ "name": 4, "type": "oneshot" }),
    ] {
        assert_eq!(
            prepare(&wire(payload.clone()), FRESH),
            Err(Refusal::NoName),
            "{payload}"
        );
    }
}

#[test]
fn a_campaign_of_a_kind_there_is_not_is_refused() {
    for payload in [
        json!({ "name": "Le Bal" }),
        json!({ "name": "Le Bal", "type": "Oneshot" }),
        json!({ "name": "Le Bal", "type": "saga" }),
        json!({ "name": "Le Bal", "kind": "oneshot" }),
    ] {
        assert_eq!(
            prepare(&wire(payload.clone()), FRESH),
            Err(Refusal::UnknownType),
            "{payload}"
        );
    }
}

#[test]
fn a_refusal_says_why_in_the_dm_s_words() {
    assert_eq!(Refusal::NoName.to_string(), "une campagne a un nom");
    assert!(Refusal::UnknownType.to_string().contains("oneshot"));
}

#[test]
fn a_creation_s_time_is_rfc_3339_in_utc_to_the_millisecond() {
    assert_eq!(timestamp(0), "1970-01-01T00:00:00.000Z");
    // 2026-10-21T09:00:00.123Z; 2024-02-29, a leap day; 2000-03-01, past a
    // century's leap day.
    assert_eq!(timestamp(1_792_573_200_123), "2026-10-21T09:00:00.123Z");
    assert_eq!(timestamp(1_709_251_199_999), "2024-02-29T23:59:59.999Z");
    assert_eq!(timestamp(951_868_800_007), "2000-03-01T00:00:00.007Z");
    // 2100 is no leap year, 2400 is one: the calendar's century rules.
    assert_eq!(timestamp(4_107_542_399_999), "2100-02-28T23:59:59.999Z");
    assert_eq!(timestamp(4_107_542_400_000), "2100-03-01T00:00:00.000Z");
    assert_eq!(timestamp(13_574_606_400_000), "2400-02-29T12:00:00.000Z");
    assert_eq!(timestamp(13_574_649_600_001), "2400-03-01T00:00:00.001Z");
}
