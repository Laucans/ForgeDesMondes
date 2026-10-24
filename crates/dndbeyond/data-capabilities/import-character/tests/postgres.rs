//! dndbeyond.importCharacter over PostgreSQL (`DATABASE_URL`), as the server
//! plants it: an import is prepared against the PCs the tables hold — the
//! mock-up's fixtures among them, which the engine never wrote —, judged by
//! the DataGuard and projected into `entity`, `character` and
//! `entity_version` in the transaction that applies it. The PC of the
//! campaign already linked to the character is updated; another is created;
//! a campaign the tables do not hold, or a player of another campaign, is
//! refused and nothing is queued. Each test works in a scratch database of
//! its own, migrated as the server migrates it, dropped when it ends.

use std::path::Path;
use std::sync::Arc;

use harness_dataguard::aggregate::Aggregate;
use harness_dataguard::api::Submission;
use harness_dataguard::command::{CommandResult, State};
use harness_dataguard::engine::Engine;
use harness_dataguard::manifest::DataCapability;
use harness_dataguard::memory::MemoryNotifier;
use harness_dataguard::plugins::{Plugins, RefusalKind, Shared};
use harness_dataguard::postgres::{PgStore, ReadOnly, SystemClock};
use harness_dataguard::store::RandomIds;
use harness_dataguard::tokio_postgres::error::SqlState;
use harness_dataguard::tokio_postgres::{self, Client, NoTls};
use harness_dataguard::{Error, Result};
use ports_character_sheets::SharedCharacterSheets;
use ports_character_sheets::fake::FakeCharacterSheets;
use serde_json::{Value, json};

mod support;
use support::{BRANNOC, ELOWEN, elowen_url, id, sheet};

const VALOMBRE: &str = "c1000000-0000-4000-8000-000000000001";
const BRUME: &str = "c1000000-0000-4000-8000-000000000002";
/// Valombre's player.
const JULIE: &str = "a0000000-0000-4000-8000-000000000001";
/// La Brume's.
const INES: &str = "a0000000-0000-4000-8000-000000000002";
/// Elowen, a PC of Valombre the fixtures link to D&D Beyond.
const ELOWEN_PC: &str = "e1000000-0000-4000-8000-000000000001";
/// Gaspard, a PC of Valombre the DM made, linked to nothing.
const GASPARD: &str = "e1000000-0000-4000-8000-000000000002";

/// The time the fixtures were written at, long before any import.
const LONG_AGO: &str = "2026-09-26T18:12:00Z";

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

/// A database of its own, migrated as the server migrates it, holding — as
/// the fixtures write them — Valombre with Julie, Elowen linked to D&D
/// Beyond (her aliases, notes and token colour the DM's) and Gaspard linked
/// to nothing, each at version 1; La Brume with Inès.
struct Scratch {
    name: String,
    url: String,
    client: Client,
}

impl Scratch {
    async fn new(name: &str) -> Scratch {
        let name = format!("import_character_test_{name}_{}", std::process::id());
        let admin = connect(&url()).await;
        for sql in [
            format!("DROP DATABASE IF EXISTS {name} WITH (FORCE)"),
            format!("CREATE DATABASE {name}"),
        ] {
            admin.batch_execute(&sql).await.expect("a scratch database");
        }
        let url = naming(&name);
        let mut client = connect(&url).await;
        let migrations = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../data/migrations");
        data::migrate(&mut client, &migrations)
            .await
            .expect("migrated");
        let actions = json!([{ "name": "Attaque supplémentaire",
                               "description": "Deux attaques quand elle utilise l’action Attaquer." }]);
        let elowen = json!({
            "type": "pj", "name": "Elowen Ventargent", "notes": "Doit une faveur au gué.",
            "playerId": JULIE, "strength": 12, "dexterity": 18, "constitution": 14,
            "intelligence": 10, "wisdom": 15, "charisma": 9, "armorClass": 16, "hpMax": 44,
            "spells": [], "actions": actions, "tokenColor": "success",
            "dndbeyondId": ELOWEN.to_string(), "dndbeyondSyncedAt": LONG_AGO,
            "aliases": ["la rôdeuse"],
        });
        let gaspard = json!({
            "type": "pj", "name": "Gaspard", "strength": 10, "dexterity": 10,
            "constitution": 10, "intelligence": 10, "wisdom": 10, "charisma": 10,
            "armorClass": 10, "hpMax": 8, "spells": [], "actions": [], "tokenColor": "primary",
            "aliases": [],
        });
        client
            .batch_execute(&format!(
                "INSERT INTO campaign (id, name, kind) VALUES
                     ('{VALOMBRE}', 'Les Cendres de Valombre', 'campagne'),
                     ('{BRUME}', 'La Brume', 'campagne');
                 INSERT INTO player (id, campaign_id, name) VALUES
                     ('{JULIE}', '{VALOMBRE}', 'Julie'), ('{INES}', '{BRUME}', 'Inès');
                 INSERT INTO entity (id, campaign_id, type, name, notes, created_at, updated_at) VALUES
                     ('{ELOWEN_PC}', '{VALOMBRE}', 'pj', 'Elowen Ventargent', 'Doit une faveur au gué.',
                      '{LONG_AGO}', '{LONG_AGO}'),
                     ('{GASPARD}', '{VALOMBRE}', 'pj', 'Gaspard', NULL, '{LONG_AGO}', '{LONG_AGO}');
                 INSERT INTO character (entity_id, player_id, strength, dexterity, constitution,
                         intelligence, wisdom, charisma, armor_class, hp_max, actions, token_color,
                         dndbeyond_id, dndbeyond_synced_at) VALUES
                     ('{ELOWEN_PC}', '{JULIE}', 12, 18, 14, 10, 15, 9, 16, 44, '{actions}', 'success',
                      '{ELOWEN}', '{LONG_AGO}'),
                     ('{GASPARD}', NULL, 10, 10, 10, 10, 10, 10, 10, 8, '[]', 'primary', NULL, NULL);
                 INSERT INTO entity_alias (entity_id, alias) VALUES ('{ELOWEN_PC}', 'la rôdeuse');
                 INSERT INTO entity_version (entity_id, version_number, source, snapshot, diff, created_at)
                     VALUES ('{ELOWEN_PC}', 1, 'dm', '{elowen}', '[]', '{LONG_AGO}'),
                         ('{GASPARD}', 1, 'dm', '{gaspard}', '[]', '{LONG_AGO}');"
            ))
            .await
            .expect("Valombre, La Brume, their players, Elowen and Gaspard");
        Scratch { name, url, client }
    }

    /// The engine as the server plants this unit and its target aggregate
    /// over this database — the aggregate's projection in the apply
    /// transaction, the tables lent to the Prepare — with the port's fake,
    /// knowing Elowen as she levelled up (52 HP) and Brannoc.
    async fn serve(&self) -> Engine {
        let plugins = || -> Plugins {
            [aggregate_entity::plugin(), import_character::plugin()]
                .into_iter()
                .collect()
        };
        let store = PgStore::open(connect(&self.url).await)
            .await
            .expect("the store")
            .with_projections(&plugins())
            .await
            .expect("warmed");
        let sheets = FakeCharacterSheets::new()
            .with_sheet(sheet(ELOWEN, "Elowen la Grande", 52))
            .with_sheet(sheet(BRANNOC, "Brannoc", 51));
        let mut shared = Shared::default();
        shared.insert(Arc::new(SharedCharacterSheets::new(sheets)));
        let engine = Engine::new(
            Arc::new(store),
            Arc::new(MemoryNotifier::default()),
            Arc::new(SystemClock),
        )
        .with_rows(Arc::new(ReadOnly::new(connect(&self.url).await)))
        .with_ids(Arc::new(RandomIds))
        .with_aggregate(Aggregate::parse(aggregate_entity::MANIFEST).expect("contract I"))
        .with_data_capability(
            DataCapability::parse(import_character::MANIFEST).expect("contract F"),
        )
        .with_plugins(plugins())
        .with_shared(shared);
        engine.load().await.expect("the engine loads");
        engine
    }

    /// The rows `sql` answers, each its one JSON column.
    async fn rows(&self, sql: &str) -> Vec<Value> {
        self.client
            .query(sql, &[])
            .await
            .expect("the tables")
            .iter()
            .map(|row| serde_json::from_str(row.get::<_, &str>(0)).expect("JSON"))
            .collect()
    }

    /// Valombre's PCs with their sheet, by name.
    async fn pcs(&self) -> Vec<Value> {
        self.rows(&format!(
            "SELECT jsonb_build_object('id', e.id, 'name', e.name, 'notes', e.notes,
                 'player', c.player_id, 'hpMax', c.hp_max, 'tokenColor', c.token_color,
                 'dndbeyondId', c.dndbeyond_id,
                 'synced', c.dndbeyond_synced_at > '{LONG_AGO}'::timestamptz,
                 'aliases', (SELECT jsonb_agg(a.alias) FROM entity_alias a WHERE a.entity_id = e.id))::text
             FROM entity e JOIN character c ON c.entity_id = e.id
             WHERE e.campaign_id = '{VALOMBRE}' ORDER BY e.name"
        ))
        .await
    }

    /// The versions of `pc`, by number.
    async fn versions(&self, pc: &str) -> Vec<Value> {
        self.rows(&format!(
            "SELECT jsonb_build_object('number', version_number, 'source', source,
                 'fields', (SELECT jsonb_agg(c ->> 'field') FROM jsonb_array_elements(diff) c))::text
             FROM entity_version WHERE entity_id = '{pc}' ORDER BY version_number"
        ))
        .await
    }

    /// Drops the database. An autovacuum worker the writes woke may still be
    /// on it, which `FORCE` cannot end for a role that is not a superuser:
    /// the drop is tried again while the worker finishes.
    async fn drop(self) {
        drop(self.client);
        let admin = connect(&url()).await;
        let sql = format!("DROP DATABASE {} WITH (FORCE)", self.name);
        for _ in 0..20 {
            match admin.batch_execute(&sql).await {
                Ok(()) => return,
                Err(e) if e.code() == Some(&SqlState::INSUFFICIENT_PRIVILEGE) => {
                    tokio::time::sleep(std::time::Duration::from_millis(250)).await;
                }
                Err(e) => panic!("the scratch database dropped: {e:?}"),
            }
        }
        panic!("the scratch database {} is still in use", self.name);
    }
}

/// An import of `payload` on `target` by the DM, approver of their own
/// campaign: the partition it queued on.
async fn import(engine: &Engine, payload: Value, target: Option<&str>) -> Result<String> {
    let submission: Submission = serde_json::from_value(json!({
        "dataCapability": "dndbeyond.importCharacter@1",
        "targetId": target,
        "payload": payload,
        "caller": { "by": "dm", "from": "campaign.EntityEditor", "roles": ["role:approver"] },
    }))
    .expect("a submission");
    let submitted = serde_json::to_value(engine.submit(submission).await?).expect("JSON");
    Ok(submitted["partition"]
        .as_str()
        .expect("a partition")
        .to_owned())
}

/// What the applier settled, one command applied.
fn applied(settled: &[CommandResult]) {
    assert_eq!(settled.len(), 1, "{settled:?}");
    assert_eq!(settled[0].status, State::Applied, "{settled:?}");
}

/// The kind and the message of a refusal.
fn refusal(result: Result<String>) -> (RefusalKind, String) {
    match result {
        Err(Error::Refused(refusal)) => (refusal.kind, refusal.message),
        other => panic!("not refused: {other:?}"),
    }
}

#[tokio::test]
async fn an_import_of_a_url_a_pc_of_the_campaign_is_linked_to_updates_that_pc_alone() {
    let s = Scratch::new("linked").await;
    let engine = s.serve().await;
    let partition = import(
        &engine,
        json!({ "url": elowen_url(), "campaign": VALOMBRE }),
        None,
    )
    .await
    .expect("queued");
    assert_eq!(partition, format!("Entity/{ELOWEN_PC}"));
    applied(&engine.run_once().await.expect("applied"));
    // Elowen's statblock and sync date are D&D Beyond's; her name, notes,
    // aliases and token are the DM's still; no PC is added.
    assert_eq!(
        s.pcs().await,
        [
            json!({ "id": ELOWEN_PC, "name": "Elowen Ventargent", "notes": "Doit une faveur au gué.",
                    "player": JULIE, "hpMax": 52, "tokenColor": "success",
                    "dndbeyondId": ELOWEN.to_string(), "synced": true, "aliases": ["la rôdeuse"] }),
            json!({ "id": GASPARD, "name": "Gaspard", "notes": null, "player": null, "hpMax": 8,
                    "tokenColor": "primary", "dndbeyondId": null, "synced": null, "aliases": null }),
        ]
    );
    assert_eq!(
        s.versions(ELOWEN_PC).await,
        [
            json!({ "number": 1, "source": "dm", "fields": null }),
            json!({ "number": 2, "source": "dndbeyond",
                    "fields": ["dndbeyondSyncedAt", "hpMax", "spells"] }),
        ]
    );
    s.drop().await;
}

#[tokio::test]
async fn an_import_of_a_new_url_creates_its_pc_of_its_player_and_again_updates_it() {
    let s = Scratch::new("created").await;
    let engine = s.serve().await;
    let payload = json!({ "url": id(BRANNOC).url(), "campaign": VALOMBRE, "player": JULIE });
    let partition = import(&engine, payload.clone(), None)
        .await
        .expect("queued");
    applied(&engine.run_once().await.expect("applied"));
    let brannoc = import_character::derived(VALOMBRE, id(BRANNOC));
    assert_eq!(partition, format!("Entity/{brannoc}"));
    let pcs = s.pcs().await;
    assert_eq!(pcs.len(), 3, "{pcs:?}");
    assert_eq!(
        pcs[0],
        json!({ "id": brannoc, "name": "Brannoc", "notes": null, "player": JULIE, "hpMax": 51,
                "tokenColor": "accent", "dndbeyondId": BRANNOC.to_string(), "synced": true,
                "aliases": null })
    );
    let versions = s.versions(&brannoc).await;
    assert_eq!(versions.len(), 1);
    assert_eq!(versions[0]["source"], json!("dndbeyond"));
    assert!(
        versions[0]["fields"]
            .as_array()
            .is_some_and(|fields| fields.contains(&json!("name"))),
        "{versions:?}"
    );
    // Imported again — once the tables hold it — the same PC, a second version.
    let again = import(&engine, payload, None).await.expect("queued");
    assert_eq!(again, partition);
    applied(&engine.run_once().await.expect("applied"));
    assert_eq!(s.pcs().await.len(), 3, "no duplicate");
    assert_eq!(s.versions(&brannoc).await.len(), 2);
    s.drop().await;
}

#[tokio::test]
async fn an_import_into_a_campaign_the_tables_lack_or_of_another_campaign_s_player_is_refused() {
    let s = Scratch::new("refused").await;
    let engine = s.serve().await;
    let (kind, message) = refusal(
        import(
            &engine,
            json!({ "url": id(BRANNOC).url(), "campaign": "c9000000-0000-4000-8000-000000000009" }),
            None,
        )
        .await,
    );
    assert_eq!(kind, RefusalKind::NotFound, "{message}");
    let (kind, message) = refusal(
        import(
            &engine,
            json!({ "url": id(BRANNOC).url(), "campaign": VALOMBRE, "player": INES }),
            None,
        )
        .await,
    );
    assert_eq!(kind, RefusalKind::Unprocessable);
    assert!(message.contains("pas de la campagne"), "{message}");
    assert!(
        engine.run_once().await.expect("ran").is_empty(),
        "nothing queued"
    );
    assert_eq!(s.pcs().await.len(), 2, "nothing written");
    s.drop().await;
}

#[tokio::test]
async fn an_import_on_a_pc_links_it_unless_another_pc_of_its_campaign_is_linked_to_the_character() {
    let s = Scratch::new("target").await;
    let engine = s.serve().await;
    let (kind, message) =
        refusal(import(&engine, json!({ "url": elowen_url() }), Some(GASPARD)).await);
    assert_eq!(kind, RefusalKind::Unprocessable);
    assert!(message.contains("« Elowen Ventargent »"), "{message}");
    // Brannoc is linked to no PC of Valombre: Gaspard takes his statblock, and keeps his name.
    import(&engine, json!({ "url": id(BRANNOC).url() }), Some(GASPARD))
        .await
        .expect("queued");
    applied(&engine.run_once().await.expect("applied"));
    let pcs = s.pcs().await;
    assert_eq!(pcs[1]["name"], json!("Gaspard"));
    assert_eq!(pcs[1]["hpMax"], json!(51));
    assert_eq!(pcs[1]["dndbeyondId"], json!(BRANNOC.to_string()));
    assert_eq!(pcs[1]["tokenColor"], json!("primary"));
    assert_eq!(s.versions(GASPARD).await[1]["source"], json!("dndbeyond"));
    s.drop().await;
}

#[tokio::test]
async fn an_import_on_a_pc_while_one_creating_the_character_s_pc_is_queued_is_refused() {
    let s = Scratch::new("pending").await;
    let engine = s.serve().await;
    // Brannoc's import is queued: the tables do not hold his PC yet.
    import(
        &engine,
        json!({ "url": id(BRANNOC).url(), "campaign": VALOMBRE }),
        None,
    )
    .await
    .expect("queued");
    let (kind, message) =
        refusal(import(&engine, json!({ "url": id(BRANNOC).url() }), Some(GASPARD)).await);
    assert_eq!(kind, RefusalKind::Unprocessable);
    assert!(message.contains("« Brannoc »"), "{message}");
    applied(&engine.run_once().await.expect("Brannoc's import alone"));
    let pcs = s.pcs().await;
    assert_eq!(pcs.len(), 3, "{pcs:?}");
    assert_eq!(pcs[2]["name"], json!("Gaspard"));
    assert_eq!(pcs[2]["dndbeyondId"], Value::Null);
    s.drop().await;
}
