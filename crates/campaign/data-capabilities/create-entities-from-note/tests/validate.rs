//! A validation of a note's proposals made its commands: one element per
//! accepted proposal that has none and recognises none, saved as the DM's
//! editor saves one (`campaign.saveEntity`); every accepted proposal
//! checked, every other unchecked unless it created its element; and the
//! validations the tables could never hold refused.

use create_entities_from_note::{
    Batch, Fresh, Proposal, Refusal, Request, Row, SAVE_ENTITY, names, request,
};
use serde_json::{Value, json};

const VALOMBRE: &str = "c1000000-0000-4000-8000-000000000001";
const BRUMES: &str = "c1000000-0000-4000-8000-000000000002";
const NOTE: &str = "a1000000-0000-4000-8000-000000000001";

fn row(value: Value) -> Row {
    let Value::Object(fields) = value else {
        panic!("a row is an object");
    };
    fields.into_iter().collect()
}

/// A proposal of Valombre's note, unchecked by the analysis when it
/// recognises an element.
fn proposal(id: &str, kind: &str, name: &str, excerpt: &str) -> Proposal {
    Proposal {
        id: id.to_owned(),
        row: row(json!({
            "noteProposalId": id,
            "noteId": NOTE,
            "type": kind,
            "name": name,
            "excerpt": excerpt,
            "checked": true,
        })),
        campaign: Some(VALOMBRE.to_owned()),
        recognised: false,
    }
}

/// The note's proposals: an NPC, a PC, a place, an event.
fn note() -> Vec<Proposal> {
    vec![
        proposal("p1", "pnj", "Mère Ombeline", " l’herboriste aveugle "),
        proposal("p2", "pj", "Aerin Ventargent", "le frère d’Elowen"),
        proposal("p3", "lieu", "Le bois de Corbière", "à l’orée du bois"),
        proposal("p4", "evenement", "Le moulin a brûlé", ""),
    ]
}

fn fresh(n: usize) -> Vec<Fresh> {
    (1..=n)
        .map(|i| Fresh {
            entity: format!("e{i}"),
        })
        .collect()
}

fn validated(accepted: &[&str], proposals: &[Proposal]) -> Batch {
    let request = Request {
        campaign: VALOMBRE.to_owned(),
        accepted: accepted.iter().map(|id| (*id).to_owned()).collect(),
    };
    request
        .batch(proposals, fresh(accepted.len()))
        .expect("a batch")
}

/// The stamps, as (proposal, checked, created element).
fn stamps(batch: &Batch) -> Vec<(String, Value, Value)> {
    batch
        .stamps
        .iter()
        .map(|stamp| {
            assert_eq!(stamp.payload["noteId"], NOTE, "{stamp:?}");
            (
                stamp.target_id.clone(),
                stamp.payload["checked"].clone(),
                stamp
                    .payload
                    .get("createdEntityId")
                    .cloned()
                    .unwrap_or(Value::Null),
            )
        })
        .collect()
}

#[test]
fn names_knows_its_own_id_with_or_without_its_version() {
    assert!(names("campaign.createEntitiesFromNote"));
    assert!(names("campaign.createEntitiesFromNote@1"));
    assert!(!names("campaign.saveEntity"));
    assert!(!names("ai.extractFromNotes@1"));
}

#[test]
fn a_request_names_its_campaign_and_each_accepted_proposal_once_in_order() {
    let asked = request(&row(
        json!({ "campaign": VALOMBRE, "accepted": ["p2", "p1", "p2"] }),
    ))
    .expect("a request");
    assert_eq!(asked.campaign, VALOMBRE);
    assert_eq!(asked.accepted, ["p2", "p1"]);
    // A proposal's id in another case is the same one, in lowercase.
    let asked = request(&row(
        json!({ "campaign": VALOMBRE, "accepted": ["E1-AB", "e1-ab"] }),
    ))
    .expect("a request");
    assert_eq!(asked.accepted, ["e1-ab"]);
}

#[test]
fn a_request_of_no_campaign_or_no_proposal_is_refused() {
    for (wire, refusal) in [
        (json!({ "accepted": ["p1"] }), Refusal::NoCampaign),
        (
            json!({ "campaign": " ", "accepted": ["p1"] }),
            Refusal::NoCampaign,
        ),
        (
            json!({ "campaign": 7, "accepted": ["p1"] }),
            Refusal::NoCampaign,
        ),
        (json!({ "campaign": VALOMBRE }), Refusal::NothingAccepted),
        (
            json!({ "campaign": VALOMBRE, "accepted": "p1" }),
            Refusal::NothingAccepted,
        ),
        (
            json!({ "campaign": VALOMBRE, "accepted": [] }),
            Refusal::NothingAccepted,
        ),
        (
            json!({ "campaign": VALOMBRE, "accepted": ["p1", " "] }),
            Refusal::NothingAccepted,
        ),
        (
            json!({ "campaign": VALOMBRE, "accepted": ["p1", null] }),
            Refusal::NothingAccepted,
        ),
    ] {
        assert_eq!(request(&row(wire.clone())), Err(refusal), "{wire}");
    }
}

#[test]
fn a_refusal_says_why_in_french() {
    assert_eq!(
        Refusal::NoCampaign.to_string(),
        "une validation nomme sa campagne"
    );
    assert_eq!(
        Refusal::NothingAccepted.to_string(),
        "une validation nomme les propositions acceptées"
    );
    assert_eq!(
        Refusal::Unknown("p9".into()).to_string(),
        "aucune proposition p9 dans cette campagne"
    );
}

#[test]
fn each_accepted_proposal_becomes_one_element_and_records_it_the_others_unchecked() {
    let batch = validated(&["p3", "p1"], &note());
    let made: Vec<(&str, &str)> = batch
        .elements
        .iter()
        .map(|e| (e.proposal.as_str(), e.command.target_id.as_str()))
        .collect();
    assert_eq!(made, [("p3", "e1"), ("p1", "e2")]);
    assert_eq!(
        stamps(&batch),
        [
            ("p1".into(), json!(true), json!("e2")),
            ("p2".into(), json!(false), Value::Null),
            ("p3".into(), json!(true), json!("e1")),
            ("p4".into(), json!(false), Value::Null),
        ]
    );
}

#[test]
fn an_element_is_saved_as_the_dm_s_editor_saves_one_under_its_id() {
    let batch = validated(&["p1"], &note());
    let element = &batch.elements[0].command;
    assert_eq!(element.target_id, "e1");
    assert_eq!(
        element.payload,
        row(json!({
            "campaign": VALOMBRE,
            "entity": {
                "id": "e1",
                "kind": "pnj",
                "name": "Mère Ombeline",
                "description": "l’herboriste aveugle",
            },
        }))
    );
    assert_eq!(SAVE_ENTITY, "campaign.saveEntity");
}

#[test]
fn a_pc_a_place_and_an_event_are_saved_of_their_type() {
    let batch = validated(&["p2", "p3", "p4"], &note());
    let kinds: Vec<&Value> = batch
        .elements
        .iter()
        .map(|element| &element.command.payload["entity"]["kind"])
        .collect();
    assert_eq!(kinds, ["pj", "lieu", "evenement"]);
}

#[test]
fn an_element_of_a_blank_excerpt_has_no_description() {
    let batch = validated(&["p4"], &note());
    let payload = &batch.elements[0].command.payload;
    assert_eq!(
        payload["entity"],
        json!({ "id": "e1", "kind": "evenement", "name": "Le moulin a brûlé" })
    );
}

#[test]
fn an_accepted_proposal_that_created_its_element_or_recognises_one_makes_none() {
    let mut proposals = note();
    proposals[0]
        .row
        .insert("createdEntityId".into(), json!("e-old"));
    proposals[2].recognised = true;
    let batch = validated(&["p1", "p3", "p2"], &proposals);
    let made: Vec<&str> = batch.elements.iter().map(|e| e.proposal.as_str()).collect();
    assert_eq!(made, ["p2"]);
    // The pair of ids of p2, the third accepted.
    assert_eq!(batch.elements[0].command.target_id, "e3");
    assert_eq!(
        stamps(&batch),
        [
            // Checked again, its element kept.
            ("p1".into(), json!(true), Value::Null),
            ("p2".into(), json!(true), json!("e3")),
            ("p3".into(), json!(true), Value::Null),
            ("p4".into(), json!(false), Value::Null),
        ]
    );
}

#[test]
fn a_proposal_left_out_that_created_its_element_or_is_unchecked_already_is_not_written() {
    let mut proposals = note();
    proposals[1]
        .row
        .insert("createdEntityId".into(), json!("e-old"));
    proposals[2].row.insert("checked".into(), json!(false));
    let batch = validated(&["p1"], &proposals);
    assert_eq!(
        stamps(&batch),
        [
            ("p1".into(), json!(true), json!("e1")),
            ("p4".into(), json!(false), Value::Null),
        ]
    );
    // A null element is none.
    let mut proposals = note();
    proposals[1]
        .row
        .insert("createdEntityId".into(), Value::Null);
    let batch = validated(&["p1"], &proposals);
    assert!(
        stamps(&batch)
            .iter()
            .any(|(id, checked, _)| id == "p2" && checked == &json!(false))
    );
}

#[test]
fn an_element_is_of_the_campaign_its_note_is_of_whatever_the_case_given() {
    let request = Request {
        campaign: VALOMBRE.to_uppercase(),
        accepted: vec!["p1".to_owned()],
    };
    let batch = request.batch(&note(), fresh(1)).expect("a batch");
    assert_eq!(batch.elements[0].command.payload["campaign"], VALOMBRE);
}

#[test]
fn a_proposal_of_another_campaign_or_of_no_note_is_unknown_and_never_unchecked() {
    let mut proposals = note();
    proposals[0].campaign = Some(BRUMES.to_owned());
    proposals[1].campaign = None;
    proposals[2].row.remove("noteId");
    proposals.push(Proposal {
        row: row(json!({ "noteId": NOTE, "type": "dragon", "name": "Smaug" })),
        ..proposal("p5", "pnj", "x", "")
    });
    proposals.push(Proposal {
        row: row(json!({ "noteId": NOTE, "type": "pnj", "name": "  " })),
        ..proposal("p6", "pnj", "x", "")
    });
    for unknown in ["p1", "p2", "p3", "p5", "p6", "p9"] {
        let request = Request {
            campaign: VALOMBRE.to_owned(),
            accepted: vec!["p4".to_owned(), unknown.to_owned()],
        };
        assert_eq!(
            request.batch(&proposals, fresh(2)),
            Err(Refusal::Unknown(unknown.to_owned()))
        );
    }
    // Accepting the event alone unchecks nothing that is not of the campaign.
    assert_eq!(validated(&["p4"], &proposals).stamps.len(), 1);
}

#[test]
#[should_panic(expected = "one id per accepted proposal")]
fn a_batch_wants_one_id_per_accepted_proposal() {
    let request = Request {
        campaign: VALOMBRE.to_owned(),
        accepted: vec!["p1".to_owned(), "p2".to_owned()],
    };
    let _ = request.batch(&note(), fresh(1));
}

#[test]
fn an_element_is_given_the_same_uuid_every_time_from_its_proposal() {
    let fresh = Fresh::of("p1");
    assert_eq!(fresh, Fresh::of("p1"));
    assert_ne!(fresh, Fresh::of("p2"));
    // Derived from this DataCapability, what it names and the proposal.
    assert_eq!(fresh.entity, "27be6454-e5d3-5651-b5d2-4fd54b17cee8");
    let id = &fresh.entity;
    let parts: Vec<usize> = id.split('-').map(str::len).collect();
    assert_eq!(parts, [8, 4, 4, 4, 12], "{id}");
    assert!(
        id.chars().all(|c| c == '-' || c.is_ascii_hexdigit()),
        "{id}"
    );
    // Version 5, variant 10.
    assert_eq!(id.as_bytes()[14], b'5', "{id}");
    assert!(
        matches!(id.as_bytes()[19], b'8' | b'9' | b'a' | b'b'),
        "{id}"
    );
}
