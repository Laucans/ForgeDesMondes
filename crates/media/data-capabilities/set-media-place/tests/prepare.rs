//! What a change of place becomes before it is submitted: the media and the
//! place the request names, read ([`asked`]), then judged against the rows
//! the store holds ([`prepare`]) — `media-place-is-lieu` first.

use serde_json::{Value, json};
use set_media_place::{Asked, MEDIA_PLACE_IS_LIEU, Refusal, Row, asked, names, prepare};

const VALOMBRE: &str = "c1000000-0000-4000-8000-000000000001";
const BRUME: &str = "c1000000-0000-4000-8000-000000000002";
const CARTE: &str = "a1000000-0000-4000-8000-000000000001";
const TAVERNE: &str = "e1000000-0000-4000-8000-000000000002";

fn row(value: Value) -> Row {
    let Value::Object(row) = value else {
        panic!("a row is an object");
    };
    row.into_iter().collect()
}

/// The media of Valombre, as the store holds it.
fn carte() -> Row {
    row(json!({ "campaignId": VALOMBRE, "kind": "battlemap", "title": "Le Gué" }))
}

/// An element of `campaign`, of `kind`.
fn element(kind: &str, campaign: &str) -> Row {
    row(json!({ "type": kind, "entityCampaignId": campaign, "name": "La Taverne" }))
}

fn attaching(place: &str) -> Asked {
    Asked {
        media: CARTE.to_string(),
        place: Some(place.to_string()),
    }
}

#[test]
fn the_id_names_it_with_or_without_its_version() {
    assert!(names("media.setMediaPlace"));
    assert!(names("media.setMediaPlace@1"));
    assert!(!names("media.saveGridSettings@1"));
    assert!(!names("media.setMediaPlaces"));
}

#[test]
fn the_media_is_the_target_or_the_payload_s_lowercase_and_so_is_the_place() {
    let upper = TAVERNE.to_uppercase();
    let wire = row(json!({ "place": upper }));
    assert_eq!(
        asked(&wire, Some(&CARTE.to_uppercase())),
        Ok(attaching(TAVERNE))
    );
    let wire = row(json!({ "media": CARTE, "place": TAVERNE }));
    assert_eq!(asked(&wire, None), Ok(attaching(TAVERNE)));
    // A blank target names nothing: the payload's media does.
    assert_eq!(asked(&wire, Some(" ")), Ok(attaching(TAVERNE)));
}

#[test]
fn a_media_named_by_something_other_than_a_uuid_is_no_media() {
    let wire = row(json!({ "place": TAVERNE }));
    assert_eq!(asked(&wire, Some("bm-gue")), Err(Refusal::UnknownMedia));
    let wire = row(json!({ "media": "bm-gue", "place": TAVERNE }));
    assert_eq!(asked(&wire, None), Err(Refusal::UnknownMedia));
}

#[test]
fn an_id_of_hex_digits_and_dashes_but_not_36_long_is_no_uuid() {
    // Right characters at the right places, wrong length.
    let short = &CARTE[..35];
    let long = format!("{CARTE}0");
    for id in [short, long.as_str(), "abc", "0"] {
        let wire = row(json!({ "media": id, "place": TAVERNE }));
        assert_eq!(asked(&wire, None), Err(Refusal::UnknownMedia), "{id}");
        let wire = row(json!({ "media": CARTE, "place": id }));
        assert_eq!(asked(&wire, None), Err(Refusal::InvalidPlace), "{id}");
    }
}

#[test]
fn a_null_place_detaches_the_media() {
    let wire = row(json!({ "media": CARTE, "place": null }));
    assert_eq!(
        asked(&wire, None),
        Ok(Asked {
            media: CARTE.to_string(),
            place: None
        })
    );
}

#[test]
fn a_request_naming_no_media_no_place_or_another_field_is_refused() {
    assert_eq!(
        asked(&row(json!({ "place": TAVERNE })), None),
        Err(Refusal::NoMedia)
    );
    assert_eq!(
        asked(&row(json!({ "media": " ", "place": TAVERNE })), None),
        Err(Refusal::NoMedia)
    );
    assert_eq!(
        asked(&row(json!({ "media": CARTE })), None),
        Err(Refusal::NoPlace)
    );
    assert_eq!(
        asked(
            &row(json!({ "media": CARTE, "place": TAVERNE, "title": "x" })),
            None
        ),
        Err(Refusal::Unknown("title".to_string()))
    );
    for place in [json!("la-taverne"), json!(3), json!({ "id": TAVERNE })] {
        assert_eq!(
            asked(&row(json!({ "media": CARTE, "place": place })), None),
            Err(Refusal::InvalidPlace),
            "{place}"
        );
    }
}

#[test]
fn a_lieu_of_the_media_s_campaign_is_written_as_its_place_and_nothing_else() {
    let prepared = prepare(
        &attaching(TAVERNE),
        Some(&carte()),
        Some(&element("lieu", VALOMBRE)),
    )
    .expect("a lieu of Valombre");
    assert_eq!(prepared.target_id, CARTE);
    assert_eq!(prepared.payload, row(json!({ "placeId": TAVERNE })));
}

#[test]
fn campaigns_are_told_apart_by_their_id_in_either_case() {
    let place = element("lieu", &VALOMBRE.to_uppercase());
    assert!(prepare(&attaching(TAVERNE), Some(&carte()), Some(&place)).is_ok());
}

#[test]
fn detaching_writes_a_null_place_and_needs_no_element() {
    let detaching = Asked {
        media: CARTE.to_string(),
        place: None,
    };
    let prepared = prepare(&detaching, Some(&carte()), None).expect("a media may have no place");
    assert_eq!(prepared.payload, row(json!({ "placeId": null })));
}

#[test]
fn a_place_that_is_no_lieu_of_the_media_s_campaign_breaks_media_place_is_lieu() {
    for (place, why) in [
        (Some(element("pnj", VALOMBRE)), "an NPC"),
        (Some(element("lieu", BRUME)), "a lieu of another campaign"),
        (
            Some(row(json!({ "entityCampaignId": VALOMBRE }))),
            "no type",
        ),
        (Some(row(json!({ "type": "lieu" }))), "no campaign"),
        (None, "no element"),
    ] {
        assert_eq!(
            prepare(&attaching(TAVERNE), Some(&carte()), place.as_ref()),
            Err(Refusal::NotALieu),
            "{why}"
        );
    }
    // A media whose campaign the row does not say has no lieu of its own.
    let nameless = row(json!({ "kind": "illustration" }));
    assert_eq!(
        prepare(
            &attaching(TAVERNE),
            Some(&nameless),
            Some(&element("lieu", VALOMBRE))
        ),
        Err(Refusal::NotALieu)
    );
    assert!(
        Refusal::NotALieu
            .to_string()
            .starts_with(MEDIA_PLACE_IS_LIEU)
    );
}

#[test]
fn a_media_the_store_does_not_hold_is_refused_whatever_the_place() {
    let detaching = Asked {
        media: CARTE.to_string(),
        place: None,
    };
    for media in [None, Some(Row::new())] {
        assert_eq!(
            prepare(&detaching, media.as_ref(), None),
            Err(Refusal::UnknownMedia)
        );
        assert_eq!(
            prepare(
                &attaching(TAVERNE),
                media.as_ref(),
                Some(&element("lieu", VALOMBRE))
            ),
            Err(Refusal::UnknownMedia)
        );
    }
}
