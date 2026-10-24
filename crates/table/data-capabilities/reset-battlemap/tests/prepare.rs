//! What table.resetBattlemap reads of a submission and of the instance's
//! listing, and the deletions it makes of them: entries, then combats, then
//! tokens, the first the command, the others its follow-ups, each stamped
//! with the instance and when the reset was asked (`state-persisted-reset`).

use harness_dataguard::api::Submission;
use reset_battlemap::{
    DATA_CAPABILITY, Listing, Refusal, Request, Row, deletions, holds, listing, partition, request,
    reset, stamp,
};
use serde_json::{Value, json};

const INSTANCE: &str = "b1000000-0000-4000-8000-000000000001";
const AT: &str = "2026-10-21T11:00:00.000Z";

fn row(value: Value) -> Row {
    let Value::Object(map) = value else {
        unreachable!("a row is an object");
    };
    map.into_iter().collect()
}

fn ids(prefix: &str, count: u8) -> Vec<String> {
    (1..=count)
        .map(|n| format!("{prefix}000000-0000-4000-8000-00000000000{n}"))
        .collect()
}

fn map(entries: u8, combats: u8, tokens: u8) -> Listing {
    Listing {
        instance: true,
        entries: ids("ee", entries),
        combats: ids("c0", combats),
        tokens: ids("70", tokens),
        under_way: false,
        now: Some(AT.to_owned()),
    }
}

#[test]
fn the_instance_is_the_target_or_named_in_the_payload_in_any_case() {
    assert_eq!(
        request(&Row::new(), Some(&format!(" {} ", INSTANCE.to_uppercase()))),
        Ok(Request::Reset {
            instance: INSTANCE.to_owned()
        })
    );
    assert_eq!(
        request(&row(json!({ "instance": INSTANCE, "of": null })), None),
        Ok(Request::Reset {
            instance: INSTANCE.to_owned()
        })
    );
    // The target wins over the payload.
    assert_eq!(
        request(&row(json!({ "instance": "pas-un-id" })), Some(INSTANCE)),
        Ok(Request::Reset {
            instance: INSTANCE.to_owned()
        })
    );
    for (wire, target) in [
        (json!({}), None),
        (json!({ "instance": 3 }), None),
        (json!({}), Some("pas-un-id")),
    ] {
        assert_eq!(
            request(&row(wire), target),
            Err(Refusal::NotAnId("instance"))
        );
    }
}

#[test]
fn a_follow_up_names_its_row_as_target_and_the_instance_under_of() {
    let token = ids("70", 1).remove(0);
    assert_eq!(
        request(
            &row(json!({ "of": INSTANCE.to_uppercase(), "resetAt": format!(" {AT} ") })),
            Some(&token)
        ),
        Ok(Request::Delete {
            of: INSTANCE.to_owned(),
            row: token.clone(),
            reset_at: AT.to_owned(),
        })
    );
    // A deletion carrying no stamp is no reset's.
    for reset_at in [json!(""), json!(" "), json!(12), Value::Null] {
        assert_eq!(
            request(
                &row(json!({ "of": INSTANCE, "resetAt": reset_at })),
                Some(&token)
            ),
            Err(Refusal::NotOfAReset)
        );
    }
    assert_eq!(
        request(&row(json!({ "of": 3 })), Some(&token)),
        Err(Refusal::NotAnId("of"))
    );
    assert_eq!(
        request(&row(json!({ "of": INSTANCE })), None),
        Err(Refusal::NotAnId("row"))
    );
}

#[test]
fn the_listing_reads_ids_lowercase_and_drops_what_is_no_uuid() {
    let read = listing(&json!([{
        "instance": true,
        "entries": [INSTANCE.to_uppercase(), "pas-un-id", 3],
        "combats": [],
        "tokens": null,
        "under_way": true,
        "now": AT,
    }]));
    assert_eq!(
        read,
        Listing {
            instance: true,
            entries: vec![INSTANCE.to_owned()],
            combats: Vec::new(),
            tokens: Vec::new(),
            under_way: true,
            now: Some(AT.to_owned()),
        }
    );
    assert_eq!(listing(&json!([])), Listing::default());
}

#[test]
fn entries_go_first_then_combats_then_tokens_each_once() {
    let mut held = map(2, 1, 2);
    held.tokens.push(held.entries[0].clone());
    let order = deletions(&held);
    assert_eq!(
        order,
        [ids("ee", 2), ids("c0", 1), ids("70", 2)].concat(),
        "an id listed twice is deleted once"
    );
    assert!(holds(&held, &ids("c0", 1)[0]));
    assert!(holds(&held, &ids("70", 2)[1]));
    assert!(!holds(&held, INSTANCE));
}

#[test]
fn the_first_deletion_is_the_command_and_the_others_its_stamped_follow_ups() {
    let prepared = reset(INSTANCE, &map(1, 1, 2), AT).expect("a reset");
    let first = ids("ee", 1).remove(0);
    assert_eq!(prepared.target_id, Some(first));
    assert_eq!(prepared.payload, stamp(INSTANCE, AT));
    assert_eq!(
        prepared.payload,
        row(json!({ "of": INSTANCE, "resetAt": AT }))
    );
    let expected: Vec<Submission> = [ids("c0", 1), ids("70", 2)]
        .concat()
        .into_iter()
        .map(|id| Submission {
            data_capability: DATA_CAPABILITY.to_owned(),
            payload: stamp(INSTANCE, AT),
            target_id: Some(id),
            ..Submission::default()
        })
        .collect();
    assert_eq!(prepared.then, expected);
    assert!(prepared.warnings.is_empty() && prepared.attachment.is_none());
    // A map of one row is one command.
    let lone = reset(INSTANCE, &map(0, 0, 1), AT).expect("a reset");
    assert!(lone.then.is_empty());
}

#[test]
fn state_persisted_reset_an_empty_map_has_nothing_to_reset() {
    assert_eq!(
        reset(INSTANCE, &map(0, 0, 0), AT),
        Err(Refusal::NothingToReset)
    );
}

#[test]
fn what_is_not_there_is_not_found_the_rest_unprocessable() {
    for (refusal, found) in [
        (Refusal::UnknownInstance, true),
        (Refusal::NotOfTheInstance, true),
        (Refusal::NotAnId("instance"), false),
        (Refusal::NothingToReset, false),
        (Refusal::NotOfAReset, false),
        (Refusal::Unlisted, false),
    ] {
        assert_eq!(refusal.not_found(), found, "{refusal:?}");
        assert!(!refusal.to_string().is_empty());
    }
    assert!(
        Refusal::NothingToReset
            .to_string()
            .starts_with("state-persisted-reset")
    );
    assert!(
        Refusal::NotOfAReset
            .to_string()
            .starts_with("state-persisted-reset")
    );
    assert!(
        Refusal::NotOfTheInstance
            .to_string()
            .starts_with("instance-isolated")
    );
    assert_eq!(partition(INSTANCE), format!("BattlemapInstance/{INSTANCE}"));
}
