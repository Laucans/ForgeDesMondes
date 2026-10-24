//! dndbeyond.importCharacter through the engine, over its in-memory store,
//! as the server plants it with the Entity aggregate and the character-sheets
//! port's fake: an import is fetched and made before anything is queued,
//! judged and applied on the PC's partition with the version of source
//! `dndbeyond` it adds; importing the same character again updates that PC.
//! What D&D Beyond does not give, or a URL that is not its own, is refused
//! and nothing is queued. The tables it reaches are `tests/postgres.rs`.

use std::sync::Arc;

use harness_dataguard::Error;
use harness_dataguard::aggregate::Aggregate;
use harness_dataguard::api::Submission;
use harness_dataguard::command::{CommandResult, State};
use harness_dataguard::engine::Engine;
use harness_dataguard::manifest::DataCapability;
use harness_dataguard::memory::{FixedClock, MemoryNotifier, MemoryStore};
use harness_dataguard::plugins::{RefusalKind, Shared};
use harness_dataguard::store::{RandomIds, Store};
use ports_character_sheets::fake::FakeCharacterSheets;
use ports_character_sheets::{
    CharacterId, CharacterSheet, CharacterSheets, CharacterSheetsError, SharedCharacterSheets,
};
use serde_json::{Map, Value, json};

mod support;
use support::{BRANNOC, ELOWEN, elowen, elowen_url, id, sheet};

const VALOMBRE: &str = "c1000000-0000-4000-8000-000000000001";
const JULIE: &str = "a0000000-0000-4000-8000-000000000001";

/// Where the test's clock starts, in milliseconds since the epoch.
const NOW_MILLIS: u64 = 1_792_656_000_123;

/// What became of a submission: queued, or refused before it was — as the
/// server answers it, 202, 404 or 422.
#[derive(Debug, PartialEq, Eq)]
enum Status {
    Accepted,
    NotFound,
    Unprocessable,
}

/// The engine as the server plants this DataCapability and its aggregate,
/// with the character-sheets port's fake, whose characters a clone shares,
/// and a clock the test moves: two imports never share an instant by chance.
struct Served {
    engine: Engine,
    clock: Arc<FixedClock>,
    store: Arc<MemoryStore>,
    sheets: FakeCharacterSheets,
}

async fn served() -> Served {
    let sheets = FakeCharacterSheets::new()
        .with_sheet(elowen())
        .with_sheet(sheet(BRANNOC, "Brannoc", 51))
        .with_private(id(112_851_877))
        .with_changed_shape(id(112_853_002));
    served_by(SharedCharacterSheets::new(sheets.clone()), sheets).await
}

/// [`served`], the engine fetching from `port`.
async fn served_by(port: SharedCharacterSheets, sheets: FakeCharacterSheets) -> Served {
    let store = Arc::new(MemoryStore::default());
    let clock = Arc::new(FixedClock::at_millis(NOW_MILLIS));
    let mut shared = Shared::default();
    shared.insert(Arc::new(port));
    let engine = Engine::new(
        store.clone(),
        Arc::new(MemoryNotifier::default()),
        clock.clone(),
    )
    .with_aggregate(Aggregate::parse(aggregate_entity::MANIFEST).expect("contract I"))
    .with_data_capability(DataCapability::parse(import_character::MANIFEST).expect("contract F"))
    .with_ids(Arc::new(RandomIds))
    .with_plugins(
        [aggregate_entity::plugin(), import_character::plugin()]
            .into_iter()
            .collect(),
    )
    .with_shared(shared);
    engine.load().await.expect("the units fit");
    Served {
        engine,
        clock,
        store,
        sheets,
    }
}

impl Served {
    /// An import of `payload` on `target` by the DM, approver of their own
    /// campaign: what became of it, and the answer.
    async fn import(&self, payload: Value, target: Option<&str>) -> (Status, Value) {
        let mut body = json!({
            "dataCapability": "dndbeyond.importCharacter@1",
            "payload": payload,
            "caller": { "by": "dm", "from": "campaign.EntityEditor", "roles": ["role:approver"] },
        });
        if let Some(target) = target {
            body["targetId"] = json!(target);
        }
        let submission: Submission = serde_json::from_value(body).expect("a submission");
        match self.engine.submit(submission).await {
            Ok(submitted) => (
                Status::Accepted,
                serde_json::to_value(submitted).expect("JSON"),
            ),
            Err(Error::Refused(refusal)) => (
                match refusal.kind {
                    RefusalKind::NotFound => Status::NotFound,
                    RefusalKind::Unprocessable => Status::Unprocessable,
                },
                json!({ "error": refusal.message }),
            ),
            Err(e) => panic!("{e}"),
        }
    }

    /// One pass of the single applier: what it settled.
    async fn settled(&self) -> Vec<CommandResult> {
        self.engine.run_once().await.expect("the applier ran")
    }

    /// An import of `payload` on `target`, queued with no warning and
    /// applied: the PC's id.
    async fn imported(&self, payload: Value, target: Option<&str>) -> String {
        let (status, answer) = self.import(payload, target).await;
        assert_eq!(status, Status::Accepted, "{answer}");
        assert_eq!(answer["warnings"], json!([]), "{answer}");
        let settled = self.settled().await;
        assert_eq!(settled.len(), 1, "{settled:?}");
        assert_eq!(settled[0].status, State::Applied, "{settled:?}");
        let partition = answer["partition"].as_str().expect("a partition");
        partition.trim_start_matches("Entity/").to_owned()
    }

    /// The PC's row, as the engine holds it.
    async fn row(&self, id: &str) -> Map<String, Value> {
        self.store
            .row(&format!("Entity/{id}"))
            .await
            .expect("the row")
            .into_iter()
            .collect()
    }
}

/// The fields a version's diff names.
fn changed(row: &Map<String, Value>) -> Vec<String> {
    row["diff"]
        .as_array()
        .expect("a diff")
        .iter()
        .map(|change| change["field"].as_str().expect("a field").to_owned())
        .collect()
}

#[tokio::test]
async fn an_import_creates_the_pc_of_its_player_with_its_sheet_a_default_token_and_version_1() {
    let s = served().await;
    let pc = s
        .imported(
            json!({ "url": elowen_url(), "campaign": VALOMBRE, "player": JULIE }),
            None,
        )
        .await;
    assert_eq!(pc, import_character::derived(VALOMBRE, id(ELOWEN)));
    let row = s.row(&pc).await;
    for (field, value) in [
        ("entityCampaignId", json!(VALOMBRE)),
        ("type", json!("pj")),
        ("name", json!("Elowen Ventargent")),
        ("playerId", json!(JULIE)),
        ("wisdom", json!(15)),
        ("armorClass", json!(16)),
        ("hpMax", json!(44)),
        ("tokenColor", json!("accent")),
        ("dndbeyondId", json!(ELOWEN.to_string())),
        ("source", json!("dndbeyond")),
        ("versionNumber", json!(1)),
    ] {
        assert_eq!(row.get(field), Some(&value), "{field}");
    }
    assert!(
        row["dndbeyondSyncedAt"]
            .as_str()
            .is_some_and(|at| at.ends_with('Z'))
    );
    assert_eq!(row["actions"][0]["name"], json!("Attaque supplémentaire"));
    assert_eq!(row["spells"][0]["name"], json!("Marque du chasseur"));
    // No portrait and no token image: the token is the disc of its colour.
    assert!(!row.contains_key("portraitMediaId") && !row.contains_key("tokenMediaId"));
    assert!(changed(&row).contains(&"name".to_owned()));
}

#[tokio::test]
async fn the_same_url_imported_again_updates_the_same_pc_with_a_version_naming_what_changed() {
    let s = served().await;
    let pc = s
        .imported(json!({ "url": elowen_url(), "campaign": VALOMBRE }), None)
        .await;
    // Elowen levels up on D&D Beyond, and is renamed there.
    let mut levelled = sheet(ELOWEN, "Elowen la Grande", 52);
    levelled.statblock.wisdom = 16;
    let _ = s.sheets.clone().with_sheet(levelled);
    // Time passes between the two imports: the sync date moves with it.
    s.clock.set_millis(NOW_MILLIS + 1);
    let again = s
        .imported(
            json!({ "url": format!("https://dndbeyond.com/characters/{ELOWEN}/builder"),
                    "campaign": VALOMBRE.to_uppercase() }),
            None,
        )
        .await;
    assert_eq!(again, pc, "no duplicate");
    let row = s.row(&pc).await;
    assert_eq!(row["hpMax"], json!(52));
    assert_eq!(row["wisdom"], json!(16));
    assert_eq!(row["versionNumber"], json!(2));
    assert_eq!(row["source"], json!("dndbeyond"));
    // The name is the PC's: an import of a PC there writes its statblock alone.
    assert_eq!(row["name"], json!("Elowen Ventargent"));
    assert_eq!(changed(&row), ["dndbeyondSyncedAt", "hpMax", "wisdom"]);
}

#[tokio::test]
async fn an_import_queued_behind_the_first_before_it_applies_updates_the_pc_it_creates() {
    let s = served().await;
    let payload = json!({ "url": elowen_url(), "campaign": VALOMBRE });
    let (first, one) = s.import(payload.clone(), None).await;
    let (second, two) = s.import(payload, None).await;
    assert_eq!(
        (first, second),
        (Status::Accepted, Status::Accepted),
        "{two}"
    );
    assert_eq!(one["partition"], two["partition"]);
    let settled = s.settled().await;
    assert_eq!(settled.len(), 2, "{settled:?}");
    assert!(
        settled.iter().all(|r| r.status == State::Applied),
        "{settled:?}"
    );
    let pc = import_character::derived(VALOMBRE, id(ELOWEN));
    assert_eq!(s.row(&pc).await["versionNumber"], json!(2));
}

#[tokio::test]
async fn an_import_on_its_pc_needs_no_campaign_and_one_on_another_pc_or_character_is_refused() {
    let s = served().await;
    let elowen = s
        .imported(json!({ "url": elowen_url(), "campaign": VALOMBRE }), None)
        .await;
    // The PC targeted gives its campaign.
    assert_eq!(
        s.imported(json!({ "url": elowen_url() }), Some(&elowen))
            .await,
        elowen
    );
    assert_eq!(s.row(&elowen).await["versionNumber"], json!(2));
    // Linked to Elowen, it is not filled with Brannoc.
    let (status, answer) = s
        .import(json!({ "url": id(BRANNOC).url() }), Some(&elowen))
        .await;
    assert_eq!(status, Status::Unprocessable);
    assert!(
        text(&answer).contains("autre personnage D&D Beyond"),
        "{answer}"
    );
    // Nor imported into another campaign.
    let (status, answer) = s
        .import(
            json!({ "url": elowen_url(), "campaign": "c1000000-0000-4000-8000-000000000002" }),
            Some(&elowen),
        )
        .await;
    assert_eq!(status, Status::Unprocessable);
    assert!(text(&answer).contains("autre campagne"), "{answer}");
    // An element the engine does not hold is not found.
    let (status, _) = s
        .import(
            json!({ "url": elowen_url() }),
            Some("e9000000-0000-4000-8000-000000000009"),
        )
        .await;
    assert_eq!(status, Status::NotFound);
    assert!(s.settled().await.is_empty(), "nothing queued");
}

fn text(answer: &Value) -> &str {
    answer["error"].as_str().unwrap_or_default()
}

#[tokio::test]
async fn what_d_d_beyond_does_not_give_or_a_url_not_its_own_is_refused_with_its_reason_and_nothing_queued()
 {
    let s = served().await;
    for (url, status, reason) in [
        (
            "https://www.dndbeyond.com/characters/112851877".to_owned(),
            Status::Unprocessable,
            "n'est pas public",
        ),
        (
            "https://www.dndbeyond.com/characters/999".to_owned(),
            Status::NotFound,
            "ne connaît pas",
        ),
        (
            "https://www.dndbeyond.com/characters/112853002".to_owned(),
            Status::Unprocessable,
            "a changé de forme",
        ),
        (
            "https://example.org/characters/112847390".to_owned(),
            Status::Unprocessable,
            "n'est pas celle de D&D Beyond",
        ),
        (
            "elowen".to_owned(),
            Status::Unprocessable,
            "ni l'identifiant ni l'adresse",
        ),
    ] {
        let (got, answer) = s
            .import(json!({ "url": url, "campaign": VALOMBRE }), None)
            .await;
        assert_eq!(got, status, "{url}: {answer}");
        assert!(text(&answer).contains(reason), "{url}: {answer}");
    }
    let (got, answer) = s.import(json!({ "campaign": VALOMBRE }), None).await;
    assert_eq!(got, Status::Unprocessable, "{answer}");
    assert!(s.settled().await.is_empty(), "nothing queued");
    let pc = import_character::derived(VALOMBRE, id(112_851_877));
    assert!(s.row(&pc).await.is_empty(), "nothing written");
}

#[tokio::test]
async fn an_import_of_no_campaign_or_naming_what_is_no_id_is_refused() {
    let s = served().await;
    for (payload, reason) in [
        (json!({ "url": elowen_url() }), "nomme sa campagne"),
        (
            json!({ "url": elowen_url(), "campaign": "valombre" }),
            "campaign",
        ),
        (
            json!({ "url": elowen_url(), "campaign": VALOMBRE, "player": "julie" }),
            "player",
        ),
        (
            json!({ "url": elowen_url(), "campaign": VALOMBRE, "entityId": 3 }),
            "entityId",
        ),
    ] {
        let (status, answer) = s.import(payload.clone(), None).await;
        assert_eq!(status, Status::Unprocessable, "{payload}: {answer}");
        assert!(text(&answer).contains(reason), "{payload}: {answer}");
    }
    // A null or blank player is none.
    s.imported(
        json!({ "url": elowen_url(), "campaign": VALOMBRE, "player": "" }),
        None,
    )
    .await;
    assert!(s.settled().await.is_empty());
}

/// A port answering Brannoc's sheet whatever it is asked.
struct Mismatched;

impl CharacterSheets for Mismatched {
    async fn fetch(&self, _id: CharacterId) -> Result<CharacterSheet, CharacterSheetsError> {
        Ok(sheet(BRANNOC, "Brannoc", 51))
    }
}

#[tokio::test]
async fn a_sheet_of_another_character_than_the_one_asked_for_is_refused_and_nothing_queued() {
    let s = served_by(
        SharedCharacterSheets::new(Mismatched),
        FakeCharacterSheets::new(),
    )
    .await;
    let (status, answer) = s
        .import(json!({ "url": elowen_url(), "campaign": VALOMBRE }), None)
        .await;
    assert_eq!(status, Status::Unprocessable, "{answer}");
    assert!(text(&answer).contains("a changé de forme"), "{answer}");
    assert!(
        text(&answer).contains(&format!("{BRANNOC} répondu pour {ELOWEN}")),
        "{answer}"
    );
    assert!(s.settled().await.is_empty(), "nothing queued");
}
