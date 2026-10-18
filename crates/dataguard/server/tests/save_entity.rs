//! campaign.saveEntity through the wire protocol, over the in-memory engine
//! as the server plants it: a save is prepared against the element's row,
//! judged, queued per element and applied with the version it adds
//! (EntityVersion@1). The tables it reaches are `save_entity_projection.rs`'s.

use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use dataguard_server::{Engine, MEMORY, Planted, Rows, planted, router};
use harness_dataguard::command::{CommandResult, State};
use serde_json::{Map, Value, json};
use tower::ServiceExt;

const VALOMBRE: &str = "c1000000-0000-4000-8000-000000000001";
const BRUME: &str = "c1000000-0000-4000-8000-000000000002";
const CLAIRE: &str = "b1000000-0000-4000-8000-000000000001";

/// The engine as the server plants it, its rows and its wire protocol.
struct Served {
    engine: Arc<Engine>,
    rows: Rows,
    app: Router,
}

async fn served() -> Served {
    let Planted { engine, rows } = planted(MEMORY).await.expect("the in-memory engine");
    let engine = Arc::new(engine);
    let app = router(
        engine.clone(),
        rows.clone(),
        Arc::new(ports_llm::fake::FakeLlm::new()),
    );
    Served { engine, rows, app }
}

impl Served {
    /// `POST /commands` of campaign.saveEntity by `by`, the DM's editor
    /// calling it — the DM approves their own campaign's critical fields, as
    /// the AiSettings Micro-UI has them approve their settings: the answer,
    /// as JSON.
    async fn submit(&self, by: &str, payload: Value, key: Option<&str>) -> Value {
        let mut body = json!({
            "dataCapability": "campaign.saveEntity@1",
            "payload": payload,
            "caller": { "by": by, "from": "campaign.EntityEditor", "roles": ["role:approver"] },
        });
        if let Some(key) = key {
            body["idempotencyKey"] = json!(key);
        }
        let (status, answer) = self.post("/commands", body).await;
        assert_eq!(status, StatusCode::ACCEPTED, "{answer}");
        answer
    }

    async fn post(&self, uri: &str, body: Value) -> (StatusCode, Value) {
        let request = Request::post(uri)
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .expect("a request");
        let response = self.app.clone().oneshot(request).await.expect("an answer");
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("a body");
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
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
        self.rows
            .projected(&format!("Entity/{element}"))
            .await
            .expect("the row")
            .into_iter()
            .collect()
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
fn changed(row: &Map<String, Value>) -> Vec<&str> {
    row["diff"]
        .as_array()
        .expect("a diff")
        .iter()
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
    assert_eq!(
        row["snapshot"],
        json!({
            "type": "pnj",
            "name": "Morlox",
            "notes": "Doit 40 po à Vesna.",
            "aliases": ["le gobelin", "le marchand"],
            "strength": 8, "dexterity": 15, "constitution": 10,
            "intelligence": 13, "wisdom": 9, "charisma": 12,
            "armorClass": 13, "hpMax": 21,
            "spells": [], "actions": [],
            "tokenColor": "danger",
        })
    );
    // The first version is compared with an empty snapshot.
    let snapshot = row["snapshot"].as_object().expect("a snapshot");
    let mut every: Vec<&str> = snapshot.keys().map(String::as_str).collect();
    every.sort_unstable();
    assert_eq!(changed(&row), every);
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
    assert_eq!(
        row["diff"],
        json!([{ "field": "notes", "old": "Cherche son frère.", "new": "Cherche son frère Aerin." }])
    );
    assert_eq!(row["entityCreatedAt"], first["entityCreatedAt"]);
    assert_eq!(row["playerId"], CLAIRE);
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
        assert!(
            !row["snapshot"]
                .as_object()
                .expect("a snapshot")
                .contains_key(field),
            "{field}"
        );
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
    let (status, confirmed) = s
        .post(
            &format!("/commands/{command}/confirm"),
            json!({ "by": "dm-2" }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{confirmed}");
    applied(&s.settled().await);
    let row = s.row(&id).await;
    assert_eq!(row["notes"], "v3");
    assert_eq!(row["versionNumber"], 3);
    assert_eq!(
        row["diff"],
        json!([{ "field": "notes", "old": "v2", "new": "v3" }])
    );
}

#[tokio::test]
async fn a_save_queued_behind_one_cancelled_takes_the_next_number_and_its_diff_from_the_real_row() {
    let s = served().await;
    let (id, result) = s.save(elowen(None, "v1")).await;
    applied(&result);
    // Held, so neither reaches the head before the first is cancelled.
    let (status, hold) = s
        .post(
            "/holds",
            json!({ "partition": format!("Entity/{id}"), "by": "dm" }),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{hold}");
    let first = s.submit("dm", elowen(Some(&id), "v2"), None).await;
    let second = s.submit("dm-2", elowen(Some(&id), "v3"), None).await;
    // Numbered after the one ahead when it was submitted.
    let command = first["commandId"].as_str().expect("an id");
    let (status, _) = s
        .post(
            &format!("/commands/{command}/cancel"),
            json!({ "by": "dm" }),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let command = second["commandId"].as_str().expect("an id");
    s.post(
        &format!("/commands/{command}/confirm"),
        json!({ "by": "dm-2" }),
    )
    .await;
    let hold = hold["id"].as_str().expect("a hold");
    s.post(&format!("/holds/{hold}/release"), json!({})).await;
    applied(&s.settled().await);
    let row = s.row(&id).await;
    assert_eq!(row["versionNumber"], 2);
    assert_eq!(
        row["diff"],
        json!([{ "field": "notes", "old": "v1", "new": "v3" }])
    );
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
    assert_eq!(s.row(&id).await["versionNumber"], 2);
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
async fn a_save_the_tables_could_never_hold_is_refused_422_and_never_queued() {
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
        let (status, answer) = s
            .post(
                "/commands",
                json!({
                    "dataCapability": "campaign.saveEntity@1",
                    "payload": payload,
                    "caller": { "by": "dm", "from": "campaign.EntityEditor", "roles": ["role:approver"] },
                }),
            )
            .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{answer}");
        assert_eq!(answer["error"], why);
    }
    assert!(
        s.engine
            .run_once()
            .await
            .expect("the applier ran")
            .is_empty()
    );
}
