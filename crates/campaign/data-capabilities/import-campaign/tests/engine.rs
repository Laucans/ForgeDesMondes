//! `campaign.importCampaign` as the server plants it beside Campaign, over the
//! in-memory store (`platform_test_kit::Bench`): an archive imports as a new
//! campaign whatever campaign it was made from, the original's row is never
//! touched, the same archive twice is two copies, and an archive that is none
//! is refused 422 before anything is queued.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use harness_dataguard::command::State;
use harness_dataguard::plugins::RefusalKind;
use harness_dataguard::store::Ids;
use platform_test_kit::{Bench, NOW, Served, file, planter, refused, uuid};
use serde_json::{Value, json};

/// The id of the campaign the archive was made from — which exists.
const ORIGINAL: &str = "c0000001-0000-4000-8000-000000000001";

/// Row ids as the product draws them, uuids, none the archive's.
struct Draw(AtomicU64);

impl Ids for Draw {
    fn fresh(&self) -> String {
        uuid(0xd0, self.0.fetch_add(1, Ordering::SeqCst) + 1)
    }
}

/// The engine as the server plants this unit and its aggregate, the original
/// campaign planted.
async fn served() -> Served {
    let served = Bench::new(
        include_str!("../data-capability.json"),
        &file("crates/campaign/aggregates/Campaign/aggregate.json"),
        [aggregate_campaign::plugin(), import_campaign::plugin()],
    )
    .planting(&planter("test.plantCampaign", "Campaign", "$id", "insert"))
    .ids(Arc::new(Draw(AtomicU64::new(0))))
    .memory()
    .await;
    served
        .plant(
            "test.plantCampaign",
            [json!({
                "id": ORIGINAL, "name": "Les cendres de Valombre", "kind": "campagne",
                "pitch": "Une vallée qui brûle encore.",
                "lastExportedAt": "2026-09-01T18:00:00+00:00", "soundMasterVolume": 0.8
            })],
        )
        .await;
    served
}

/// The archive of the Concept's full case: made from [`ORIGINAL`].
fn archive() -> Value {
    platform_conformance::fixtures_of("CampaignArchive@1")
        .cases
        .into_iter()
        .find(|case| case.name.starts_with("a full archive"))
        .expect("the full case")
        .input["archive"]
        .clone()
}

/// An import queued then settled: the partition it landed on, and what it
/// became.
async fn imported(served: &Served, archive: Value) -> (String, State) {
    let submitted = served
        .submit(
            "campaign.importCampaign",
            json!({ "archive": archive }),
            None,
        )
        .await
        .expect("queued");
    let mut settled = served.settled().await;
    assert_eq!(settled.len(), 1, "one command settled: {settled:?}");
    (submitted.partition, settled.remove(0).status)
}

#[tokio::test]
async fn the_unit_is_planted_beside_its_aggregate() {
    let served = served().await;
    assert_eq!(served.settled().await.len(), 0, "nothing queued yet");
}

#[tokio::test]
async fn import_creates_copy_an_archive_imports_as_a_new_campaign_though_the_original_exists() {
    let served = served().await;
    let before = served.row(&format!("Campaign/{ORIGINAL}"));
    assert!(!before.is_empty(), "the original is there");

    let (partition, status) = imported(&served, archive()).await;
    assert_eq!(status, State::Applied);

    assert_ne!(partition, format!("Campaign/{ORIGINAL}"));
    let copy = served.row(&partition);
    assert_eq!(copy["id"], json!(partition.trim_start_matches("Campaign/")));
    assert_eq!(copy["name"], json!("Les cendres de Valombre"));
    assert_eq!(copy["originCampaignId"], json!(ORIGINAL));
    assert_eq!(
        copy["importedAt"],
        json!(import_campaign::timestamp(NOW * 1000))
    );
    assert!(
        !copy.contains_key("lastExportedAt"),
        "a copy has never been exported"
    );
    for section in ["players", "elements", "media", "sounds", "sessions"] {
        assert!(
            copy[section]
                .as_array()
                .is_some_and(|rows| !rows.is_empty()),
            "{section}"
        );
    }
    assert_eq!(
        served.row(&format!("Campaign/{ORIGINAL}")),
        before,
        "the original is never changed"
    );
}

#[tokio::test]
async fn import_creates_copy_the_same_archive_twice_makes_two_copies() {
    let served = served().await;
    let (first, status) = imported(&served, archive()).await;
    assert_eq!(status, State::Applied);
    let (second, status) = imported(&served, archive()).await;
    assert_eq!(status, State::Applied);
    assert_ne!(first, second, "two separate copies");
    let (first, second) = (served.row(&first), served.row(&second));
    assert_eq!(first["name"], second["name"]);
    assert_ne!(first["id"], second["id"]);
    assert_eq!(first["originCampaignId"], second["originCampaignId"]);
    let held = archive_ids();
    for copy in [&first, &second] {
        assert!(!held.contains(&copy["id"].as_str().expect("an id").to_owned()));
    }
}

/// Every id the archive holds.
fn archive_ids() -> Vec<String> {
    let archive = archive();
    let mut ids = Vec::new();
    let mut stack = vec![&archive];
    while let Some(value) = stack.pop() {
        match value {
            Value::Object(members) => {
                if let Some(id) = members.get("id").and_then(Value::as_str) {
                    ids.push(id.to_owned());
                }
                stack.extend(members.values());
            }
            Value::Array(rows) => stack.extend(rows),
            _ => {}
        }
    }
    ids
}

#[tokio::test]
async fn an_archive_of_another_format_or_version_is_refused_before_anything_is_queued() {
    let served = served().await;
    for (member, value) in [("format", json!("campaign-export")), ("version", json!(2))] {
        let mut wrong = archive();
        wrong[member] = value;
        let message = refused(
            served
                .submit("campaign.importCampaign", json!({ "archive": wrong }), None)
                .await,
            RefusalKind::Unprocessable,
        );
        assert!(message.contains(member), "{member}: {message}");
    }
    assert!(served.settled().await.is_empty(), "nothing was queued");
    assert!(
        served
            .row(&format!("Campaign/{}", uuid(0xd0, 1)))
            .is_empty(),
        "no row was written"
    );
}

#[tokio::test]
async fn a_payload_with_no_archive_is_refused_and_so_is_a_command_naming_its_campaign() {
    let served = served().await;
    let none = refused(
        served
            .submit("campaign.importCampaign", json!({}), None)
            .await,
        RefusalKind::Unprocessable,
    );
    assert!(none.contains("archive"), "{none}");
    let named = refused(
        served
            .submit(
                "campaign.importCampaign",
                json!({ "archive": archive() }),
                Some(ORIGINAL),
            )
            .await,
        RefusalKind::Unprocessable,
    );
    assert!(named.contains("nouvel identifiant"), "{named}");
    assert!(served.settled().await.is_empty(), "nothing was queued");
}

#[tokio::test]
async fn import_creates_copy_an_id_drawn_that_is_the_originals_is_refused() {
    // The engine would hand out the original's own id: no copy is written
    // over it.
    struct Original;
    impl Ids for Original {
        fn fresh(&self) -> String {
            ORIGINAL.to_owned()
        }
    }
    let served = Bench::new(
        include_str!("../data-capability.json"),
        &file("crates/campaign/aggregates/Campaign/aggregate.json"),
        [aggregate_campaign::plugin(), import_campaign::plugin()],
    )
    .ids(Arc::new(Original))
    .memory()
    .await;
    let message = refused(
        served
            .submit(
                "campaign.importCampaign",
                json!({ "archive": archive() }),
                None,
            )
            .await,
        RefusalKind::Unprocessable,
    );
    assert!(message.contains("nouveaux"), "{message}");
    assert!(served.settled().await.is_empty(), "nothing was queued");
}
