//! campaign.saveEntity through the engine, over its in-memory store, as the
//! server plants it with the Entity aggregate: a save is prepared against
//! the element's row, judged, queued per element and applied, the version it
//! adds settled after the element's last one (EntityVersion@1). The tables
//! it reaches are the Entity aggregate's `tests/projection.rs`.

use std::sync::Arc;

use aggregate_entity::{History, Kept};
use harness_dataguard::Error;
use harness_dataguard::aggregate::Aggregate;
use harness_dataguard::api::Submission;
use harness_dataguard::command::{CommandResult, State};
use harness_dataguard::engine::Engine;
use harness_dataguard::manifest::DataCapability;
use harness_dataguard::memory::{MemoryNotifier, MemoryStore};
use harness_dataguard::plugins::RefusalKind;
use harness_dataguard::postgres::SystemClock;
use harness_dataguard::store::{RandomIds, Store};
use serde_json::{Map, Value, json};

const VALOMBRE: &str = "c1000000-0000-4000-8000-000000000001";
const BRUME: &str = "c1000000-0000-4000-8000-000000000002";
const CLAIRE: &str = "b1000000-0000-4000-8000-000000000001";

/// The engine as the server plants this DataCapability and its aggregate,
/// its store and the versions the aggregate keeps.
struct Served {
    engine: Engine,
    store: Arc<MemoryStore>,
    history: Arc<History>,
}

async fn served() -> Served {
    let store = Arc::new(MemoryStore::default());
    let history = Arc::new(History::default());
    let engine = Engine::new(
        store.clone(),
        Arc::new(MemoryNotifier::default()),
        Arc::new(SystemClock),
    )
    .with_aggregate(Aggregate::parse(aggregate_entity::MANIFEST).expect("contract I"))
    .with_data_capability(DataCapability::parse(save_entity::MANIFEST).expect("contract F"))
    .with_ids(Arc::new(RandomIds))
    .with_plugins(
        [
            aggregate_entity::keeping(history.clone()),
            save_entity::plugin(),
        ]
        .into_iter()
        .collect(),
    );
    engine.load().await.expect("the units fit");
    Served {
        engine,
        store,
        history,
    }
}

impl Served {
    /// campaign.saveEntity by `by`, the DM's editor calling it — the DM
    /// approves their own campaign's critical fields: the answer, or why
    /// it was refused.
    async fn submitted(&self, by: &str, payload: Value, key: Option<&str>) -> Result<Value, Error> {
        let mut body = json!({
            "dataCapability": "campaign.saveEntity@1",
            "payload": payload,
            "caller": { "by": by, "from": "campaign.EntityEditor", "roles": ["role:approver"] },
        });
        if let Some(key) = key {
            body["idempotencyKey"] = json!(key);
        }
        let submission: Submission = serde_json::from_value(body).expect("a submission");
        self.engine
            .submit(submission)
            .await
            .map(|submitted| serde_json::to_value(submitted).expect("JSON"))
    }

    /// The same, queued.
    async fn submit(&self, by: &str, payload: Value, key: Option<&str>) -> Value {
        self.submitted(by, payload, key).await.expect("queued")
    }

    /// One pass of the single applier, which settles one command.
    async fn settled(&self) -> CommandResult {
        let mut settled = self.engine.run_once().await.expect("the applier ran");
        assert_eq!(settled.len(), 1, "{settled:?}");
        settled.remove(0)
    }

    /// Saves `payload` as the DM: the element's id, and what the applier
    /// made of the save.
    async fn save(&self, payload: Value) -> (String, CommandResult) {
        let answer = self.submit("dm", payload, None).await;
        (element(&answer), self.settled().await)
    }

    /// The row the engine holds for `element`.
    async fn row(&self, element: &str) -> Map<String, Value> {
        self.store
            .row(&format!("Entity/{element}"))
            .await
            .expect("the row")
            .into_iter()
            .collect()
    }

    /// `element`'s last version, as it was settled.
    fn last(&self, element: &str) -> Kept {
        self.history
            .of(element)
            .pop()
            .unwrap_or_else(|| panic!("a version of {element}"))
    }
}

/// The element a submission's partition names.
fn element(answer: &Value) -> String {
    answer["partition"]
        .as_str()
        .and_then(|p| p.strip_prefix("Entity/"))
        .unwrap_or_else(|| panic!("an Entity partition: {answer}"))
        .to_owned()
}

fn applied(result: &CommandResult) {
    assert_eq!(result.status, State::Applied, "{result:?}");
}

/// The fields a version's diff names.
fn changed(diff: &[Value]) -> Vec<&str> {
    diff.iter()
        .map(|change| change["field"].as_str().expect("a field"))
        .collect()
}

/// Morlox, a new NPC of Valombre with his stats and two aliases.
fn morlox() -> Value {
    json!({
        "campaign": VALOMBRE,
        "entity": {
            "kind": "pnj",
            "name": "Morlox",
            "aliases": ["le gobelin", "le marchand"],
            "notes": "Doit 40 po à Vesna.",
            "stats": { "FOR": 8, "DEX": 15, "CON": 10, "INT": 13, "SAG": 9, "CHA": 12 },
            "ca": 13,
            "pvMax": 21,
            "color": "danger",
        }
    })
}

/// Elowen, Claire's PC at Valombre, as `notes` says.
fn elowen(id: Option<&str>, notes: &str) -> Value {
    let mut entity = json!({
        "kind": "pj",
        "name": "Elowen Ventargent",
        "aliases": ["la Rôdeuse"],
        "player": CLAIRE,
        "notes": notes,
        "stats": { "DEX": 18 },
    });
    if let Some(id) = id {
        entity["id"] = json!(id);
    }
    json!({ "campaign": VALOMBRE, "entity": entity })
}

#[tokio::test]
async fn a_new_npc_is_saved_with_its_sheet_aliases_and_version_1_holding_the_full_snapshot() {
    let s = served().await;
    let (id, result) = s.save(morlox()).await;
    applied(&result);
    let row = s.row(&id).await;
    assert_eq!(row["entityId"], id.as_str());
    assert_eq!(row["entityCampaignId"], VALOMBRE);
    assert_eq!(row["type"], "pnj");
    assert_eq!(row["name"], "Morlox");
    assert_eq!(row["characterEntityId"], id.as_str());
    assert_eq!(row["dexterity"], 15);
    assert_eq!(row["armorClass"], 13);
    assert_eq!(row["hpMax"], 21);
    assert_eq!(row["tokenColor"], "danger");
    assert_eq!(row["alias"], json!(["le gobelin", "le marchand"]));
    assert_eq!(row["versionNumber"], 1);
    assert_eq!(row["source"], "dm");
    // Its dates, to the millisecond.
    let created = row["entityCreatedAt"].as_str().expect("a date");
    assert_eq!(created.len(), "2026-10-17T10:00:00.000Z".len(), "{created}");
    assert_eq!(row["updatedAt"], created);
    let snapshot = json!({
        "type": "pnj",
        "name": "Morlox",
        "notes": "Doit 40 po à Vesna.",
        "aliases": ["le gobelin", "le marchand"],
        "strength": 8, "dexterity": 15, "constitution": 10,
        "intelligence": 13, "wisdom": 9, "charisma": 12,
        "armorClass": 13, "hpMax": 21,
        "spells": [], "actions": [],
        "tokenColor": "danger",
    });
    assert_eq!(row["snapshot"], snapshot);
    let version = s.last(&id);
    assert_eq!(version.number, 1);
    assert_eq!(Some(&json!(version.id)), row.get("entityVersionId"));
    assert_eq!(Value::Object(version.snapshot.clone()), snapshot);
    // The first version is compared with an empty snapshot.
    let mut every: Vec<&str> = version.snapshot.keys().map(String::as_str).collect();
    every.sort_unstable();
    assert_eq!(changed(&version.diff), every);
}

#[tokio::test]
async fn a_pc_whose_notes_change_takes_a_new_version_whose_diff_lists_only_notes() {
    let s = served().await;
    let (id, result) = s.save(elowen(None, "Cherche son frère.")).await;
    applied(&result);
    let first = s.row(&id).await;
    let (again, result) = s.save(elowen(Some(&id), "Cherche son frère Aerin.")).await;
    applied(&result);
    assert_eq!(again, id);
    let row = s.row(&id).await;
    assert_eq!(row["versionNumber"], 2);
    assert_ne!(row["entityVersionId"], first["entityVersionId"]);
    let version = s.last(&id);
    assert_eq!(version.number, 2);
    assert_eq!(
        version.diff,
        [
            json!({ "field": "notes", "old": "Cherche son frère.", "new": "Cherche son frère Aerin." })
        ]
    );
    assert_eq!(row["entityCreatedAt"], first["entityCreatedAt"]);
    assert_eq!(row["playerId"], CLAIRE);
}

#[tokio::test]
async fn an_element_the_command_alone_targets_is_saved_on_its_own_row() {
    let s = served().await;
    let (id, result) = s.save(elowen(None, "v1")).await;
    applied(&result);
    // The editor names it as the command's target, not in `entity`.
    let submission: Submission = serde_json::from_value(json!({
        "dataCapability": "campaign.saveEntity@1",
        "target": { "aggregate": "Entity", "id": id },
        "payload": { "entity": { "notes": "v2" } },
        "caller": { "by": "dm", "from": "campaign.EntityEditor", "roles": ["role:approver"] },
    }))
    .expect("a submission");
    let answer = s.engine.submit(submission).await.expect("queued");
    assert_eq!(answer.partition, format!("Entity/{id}"));
    applied(&s.settled().await);
    let row = s.row(&id).await;
    assert_eq!(row["notes"], "v2");
    assert_eq!(row["name"], "Elowen Ventargent");
    assert_eq!(s.last(&id).number, 2);
}

#[tokio::test]
async fn an_alias_naming_another_element_by_its_alias_or_name_is_rejected_and_nothing_changes() {
    let s = served().await;
    let (elowen_id, result) = s.save(elowen(None, "")).await;
    applied(&result);
    let (morlox_id, result) = s.save(morlox()).await;
    applied(&result);
    let before = s.row(&morlox_id).await;
    for alias in ["  LA RODEUSE ", "elowen ventargent"] {
        let mut payload = morlox();
        payload["entity"]["id"] = json!(morlox_id);
        payload["entity"]["aliases"] = json!(["le gobelin", alias]);
        let (_, result) = s.save(payload).await;
        assert_eq!(result.status, State::Rejected, "{alias}: {result:?}");
        assert_eq!(result.violations, ["alias-unique-in-campaign"], "{alias}");
        assert_eq!(s.row(&morlox_id).await, before, "{alias}");
        assert_eq!(s.history.of(&morlox_id).len(), 1, "{alias}");
    }
    // A new element whose name doubles another's alias is refused too.
    let mut payload = morlox();
    payload["entity"]["name"] = json!("La Rôdeuse");
    payload["entity"]["aliases"] = json!([]);
    let (_, result) = s.save(payload).await;
    assert_eq!(result.violations, ["alias-unique-in-campaign"]);
    // Another campaign's element may answer to it.
    let mut payload = morlox();
    payload["campaign"] = json!(BRUME);
    payload["entity"]["aliases"] = json!(["la rodeuse"]);
    applied(&s.save(payload).await.1);
    // Once Elowen drops it, it is free.
    let mut payload = elowen(Some(&elowen_id), "");
    payload["entity"]["aliases"] = json!([]);
    applied(&s.save(payload).await.1);
    let mut payload = morlox();
    payload["entity"]["id"] = json!(morlox_id);
    payload["entity"]["aliases"] = json!(["la rôdeuse"]);
    applied(&s.save(payload).await.1);
}

#[tokio::test]
async fn a_dex_modifier_given_is_never_written() {
    let s = served().await;
    let mut payload = morlox();
    payload["entity"]["dexterityModifier"] = json!(7);
    payload["entity"]["stats"]["mods"] = json!({ "DEX": 7 });
    payload["entity"]["mods"] = json!({ "DEX": 7 });
    let (id, result) = s.save(payload).await;
    applied(&result);
    let row = s.row(&id).await;
    assert_eq!(row["dexterity"], 15);
    for field in ["dexterityModifier", "mods"] {
        assert!(!row.contains_key(field), "{field}");
        assert!(!s.last(&id).snapshot.contains_key(field), "{field}");
    }
}

#[tokio::test]
async fn two_saves_of_one_element_submitted_together_queue_and_neither_is_rejected() {
    let s = served().await;
    let (id, result) = s.save(elowen(None, "v1")).await;
    applied(&result);
    let first = s.submit("dm", elowen(Some(&id), "v2"), None).await;
    let second = s.submit("dm-2", elowen(Some(&id), "v3"), None).await;
    assert_eq!(first["partition"], second["partition"]);
    // The second is not refused: its author confirms it overwrites the first.
    assert_eq!(
        second["result"]["status"], "awaiting_confirmation",
        "{second}"
    );
    applied(&s.settled().await);
    let command = second["commandId"].as_str().expect("an id");
    s.engine.confirm(command, "dm-2").await.expect("confirmed");
    applied(&s.settled().await);
    let row = s.row(&id).await;
    assert_eq!(row["notes"], "v3");
    let version = s.last(&id);
    assert_eq!(version.number, 3);
    assert_eq!(
        version.diff,
        [json!({ "field": "notes", "old": "v2", "new": "v3" })]
    );
}

#[tokio::test]
async fn a_save_queued_behind_one_cancelled_takes_the_next_number_and_its_diff_from_the_real_row() {
    let s = served().await;
    let (id, result) = s.save(elowen(None, "v1")).await;
    applied(&result);
    // Held, so neither reaches the head before the first is cancelled.
    let hold = s
        .engine
        .hold(&format!("Entity/{id}"), "dm", None)
        .await
        .expect("held");
    let first = s.submit("dm", elowen(Some(&id), "v2"), None).await;
    let second = s.submit("dm-2", elowen(Some(&id), "v3"), None).await;
    let command = first["commandId"].as_str().expect("an id");
    s.engine.cancel(command, "dm").await.expect("cancelled");
    let command = second["commandId"].as_str().expect("an id");
    // Numbered after the one ahead when it was submitted.
    let _ = s.engine.confirm(command, "dm-2").await;
    s.engine.release(&hold.id).await.expect("released");
    applied(&s.settled().await);
    // The engine's row keeps the number the save was prepared with; the
    // version settled follows the real one.
    assert_eq!(s.row(&id).await["versionNumber"], 3);
    let versions = s.history.of(&id);
    assert_eq!(versions.len(), 2, "{versions:?}");
    assert_eq!(versions[1].number, 2);
    assert_eq!(
        versions[1].diff,
        [json!({ "field": "notes", "old": "v1", "new": "v3" })]
    );
}

#[tokio::test]
async fn an_edit_queued_behind_a_create_rejected_is_rejected_too_and_no_element_exists() {
    let s = served().await;
    let (_, result) = s.save(elowen(None, "v1")).await;
    applied(&result);
    // A place: no sheet trips its edit by accident.
    let id = "e1000000-0000-4000-8000-0000000000aa";
    let hold = s
        .engine
        .hold(&format!("Entity/{id}"), "dm", None)
        .await
        .expect("held");
    // Its alias is Elowen's: the create is rejected once it reaches the head.
    s.submit(
        "dm",
        json!({
            "campaign": VALOMBRE,
            "entity": { "id": id, "kind": "lieu", "name": "La Clairière", "aliases": ["la rodeuse"] },
        }),
        None,
    )
    .await;
    let edit = s
        .submit(
            "dm-2",
            json!({ "entity": { "id": id, "notes": "Un cercle de pierres." } }),
            None,
        )
        .await;
    let command = edit["commandId"].as_str().expect("an id");
    if edit["result"]["status"] == "awaiting_confirmation" {
        s.engine.confirm(command, "dm-2").await.expect("confirmed");
    }
    s.engine.release(&hold.id).await.expect("released");
    let mut settled = s.engine.run_once().await.expect("the applier ran");
    settled.extend(s.engine.run_once().await.expect("the applier ran"));
    assert_eq!(settled.len(), 2, "{settled:?}");
    assert_eq!(settled[0].violations, ["alias-unique-in-campaign"]);
    assert_eq!(settled[1].status, State::Rejected, "{settled:?}");
    assert_eq!(settled[1].violations, ["every-change-versioned"]);
    assert!(s.row(id).await.is_empty(), "{:?}", s.row(id).await);
    assert!(s.history.of(id).is_empty());
}

#[tokio::test]
async fn the_same_idempotency_key_twice_adds_one_version() {
    let s = served().await;
    let (id, result) = s.save(elowen(None, "v1")).await;
    applied(&result);
    let first = s
        .submit("dm", elowen(Some(&id), "v2"), Some("save-elowen-2"))
        .await;
    let second = s
        .submit("dm", elowen(Some(&id), "v2"), Some("save-elowen-2"))
        .await;
    assert_eq!(second["replayed"], true, "{second}");
    assert_eq!(second["commandId"], first["commandId"]);
    applied(&s.settled().await);
    assert!(
        s.engine
            .run_once()
            .await
            .expect("the applier ran")
            .is_empty()
    );
    assert_eq!(s.history.of(&id).len(), 2);
}

#[tokio::test]
async fn a_player_on_an_npc_or_a_sheet_on_a_place_is_rejected_by_character_is_pj_or_pnj() {
    let s = served().await;
    let mut payload = morlox();
    payload["entity"]["player"] = json!(CLAIRE);
    let (_, result) = s.save(payload).await;
    assert_eq!(result.status, State::Rejected, "{result:?}");
    assert_eq!(result.violations, ["character-is-pj-or-pnj"]);
    // A place saved with stats is a place: no sheet is made for it.
    let (id, result) = s
        .save(json!({
            "campaign": VALOMBRE,
            "entity": { "kind": "lieu", "name": "La Taverne du Sanglier", "aliases": ["la taverne"] },
        }))
        .await;
    applied(&result);
    let row = s.row(&id).await;
    assert!(!row.contains_key("characterEntityId"));
    assert!(!row.contains_key("tokenColor"));
}

#[tokio::test]
async fn a_save_the_tables_could_never_hold_is_refused_unprocessable_and_never_queued() {
    let s = served().await;
    for (payload, why) in [
        (
            json!({ "entity": { "kind": "pnj", "name": "Morlox" } }),
            "un nouvel élément nomme sa campagne",
        ),
        (
            json!({ "campaign": "cendres-valombre", "entity": { "kind": "pnj", "name": "Morlox" } }),
            "entityCampaignId n'est pas un identifiant",
        ),
        (
            json!({ "campaign": VALOMBRE, "entity": { "kind": "PNJ", "name": "Morlox" } }),
            "un élément est un pj, un pnj, un lieu ou un evenement",
        ),
    ] {
        match s.submitted("dm", payload, None).await {
            Err(Error::Refused(refusal)) => {
                assert_eq!(refusal.kind, RefusalKind::Unprocessable, "{why}");
                assert_eq!(refusal.message, why);
            }
            other => panic!("{why}: {other:?}"),
        }
    }
    assert!(
        s.engine
            .run_once()
            .await
            .expect("the applier ran")
            .is_empty()
    );
}
