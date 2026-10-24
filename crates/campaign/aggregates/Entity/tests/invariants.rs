//! The Entity aggregate's invariants, judged by the DataGuard as `plant()`
//! binds it, over the in-memory store: a command that breaks one is rejected
//! under its id, and the row stays as it was. What reads other rows — the
//! player's campaign, the other elements' aliases, the versions written — is
//! the tables' (`entity_migration.rs`). The DataCapabilities here are
//! test-only: no DataCapability of the product writes an element yet.

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

/// An element created under the id its payload names.
const CREATE: &str = r#"{"dataCapability":"test.createEntity","owner":"dataguard","version":1,"effect":"insert","target":{"aggregate":"Entity","id":"$entityId"},"mode":"overwrite","callableBy":["*"]}"#;
/// An element saved.
const SAVE: &str = r#"{"dataCapability":"test.saveEntity","owner":"dataguard","version":1,"effect":"update","target":{"aggregate":"Entity","id":"$entityId"},"mode":"overwrite","callableBy":["*"]}"#;
/// Hit points taken off, composed with whatever is ahead.
const DAMAGE: &str = r#"{"dataCapability":"test.damageEntity","owner":"dataguard","version":1,"effect":"update","target":{"aggregate":"Entity","id":"$entityId"},"mode":"relative","callableBy":["*"]}"#;
/// A command that declares it writes the DEX modifier.
const DEX_MODIFIER: &str = r#"{"dataCapability":"test.typeDexModifier","owner":"dataguard","version":1,"effect":"update","target":{"aggregate":"Entity","id":"$entityId"},"touches":["Entity.dexterity","Entity.dexterityModifier"],"mode":"overwrite","callableBy":["*"]}"#;
/// A command that declares it writes the DEX score alone.
const DEXTERITY: &str = r#"{"dataCapability":"test.setDexterity","owner":"dataguard","version":1,"effect":"update","target":{"aggregate":"Entity","id":"$entityId"},"touches":["Entity.dexterity"],"mode":"overwrite","callableBy":["*"]}"#;
/// An element removed: the engine's soft delete, a tombstone.
const REMOVE: &str = r#"{"dataCapability":"test.removeEntity","owner":"dataguard","version":1,"effect":"delete","target":{"aggregate":"Entity","id":"$entityId"},"mode":"overwrite","callableBy":["*"]}"#;
/// A campaign created: another aggregate, which these invariants leave alone.
const CAMPAIGN: &str = r#"{"dataCapability":"test.createCampaign","owner":"dataguard","version":1,"effect":"insert","target":{"aggregate":"Campaign","id":"$id"},"mode":"overwrite","callableBy":["*"]}"#;

const VALOMBRE: &str = "c1000000-0000-4000-8000-000000000001";
const CLAIRE: &str = "b1000000-0000-4000-8000-000000000001";
const ELOWEN: &str = "e1000000-0000-4000-8000-000000000001";
const TAVERNE: &str = "e1000000-0000-4000-8000-000000000002";
const PORTRAIT: &str = "d1000000-0000-4000-8000-000000000001";
/// A PC imported from D&D Beyond.
const THORIN: &str = "e1000000-0000-4000-8000-000000000004";
/// When an element was made: a whole element holds it (every-change-versioned).
const CREATED: &str = "2026-10-17T10:00:00Z";

/// The engine as the server plants this aggregate — its manifest and its
/// plugin — over the in-memory store.
async fn plant() -> harness_dataguard::Result<Engine> {
    let engine = Engine::new(
        Arc::new(MemoryStore::default()),
        Arc::new(MemoryNotifier::default()),
        Arc::new(SystemClock),
    )
    .with_aggregate(Aggregate::parse(include_str!("../aggregate.json")).expect("contract I"))
    .with_plugins(std::iter::once(aggregate_entity::plugin()).collect());
    engine.load().await?;
    Ok(engine)
}

/// The engine as the server plants it, with the test-only DataCapabilities.
async fn engine() -> Engine {
    let mut engine = plant().await.expect("the in-memory engine");
    for manifest in [
        CREATE,
        SAVE,
        DAMAGE,
        DEX_MODIFIER,
        DEXTERITY,
        REMOVE,
        CAMPAIGN,
    ] {
        engine = engine.with_data_capability(DataCapability::parse(manifest).expect("contract F"));
    }
    engine
}

/// The submission of one command, which the engine refuses before it is
/// queued: why.
async fn refused(engine: &Engine, data_capability: &str, payload: Value) -> String {
    let Value::Object(payload) = payload else {
        panic!("a payload is an object");
    };
    let refusal = engine
        .submit(Submission {
            data_capability: data_capability.to_string(),
            payload: payload.into_iter().collect(),
            caller: Caller {
                by: "dm".to_string(),
                from: "campaign.entityEditor".to_string(),
                permissions: Vec::new(),
                roles: vec!["role:approver".to_string()],
            },
            ..Submission::default()
        })
        .await
        .expect_err("refused");
    assert!(
        engine.run_once().await.expect("the applier ran").is_empty(),
        "nothing was queued"
    );
    refusal.to_string()
}

/// What the single applier made of one command by `by`.
async fn written_by(
    engine: &Engine,
    by: &str,
    data_capability: &str,
    payload: Value,
) -> CommandResult {
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
                by: by.to_string(),
                from: "campaign.entityEditor".to_string(),
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

/// What the single applier made of one command by the DM.
async fn written(engine: &Engine, data_capability: &str, payload: Value) -> CommandResult {
    written_by(engine, "dm", data_capability, payload).await
}

fn rejected_by(result: &CommandResult, id: &str) {
    assert_eq!(result.status, State::Rejected, "{result:?}");
    assert_eq!(result.violations, [id], "{result:?}");
}

fn applied(result: &CommandResult) {
    assert_eq!(result.status, State::Applied, "{result:?}");
}

fn with(mut base: Value, field: &str, value: Value) -> Value {
    base[field] = value;
    base
}

fn without(mut base: Value, field: &str) -> Value {
    base.as_object_mut().expect("an object").remove(field);
    base
}

/// Version `number` of an element, from `source`, with its snapshot and diff.
fn version(mut save: Value, number: u64, source: &str) -> Value {
    let id = format!("f1000000-0000-4000-8000-{number:012}");
    for (field, value) in [
        ("entityVersionId", json!(id)),
        ("versionNumber", json!(number)),
        ("source", json!(source)),
        ("snapshot", json!({ "name": "Elowen", "type": "pj" })),
        ("diff", json!([])),
    ] {
        save[field] = value;
    }
    save
}

/// Elowen, the ranger Claire plays at Valombre, as her first save writes her.
fn elowen() -> Value {
    version(
        json!({
            "entityId": ELOWEN,
            "entityCampaignId": VALOMBRE,
            "entityCreatedAt": CREATED,
            "type": "pj",
            "name": "Elowen",
            "notes": "Cache une lettre de sa sœur.",
            "characterEntityId": ELOWEN,
            "playerId": CLAIRE,
            "background": "Rôdeuse des marais de Valombre.",
            "dexterity": 18,
            "armorClass": 15,
            "hpMax": 24,
            "spells": [],
            "actions": [{ "name": "Arc long", "text": "+6 au toucher, 1d8+4" }],
            "tokenColor": "#2f7d5b",
        }),
        1,
        "dm",
    )
}

/// La Taverne du Sanglier, a place: no sheet.
fn taverne() -> Value {
    version(
        json!({
            "entityId": TAVERNE,
            "entityCampaignId": VALOMBRE,
            "entityCreatedAt": CREATED,
            "type": "lieu",
            "name": "La Taverne du Sanglier",
            "description": "Une salle basse, enfumée.",
        }),
        1,
        "dm",
    )
}

/// A second save of Elowen, version 2 from `source`, writing `fields`.
fn save(source: &str, fields: Value) -> Value {
    let mut save = version(json!({ "entityId": ELOWEN }), 2, source);
    for (field, value) in fields.as_object().expect("an object") {
        save[field] = value.clone();
    }
    save
}

/// An engine holding Elowen, version 1.
async fn with_elowen() -> Engine {
    let engine = engine().await;
    applied(&written(&engine, "test.createEntity", elowen()).await);
    engine
}

#[tokio::test]
async fn an_element_saved_whole_with_its_version_is_applied() {
    let engine = with_elowen().await;
    applied(&written(&engine, "test.createEntity", taverne()).await);
    applied(
        &written(
            &engine,
            "test.saveEntity",
            save("dm", json!({ "hpMax": 27 })),
        )
        .await,
    );
    // An NPC has a sheet and no player.
    applied(
        &written(
            &engine,
            "test.createEntity",
            version(
                json!({
                    "entityId": "e1000000-0000-4000-8000-000000000003",
                    "entityCampaignId": VALOMBRE,
                    "entityCreatedAt": CREATED,
                    "type": "pnj",
                    "name": "Morlox",
                    "hpMax": 18,
                    "tokenColor": "#a33",
                }),
                1,
                "brief",
            ),
        )
        .await,
    );
}

#[tokio::test]
async fn character_is_pj_or_pnj_rejects_a_sheet_on_a_place_or_an_event() {
    let engine = engine().await;
    for (kind, field, value) in [
        ("lieu", "hpMax", json!(12)),
        ("lieu", "tokenColor", json!("#555")),
        ("evenement", "background", json!("Un orage.")),
        ("evenement", "characterEntityId", json!(TAVERNE)),
    ] {
        // With its token's colour, so the sheet is all that is wrong.
        let place = with(
            with(taverne(), "type", json!(kind)),
            "tokenColor",
            json!("#888"),
        );
        let place = with(place, field, value);
        rejected_by(
            &written(&engine, "test.createEntity", place).await,
            "character-is-pj-or-pnj",
        );
    }
    // Nothing was written: the place is still created, with no sheet.
    applied(&written(&engine, "test.createEntity", taverne()).await);
    // Nor can it become a character's sheet afterwards.
    let sheet = version(
        json!({ "entityId": TAVERNE, "hpMax": 12, "tokenColor": "#888" }),
        2,
        "dm",
    );
    rejected_by(
        &written(&engine, "test.saveEntity", sheet).await,
        "character-is-pj-or-pnj",
    );
}

#[tokio::test]
async fn character_is_pj_or_pnj_rejects_a_player_on_an_npc() {
    let engine = engine().await;
    let npc = with(elowen(), "type", json!("pnj"));
    rejected_by(
        &written(&engine, "test.createEntity", npc).await,
        "character-is-pj-or-pnj",
    );
    // A PC turned NPC keeps no player.
    applied(&written(&engine, "test.createEntity", elowen()).await);
    rejected_by(
        &written(
            &engine,
            "test.saveEntity",
            save("dm", json!({ "type": "pnj" })),
        )
        .await,
        "character-is-pj-or-pnj",
    );
    applied(
        &written(
            &engine,
            "test.saveEntity",
            save("dm", json!({ "type": "pnj", "playerId": null })),
        )
        .await,
    );
    // A sheet with no type is no PJ's or PNJ's either — nor a whole element.
    let result = written(
        &engine,
        "test.createEntity",
        without(
            with(
                elowen(),
                "entityId",
                json!("e1000000-0000-4000-8000-000000000009"),
            ),
            "type",
        ),
    )
    .await;
    assert_eq!(result.status, State::Rejected, "{result:?}");
    assert_eq!(
        result.violations,
        ["character-is-pj-or-pnj", "every-change-versioned"]
    );
}

#[tokio::test]
async fn every_change_versioned_rejects_a_save_with_no_complete_version() {
    let engine = with_elowen().await;
    for broken in [
        without(save("dm", json!({ "notes": "…" })), "versionNumber"),
        without(save("dm", json!({ "notes": "…" })), "snapshot"),
        without(save("dm", json!({ "notes": "…" })), "diff"),
        without(save("dm", json!({ "notes": "…" })), "source"),
        save("journal", json!({ "notes": "…" })),
        with(
            save("dm", json!({ "notes": "…" })),
            "snapshot",
            json!("Elowen"),
        ),
        with(save("dm", json!({ "notes": "…" })), "diff", json!({})),
        with(
            save("dm", json!({ "notes": "…" })),
            "versionNumber",
            json!(0),
        ),
        // A bare write of the element, no version at all.
        json!({ "entityId": ELOWEN, "notes": "…" }),
    ] {
        rejected_by(
            &written(&engine, "test.saveEntity", broken).await,
            "every-change-versioned",
        );
    }
    // A new element missing what an element holds from its creation on — as
    // a save queued behind a creation that was never applied has it.
    for (field, id) in [
        ("entityCampaignId", "e1000000-0000-4000-8000-000000000011"),
        ("name", "e1000000-0000-4000-8000-000000000012"),
        ("entityCreatedAt", "e1000000-0000-4000-8000-000000000013"),
    ] {
        let place = with(taverne(), "entityId", json!(id));
        rejected_by(
            &written(&engine, "test.createEntity", without(place, field)).await,
            "every-change-versioned",
        );
    }
    let place = with(
        taverne(),
        "entityId",
        json!("e1000000-0000-4000-8000-000000000014"),
    );
    rejected_by(
        &written(&engine, "test.createEntity", without(place, "type")).await,
        "every-change-versioned",
    );
}

#[tokio::test]
async fn every_change_versioned_rejects_a_version_written_again() {
    let engine = with_elowen().await;
    // Version 1 again, edited.
    let edited = version(json!({ "entityId": ELOWEN, "notes": "…" }), 1, "dm");
    rejected_by(
        &written(&engine, "test.saveEntity", edited).await,
        "every-change-versioned",
    );
    // Its number again, under a new id.
    let renumbered = with(
        version(json!({ "entityId": ELOWEN, "notes": "…" }), 1, "dm"),
        "entityVersionId",
        json!("f1000000-0000-4000-8000-0000000000ff"),
    );
    rejected_by(
        &written(&engine, "test.saveEntity", renumbered).await,
        "every-change-versioned",
    );
    // A new number under the id of the version written.
    let reused = with(
        save("dm", json!({ "notes": "…" })),
        "entityVersionId",
        json!("f1000000-0000-4000-8000-000000000001"),
    );
    rejected_by(
        &written(&engine, "test.saveEntity", reused).await,
        "every-change-versioned",
    );
    applied(
        &written(
            &engine,
            "test.saveEntity",
            save("dm", json!({ "notes": "…" })),
        )
        .await,
    );
}

#[tokio::test]
async fn restore_exact_wants_a_restoration_to_name_the_version_it_restores() {
    let engine = with_elowen().await;
    rejected_by(
        &written(&engine, "test.saveEntity", save("restauration", json!({}))).await,
        "restore-exact",
    );
    // Not itself.
    let itself = save(
        "restauration",
        json!({ "restoredFromVersionId": "f1000000-0000-4000-8000-000000000002" }),
    );
    rejected_by(
        &written(&engine, "test.saveEntity", itself).await,
        "restore-exact",
    );
    // A save that is not a restoration restores nothing.
    let claimed = save(
        "dm",
        json!({ "restoredFromVersionId": "f1000000-0000-4000-8000-000000000001" }),
    );
    rejected_by(
        &written(&engine, "test.saveEntity", claimed).await,
        "restore-exact",
    );
    let restoration = save(
        "restauration",
        json!({ "restoredFromVersionId": "f1000000-0000-4000-8000-000000000001" }),
    );
    applied(&written(&engine, "test.saveEntity", restoration).await);
    // The next save does not inherit what the restoration restored.
    let next = version(json!({ "entityId": ELOWEN, "hpMax": 30 }), 3, "dm");
    applied(&written(&engine, "test.saveEntity", next).await);
}

#[tokio::test]
async fn dndbeyond_resync_scope_changes_stats_ac_hp_spells_and_actions_alone() {
    let engine = with_elowen().await;
    for (field, value) in [
        ("alias", json!(["la rôdeuse"])),
        ("notes", json!("Effacées par la resynchro.")),
        ("tokenColor", json!("#000")),
        ("portraitMediaId", json!(PORTRAIT)),
        ("name", json!("Elowen Brightwood")),
    ] {
        rejected_by(
            &written(
                &engine,
                "test.saveEntity",
                save("dndbeyond", json!({ field: value, "hpMax": 26 })),
            )
            .await,
            "dndbeyond-resync-scope",
        );
    }
    // What D&D Beyond says, and what stays as it was sent again unchanged.
    applied(
        &written(
            &engine,
            "test.saveEntity",
            save(
                "dndbeyond",
                json!({
                    "dexterity": 19, "armorClass": 16, "hpMax": 26,
                    "spells": [{ "name": "Marque du chasseur" }],
                    "actions": [{ "name": "Arc long", "text": "+7 au toucher" }],
                    "dndbeyondId": "ddb-118", "dndbeyondSyncedAt": "2026-10-17T10:00:00Z",
                    "notes": "Cache une lettre de sa sœur.", "tokenColor": "#2f7d5b",
                }),
            ),
        )
        .await,
    );
}

/// Thorin, a PC imported from D&D Beyond: what the import writes on an empty
/// row, version 1.
fn thorin() -> Value {
    version(
        json!({
            "entityId": THORIN,
            "entityCampaignId": VALOMBRE,
            "entityCreatedAt": CREATED,
            "type": "pj",
            "name": "Thorin Brisepierre",
            "characterEntityId": THORIN,
            "playerId": CLAIRE,
            "strength": 16, "dexterity": 10, "constitution": 16,
            "intelligence": 9, "wisdom": 13, "charisma": 11,
            "armorClass": 18,
            "hpMax": 44,
            "spells": [{ "name": "Bénédiction" }],
            "actions": [{ "name": "Marteau de guerre", "text": "+6 au toucher, 1d8+3" }],
            "tokenColor": "#8a5a2b",
            "dndbeyondId": "112850214",
            "dndbeyondSyncedAt": "2026-10-17T10:00:00Z",
        }),
        1,
        "dndbeyond",
    )
}

/// A resync of Thorin, version 2 from D&D Beyond, writing `fields`.
fn resync(fields: Value) -> Value {
    let mut save = version(json!({ "entityId": THORIN }), 2, "dndbeyond");
    for (field, value) in fields.as_object().expect("an object") {
        save[field] = value.clone();
    }
    save
}

#[tokio::test]
async fn dndbeyond_resync_scope_lets_an_import_create_its_pc_whole() {
    let engine = engine().await;
    let created = written(&engine, "test.createEntity", thorin()).await;
    applied(&created);
    assert!(created.violations.is_empty(), "{created:?}");
}

#[tokio::test]
async fn dndbeyond_resync_scope_holds_a_pc_linked_to_dndbeyond_to_its_sheet_numbers() {
    let engine = engine().await;
    applied(&written(&engine, "test.createEntity", thorin()).await);
    for (field, value) in [
        ("name", json!("Thorin le Rouge")),
        ("alias", json!(["le nain"])),
        ("notes", json!("Effacées par la resynchro.")),
        ("tokenColor", json!("#000")),
        ("portraitMediaId", json!(PORTRAIT)),
    ] {
        rejected_by(
            &written(
                &engine,
                "test.saveEntity",
                resync(json!({ field: value, "hpMax": 52 })),
            )
            .await,
            "dndbeyond-resync-scope",
        );
    }
    // A level up: the scores, the AC, the HP max, the spells, the actions.
    applied(
        &written(
            &engine,
            "test.saveEntity",
            resync(json!({
                "strength": 17, "dexterity": 10, "constitution": 16,
                "intelligence": 9, "wisdom": 14, "charisma": 11,
                "armorClass": 19, "hpMax": 52,
                "spells": [{ "name": "Bénédiction" }, { "name": "Arme spirituelle" }],
                "actions": [{ "name": "Marteau de guerre", "text": "+7 au toucher, 1d8+3" }],
                "dndbeyondSyncedAt": "2026-10-24T10:00:00Z",
            })),
        )
        .await,
    );
}

#[tokio::test]
async fn proposal_fills_sheet_fields_and_nothing_else() {
    let engine = with_elowen().await;
    for (field, value) in [
        ("notes", json!("Le DM seul les écrit.")),
        ("spells", json!([{ "name": "Soins" }])),
        ("portraitMediaId", json!(PORTRAIT)),
        ("name", json!("Elowen l'IA")),
    ] {
        rejected_by(
            &written(
                &engine,
                "test.saveEntity",
                save("ia", json!({ field: value, "behavior": "Taciturne." })),
            )
            .await,
            "proposal-fills-sheet-fields",
        );
    }
    applied(
        &written(
            &engine,
            "test.saveEntity",
            save(
                "ia",
                json!({
                    "background": "Née dans les marais.", "behavior": "Taciturne.",
                    "strength": 12, "dexterity": 18, "armorClass": 15, "hpMax": 24,
                    "actions": [{ "name": "Dague", "text": "+6, 1d4+4" }],
                    "initialPrompt": "Une rôdeuse taciturne des marais.",
                }),
            ),
        )
        .await,
    );
}

#[tokio::test]
async fn explicit_save_only_rejects_an_increment_or_an_authorless_write() {
    let engine = with_elowen().await;
    rejected_by(
        &written(
            &engine,
            "test.damageEntity",
            save("dm", json!({ "hpMax": -3 })),
        )
        .await,
        "explicit-save-only",
    );
    rejected_by(
        &written_by(
            &engine,
            " ",
            "test.saveEntity",
            save("dm", json!({ "hpMax": 21 })),
        )
        .await,
        "explicit-save-only",
    );
    applied(
        &written(
            &engine,
            "test.saveEntity",
            save("dm", json!({ "hpMax": 21 })),
        )
        .await,
    );
}

#[tokio::test]
async fn default_token_wants_a_colour_and_a_portrait_under_a_crop() {
    let engine = engine().await;
    for sheet in [
        without(elowen(), "tokenColor"),
        with(elowen(), "tokenColor", json!("  ")),
        with(
            elowen(),
            "tokenCrop",
            json!({ "x": 0.2, "y": 0.1, "size": 0.5 }),
        ),
    ] {
        rejected_by(
            &written(&engine, "test.createEntity", sheet).await,
            "default-token",
        );
    }
    applied(&written(&engine, "test.createEntity", elowen()).await);
    // A crop of the portrait the sheet has.
    applied(
        &written(
            &engine,
            "test.saveEntity",
            save(
                "dm",
                json!({ "portraitMediaId": PORTRAIT, "tokenCrop": { "x": 0.2, "y": 0.1, "size": 0.5 } }),
            ),
        )
        .await,
    );
    // The portrait taken away leaves a crop of nothing.
    let removed = version(
        json!({ "entityId": ELOWEN, "portraitMediaId": null }),
        3,
        "dm",
    );
    rejected_by(
        &written(&engine, "test.saveEntity", removed).await,
        "default-token",
    );
}

#[tokio::test]
async fn dex_modifier_derived_refuses_a_typed_modifier() {
    let engine = engine().await;
    // dex-modifier-derived: the modifier is derived, so the engine refuses a
    // command writing it before it is queued.
    let why = refused(
        &engine,
        "test.createEntity",
        with(elowen(), "dexterityModifier", json!(4)),
    )
    .await;
    assert!(why.contains("dexterityModifier"), "{why}");
    applied(&written(&engine, "test.createEntity", elowen()).await);
    // A command declaring it writes the modifier is refused, whatever it sends.
    let why = refused(
        &engine,
        "test.typeDexModifier",
        save("dm", json!({ "dexterity": 16 })),
    )
    .await;
    assert!(why.contains("dexterityModifier"), "{why}");
    // The score is written; the modifier follows from it.
    applied(
        &written(
            &engine,
            "test.setDexterity",
            save("dm", json!({ "dexterity": 16 })),
        )
        .await,
    );
}

#[tokio::test]
async fn a_removal_and_another_aggregate_are_not_judged() {
    let engine = with_elowen().await;
    applied(&written(&engine, "test.removeEntity", json!({ "entityId": ELOWEN })).await);
    // A campaign's row is no element's: no version, no sheet rule.
    applied(
        &written(
            &engine,
            "test.createCampaign",
            json!({ "id": VALOMBRE, "name": "Valombre", "kind": "campagne", "hpMax": 3 }),
        )
        .await,
    );
}

#[tokio::test]
async fn alias_unique_in_campaign_reads_the_terms_the_plugin_keeps_of_every_applied_element() {
    let engine = with_elowen().await;
    // The tavern answering to « Elowen »: her name, recorded once applied.
    let alias = |name: &str| with(taverne(), "alias", json!([name]));
    rejected_by(
        &written(&engine, "test.createEntity", alias("ELOWEN")).await,
        "alias-unique-in-campaign",
    );
    // Removed, she frees her name.
    applied(&written(&engine, "test.removeEntity", json!({ "entityId": ELOWEN })).await);
    applied(&written(&engine, "test.createEntity", alias("Elowen")).await);
}
