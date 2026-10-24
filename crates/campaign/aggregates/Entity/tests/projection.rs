//! The Entity aggregate over PostgreSQL (`DATABASE_URL`), as the server
//! plants it with campaign.saveEntity and campaign.restoreEntityVersion:
//! what the engine applies on `Entity/<id>` reaches `entity`, `character`,
//! `entity_alias` and `entity_version` in the apply transaction, each save
//! adding its version — settled after the element's last one there — and
//! leaving the earlier ones as they were; a write the tables refuse is
//! rejected and rolled back; and a start reads the elements and the versions
//! the tables hold, and projects what they lost. Each test works in a
//! scratch database of its own, migrated as the server migrates it, dropped
//! when it ends.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::{Arc, Mutex};

use aggregate_entity::History;
use async_trait::async_trait;
use harness_dataguard::aggregate::Aggregate;
use harness_dataguard::api::Submission;
use harness_dataguard::command::{CommandResult, State};
use harness_dataguard::engine::Engine;
use harness_dataguard::manifest::DataCapability;
use harness_dataguard::memory::MemoryNotifier;
use harness_dataguard::plugins::{Context, Plugin, Plugins, Prepare, Prepared, Refusal};
use harness_dataguard::postgres::{PgStore, SystemClock};
use harness_dataguard::store::RandomIds;
use harness_dataguard::tokio_postgres::{self, Client, NoTls};
use serde_json::{Value, json};

const VALOMBRE: &str = "c1000000-0000-4000-8000-000000000001";
const CLAIRE: &str = "b1000000-0000-4000-8000-000000000001";
/// A player the tables do not hold, until a test adds her.
const NOBODY: &str = "b1000000-0000-4000-8000-0000000000ff";

fn url() -> String {
    std::env::var("DATABASE_URL").expect("DATABASE_URL names the harness's PostgreSQL")
}

async fn connect(url: &str) -> Client {
    let (client, connection) = tokio_postgres::connect(url, NoTls)
        .await
        .expect("PostgreSQL");
    tokio::spawn(connection);
    client
}

/// `DATABASE_URL` naming the database `name` instead.
fn naming(name: &str) -> String {
    let url = url();
    let (address, query) = url.split_once('?').unwrap_or((&url, ""));
    let (server, _) = address.rsplit_once('/').expect("a URL naming its database");
    let query = if query.is_empty() {
        String::new()
    } else {
        format!("?{query}")
    };
    format!("{server}/{name}{query}")
}

/// A database of its own.
struct Scratch {
    name: String,
    url: String,
    client: Client,
}

impl Scratch {
    /// Migrated as the server migrates it, holding Valombre and Claire, its
    /// player.
    async fn new(name: &str) -> Scratch {
        let mut s = Self::bare(name).await;
        let migrations = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../data/migrations");
        data::migrate(&mut s.client, &migrations)
            .await
            .expect("migrated");
        s.client
            .batch_execute(&format!(
                "INSERT INTO campaign (id, name, kind) VALUES ('{VALOMBRE}', 'Les Cendres de Valombre', 'campagne');
                 INSERT INTO player (id, campaign_id, name) VALUES ('{CLAIRE}', '{VALOMBRE}', 'Claire');"
            ))
            .await
            .expect("Valombre and Claire");
        s
    }

    /// Not migrated: none of the product's tables.
    async fn bare(name: &str) -> Scratch {
        let name = format!("entity_projection_test_{name}_{}", std::process::id());
        let admin = connect(&url()).await;
        for sql in [
            format!("DROP DATABASE IF EXISTS {name} WITH (FORCE)"),
            format!("CREATE DATABASE {name}"),
        ] {
            admin.batch_execute(&sql).await.expect("a scratch database");
        }
        let url = naming(&name);
        let client = connect(&url).await;
        Scratch { name, url, client }
    }

    /// The engine as the server plants the aggregate and the two
    /// DataCapabilities that write it over this database, and the versions
    /// the aggregate keeps.
    async fn serve(&self) -> (Engine, Arc<History>) {
        self.telling(Arc::default()).await
    }

    /// The same, its messages sent through `notifier`.
    async fn telling(&self, notifier: Arc<MemoryNotifier>) -> (Engine, Arc<History>) {
        self.planting(notifier, Plugin::new(), &[]).await
    }

    /// The same, with a test's own `plugin` and DataCapabilities beside the
    /// product's.
    async fn planting(
        &self,
        notifier: Arc<MemoryNotifier>,
        plugin: Plugin,
        manifests: &[&str],
    ) -> (Engine, Arc<History>) {
        let history = Arc::new(History::default());
        let plugins: Plugins = [
            aggregate_entity::keeping(history.clone()),
            save_entity::plugin(),
            restore_entity_version::plugin(),
            plugin,
        ]
        .into_iter()
        .collect();
        let store = PgStore::open(connect(&self.url).await)
            .await
            .expect("the engine's tables")
            .with_projections(&plugins)
            .await
            .expect("warmed");
        let engine = Engine::new(Arc::new(store), notifier, Arc::new(SystemClock))
            .with_aggregate(Aggregate::parse(aggregate_entity::MANIFEST).expect("contract I"))
            .with_data_capability(DataCapability::parse(save_entity::MANIFEST).expect("contract F"))
            .with_data_capability(
                DataCapability::parse(restore_entity_version::MANIFEST).expect("contract F"),
            )
            .with_ids(Arc::new(RandomIds))
            .with_plugins(plugins);
        let engine = manifests.iter().fold(engine, |engine, manifest| {
            engine.with_data_capability(DataCapability::parse(manifest).expect("contract F"))
        });
        engine.load().await.expect("the units fit");
        (engine, history)
    }

    /// The rows `sql` selects, each as JSON.
    async fn rows(&self, sql: &str) -> Vec<Value> {
        self.client
            .query(&format!("SELECT to_jsonb(q)::text FROM ({sql}) q"), &[])
            .await
            .expect("the tables")
            .iter()
            .map(|row| serde_json::from_str(row.get::<_, &str>(0)).expect("JSON"))
            .collect()
    }

    async fn drop(self) {
        drop(self.client);
        connect(&url())
            .await
            .batch_execute(&format!("DROP DATABASE {} WITH (FORCE)", self.name))
            .await
            .expect("the scratch database dropped");
    }
}

/// Submits `body` as the DM, approver of their own campaign: the answer.
async fn submit(engine: &Engine, mut body: Value) -> Value {
    body["caller"] =
        json!({ "by": "dm", "from": "campaign.EntityEditor", "roles": ["role:approver"] });
    let submission: Submission = serde_json::from_value(body).expect("a submission");
    let answer = engine.submit(submission).await.expect("queued");
    serde_json::to_value(answer).expect("JSON")
}

/// Submits `body`, then what the applier made of it.
async fn command(engine: &Engine, body: Value) -> (Value, CommandResult) {
    let answer = submit(engine, body).await;
    let mut settled = engine.run_once().await.expect("the applier ran");
    assert_eq!(settled.len(), 1, "{settled:?}");
    (answer, settled.remove(0))
}

/// The element a submission's partition names.
fn element(answer: &Value) -> String {
    answer["partition"]
        .as_str()
        .and_then(|p| p.strip_prefix("Entity/"))
        .expect("an Entity partition")
        .to_owned()
}

/// Saves `payload` as the DM: the element's id, and what the applier made
/// of the save.
async fn save(engine: &Engine, payload: Value) -> (String, CommandResult) {
    let (answer, result) = command(
        engine,
        json!({ "dataCapability": "campaign.saveEntity@1", "payload": payload }),
    )
    .await;
    (element(&answer), result)
}

/// Restores `version` of `id`, as the VersionHistory panel names it.
async fn restore(engine: &Engine, id: &str, version: Value) -> CommandResult {
    command(
        engine,
        json!({
            "dataCapability": "campaign.restoreEntityVersion@1",
            "target": { "aggregate": "Entity", "id": id },
            "payload": { "version": version },
        }),
    )
    .await
    .1
}

fn applied(result: &CommandResult) {
    assert_eq!(result.status, State::Applied, "{result:?}");
}

/// Elowen, Claire's PC, as `notes` and `aliases` say.
fn elowen(id: Option<&str>, notes: &str, aliases: &[&str]) -> Value {
    elowen_at(id, notes, aliases, 44)
}

/// The same, her HP max `hp`.
fn elowen_at(id: Option<&str>, notes: &str, aliases: &[&str], hp: i64) -> Value {
    let mut entity = json!({
        "kind": "pj",
        "name": "Elowen Ventargent",
        "aliases": aliases,
        "player": CLAIRE,
        "notes": notes,
        "stats": { "FOR": 12, "DEX": 18, "CON": 14, "INT": 10, "SAG": 15, "CHA": 9 },
        "ca": 16,
        "pvMax": hp,
        "features": [{ "name": "Attaque supplémentaire", "text": "Deux attaques." }],
        "color": "success",
    });
    if let Some(id) = id {
        entity["id"] = json!(id);
    }
    json!({ "campaign": VALOMBRE, "entity": entity })
}

/// Morlox, a new NPC of Valombre, answering to `alias`.
fn morlox(alias: &str) -> Value {
    json!({
        "campaign": VALOMBRE,
        "entity": { "kind": "pnj", "name": "Morlox", "aliases": [alias] },
    })
}

/// Elowen, the PC of a player the tables do not hold: the tables refuse her
/// (`character.player_id`'s foreign key).
fn orphan() -> Value {
    let mut elowen = elowen(None, "", &["la Rôdeuse"]);
    elowen["entity"]["player"] = json!(NOBODY);
    elowen
}

/// The versions of `id` the tables hold, oldest first.
fn versions_of(id: &str) -> String {
    format!(
        "SELECT id, version_number, source, snapshot, diff, restored_from_version_id \
         FROM entity_version WHERE entity_id = '{id}' ORDER BY version_number"
    )
}

#[tokio::test]
async fn a_save_reaches_the_tables_and_each_save_adds_a_version_leaving_the_earlier_ones() {
    let s = Scratch::new("tables").await;
    let (engine, _) = s.serve().await;
    let (id, result) = save(
        &engine,
        elowen(None, "Cherche son frère.", &["Elowen", "la Rôdeuse"]),
    )
    .await;
    applied(&result);

    let entity = s
        .rows(&format!(
            "SELECT campaign_id, type, name, notes FROM entity WHERE id = '{id}'"
        ))
        .await;
    assert_eq!(
        entity,
        [
            json!({ "campaign_id": VALOMBRE, "type": "pj", "name": "Elowen Ventargent", "notes": "Cherche son frère." })
        ]
    );
    let character = s
        .rows(&format!(
            "SELECT player_id, strength, dexterity, armor_class, hp_max, actions, spells, token_color \
             FROM character WHERE entity_id = '{id}'"
        ))
        .await;
    assert_eq!(
        character,
        [json!({
            "player_id": CLAIRE, "strength": 12, "dexterity": 18, "armor_class": 16, "hp_max": 44,
            "actions": [{ "name": "Attaque supplémentaire", "text": "Deux attaques." }],
            "spells": [], "token_color": "success",
        })]
    );
    let aliases = format!("SELECT alias FROM entity_alias WHERE entity_id = '{id}' ORDER BY alias");
    assert_eq!(
        s.rows(&aliases).await,
        [
            json!({ "alias": "Elowen" }),
            json!({ "alias": "la Rôdeuse" })
        ]
    );
    let versions = versions_of(&id);
    let first = s.rows(&versions).await;
    assert_eq!(first.len(), 1, "{first:?}");
    assert_eq!(first[0]["version_number"], 1);
    assert_eq!(first[0]["source"], "dm");
    assert_eq!(
        first[0]["snapshot"]["aliases"],
        json!(["Elowen", "la Rôdeuse"])
    );
    assert_eq!(first[0]["snapshot"]["dexterity"], 18);
    assert_eq!(first[0]["snapshot"]["playerId"], CLAIRE);

    // Her notes change and she drops an alias for another.
    let (_, result) = save(
        &engine,
        elowen(
            Some(&id),
            "Cherche son frère Aerin.",
            &["la Rôdeuse", "l'archère"],
        ),
    )
    .await;
    applied(&result);
    let both = s.rows(&versions).await;
    assert_eq!(both.len(), 2, "{both:?}");
    assert_eq!(both[0], first[0], "version 1 is never modified");
    assert_eq!(both[1]["version_number"], 2);
    let fields: Vec<&str> = both[1]["diff"]
        .as_array()
        .expect("a diff")
        .iter()
        .map(|c| c["field"].as_str().expect("a field"))
        .collect();
    assert_eq!(fields, ["aliases", "notes"]);
    assert_eq!(
        s.rows(&aliases).await,
        [
            json!({ "alias": "l'archère" }),
            json!({ "alias": "la Rôdeuse" })
        ]
    );

    // Only her notes: the diff lists notes alone.
    let (_, result) = save(
        &engine,
        elowen(Some(&id), "Aerin est vivant.", &["la Rôdeuse", "l'archère"]),
    )
    .await;
    applied(&result);
    let all = s.rows(&versions).await;
    assert_eq!(&all[..2], &both[..]);
    assert_eq!(
        all[2]["diff"],
        json!([{ "field": "notes", "old": "Cherche son frère Aerin.", "new": "Aerin est vivant." }])
    );
    // Another element answering to one of her aliases is judged against
    // her, applied a moment ago.
    let (_, result) = save(&engine, morlox("L'ARCHERE")).await;
    assert_eq!(result.violations, ["alias-unique-in-campaign"]);
    s.drop().await;
}

#[tokio::test]
async fn a_save_queued_behind_one_cancelled_is_numbered_after_the_last_version_the_tables_hold() {
    let s = Scratch::new("cancelled").await;
    let (engine, history) = s.serve().await;
    let (id, result) = save(&engine, elowen(Some(&new_id()), "v1", &[])).await;
    applied(&result);
    // Held, so neither reaches the head before the first is cancelled.
    let hold = engine
        .hold(&format!("Entity/{id}"), "dm", None)
        .await
        .expect("held");
    let save_body = |notes: &str| json!({ "dataCapability": "campaign.saveEntity@1", "payload": elowen(Some(&id), notes, &[]) });
    let first = submit(&engine, save_body("v2")).await;
    let second = submit(&engine, save_body("v3")).await;
    engine
        .cancel(first["commandId"].as_str().expect("an id"), "dm")
        .await
        .expect("cancelled");
    let _ = engine
        .confirm(second["commandId"].as_str().expect("an id"), "dm")
        .await;
    engine.release(&hold.id).await.expect("released");
    let settled = engine.run_once().await.expect("the applier ran");
    assert_eq!(settled.len(), 1, "{settled:?}");
    applied(&settled[0]);
    let versions = s.rows(&versions_of(&id)).await;
    assert_eq!(versions.len(), 2, "{versions:?}");
    assert_eq!(versions[1]["version_number"], 2);
    assert_eq!(
        versions[1]["diff"],
        json!([{ "field": "notes", "old": "v1", "new": "v3" }])
    );
    // The versions kept are the tables'.
    let kept = history.of(&id);
    assert_eq!(kept.len(), 2);
    assert_eq!(json!(kept[1].id), versions[1]["id"]);
    assert_eq!(kept[1].number, 2);
    assert_eq!(json!(kept[1].diff), versions[1]["diff"]);
    s.drop().await;
}

/// A fresh element id, as the editor may name one.
fn new_id() -> String {
    use harness_dataguard::store::Ids;
    RandomIds.fresh()
}

#[tokio::test]
async fn a_start_judges_an_alias_against_the_elements_the_tables_hold() {
    let s = Scratch::new("restart").await;
    {
        let (engine, _) = s.serve().await;
        applied(&save(&engine, elowen(None, "", &["la Rôdeuse"])).await.1);
    }
    // Another server over the same database: Elowen is the tables'.
    let (engine, _) = s.serve().await;
    let (id, result) = save(&engine, morlox("LA RODEUSE")).await;
    assert_eq!(result.status, State::Rejected, "{result:?}");
    assert_eq!(result.violations, ["alias-unique-in-campaign"]);
    assert!(
        s.rows(&format!("SELECT id FROM entity WHERE id = '{id}'"))
            .await
            .is_empty()
    );
    s.drop().await;
}

#[tokio::test]
async fn a_save_the_tables_refuse_is_rejected_whole_and_leaves_its_aliases_free() {
    let s = Scratch::new("refused").await;
    let notifier = Arc::new(MemoryNotifier::default());
    let (engine, history) = s.telling(notifier.clone()).await;
    let (elowen, result) = save(&engine, orphan()).await;
    // Refused in the apply transaction: rejected, its author told why,
    // nothing kept.
    assert_eq!(result.status, State::Rejected, "{result:?}");
    assert!(result.violations.is_empty(), "{result:?}");
    let told: Vec<String> = notifier
        .to("dm")
        .into_iter()
        .filter_map(|message| message.reason)
        .collect();
    assert!(
        told.iter()
            .any(|reason| reason.starts_with("the tables refuse it: ")
                && reason.ends_with("(23503)")),
        "{told:?}"
    );
    assert!(
        s.rows(&format!("SELECT id FROM entity WHERE id = '{elowen}'"))
            .await
            .is_empty()
    );
    assert!(
        s.rows(&format!(
            "SELECT partition FROM dataguard_rows WHERE partition = 'Entity/{elowen}'"
        ))
        .await
        .is_empty(),
        "the engine's row is rolled back with the tables'"
    );
    assert!(history.of(&elowen).is_empty());
    // Free: the refused save took no term.
    let (id, result) = save(&engine, morlox("la rodeuse")).await;
    applied(&result);
    assert_eq!(
        s.rows(&format!(
            "SELECT alias FROM entity_alias WHERE entity_id = '{id}'"
        ))
        .await,
        [json!({ "alias": "la rodeuse" })]
    );
    s.drop().await;
}

#[tokio::test]
async fn what_only_a_deferred_trigger_judges_is_refused_in_the_apply_too() {
    let s = Scratch::new("deferred").await;
    let brume = "c1000000-0000-4000-8000-000000000002";
    let ines = "b1000000-0000-4000-8000-000000000002";
    s.client
        .batch_execute(&format!(
            "INSERT INTO campaign (id, name, kind) VALUES ('{brume}', 'La Brume', 'campagne');
             INSERT INTO player (id, campaign_id, name) VALUES ('{ines}', '{brume}', 'Inès');"
        ))
        .await
        .expect("another campaign's player");
    let (engine, _) = s.serve().await;
    // Inès plays at La Brume: the row cannot tell, `character_is_pj_or_pnj`
    // can, once the transaction ends.
    let mut elowen = elowen(None, "", &[]);
    elowen["entity"]["player"] = json!(ines);
    let (id, result) = save(&engine, elowen).await;
    assert_eq!(result.status, State::Rejected, "{result:?}");
    assert!(
        s.rows(&format!("SELECT id FROM entity WHERE id = '{id}'"))
            .await
            .is_empty()
    );
    // The applier goes on.
    applied(&save(&engine, morlox("le gobelin")).await.1);
    s.drop().await;
}

#[tokio::test]
async fn a_save_the_tables_refused_is_made_once_they_can_hold_it_and_a_start_projects_one_they_lost()
 {
    let s = Scratch::new("repair").await;
    let (engine, _) = s.serve().await;
    let julie = new_id();
    let mut orphan = orphan();
    orphan["entity"]["id"] = json!(julie);
    assert_eq!(
        save(&engine, orphan.clone()).await.1.status,
        State::Rejected
    );
    s.client
        .batch_execute(&format!(
            "INSERT INTO player (id, campaign_id, name) VALUES ('{NOBODY}', '{VALOMBRE}', 'Julie')"
        ))
        .await
        .expect("her player");
    // Saved again once her player is there: version 1.
    applied(&save(&engine, orphan).await.1);
    let projected = format!(
        "SELECT e.name, c.player_id, v.version_number FROM entity e \
         JOIN character c ON c.entity_id = e.id JOIN entity_version v ON v.entity_id = e.id \
         WHERE e.id = '{julie}'"
    );
    let whole = [json!({ "name": "Elowen Ventargent", "player_id": NOBODY, "version_number": 1 })];
    assert_eq!(s.rows(&projected).await, whole);

    // Lost from the tables: a start projects her again, her alias with her.
    s.client
        .batch_execute(&format!("DELETE FROM entity WHERE id = '{julie}'"))
        .await
        .expect("lost");
    drop(engine);
    let (engine, history) = s.serve().await;
    assert_eq!(s.rows(&projected).await, whole);
    assert_eq!(history.of(&julie).len(), 1);
    let (_, result) = save(&engine, morlox("la rôdeuse")).await;
    assert_eq!(result.violations, ["alias-unique-in-campaign"]);
    s.drop().await;
}

#[tokio::test]
async fn an_npc_made_a_place_loses_its_sheet_in_the_tables() {
    let s = Scratch::new("kind").await;
    let (engine, _) = s.serve().await;
    let (id, result) = save(
        &engine,
        json!({
            "campaign": VALOMBRE,
            "entity": { "kind": "pnj", "name": "Le Moulin", "stats": { "DEX": 10 }, "color": "danger" },
        }),
    )
    .await;
    applied(&result);
    let sheet = format!("SELECT entity_id FROM character WHERE entity_id = '{id}'");
    assert_eq!(s.rows(&sheet).await.len(), 1);
    let (_, result) = save(
        &engine,
        json!({ "campaign": VALOMBRE, "entity": { "id": id, "kind": "lieu" } }),
    )
    .await;
    applied(&result);
    assert!(s.rows(&sheet).await.is_empty());
    let versions = s
        .rows(&format!(
            "SELECT version_number, snapshot -> 'type' AS type FROM entity_version \
             WHERE entity_id = '{id}' ORDER BY version_number"
        ))
        .await;
    assert_eq!(
        versions,
        [
            json!({ "version_number": 1, "type": "pnj" }),
            json!({ "version_number": 2, "type": "lieu" })
        ]
    );
    s.drop().await;
}

/// Elowen brought to version 4, each save moving her notes, aliases and HP.
async fn at_version_4(engine: &Engine) -> String {
    let (id, result) = save(
        engine,
        elowen_at(None, "Cherche son frère.", &["Elowen"], 44),
    )
    .await;
    applied(&result);
    for (notes, aliases, hp) in [
        (
            "Cherche son frère Aerin.",
            &["Elowen", "la Rôdeuse"][..],
            40,
        ),
        ("A retrouvé Aerin.", &["la Rôdeuse"][..], 52),
        ("Aerin est mort.", &["l'archère"][..], 52),
    ] {
        applied(
            &save(engine, elowen_at(Some(&id), notes, aliases, hp))
                .await
                .1,
        );
    }
    id
}

#[tokio::test]
async fn a_restoration_rewrites_the_tables_as_the_version_and_adds_a_fifth_leaving_the_others() {
    let s = Scratch::new("restored").await;
    let (engine, _) = s.serve().await;
    let id = at_version_4(&engine).await;
    let versions = versions_of(&id);
    let before = s.rows(&versions).await;
    assert_eq!(before.len(), 4, "{before:?}");
    let v2 = &before[1];

    let result = restore(&engine, &id, json!(2)).await;
    applied(&result);

    // restore-exact: the element, its sheet and its aliases as version 2.
    let entity = s
        .rows(&format!(
            "SELECT type, name, description, notes FROM entity WHERE id = '{id}'"
        ))
        .await;
    assert_eq!(
        entity,
        [
            json!({ "type": "pj", "name": "Elowen Ventargent", "description": null, "notes": "Cherche son frère Aerin." })
        ]
    );
    let character = s
        .rows(&format!(
            "SELECT player_id, strength, dexterity, constitution, intelligence, wisdom, charisma, \
             armor_class, hp_max, token_color FROM character WHERE entity_id = '{id}'"
        ))
        .await;
    assert_eq!(
        character,
        [json!({
            "player_id": CLAIRE, "strength": 12, "dexterity": 18, "constitution": 14,
            "intelligence": 10, "wisdom": 15, "charisma": 9, "armor_class": 16, "hp_max": 40,
            "token_color": "success",
        })]
    );
    assert_eq!(
        character[0]["hp_max"], v2["snapshot"]["hpMax"],
        "the sheet is version 2's"
    );
    let aliases = s
        .rows(&format!(
            "SELECT alias FROM entity_alias WHERE entity_id = '{id}' ORDER BY alias"
        ))
        .await;
    assert_eq!(
        aliases,
        [
            json!({ "alias": "Elowen" }),
            json!({ "alias": "la Rôdeuse" })
        ]
    );

    // every-change-versioned: version 5, a restoration naming version 2;
    // versions 1 to 4 as they were.
    let after = s.rows(&versions).await;
    assert_eq!(after.len(), 5, "{after:?}");
    assert_eq!(after[..4], before[..]);
    assert_eq!(after[4]["version_number"], 5);
    assert_eq!(after[4]["source"], "restauration");
    assert_eq!(after[4]["restored_from_version_id"], v2["id"]);
    let mut restored = after[4]["snapshot"].clone();
    let mut expected = v2["snapshot"].clone();
    // A field version 2 lacks is null in the restoration's snapshot.
    for snapshot in [&mut restored, &mut expected] {
        snapshot
            .as_object_mut()
            .expect("a snapshot")
            .retain(|_, value| !value.is_null());
    }
    assert_eq!(restored, expected);
    s.drop().await;
}

#[tokio::test]
async fn after_a_restart_the_versions_restored_are_the_tables_by_number_or_by_id() {
    let s = Scratch::new("history").await;
    let id = {
        let (engine, _) = s.serve().await;
        at_version_4(&engine).await
    };
    let v1 = s
        .rows(&format!(
            "SELECT id FROM entity_version WHERE entity_id = '{id}' AND version_number = 1"
        ))
        .await[0]["id"]
        .clone();
    // A new start: the history is read from entity_version.
    let (engine, history) = s.serve().await;
    assert_eq!(history.of(&id).len(), 4);
    applied(&restore(&engine, &id, json!(1)).await);
    applied(&restore(&engine, &id, json!(5)).await);
    applied(&restore(&engine, &id, v1.clone()).await);
    let last = s
        .rows(&format!(
            "SELECT version_number, restored_from_version_id FROM entity_version \
             WHERE entity_id = '{id}' ORDER BY version_number DESC LIMIT 1"
        ))
        .await;
    assert_eq!(
        last,
        [json!({ "version_number": 7, "restored_from_version_id": v1 })]
    );
    let notes = s
        .rows(&format!("SELECT notes FROM entity WHERE id = '{id}'"))
        .await;
    assert_eq!(notes, [json!({ "notes": "Cherche son frère." })]);
    s.drop().await;
}

#[tokio::test]
async fn restoring_a_version_saved_with_no_aliases_leaves_the_element_none() {
    let s = Scratch::new("no_aliases").await;
    let (engine, _) = s.serve().await;
    // Version 1 saved without `aliases`: its snapshot holds none.
    let (id, result) = save(
        &engine,
        json!({ "campaign": VALOMBRE, "entity": { "kind": "lieu", "name": "Le Moulin" } }),
    )
    .await;
    applied(&result);
    let (_, result) = save(
        &engine,
        json!({ "campaign": VALOMBRE, "entity": {
            "id": id, "kind": "lieu", "name": "Le Moulin", "aliases": ["le moulin brûlé"],
        } }),
    )
    .await;
    applied(&result);

    // restore-exact: version 1's aliases, none — the trigger agrees.
    applied(&restore(&engine, &id, json!(1)).await);
    let aliases = s
        .rows(&format!(
            "SELECT alias FROM entity_alias WHERE entity_id = '{id}'"
        ))
        .await;
    assert_eq!(aliases, Vec::<Value>::new());
    let last = s
        .rows(&format!(
            "SELECT version_number, source FROM entity_version \
             WHERE entity_id = '{id}' ORDER BY version_number DESC LIMIT 1"
        ))
        .await;
    assert_eq!(
        last,
        [json!({ "version_number": 3, "source": "restauration" })]
    );
    s.drop().await;
}

#[tokio::test]
async fn a_proposal_validated_before_its_element_records_it_once_the_tables_hold_it() {
    let s = Scratch::new("proposal").await;
    let (engine, _) = s.serve().await;
    let (note, proposal, element) = (new_id(), new_id(), new_id());
    s.client
        .batch_execute(&format!(
            "INSERT INTO imported_note (id, campaign_id, raw_text) VALUES ('{note}', '{VALOMBRE}', 'Morlox ment.');
             INSERT INTO note_proposal (id, note_id, type, name, excerpt, checked)
                 VALUES ('{proposal}', '{note}', 'pnj', 'Morlox', 'Morlox ment.', false);
             INSERT INTO dataguard_rows (partition, fields) VALUES ('NoteImport/{proposal}',
                 '{{\"noteId\": \"{note}\", \"checked\": true, \"createdEntityId\": \"{}\"}}');",
            element.to_uppercase()
        ))
        .await
        .expect("a proposal validated, its element not applied yet");
    let stamped =
        format!("SELECT checked, created_entity_id FROM note_proposal WHERE id = '{proposal}'");
    assert_eq!(
        s.rows(&stamped).await,
        [json!({ "checked": false, "created_entity_id": null })]
    );
    let mut morlox = morlox("le menteur");
    morlox["entity"]["id"] = json!(element);
    applied(&save(&engine, morlox).await.1);
    assert_eq!(
        s.rows(&stamped).await,
        [json!({ "checked": true, "created_entity_id": element })]
    );
    s.drop().await;
}

#[tokio::test]
async fn without_the_tables_a_start_reads_nothing_and_a_save_fails_the_step_unrefused() {
    let s = Scratch::bare("bare").await;
    // The engine's own tables alone: the projection warms on nothing.
    let (engine, history) = s.serve().await;
    submit(
        &engine,
        json!({ "dataCapability": "campaign.saveEntity@1", "payload": morlox("le gobelin") }),
    )
    .await;
    // The store fails, the command is neither applied nor rejected: it
    // stays at the head.
    assert!(engine.run_once().await.is_err());
    assert!(engine.run_once().await.is_err());
    let rows = s
        .rows("SELECT partition FROM dataguard_rows WHERE partition LIKE 'Entity/%'")
        .await;
    assert!(rows.is_empty(), "{rows:?}");
    drop(history);
    s.drop().await;
}

/// Morlox, Valombre's NPC, as the mock-up's fixtures load him straight into
/// the tables (`crates/server/src/fixtures.rs`): his row, his sheet, his
/// aliases — in another order than his snapshot's — and his version 1, which
/// the engine never wrote.
const MORLOX: &str = "e1000000-0000-4000-8000-000000000003";
/// His version 1.
const MORLOX_1: &str = "f1000000-0000-4000-8000-000000000003";
/// La Taverne du Sanglier, a place of the fixtures: no sheet, no alias.
const TAVERNE: &str = "e1000000-0000-4000-8000-000000000002";

/// Morlox's version 1 snapshot, the fixtures' nulls left out.
fn morlox_snapshot() -> Value {
    json!({
        "type": "pnj", "name": "Morlox",
        "description": "Gobelin marchand du marché noir de Valombre.",
        "notes": "Doit 40 po à Vesna.",
        "aliases": ["le gobelin", "le marchand", "Morlo"],
        "initialPrompt": "Un gobelin marchand, nerveux.",
        "background": "Chassé de sa tribu.",
        "behavior": "Parle vite.",
        "strength": 8, "dexterity": 14, "constitution": 10,
        "intelligence": 13, "wisdom": 9, "charisma": 12,
        "armorClass": 13, "hpMax": 18,
        "actions": [{ "name": "Dague", "text": "+4 pour toucher." }],
        "tokenColor": "secondary",
        "spells": [],
    })
}

impl Scratch {
    /// Morlox and the Taverne, as the fixtures load them.
    async fn fixtures(&self) {
        let snapshot = morlox_snapshot();
        let diff: Vec<Value> = snapshot
            .as_object()
            .expect("an object")
            .iter()
            .map(|(field, new)| json!({ "field": field, "old": null, "new": new }))
            .collect();
        let at = "2026-08-20T10:00:00.000003Z";
        self.client
            .batch_execute(&format!(
                "INSERT INTO entity (id, campaign_id, type, name, description, notes, created_at, updated_at)
                    VALUES ('{MORLOX}', '{VALOMBRE}', 'pnj', 'Morlox', {description}, {notes}, '{at}', '{at}'),
                        ('{TAVERNE}', '{VALOMBRE}', 'lieu', 'La Taverne du Sanglier', NULL, NULL, '{at}', '{at}');
                 INSERT INTO character (entity_id, initial_prompt, background, behavior, strength, dexterity,
                        constitution, intelligence, wisdom, charisma, armor_class, hp_max, actions, token_color)
                    VALUES ('{MORLOX}', {prompt}, {background}, {behavior}, 8, 14, 10, 13, 9, 12, 13, 18,
                        '{actions}', 'secondary');
                 INSERT INTO entity_alias (entity_id, alias)
                    VALUES ('{MORLOX}', 'Morlo'), ('{MORLOX}', 'le marchand'), ('{MORLOX}', 'le gobelin');
                 INSERT INTO entity_version (id, entity_id, version_number, source, snapshot, diff, created_at)
                    VALUES ('{MORLOX_1}', '{MORLOX}', 1, 'dm', '{snapshot}', '{diff}', '{at}'),
                        ('f1000000-0000-4000-8000-000000000002', '{TAVERNE}', 1, 'dm',
                         '{{\"type\": \"lieu\", \"name\": \"La Taverne du Sanglier\", \"aliases\": []}}', '[]', '{at}');",
                description = text(&snapshot["description"]),
                notes = text(&snapshot["notes"]),
                prompt = text(&snapshot["initialPrompt"]),
                background = text(&snapshot["background"]),
                behavior = text(&snapshot["behavior"]),
                actions = snapshot["actions"],
                diff = Value::Array(diff),
            ))
            .await
            .expect("the fixtures");
    }
}

/// `value`, a string, as an SQL literal.
fn text(value: &Value) -> String {
    format!(
        "'{}'",
        value.as_str().expect("a string").replace('\'', "''")
    )
}

/// A read of the row `read` names, refused once read: nothing is queued.
const PEEK: &str = r#"{"dataCapability":"test.peekEntity","owner":"dataguard","version":1,"effect":"update","target":{"aggregate":"Entity","id":"$entityId"},"mode":"overwrite","callableBy":["*"]}"#;

/// A `Prepare` keeping what `Context::row` answers it.
#[derive(Clone, Default)]
struct Peek(Arc<Mutex<Vec<BTreeMap<String, Value>>>>);

#[async_trait]
impl Prepare for Peek {
    async fn prepare(
        &self,
        context: &Context<'_>,
        payload: &BTreeMap<String, Value>,
        _target_id: Option<&str>,
    ) -> Result<Prepared, Refusal> {
        let partition = payload["read"].as_str().expect("a partition");
        let row = context.row(partition).await?;
        self.0.lock().expect("the rows read").push(row);
        Err(Refusal::unprocessable("read"))
    }
}

impl Peek {
    /// An engine over `s` reading through this `Prepare`.
    async fn over(&self, s: &Scratch) -> Engine {
        s.planting(
            Arc::default(),
            Plugin::new().prepare("test.peekEntity", self.clone()),
            &[PEEK],
        )
        .await
        .0
    }

    /// The row of `partition`, as a `Prepare` reads it.
    async fn row(&self, engine: &Engine, partition: &str) -> BTreeMap<String, Value> {
        let refused = submit_raw(
            engine,
            json!({
                "dataCapability": "test.peekEntity",
                "payload": { "entityId": MORLOX, "read": partition },
            }),
        )
        .await;
        assert!(refused.is_err(), "nothing is queued");
        self.0.lock().expect("the rows read").pop().expect("read")
    }
}

/// Submits `body` as the DM: the engine's answer, a refusal kept.
async fn submit_raw(engine: &Engine, mut body: Value) -> harness_dataguard::Result<Value> {
    body["caller"] =
        json!({ "by": "dm", "from": "campaign.EntityEditor", "roles": ["role:approver"] });
    let submission: Submission = serde_json::from_value(body).expect("a submission");
    let answer = engine.submit(submission).await?;
    Ok(serde_json::to_value(answer).expect("JSON"))
}

#[tokio::test]
async fn a_prepare_reads_an_element_only_the_tables_hold_as_the_engine_row_it_writes() {
    let s = Scratch::new("hydrate").await;
    s.fixtures().await;
    let peek = Peek::default();
    let engine = peek.over(&s).await;

    let morlox = peek.row(&engine, &format!("Entity/{MORLOX}")).await;
    let expected = json!({
        "entityId": MORLOX, "entityCampaignId": VALOMBRE, "type": "pnj", "name": "Morlox",
        "description": "Gobelin marchand du marché noir de Valombre.",
        "notes": "Doit 40 po à Vesna.",
        "entityCreatedAt": "2026-08-20T10:00:00.000003Z",
        "updatedAt": "2026-08-20T10:00:00.000003Z",
        // In his snapshot's order, not the table's.
        "alias": ["le gobelin", "le marchand", "Morlo"],
        "characterEntityId": MORLOX,
        "initialPrompt": "Un gobelin marchand, nerveux.",
        "background": "Chassé de sa tribu.", "behavior": "Parle vite.",
        "strength": 8, "dexterity": 14, "constitution": 10,
        "intelligence": 13, "wisdom": 9, "charisma": 12,
        "armorClass": 13, "hpMax": 18, "spells": [],
        "actions": [{ "name": "Dague", "text": "+4 pour toucher." }],
        "tokenColor": "secondary",
        "entityVersionId": MORLOX_1, "versionNumber": 1, "snapshot": morlox_snapshot(),
    });
    assert_eq!(
        json!(morlox),
        expected,
        "no null: a field he has not is absent"
    );

    // A place: no sheet, no alias.
    let taverne = peek.row(&engine, &format!("Entity/{TAVERNE}")).await;
    assert_eq!(
        json!(taverne),
        json!({
            "entityId": TAVERNE, "entityCampaignId": VALOMBRE, "type": "lieu",
            "name": "La Taverne du Sanglier",
            "entityCreatedAt": "2026-08-20T10:00:00.000003Z",
            "updatedAt": "2026-08-20T10:00:00.000003Z",
            "alias": [],
            "entityVersionId": "f1000000-0000-4000-8000-000000000002", "versionNumber": 1,
            "snapshot": { "type": "lieu", "name": "La Taverne du Sanglier", "aliases": [] },
        })
    );
    // An element the tables do not hold, another aggregate's row: none.
    assert!(
        peek.row(&engine, &format!("Entity/{}", new_id()))
            .await
            .is_empty()
    );
    assert!(
        peek.row(&engine, &format!("Campaign/{MORLOX}"))
            .await
            .is_empty()
    );
    drop(engine);
    s.drop().await;
}

#[tokio::test]
async fn a_text_that_is_no_uuid_or_tables_not_migrated_hydrate_nothing_without_an_sql_error() {
    let s = Scratch::new("hydrate_none").await;
    s.fixtures().await;
    let peek = Peek::default();
    let engine = peek.over(&s).await;
    for id in [
        "morlox",
        "",
        "e1000000-0000-4000-8000-00000000000g",
        "e1000000-0000-4000-8000-0000000000033",
        "e1000000x0000-4000-8000-000000000003",
        "e1000000-0000x4000-8000-000000000003",
        "e1000000-0000-4000x8000-000000000003",
        "e1000000-0000-4000-8000x000000000003",
        "e1000000-0000-4000-8000-00000000003'",
        // In capitals, a second partition of the same element.
        "E1000000-0000-4000-8000-000000000003",
        "e1000000-0000-4000-8000-00000000000F",
    ] {
        let row = peek.row(&engine, &format!("Entity/{id}")).await;
        assert!(row.is_empty(), "{id}: {row:?}");
    }
    drop(engine);
    s.drop().await;

    let s = Scratch::bare("hydrate_bare").await;
    let peek = Peek::default();
    let engine = peek.over(&s).await;
    assert!(
        peek.row(&engine, &format!("Entity/{MORLOX}"))
            .await
            .is_empty()
    );
    drop(engine);
    s.drop().await;
}

#[tokio::test]
async fn a_save_of_an_element_only_the_tables_hold_applies_and_versions_its_change_alone() {
    let s = Scratch::new("fixture_save").await;
    s.fixtures().await;
    let (engine, history) = s.serve().await;
    // The editor names him and his new description, not his campaign: his
    // row is the tables'.
    let (id, result) = save(
        &engine,
        json!({ "entity": { "id": MORLOX, "description": "Gobelin marchand, ruiné." } }),
    )
    .await;
    assert_eq!(id, MORLOX);
    applied(&result);
    assert_eq!(
        s.rows(&format!(
            "SELECT campaign_id, type, name, description, notes FROM entity WHERE id = '{MORLOX}'"
        ))
        .await,
        [json!({
            "campaign_id": VALOMBRE, "type": "pnj", "name": "Morlox",
            "description": "Gobelin marchand, ruiné.", "notes": "Doit 40 po à Vesna.",
        })]
    );
    assert_eq!(
        s.rows(&format!(
            "SELECT hp_max, token_color, actions FROM character WHERE entity_id = '{MORLOX}'"
        ))
        .await,
        [
            json!({ "hp_max": 18, "token_color": "secondary", "actions": [{ "name": "Dague", "text": "+4 pour toucher." }] })
        ]
    );
    let versions = s.rows(&versions_of(MORLOX)).await;
    assert_eq!(versions.len(), 2, "{versions:?}");
    assert_eq!(versions[0]["id"], MORLOX_1, "version 1 is never modified");
    assert_eq!(versions[0]["snapshot"], morlox_snapshot());
    assert_eq!(versions[1]["version_number"], 2);
    assert_eq!(versions[1]["source"], "dm");
    assert_eq!(
        versions[1]["diff"],
        json!([{
            "field": "description",
            "old": "Gobelin marchand du marché noir de Valombre.",
            "new": "Gobelin marchand, ruiné.",
        }])
    );
    // The engine's history and its own row agree with the tables.
    let kept = history.of(MORLOX);
    assert_eq!(kept.len(), 2);
    assert_eq!(json!(kept[1].diff), versions[1]["diff"]);
    let row = s
        .rows(&format!(
            "SELECT fields FROM dataguard_rows WHERE partition = 'Entity/{MORLOX}'"
        ))
        .await;
    assert_eq!(row[0]["fields"]["versionNumber"], 2);
    assert_eq!(row[0]["fields"]["diff"], versions[1]["diff"]);
    drop(engine);
    s.drop().await;
}

/// A PC imported from D&D Beyond on an empty row, its whole sheet and its
/// first version: what dndbeyond.importCharacter writes.
const IMPORT: &str = r#"{"dataCapability":"test.importCharacter","owner":"dataguard","version":1,"effect":"insert","target":{"aggregate":"Entity","id":"$entityId"},"mode":"overwrite","callableBy":["*"]}"#;

#[tokio::test]
async fn an_import_from_dndbeyond_creates_its_pc_in_the_tables_under_dndbeyond_resync_scope() {
    let s = Scratch::new("import").await;
    let (engine, _) = s.serve().await;
    let engine = engine.with_data_capability(DataCapability::parse(IMPORT).expect("contract F"));
    let thorin = new_id();
    let (_, result) = command(
        &engine,
        json!({
            "dataCapability": "test.importCharacter",
            "payload": {
                "entityId": thorin, "entityCampaignId": VALOMBRE, "type": "pj",
                "name": "Thorin Brisepierre", "entityCreatedAt": "2026-10-24T10:00:00.000Z",
                "characterEntityId": thorin, "playerId": CLAIRE,
                "strength": 16, "dexterity": 10, "constitution": 16,
                "intelligence": 9, "wisdom": 13, "charisma": 11,
                "armorClass": 18, "hpMax": 44,
                "spells": [{ "name": "Bénédiction" }],
                "actions": [{ "name": "Marteau de guerre", "text": "+6 au toucher" }],
                "tokenColor": "#8a5a2b",
                "dndbeyondId": "112850214", "dndbeyondSyncedAt": "2026-10-24T10:00:00.000Z",
                "entityVersionId": new_id(), "versionNumber": 1, "source": "dndbeyond",
                "snapshot": {}, "diff": [],
            },
        }),
    )
    .await;
    applied(&result);
    assert!(result.violations.is_empty(), "{result:?}");
    let versions = s.rows(&versions_of(&thorin)).await;
    assert_eq!(versions.len(), 1, "{versions:?}");
    assert_eq!(versions[0]["source"], "dndbeyond");
    // The tables hold his whole creation: his name and type among the
    // changes, outside a resync's scope.
    let fields: Vec<&str> = versions[0]["diff"]
        .as_array()
        .expect("a diff")
        .iter()
        .filter_map(|change| change["field"].as_str())
        .collect();
    assert!(
        fields.contains(&"name") && fields.contains(&"type"),
        "{fields:?}"
    );
    assert_eq!(
        s.rows(&format!(
            "SELECT dndbeyond_id, hp_max FROM character WHERE entity_id = '{thorin}'"
        ))
        .await,
        [json!({ "dndbeyond_id": "112850214", "hp_max": 44 })]
    );
    drop(engine);
    s.drop().await;
}
