//! campaign.restoreEntityVersion through the engine, over its in-memory
//! store, as the server plants it with the Entity aggregate and
//! campaign.saveEntity: a restoration is prepared against the element's row
//! and the versions the aggregate keeps, judged, queued on the element and
//! applied with the version it adds (EntityVersion@1). The tables it
//! reaches are the Entity aggregate's `tests/projection.rs`.

use std::sync::Arc;

use aggregate_entity::History;
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

/// The fields of the element and of its sheet a snapshot holds, beside its
/// aliases.
const SNAPSHOT: [&str; 24] = [
    "type",
    "name",
    "description",
    "notes",
    "playerId",
    "initialPrompt",
    "background",
    "behavior",
    "strength",
    "dexterity",
    "constitution",
    "intelligence",
    "wisdom",
    "charisma",
    "armorClass",
    "hpMax",
    "spells",
    "actions",
    "portraitMediaId",
    "tokenMediaId",
    "tokenCrop",
    "tokenColor",
    "dndbeyondId",
    "dndbeyondSyncedAt",
];

/// What became of a submission: queued, or refused before it was — as the
/// server answers it, 202, 404 or 422.
#[derive(Debug, PartialEq, Eq)]
enum Status {
    Accepted,
    NotFound,
    Unprocessable,
}

/// The engine as the server plants these DataCapabilities and their
/// aggregate, its store and the versions the aggregate keeps.
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
    .with_data_capability(
        DataCapability::parse(restore_entity_version::MANIFEST).expect("contract F"),
    )
    .with_data_capability(DataCapability::parse(save_entity::MANIFEST).expect("contract F"))
    .with_ids(Arc::new(RandomIds))
    .with_plugins(
        [
            aggregate_entity::keeping(history.clone()),
            save_entity::plugin(),
            restore_entity_version::plugin(),
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
    /// Submits `body`: what became of it, and the answer.
    async fn post(&self, body: Value) -> (Status, Value) {
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

    /// One pass of the single applier, which settles one command.
    async fn settled(&self) -> CommandResult {
        let mut settled = self.engine.run_once().await.expect("the applier ran");
        assert_eq!(settled.len(), 1, "{settled:?}");
        settled.remove(0)
    }

    /// Saves `entity` as the DM, applied: the element's id.
    async fn save(&self, entity: Value) -> String {
        let (status, answer) = self
            .post(json!({
                "dataCapability": "campaign.saveEntity@1",
                "payload": { "campaign": VALOMBRE, "entity": entity },
                "caller": { "by": "dm", "from": "campaign.EntityEditor", "roles": ["role:approver"] },
            }))
            .await;
        assert_eq!(status, Status::Accepted, "{answer}");
        let result = self.settled().await;
        assert_eq!(result.status, State::Applied, "{result:?}");
        answer["partition"]
            .as_str()
            .and_then(|p| p.strip_prefix("Entity/"))
            .expect("an Entity partition")
            .to_owned()
    }

    /// Asks the restoration of `element` that `payload` says, from the
    /// VersionHistory panel, by the DM — who approves their own campaign's
    /// critical fields, as a save of theirs does: the answer.
    async fn restore(&self, element: &str, payload: Value) -> (Status, Value) {
        self.restore_as(&["role:approver"], element, payload).await
    }

    /// The same by a caller of `roles`.
    async fn restore_as(&self, roles: &[&str], element: &str, payload: Value) -> (Status, Value) {
        self.post(json!({
            "dataCapability": "campaign.restoreEntityVersion@1",
            "target": { "aggregate": "Entity", "id": element },
            "payload": payload,
            "idempotencyKey": format!("VersionHistory:{element}:{payload}"),
            "caller": { "by": "dm", "from": "VersionHistory", "roles": roles },
        }))
        .await
    }

    /// The same, accepted, and what the applier made of it.
    async fn restored(&self, element: &str, payload: Value) -> CommandResult {
        let (status, answer) = self.restore(element, payload).await;
        assert_eq!(status, Status::Accepted, "{answer}");
        self.settled().await
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

    /// `element`'s versions, oldest first: number, id, snapshot.
    fn versions(&self, element: &str) -> Vec<(u64, String, Map<String, Value>)> {
        self.history
            .of(element)
            .into_iter()
            .map(|v| (v.number, v.id, v.snapshot))
            .collect()
    }
}

/// Morlox, an NPC of Valombre, as `notes` and `aliases` say, `id` once made.
fn morlox(id: Option<&str>, notes: &str, aliases: &[&str], hp: i64) -> Value {
    let mut entity = json!({
        "kind": "pnj",
        "name": "Morlox",
        "aliases": aliases,
        "notes": notes,
        "stats": { "FOR": 8, "DEX": 15, "CON": 10, "INT": 13, "SAG": 9, "CHA": 12 },
        "ca": 13,
        "pvMax": hp,
        "color": "danger",
    });
    if let Some(id) = id {
        entity["id"] = json!(id);
    }
    entity
}

/// Morlox brought to version 4: his notes, his aliases and his HP moved at
/// each save, version 2 the one restored.
async fn at_version_4(s: &Served) -> String {
    let id = s
        .save(morlox(None, "Doit 40 po à Vesna.", &["le gobelin"], 21))
        .await;
    for (notes, aliases, hp) in [
        (
            "Connaît le passage.",
            &["le gobelin", "le marchand"][..],
            18,
        ),
        ("A trahi le groupe.", &["le marchand", "Morlo"][..], 30),
        ("S'est enfui.", &["Morlo"][..], 30),
    ] {
        s.save(morlox(Some(&id), notes, aliases, hp)).await;
    }
    id
}

/// What the row holds of what a snapshot holds: every field, a `null` for
/// one it lacks, and the aliases.
fn held(row: &Map<String, Value>) -> Map<String, Value> {
    let mut held: Map<String, Value> = SNAPSHOT
        .iter()
        .map(|f| ((*f).to_owned(), row.get(*f).cloned().unwrap_or(Value::Null)))
        .collect();
    held.insert("aliases".into(), row["alias"].clone());
    held
}

/// `snapshot` as [`held`] has it: every field, a `null` for one it lacks.
fn whole(snapshot: &Map<String, Value>) -> Map<String, Value> {
    let mut whole: Map<String, Value> = SNAPSHOT
        .iter()
        .map(|f| {
            let value = snapshot.get(*f).cloned().unwrap_or(Value::Null);
            ((*f).to_owned(), value)
        })
        .collect();
    whole.insert("aliases".into(), snapshot["aliases"].clone());
    whole
}

#[tokio::test]
async fn restoring_version_2_of_an_npc_at_version_4_rewrites_it_as_its_snapshot_field_by_field() {
    let s = served().await;
    let id = at_version_4(&s).await;
    let before = s.versions(&id);
    assert_eq!(before.len(), 4);
    let (_, v2, snapshot) = before[1].clone();

    let result = s.restored(&id, json!({ "version": 2 })).await;
    assert_eq!(result.status, State::Applied, "{result:?}");

    // restore-exact: the element, its sheet and its aliases as version 2.
    let row = s.row(&id).await;
    assert_eq!(held(&row), whole(&snapshot));
    assert_eq!(row["notes"], "Connaît le passage.");
    assert_eq!(row["hpMax"], 18);
    assert_eq!(row["alias"], json!(["le gobelin", "le marchand"]));
    assert_eq!(row["characterEntityId"], id.as_str());
    assert_eq!(row["entityCampaignId"], VALOMBRE);

    // every-change-versioned: version 5, a restoration of version 2, whose
    // diff runs from version 4; versions 1 to 4 as they were.
    assert_eq!(row["versionNumber"], 5);
    assert_eq!(row["source"], "restauration");
    assert_eq!(row["restoredFromVersionId"], v2.as_str());
    let fields: Vec<&str> = row["diff"]
        .as_array()
        .expect("a diff")
        .iter()
        .map(|change| change["field"].as_str().expect("a field"))
        .collect();
    assert_eq!(fields, ["aliases", "hpMax", "notes"]);
    let after = s.versions(&id);
    assert_eq!(after.len(), 5);
    assert_eq!(after[..4], before[..]);
    assert_eq!(after[4].0, 5);
}

#[tokio::test]
async fn a_version_named_by_its_id_in_another_case_is_restored_as_by_its_number() {
    let s = served().await;
    let id = at_version_4(&s).await;
    let (_, v1, snapshot) = s.versions(&id)[0].clone();
    let result = s
        .restored(&id, json!({ "version": v1.to_uppercase() }))
        .await;
    assert_eq!(result.status, State::Applied, "{result:?}");
    let row = s.row(&id).await;
    assert_eq!(held(&row), whole(&snapshot));
    assert_eq!(row["restoredFromVersionId"], v1.as_str());
}

#[tokio::test]
async fn a_restoration_is_a_version_like_any_other_and_can_be_undone_in_turn() {
    let s = served().await;
    let id = at_version_4(&s).await;
    let v4 = s.versions(&id)[3].2.clone();
    s.restored(&id, json!({ "version": 2 })).await;
    let result = s.restored(&id, json!({ "version": 4 })).await;
    assert_eq!(result.status, State::Applied, "{result:?}");
    let row = s.row(&id).await;
    assert_eq!(held(&row), whole(&v4));
    assert_eq!(row["versionNumber"], 6);
}

#[tokio::test]
async fn undoing_some_changes_of_a_version_sets_those_fields_alone_back_to_the_version_before() {
    let s = served().await;
    let id = at_version_4(&s).await;
    let versions = s.versions(&id);
    let (_, v2, before) = versions[1].clone();
    // Version 3's diff: aliases, hpMax, notes — undo the HP and the notes.
    let v3 = s.history.of(&id)[2].diff.clone();
    let at = |field: &str| {
        v3.iter()
            .position(|change| change["field"] == field)
            .expect("a change of version 3")
    };
    let result = s
        .restored(
            &id,
            json!({ "version": 2, "changes": [at("hpMax"), at("notes")] }),
        )
        .await;
    assert_eq!(result.status, State::Applied, "{result:?}");
    let row = s.row(&id).await;
    assert_eq!(row["hpMax"], before["hpMax"]);
    assert_eq!(row["notes"], before["notes"]);
    // What those changes did not name stays as version 4 left it.
    assert_eq!(row["alias"], json!(["Morlo"]));
    assert_eq!(row["source"], "restauration");
    assert_eq!(row["restoredFromVersionId"], v2.as_str());
    assert_eq!(row["versionNumber"], 5);
}

#[tokio::test]
async fn a_restored_alias_another_element_now_holds_is_rejected_and_nothing_changes() {
    let s = served().await;
    let id = at_version_4(&s).await;
    // « le gobelin », Morlox's at version 2, is now Grik's.
    s.save(json!({ "kind": "pnj", "name": "Grik", "aliases": ["Le Gobelin"] }))
        .await;
    let before = s.row(&id).await;
    let result = s.restored(&id, json!({ "version": 2 })).await;
    assert_eq!(result.status, State::Rejected, "{result:?}");
    assert!(
        result
            .violations
            .iter()
            .any(|v| v == "alias-unique-in-campaign"),
        "{result:?}"
    );
    assert_eq!(s.row(&id).await, before);
    assert_eq!(s.versions(&id).len(), 4);
}

#[tokio::test]
async fn a_version_of_another_element_or_none_is_refused_before_it_is_queued() {
    let s = served().await;
    let id = at_version_4(&s).await;
    let other = s.save(morlox(None, "Un autre.", &["le second"], 10)).await;
    let (_, theirs, _) = s.versions(&other)[0].clone();
    for payload in [
        json!({ "version": theirs }),
        json!({ "version": 9 }),
        json!({ "version": "f0000000-0000-4000-8000-000000000000" }),
    ] {
        let (status, answer) = s.restore(&id, payload.clone()).await;
        assert_eq!(status, Status::NotFound, "{payload}: {answer}");
        assert!(answer["error"].is_string(), "{answer}");
    }
    // An element with no version has none to restore, nor does one named in
    // another case than the engine holds it by.
    for element in [
        "e1000000-0000-4000-8000-00000000dead".to_owned(),
        id.to_uppercase(),
    ] {
        let (status, answer) = s.restore(&element, json!({ "version": 1 })).await;
        assert_eq!(status, Status::NotFound, "{element}: {answer}");
    }
    // A request naming no element at all.
    let (status, answer) = s
        .post(json!({
            "dataCapability": "campaign.restoreEntityVersion@1",
            "payload": { "version": 1 },
            "caller": { "by": "dm", "from": "VersionHistory", "roles": ["role:approver"] },
        }))
        .await;
    assert_eq!(status, Status::Unprocessable, "{answer}");
    for payload in [
        json!({}),
        json!({ "version": 2, "changes": [] }),
        json!({ "version": 2, "changes": ["notes"] }),
        json!({ "version": 2, "changes": [99] }),
        json!({ "version": 4, "changes": [0] }),
    ] {
        let (status, answer) = s.restore(&id, payload.clone()).await;
        assert_eq!(status, Status::Unprocessable, "{payload}: {answer}");
    }
    assert!(
        s.engine
            .run_once()
            .await
            .expect("the applier ran")
            .is_empty()
    );
    assert_eq!(s.versions(&id).len(), 4);
}

#[tokio::test]
async fn restoring_a_version_of_another_type_brings_its_sheet_back_or_drops_it() {
    let s = served().await;
    let id = s
        .save(morlox(None, "Doit 40 po à Vesna.", &["le gobelin"], 21))
        .await;
    let v1 = s.versions(&id)[0].2.clone();
    // Morlox made a place: his sheet goes.
    s.save(json!({ "id": id, "kind": "lieu", "name": "Morlox" }))
        .await;
    assert_eq!(s.row(&id).await["characterEntityId"], Value::Null);
    let result = s.restored(&id, json!({ "version": 1 })).await;
    assert_eq!(result.status, State::Applied, "{result:?}");
    let row = s.row(&id).await;
    assert_eq!(held(&row), whole(&v1));
    assert_eq!(row["characterEntityId"], id.as_str());
    assert_eq!(row["hpMax"], 21);

    let result = s.restored(&id, json!({ "version": 2 })).await;
    assert_eq!(result.status, State::Applied, "{result:?}");
    let row = s.row(&id).await;
    assert_eq!(row["type"], "lieu");
    assert_eq!(row["characterEntityId"], Value::Null);
    assert_eq!(row["hpMax"], Value::Null);
}

#[tokio::test]
async fn a_restoration_by_a_caller_who_approves_nothing_waits_for_its_review() {
    let s = served().await;
    let id = at_version_4(&s).await;
    let (status, answer) = s.restore_as(&[], &id, json!({ "version": 2 })).await;
    assert_eq!(status, Status::Accepted, "{answer}");
    assert_eq!(answer["result"]["status"], "awaiting_review", "{answer}");
    assert!(
        s.engine
            .run_once()
            .await
            .expect("the applier ran")
            .is_empty()
    );
    assert_eq!(s.versions(&id).len(), 4);
}

#[tokio::test]
async fn the_same_idempotency_key_twice_is_one_restoration() {
    let s = served().await;
    let id = at_version_4(&s).await;
    let (_, first) = s.restore(&id, json!({ "version": 2 })).await;
    let (_, second) = s.restore(&id, json!({ "version": 2 })).await;
    assert_eq!(second["replayed"], true, "{second}");
    assert_eq!(second["commandId"], first["commandId"]);
    assert_eq!(s.settled().await.status, State::Applied);
    assert!(
        s.engine
            .run_once()
            .await
            .expect("the applier ran")
            .is_empty()
    );
    assert_eq!(s.versions(&id).len(), 5);
}

#[tokio::test]
async fn an_element_its_payload_alone_names_is_restored_as_the_one_targeted_would_be() {
    let s = served().await;
    let id = at_version_4(&s).await;
    let (status, answer) = s
        .post(json!({
            "dataCapability": "campaign.restoreEntityVersion@1",
            "payload": { "entity": id, "version": 2 },
            "caller": { "by": "dm", "from": "VersionHistory", "roles": ["role:approver"] },
        }))
        .await;
    assert_eq!(status, Status::Accepted, "{answer}");
    assert_eq!(answer["partition"], format!("Entity/{id}"));
    assert_eq!(s.settled().await.status, State::Applied);
    assert_eq!(s.row(&id).await["notes"], "Connaît le passage.");
    assert_eq!(s.versions(&id).len(), 5);
}
