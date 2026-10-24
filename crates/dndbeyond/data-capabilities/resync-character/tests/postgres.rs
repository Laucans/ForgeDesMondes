//! dndbeyond.resyncCharacter over PostgreSQL (`DATABASE_URL`), as the server
//! plants it: a resync is prepared against the PC the tables hold — as the
//! mock-up's fixtures are, which the engine never wrote —, judged by the
//! DataGuard and projected into `character` and `entity_version` in the
//! transaction that applies it, where the version trigger judges
//! `dndbeyond-resync-scope` again. A PC linked to nothing is refused and
//! nothing is queued. Each test works in a scratch database of its own,
//! migrated as the server migrates it, dropped when it ends.

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
use support::{ELOWEN, levelled_up};

const VALOMBRE: &str = "c1000000-0000-4000-8000-000000000001";
/// Valombre's player.
const JULIE: &str = "a0000000-0000-4000-8000-000000000001";
/// Elowen, a PC of Valombre the fixtures link to D&D Beyond.
const ELOWEN_PC: &str = "e1000000-0000-4000-8000-000000000001";
/// Gaspard, a PC of Valombre the DM made, linked to nothing.
const GASPARD: &str = "e1000000-0000-4000-8000-000000000002";

/// The time the fixtures were written at, long before any resync.
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
/// to nothing, each at version 1.
struct Scratch {
    name: String,
    url: String,
    client: Client,
}

impl Scratch {
    async fn new(name: &str) -> Scratch {
        let name = format!("resync_character_test_{name}_{}", std::process::id());
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
        let spells = json!([{ "name": "Marque du chasseur",
                              "description": "Sort de niveau 1 : 1d6 dégâts supplémentaires contre la cible marquée." }]);
        let elowen = json!({
            "type": "pj", "name": "Elowen Ventargent", "notes": "Cherche son frère Aerin.",
            "playerId": JULIE, "strength": 12, "dexterity": 18, "constitution": 14,
            "intelligence": 10, "wisdom": 15, "charisma": 9, "armorClass": 16, "hpMax": 44,
            "spells": spells, "actions": actions, "tokenColor": "success",
            "dndbeyondId": ELOWEN.to_string(), "dndbeyondSyncedAt": LONG_AGO,
            "aliases": ["Elowen", "la rôdeuse"],
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
                     ('{VALOMBRE}', 'Les Cendres de Valombre', 'campagne');
                 INSERT INTO player (id, campaign_id, name) VALUES ('{JULIE}', '{VALOMBRE}', 'Julie');
                 INSERT INTO entity (id, campaign_id, type, name, notes, created_at, updated_at) VALUES
                     ('{ELOWEN_PC}', '{VALOMBRE}', 'pj', 'Elowen Ventargent', 'Cherche son frère Aerin.',
                      '{LONG_AGO}', '{LONG_AGO}'),
                     ('{GASPARD}', '{VALOMBRE}', 'pj', 'Gaspard', NULL, '{LONG_AGO}', '{LONG_AGO}');
                 INSERT INTO character (entity_id, player_id, strength, dexterity, constitution,
                         intelligence, wisdom, charisma, armor_class, hp_max, spells, actions,
                         token_color, dndbeyond_id, dndbeyond_synced_at) VALUES
                     ('{ELOWEN_PC}', '{JULIE}', 12, 18, 14, 10, 15, 9, 16, 44, '{spells}', '{actions}',
                      'success', '{ELOWEN}', '{LONG_AGO}'),
                     ('{GASPARD}', NULL, 10, 10, 10, 10, 10, 10, 10, 8, '[]', '[]', 'primary',
                      NULL, NULL);
                 INSERT INTO entity_alias (entity_id, alias) VALUES
                     ('{ELOWEN_PC}', 'Elowen'), ('{ELOWEN_PC}', 'la rôdeuse');
                 INSERT INTO entity_version (entity_id, version_number, source, snapshot, diff, created_at)
                     VALUES ('{ELOWEN_PC}', 1, 'dm', '{elowen}', '[]', '{LONG_AGO}'),
                         ('{GASPARD}', 1, 'dm', '{gaspard}', '[]', '{LONG_AGO}');"
            ))
            .await
            .expect("Valombre, Julie, Elowen and Gaspard");
        Scratch { name, url, client }
    }

    /// The engine as the server plants this unit and its target aggregate
    /// over this database — the aggregate's projection in the apply
    /// transaction, the tables lent to the Prepare — with the port's fake,
    /// knowing Elowen as she levelled up.
    async fn serve(&self) -> Engine {
        let plugins = || -> Plugins {
            [aggregate_entity::plugin(), resync_character::plugin()]
                .into_iter()
                .collect()
        };
        let store = PgStore::open(connect(&self.url).await)
            .await
            .expect("the store")
            .with_projections(&plugins())
            .await
            .expect("warmed");
        let sheets = FakeCharacterSheets::new().with_sheet(levelled_up());
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
            DataCapability::parse(resync_character::MANIFEST).expect("contract F"),
        )
        .with_plugins(plugins())
        .with_shared(shared);
        engine.load().await.expect("the engine loads");
        engine
    }

    /// The JSON the one column of `sql`'s one row holds.
    async fn one(&self, sql: &str) -> Value {
        let row = self.client.query_one(sql, &[]).await.expect("the tables");
        serde_json::from_str(row.get::<_, &str>(0)).expect("JSON")
    }

    /// `pc`, its sheet and its aliases as the tables hold them.
    async fn pc(&self, pc: &str) -> Value {
        self.one(&format!(
            "SELECT jsonb_build_object('name', e.name, 'notes', e.notes, 'player', c.player_id,
                 'wisdom', c.wisdom, 'hpMax', c.hp_max, 'armorClass', c.armor_class,
                 'spells', (SELECT jsonb_agg(s ->> 'name') FROM jsonb_array_elements(c.spells) s),
                 'tokenColor', c.token_color, 'dndbeyondId', c.dndbeyond_id,
                 'synced', c.dndbeyond_synced_at > '{LONG_AGO}'::timestamptz,
                 'aliases', (SELECT jsonb_agg(a.alias ORDER BY a.alias)
                     FROM entity_alias a WHERE a.entity_id = e.id))::text
             FROM entity e JOIN character c ON c.entity_id = e.id WHERE e.id = '{pc}'"
        ))
        .await
    }

    /// The versions of `pc`, by number: their source and the fields their
    /// diff names.
    async fn versions(&self, pc: &str) -> Value {
        self.one(&format!(
            "SELECT jsonb_agg(jsonb_build_object('number', version_number, 'source', source,
                 'fields', (SELECT jsonb_agg(c ->> 'field') FROM jsonb_array_elements(diff) c))
                 ORDER BY version_number)::text
             FROM entity_version WHERE entity_id = '{pc}'"
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

/// A resync of `target` by the DM, approver of their own campaign: the
/// partition it queued on.
async fn resync(engine: &Engine, target: &str) -> Result<String> {
    let submission: Submission = serde_json::from_value(json!({
        "dataCapability": "dndbeyond.resyncCharacter@1",
        "targetId": target,
        "payload": {},
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

#[tokio::test]
async fn a_resync_of_a_pc_the_tables_hold_updates_its_sheet_and_versions_the_level_up_alone() {
    let s = Scratch::new("levelup").await;
    let engine = s.serve().await;
    let partition = resync(&engine, ELOWEN_PC).await.expect("queued");
    assert_eq!(partition, format!("Entity/{ELOWEN_PC}"));
    applied(&engine.run_once().await.expect("applied"));
    // Her stats, HP and spells are D&D Beyond's; her name, notes, aliases,
    // player and token are the DM's still.
    assert_eq!(
        s.pc(ELOWEN_PC).await,
        json!({ "name": "Elowen Ventargent", "notes": "Cherche son frère Aerin.",
                "player": JULIE, "wisdom": 16, "hpMax": 52, "armorClass": 16,
                "spells": ["Marque du chasseur", "Explorateur né amélioré",
                           "Communication avec les animaux"],
                "tokenColor": "success", "dndbeyondId": ELOWEN.to_string(), "synced": true,
                "aliases": ["Elowen", "la rôdeuse"] })
    );
    assert_eq!(
        s.versions(ELOWEN_PC).await,
        json!([
            { "number": 1, "source": "dm", "fields": null },
            { "number": 2, "source": "dndbeyond",
              "fields": ["dndbeyondSyncedAt", "hpMax", "spells", "wisdom"] },
        ])
    );
    // Resynchronised again — nothing new on D&D Beyond —, the sync date alone.
    resync(&engine, ELOWEN_PC).await.expect("queued");
    applied(&engine.run_once().await.expect("applied"));
    assert_eq!(
        s.versions(ELOWEN_PC).await[2],
        json!({ "number": 3, "source": "dndbeyond", "fields": ["dndbeyondSyncedAt"] })
    );
    drop(engine);
    s.drop().await;
}

#[tokio::test]
async fn a_resync_of_a_pc_linked_to_nothing_is_refused_and_nothing_is_written() {
    let s = Scratch::new("unlinked").await;
    let engine = s.serve().await;
    match resync(&engine, GASPARD).await {
        Err(Error::Refused(refusal)) => {
            assert_eq!(refusal.kind, RefusalKind::Unprocessable);
            assert!(
                refusal.message.contains("pas lié à D&D Beyond"),
                "{}",
                refusal.message
            );
        }
        other => panic!("not refused: {other:?}"),
    }
    assert!(
        engine.run_once().await.expect("ran").is_empty(),
        "nothing queued"
    );
    assert_eq!(
        s.versions(GASPARD).await,
        json!([{ "number": 1, "source": "dm", "fields": null }])
    );
    drop(engine);
    s.drop().await;
}
