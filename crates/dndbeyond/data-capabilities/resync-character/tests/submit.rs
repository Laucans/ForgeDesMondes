//! dndbeyond.resyncCharacter through the engine, over its in-memory store,
//! as the server plants it with the Entity aggregate and the character-sheets
//! port's fake: a resync is fetched and made before anything is queued,
//! judged (`dndbeyond-resync-scope`, `every-change-versioned`) and applied on
//! the PC's partition with the version of source `dndbeyond` it adds. A PC
//! linked to nothing, or a sheet D&D Beyond does not give, is refused and
//! nothing is queued. The tables it reaches are `tests/postgres.rs`.

use std::sync::Arc;

use harness_dataguard::Error;
use harness_dataguard::aggregate::Aggregate;
use harness_dataguard::api::Submission;
use harness_dataguard::command::{CommandResult, State};
use harness_dataguard::engine::Engine;
use harness_dataguard::manifest::DataCapability;
use harness_dataguard::memory::{MemoryNotifier, MemoryStore};
use harness_dataguard::plugins::{RefusalKind, Shared};
use harness_dataguard::postgres::SystemClock;
use harness_dataguard::store::{RandomIds, Store};
use ports_character_sheets::SharedCharacterSheets;
use ports_character_sheets::fake::FakeCharacterSheets;
use serde_json::{Map, Value, json};

mod support;
use support::{ELOWEN, actions, elowen, id, levelled_up, spells};

const VALOMBRE: &str = "c1000000-0000-4000-8000-000000000001";
/// Elowen, a PC of Valombre an import linked to D&D Beyond.
const ELOWEN_PC: &str = "e1000000-0000-4000-8000-000000000001";
/// Gaspard, a PC of Valombre the DM made, linked to nothing.
const GASPARD: &str = "e1000000-0000-4000-8000-000000000002";
/// A character D&D Beyond holds but does not share.
const PRIVATE: u64 = 112_851_877;
/// One whose answer changed shape.
const CHANGED: u64 = 112_853_002;
/// One D&D Beyond does not know.
const UNKNOWN: u64 = 112_899_999;
/// A PC of Valombre linked to one of those.
const ORIN: &str = "e1000000-0000-4000-8000-000000000003";

/// A stand-in for the saves and the import that made the PCs: it writes a
/// whole element at its version 1, as given.
const PLANT: &str = r#"{"dataCapability":"test.plantEntity","owner":"dataguard","version":1,"effect":"upsert","target":{"aggregate":"Entity","id":"$entityId"},"mode":"overwrite","callableBy":["*"]}"#;

/// What became of a submission: queued, or refused before it was — as the
/// server answers it, 202, 404 or 422.
#[derive(Debug, PartialEq, Eq)]
enum Status {
    Accepted,
    NotFound,
    Unprocessable,
}

/// The engine as the server plants this DataCapability and its aggregate,
/// with the character-sheets port's fake, whose characters a clone shares.
struct Served {
    engine: Engine,
    store: Arc<MemoryStore>,
    sheets: FakeCharacterSheets,
}

async fn served() -> Served {
    let sheets = FakeCharacterSheets::new()
        .with_sheet(elowen())
        .with_private(id(PRIVATE))
        .with_changed_shape(id(CHANGED));
    let store = Arc::new(MemoryStore::default());
    let mut shared = Shared::default();
    shared.insert(Arc::new(SharedCharacterSheets::new(sheets.clone())));
    let engine = Engine::new(
        store.clone(),
        Arc::new(MemoryNotifier::default()),
        Arc::new(SystemClock),
    )
    .with_aggregate(Aggregate::parse(aggregate_entity::MANIFEST).expect("contract I"))
    .with_data_capability(DataCapability::parse(resync_character::MANIFEST).expect("contract F"))
    .with_data_capability(DataCapability::parse(PLANT).expect("contract F"))
    .with_ids(Arc::new(RandomIds))
    .with_plugins(
        [aggregate_entity::plugin(), resync_character::plugin()]
            .into_iter()
            .collect(),
    )
    .with_shared(shared);
    engine.load().await.expect("the units fit");
    let served = Served {
        engine,
        store,
        sheets,
    };
    served
        .plant(ELOWEN_PC, "Elowen Ventargent", Some(ELOWEN))
        .await;
    served.plant(GASPARD, "Gaspard", None).await;
    served
}

impl Served {
    /// The DM's `body`: what became of it, and the answer.
    async fn submit(&self, mut body: Value) -> (Status, Value) {
        body["caller"] =
            json!({ "by": "dm", "from": "campaign.EntityEditor", "roles": ["role:approver"] });
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

    /// The PC `pc`, named `name`, linked to the character `linked`, at its
    /// version 1: aliases, notes, a cropped portrait and a token colour the
    /// DM set, its statblock as D&D Beyond gave it before the level-up.
    async fn plant(&self, pc: &str, name: &str, linked: Option<u64>) {
        let sheet = json!({
            "type": "pj", "name": name, "notes": "Cherche son frère Aerin.",
            "strength": 12, "dexterity": 18, "constitution": 14, "intelligence": 10,
            "wisdom": 15, "charisma": 9, "armorClass": 16, "hpMax": 44,
            "actions": actions(), "spells": spells(),
            "portraitMediaId": "d1000000-0000-4000-8000-000000000001",
            "tokenCrop": { "zoom": 140, "x": 50, "y": 32 }, "tokenColor": "success",
            "dndbeyondId": linked.map(|n| n.to_string()),
            "dndbeyondSyncedAt": linked.map(|_| "2026-09-26T18:12:00Z"),
        });
        let mut snapshot = sheet.clone();
        snapshot["aliases"] = json!([format!("{name} l'ancien")]);
        let mut payload = sheet;
        for (field, value) in [
            ("entityId", json!(pc)),
            ("entityCampaignId", json!(VALOMBRE)),
            ("entityCreatedAt", json!("2026-09-26T18:12:00Z")),
            ("updatedAt", json!("2026-09-26T18:12:00Z")),
            ("alias", json!([format!("{name} l'ancien")])),
            ("characterEntityId", json!(pc)),
            ("entityVersionEntityId", json!(pc)),
            ("entityVersionId", json!(format!("{}f", &pc[..35]))),
            ("versionNumber", json!(1)),
            ("source", json!("dm")),
            ("snapshot", snapshot),
            ("diff", json!([])),
        ] {
            payload[field] = value;
        }
        let (status, answer) = self
            .submit(json!({ "dataCapability": "test.plantEntity@1", "payload": payload }))
            .await;
        assert_eq!(status, Status::Accepted, "{answer}");
        applied(&self.settled().await);
    }

    /// A resync of `target` — else of the PC `payload` names — by the DM.
    async fn resync(&self, payload: Value, target: Option<&str>) -> (Status, Value) {
        let mut body =
            json!({ "dataCapability": "dndbeyond.resyncCharacter@1", "payload": payload });
        if let Some(target) = target {
            body["targetId"] = json!(target);
        }
        self.submit(body).await
    }

    /// One pass of the single applier: what it settled.
    async fn settled(&self) -> Vec<CommandResult> {
        self.engine.run_once().await.expect("the applier ran")
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

/// What the applier settled, one command applied.
fn applied(settled: &[CommandResult]) {
    assert_eq!(settled.len(), 1, "{settled:?}");
    assert_eq!(settled[0].status, State::Applied, "{settled:?}");
}

/// The fields a version's diff names.
fn changed(row: &Map<String, Value>) -> Vec<&str> {
    row["diff"]
        .as_array()
        .expect("a diff")
        .iter()
        .map(|change| change["field"].as_str().expect("a field"))
        .collect()
}

/// What the DM set locally on a PC: none of it a resync changes.
const LOCAL: [&str; 7] = [
    "name",
    "alias",
    "notes",
    "portraitMediaId",
    "tokenCrop",
    "tokenColor",
    "dndbeyondId",
];

#[tokio::test]
async fn a_resync_after_a_level_up_updates_stats_hp_and_spells_and_leaves_the_local_fields() {
    let s = served().await;
    let before = s.row(ELOWEN_PC).await;
    // Elowen levels up on D&D Beyond.
    let _ = s.sheets.clone().with_sheet(levelled_up());
    let (status, answer) = s.resync(json!({}), Some(ELOWEN_PC)).await;
    assert_eq!(status, Status::Accepted, "{answer}");
    assert_eq!(answer["partition"], json!(format!("Entity/{ELOWEN_PC}")));
    applied(&s.settled().await);
    let after = s.row(ELOWEN_PC).await;
    assert_eq!(after["hpMax"], json!(52));
    assert_eq!(after["wisdom"], json!(16));
    let spells: Vec<&str> = after["spells"]
        .as_array()
        .expect("spells")
        .iter()
        .map(|spell| spell["name"].as_str().expect("a name"))
        .collect();
    assert!(spells.contains(&"Explorateur né amélioré"), "{spells:?}");
    assert!(
        spells.contains(&"Communication avec les animaux"),
        "{spells:?}"
    );
    for field in LOCAL {
        assert_eq!(after.get(field), before.get(field), "{field} changed");
    }
    assert_ne!(after["dndbeyondSyncedAt"], before["dndbeyondSyncedAt"]);
    // Versioned, so it can be undone: the next version, of source
    // dndbeyond, naming the level-up's fields alone.
    assert_eq!(after["versionNumber"], json!(2));
    assert_eq!(after["source"], json!("dndbeyond"));
    assert_eq!(after["entityVersionEntityId"], json!(ELOWEN_PC));
    assert_eq!(
        changed(&after),
        ["dndbeyondSyncedAt", "hpMax", "spells", "wisdom"]
    );
}

#[tokio::test]
async fn a_resync_names_its_pc_by_the_payload_when_the_target_does_not() {
    let s = served().await;
    let (status, answer) = s.resync(json!({ "entityId": ELOWEN_PC }), None).await;
    assert_eq!(status, Status::Accepted, "{answer}");
    assert_eq!(answer["partition"], json!(format!("Entity/{ELOWEN_PC}")));
    applied(&s.settled().await);
    let after = s.row(ELOWEN_PC).await;
    assert_eq!(after["versionNumber"], json!(2));
    // Nothing new on D&D Beyond: the sync date alone.
    assert_eq!(changed(&after), ["dndbeyondSyncedAt"]);
}

#[tokio::test]
async fn a_pc_named_in_capitals_is_resynchronised_on_its_own_partition() {
    let s = served().await;
    let (status, answer) = s.resync(json!({}), Some(&ELOWEN_PC.to_uppercase())).await;
    assert_eq!(status, Status::Accepted, "{answer}");
    assert_eq!(answer["partition"], json!(format!("Entity/{ELOWEN_PC}")));
    applied(&s.settled().await);
    assert_eq!(s.row(ELOWEN_PC).await["versionNumber"], json!(2));
}

#[tokio::test]
async fn what_the_payload_carries_besides_the_pc_is_never_written() {
    let s = served().await;
    let before = s.row(ELOWEN_PC).await;
    let (status, answer) = s
        .resync(
            json!({ "notes": "écrasées", "alias": ["autre"], "tokenColor": "danger",
                    "name": "Autre", "hpMax": 1 }),
            Some(ELOWEN_PC),
        )
        .await;
    assert_eq!(status, Status::Accepted, "{answer}");
    applied(&s.settled().await);
    let after = s.row(ELOWEN_PC).await;
    for field in LOCAL {
        assert_eq!(after.get(field), before.get(field), "{field} changed");
    }
    assert_eq!(after["hpMax"], json!(44), "D&D Beyond's, not the payload's");
}

#[tokio::test]
async fn a_resync_of_a_pc_linked_to_nothing_is_refused_with_a_readable_reason() {
    let s = served().await;
    let (status, answer) = s.resync(json!({}), Some(GASPARD)).await;
    assert_eq!(status, Status::Unprocessable);
    assert_eq!(
        answer["error"],
        json!("ce personnage n'est pas lié à D&D Beyond : importez-le d'abord par son adresse")
    );
    assert!(s.settled().await.is_empty(), "nothing is queued");
    assert_eq!(s.row(GASPARD).await["versionNumber"], json!(1));
}

#[tokio::test]
async fn a_resync_of_no_element_or_of_no_id_is_refused() {
    let s = served().await;
    let (status, answer) = s
        .resync(json!({}), Some("e1000000-0000-4000-8000-0000000000ff"))
        .await;
    assert_eq!(status, Status::NotFound, "{answer}");
    assert_eq!(answer["error"], json!("aucun élément de cet identifiant"));
    for target in ["elowen", "e1000000-0000-4000-8000-00000000000g"] {
        let (status, answer) = s.resync(json!({}), Some(target)).await;
        assert_eq!(status, Status::Unprocessable, "{answer}");
        assert_eq!(answer["error"], json!("entityId n'est pas un identifiant"));
    }
    let (status, _) = s.resync(json!({}), None).await;
    assert_eq!(status, Status::Unprocessable);
    assert!(s.settled().await.is_empty(), "nothing is queued");
}

#[tokio::test]
async fn a_character_d_and_d_beyond_no_longer_gives_is_refused_and_nothing_queued() {
    for (character, expected, reason) in [
        (PRIVATE, Status::Unprocessable, "n'est pas public"),
        (UNKNOWN, Status::NotFound, "ne connaît pas ce personnage"),
        (CHANGED, Status::Unprocessable, "a changé de forme"),
    ] {
        let s = served().await;
        s.plant(ORIN, "Orin", Some(character)).await;
        let (status, answer) = s.resync(json!({}), Some(ORIN)).await;
        assert_eq!(status, expected, "{answer}");
        assert!(
            answer["error"]
                .as_str()
                .is_some_and(|error| error.contains(reason)),
            "{answer}"
        );
        assert!(s.settled().await.is_empty(), "nothing is queued");
    }
}

#[tokio::test]
async fn an_incomplete_sheet_is_refused_and_nothing_queued() {
    let s = served().await;
    let mut sheet = levelled_up();
    sheet.statblock.actions.clear();
    let _ = s.sheets.clone().with_sheet(sheet);
    let (status, answer) = s.resync(json!({}), Some(ELOWEN_PC)).await;
    assert_eq!(status, Status::Unprocessable);
    assert_eq!(
        answer["error"],
        json!("la fiche D&D Beyond n'a pas de statblock complet : actions")
    );
    assert!(s.settled().await.is_empty(), "nothing is queued");
}

#[tokio::test]
async fn a_server_that_shares_no_character_sheets_port_does_not_load() {
    let engine = Engine::new(
        Arc::new(MemoryStore::default()),
        Arc::new(MemoryNotifier::default()),
        Arc::new(SystemClock),
    )
    .with_aggregate(Aggregate::parse(aggregate_entity::MANIFEST).expect("contract I"))
    .with_data_capability(DataCapability::parse(resync_character::MANIFEST).expect("contract F"))
    .with_plugins(
        [aggregate_entity::plugin(), resync_character::plugin()]
            .into_iter()
            .collect(),
    );
    assert!(engine.load().await.is_err());
}
