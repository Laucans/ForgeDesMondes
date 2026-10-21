//! The Player aggregate's invariant, judged by the DataGuard as `plant()`
//! binds it, over the in-memory store: a command that makes a player a
//! campaign element — a sheet, a token, a link to a PC — is rejected under
//! `player-not-character`, and the row stays as it was. The DataCapabilities
//! here are test-only: no DataCapability of the product writes a player yet.

use std::sync::Arc;

use harness_dataguard::aggregate::Aggregate;
use harness_dataguard::api::Submission;
use harness_dataguard::command::{CommandResult, State};
use harness_dataguard::engine::Engine;
use harness_dataguard::guard::Caller;
use harness_dataguard::manifest::DataCapability;
use harness_dataguard::memory::{MemoryNotifier, MemoryStore};
use harness_dataguard::postgres::SystemClock;
use serde_json::{Value, json};

/// A player added under the id their payload names.
const ADD: &str = r#"{"dataCapability":"test.addPlayer","owner":"dataguard","version":1,"effect":"insert","target":{"aggregate":"Player","id":"$id"},"mode":"overwrite","callableBy":["*"]}"#;
/// A player's fields changed.
const UPDATE: &str = r#"{"dataCapability":"test.updatePlayer","owner":"dataguard","version":1,"effect":"update","target":{"aggregate":"Player","id":"$id"},"mode":"overwrite","callableBy":["*"]}"#;
/// A command that declares it writes a player's sheet.
const SHEET: &str = r#"{"dataCapability":"test.writePlayerSheet","owner":"dataguard","version":1,"effect":"update","target":{"aggregate":"Player","id":"$id"},"touches":["Player.sheet"],"mode":"overwrite","callableBy":["*"]}"#;
/// A command that declares it renames a player.
const RENAME: &str = r#"{"dataCapability":"test.renamePlayer","owner":"dataguard","version":1,"effect":"update","target":{"aggregate":"Player","id":"$id"},"touches":["Player.name"],"mode":"overwrite","callableBy":["*"]}"#;
/// A command on a player that declares it writes an element's name.
const ELEMENT_NAME: &str = r#"{"dataCapability":"test.nameElement","owner":"dataguard","version":1,"effect":"update","target":{"aggregate":"Player","id":"$id"},"touches":["Entity.name"],"mode":"overwrite","callableBy":["*"]}"#;
/// A player removed: the engine's soft delete, a tombstone.
const REMOVE: &str = r#"{"dataCapability":"test.removePlayer","owner":"dataguard","version":1,"effect":"delete","target":{"aggregate":"Player","id":"$id"},"mode":"overwrite","callableBy":["*"]}"#;
/// A campaign created: another aggregate, which this invariant leaves alone.
const CAMPAIGN: &str = r#"{"dataCapability":"test.createCampaign","owner":"dataguard","version":1,"effect":"insert","target":{"aggregate":"Campaign","id":"$id"},"mode":"overwrite","callableBy":["*"]}"#;

const VALOMBRE: &str = "c1000000-0000-4000-8000-000000000001";
const CLAIRE: &str = "b1000000-0000-4000-8000-000000000001";
const LYRA: &str = "e1000000-0000-4000-8000-000000000001";

/// The engine as the server plants this aggregate — its manifest and its
/// plugin — over the in-memory store.
async fn plant() -> harness_dataguard::Result<Engine> {
    let engine = Engine::new(
        Arc::new(MemoryStore::default()),
        Arc::new(MemoryNotifier::default()),
        Arc::new(SystemClock),
    )
    .with_aggregate(Aggregate::parse(include_str!("../aggregate.json")).expect("contract I"))
    .with_plugins(std::iter::once(aggregate_player::plugin()).collect());
    engine.load().await?;
    Ok(engine)
}

/// The engine as the server plants it, with the test-only DataCapabilities.
async fn engine() -> Engine {
    let mut engine = plant().await.expect("the in-memory engine");
    for manifest in [ADD, UPDATE, SHEET, RENAME, ELEMENT_NAME, REMOVE, CAMPAIGN] {
        engine = engine.with_data_capability(DataCapability::parse(manifest).expect("contract F"));
    }
    engine
}

/// What the single applier made of one command.
async fn written(engine: &Engine, data_capability: &str, payload: Value) -> CommandResult {
    let Value::Object(payload) = payload else {
        panic!("a payload is an object");
    };
    engine
        .submit(Submission {
            data_capability: data_capability.to_string(),
            payload: payload.into_iter().collect(),
            target_id: None,
            based_on: None,
            idempotency_key: None,
            caller: Caller {
                by: "dm".to_string(),
                from: "campaign.players".to_string(),
                permissions: Vec::new(),
                // The critical fields' review is not what these tests judge.
                roles: vec!["role:approver".to_string()],
            },
        })
        .await
        .expect("queued");
    let mut applied = engine.run_once().await.expect("the applier ran");
    assert_eq!(applied.len(), 1, "{applied:?}");
    applied.remove(0)
}

fn rejected_by(result: &CommandResult, id: &str) {
    assert_eq!(result.status, State::Rejected, "{result:?}");
    assert_eq!(result.violations, [id], "{result:?}");
}

fn applied(result: &CommandResult) {
    assert_eq!(result.status, State::Applied, "{result:?}");
}

/// Claire, who plays Lyra at Valombre's table.
fn claire() -> Value {
    json!({
        "id": CLAIRE,
        "campaignId": VALOMBRE,
        "name": "Claire",
        "notes": "Préfère les scènes d'enquête.",
        "createdAt": "2026-10-17T10:00:00Z",
    })
}

fn with(mut base: Value, field: &str, value: Value) -> Value {
    base[field] = value;
    base
}

/// An engine where Claire plays at Valombre.
async fn with_claire() -> Engine {
    let engine = engine().await;
    applied(&written(&engine, "test.addPlayer", claire()).await);
    engine
}

#[tokio::test]
async fn player_not_character_lets_a_player_have_a_name_and_notes() {
    let engine = with_claire().await;
    applied(
        &written(
            &engine,
            "test.updatePlayer",
            json!({ "id": CLAIRE, "name": "Claire D.", "notes": Value::Null }),
        )
        .await,
    );
    // A player with no notes is a player too.
    applied(
        &written(
            &engine,
            "test.addPlayer",
            json!({
                "id": "b1000000-0000-4000-8000-000000000002",
                "campaignId": VALOMBRE,
                "name": "Marc",
            }),
        )
        .await,
    );
}

#[tokio::test]
async fn player_not_character_rejects_a_player_added_with_a_sheet_or_a_token() {
    let engine = engine().await;
    for (field, value) in [
        ("sheet", json!({ "ac": 15, "hp": 22 })),
        (
            "tokenMediaId",
            json!("d1000000-0000-4000-8000-000000000001"),
        ),
        ("abilities", json!({ "str": 10 })),
    ] {
        rejected_by(
            &written(&engine, "test.addPlayer", with(claire(), field, value)).await,
            "player-not-character",
        );
    }
    // Nothing was written: Claire can still be added, as a player.
    applied(&written(&engine, "test.addPlayer", claire()).await);
}

#[tokio::test]
async fn player_not_character_rejects_a_sheet_or_a_token_given_to_a_player() {
    let engine = with_claire().await;
    for (field, value) in [
        ("sheet", json!({ "ac": 15 })),
        (
            "tokenMediaId",
            json!("d1000000-0000-4000-8000-000000000001"),
        ),
        ("kind", json!("pj")),
    ] {
        rejected_by(
            &written(
                &engine,
                "test.updatePlayer",
                json!({ "id": CLAIRE, field: value }),
            )
            .await,
            "player-not-character",
        );
    }
    // The row stayed a player's: a change of name still applies.
    applied(
        &written(
            &engine,
            "test.updatePlayer",
            json!({ "id": CLAIRE, "name": "Claire" }),
        )
        .await,
    );
}

#[tokio::test]
async fn player_not_character_rejects_a_player_pointing_to_their_pc() {
    // The PC points to its player (character.player_id), never the other way
    // round.
    let engine = with_claire().await;
    for field in ["characterId", "playerCharacterIds", "entityId"] {
        rejected_by(
            &written(
                &engine,
                "test.updatePlayer",
                json!({ "id": CLAIRE, field: LYRA }),
            )
            .await,
            "player-not-character",
        );
    }
}

#[tokio::test]
async fn player_not_character_rejects_a_command_declaring_it_touches_a_sheet() {
    let engine = with_claire().await;
    rejected_by(
        &written(
            &engine,
            "test.writePlayerSheet",
            json!({ "id": CLAIRE, "name": "Claire" }),
        )
        .await,
        "player-not-character",
    );
}

#[tokio::test]
async fn player_not_character_lets_a_command_touch_the_player_s_own_fields() {
    let engine = with_claire().await;
    applied(
        &written(
            &engine,
            "test.renamePlayer",
            json!({ "id": CLAIRE, "name": "Claire D." }),
        )
        .await,
    );
}

#[tokio::test]
async fn player_not_character_rejects_a_command_touching_an_element_s_field() {
    // `Entity.name` is an element's name, not the player's.
    let engine = with_claire().await;
    rejected_by(
        &written(
            &engine,
            "test.nameElement",
            json!({ "id": CLAIRE, "name": "Lyra" }),
        )
        .await,
        "player-not-character",
    );
}

#[tokio::test]
async fn player_not_character_leaves_a_player_s_removal_to_the_tombstone() {
    // Even a removal that says why: nothing of the row is left to judge.
    let engine = with_claire().await;
    applied(
        &written(
            &engine,
            "test.removePlayer",
            json!({ "id": CLAIRE, "reason": "a quitté la table" }),
        )
        .await,
    );
}

#[tokio::test]
async fn player_not_character_judges_players_alone() {
    // A campaign has fields no player has: they are not this invariant's.
    let engine = engine().await;
    applied(
        &written(
            &engine,
            "test.createCampaign",
            json!({
                "id": VALOMBRE,
                "name": "Les Cendres de Valombre",
                "kind": "campagne",
                "pitch": "Une cité sous la cendre.",
            }),
        )
        .await,
    );
}
