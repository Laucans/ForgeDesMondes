//! The BattlemapInstance aggregate (`aggregates/BattlemapInstance/
//! aggregate.json`) on the tables `0010_battlemap_instance.sql` lays,
//! against the harness's PostgreSQL (`DATABASE_URL`): each of its fields is
//! a column of `battlemap_instance`, `token`, `combat`, `initiative_entry` or
//! `initiative_order_view`, and each column one of its fields; each of its
//! relations is its foreign key's actions; what the DataGuard accepts of a
//! row the tables take, and what they refuse it refuses; the fields it
//! keeps derived are the tables' to compute. Each test works in a scratch
//! database of its own, dropped when it ends.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
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
use tokio_postgres::error::SqlState;
use tokio_postgres::{Client, NoTls};

/// Each field of the aggregate, and the column it is: `relation.column`.
const COLUMNS: [(&str, &str); 40] = [
    ("battlemapInstanceId", "battlemap_instance.id"),
    ("battlemapMediaId", "battlemap_instance.battlemap_media_id"),
    ("sessionId", "battlemap_instance.session_id"),
    ("sceneId", "battlemap_instance.scene_id"),
    ("createdAt", "battlemap_instance.created_at"),
    ("resetAt", "battlemap_instance.reset_at"),
    ("tokenId", "token.id"),
    ("tokenInstanceId", "token.instance_id"),
    ("characterId", "token.character_id"),
    ("characterType", "token.character_type"),
    ("number", "token.number"),
    ("name", "token.name"),
    ("q", "token.q"),
    ("r", "token.r"),
    ("visible", "token.visible"),
    ("hpCurrent", "token.hp_current"),
    ("hpMax", "token.hp_max"),
    ("conditions", "token.conditions"),
    ("dead", "token.dead"),
    ("healthBand", "token.health_band"),
    ("labelInitial", "token.label_initial"),
    ("combatId", "combat.id"),
    ("combatInstanceId", "combat.instance_id"),
    ("round", "combat.round"),
    ("currentEntryId", "combat.current_entry_id"),
    ("startedAt", "combat.started_at"),
    ("endedAt", "combat.ended_at"),
    ("initiativeEntryId", "initiative_entry.id"),
    ("initiativeEntryCombatId", "initiative_entry.combat_id"),
    ("initiativeEntryTokenId", "initiative_entry.token_id"),
    ("roll", "initiative_entry.roll"),
    ("initiative", "initiative_entry.initiative"),
    ("dexScore", "initiative_entry.dex_score"),
    ("tiebreak", "initiative_entry.tiebreak"),
    ("rank", "initiative_entry.rank"),
    (
        "initiativeOrderViewCombatId",
        "initiative_order_view.combat_id",
    ),
    ("entryId", "initiative_order_view.entry_id"),
    ("orderIndex", "initiative_order_view.order_index"),
    ("shownToPlayers", "initiative_order_view.shown_to_players"),
    ("skipped", "initiative_order_view.skipped"),
];

/// The relations the aggregate spans.
const RELATIONS: [&str; 5] = [
    "battlemap_instance",
    "token",
    "combat",
    "initiative_entry",
    "initiative_order_view",
];

/// The test-only DataCapabilities: their name, their effect, the payload
/// field naming the row.
const MANIFESTS: [(&str, &str, &str); 4] = [
    ("test.openInstance", "insert", "battlemapInstanceId"),
    ("test.dropToken", "insert", "tokenId"),
    ("test.startCombat", "insert", "combatId"),
    ("test.addEntry", "insert", "initiativeEntryId"),
];

const VALOMBRE: &str = "c1000000-0000-4000-8000-000000000001";
/// Valombre's PC.
const ELOWEN: &str = "e1000000-0000-4000-8000-000000000001";
/// Valombre's NPC.
const MORLOX: &str = "e1000000-0000-4000-8000-000000000002";
/// Valombre's place.
const TAVERNE: &str = "e1000000-0000-4000-8000-000000000003";
/// Valombre's battlemap, set at the Taverne.
const CARTE: &str = "d1000000-0000-4000-8000-000000000001";
const SEANCE: &str = "51000000-0000-4000-8000-000000000001";
const ARRIVEE: &str = "5c000000-0000-4000-8000-000000000001";
const INSTANCE: &str = "b1000000-0000-4000-8000-000000000001";
const JETON_ELOWEN: &str = "70000000-0000-4000-8000-000000000001";
const MORLOX_1: &str = "70000000-0000-4000-8000-000000000002";
const MORLOX_2: &str = "70000000-0000-4000-8000-000000000003";
const COMBAT: &str = "c0000000-0000-4000-8000-000000000001";
const ENTREE_MORLOX_2: &str = "1e000000-0000-4000-8000-000000000001";
const ENTREE_ELOWEN: &str = "1e000000-0000-4000-8000-000000000002";
const ENTREE_MORLOX_1: &str = "1e000000-0000-4000-8000-000000000003";

fn url() -> String {
    std::env::var("DATABASE_URL").expect("DATABASE_URL names the harness's PostgreSQL")
}

fn migrations() -> PathBuf {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/migrations")).to_path_buf()
}

async fn connect(config: &tokio_postgres::Config) -> Client {
    let (client, connection) = config.connect(NoTls).await.expect("PostgreSQL");
    tokio::spawn(connection);
    client
}

const AGGREGATE: &str = include_str!("../../table/aggregates/BattlemapInstance/aggregate.json");

fn aggregate() -> Value {
    serde_json::from_str(AGGREGATE).expect("contract I")
}

/// The column `field` is.
fn column(field: &str) -> (&'static str, &'static str) {
    COLUMNS
        .iter()
        .find(|(own, _)| *own == field)
        .and_then(|(_, column)| column.split_once('.'))
        .unwrap_or_else(|| panic!("{field} is no column"))
}

/// The SQL adding element `id` to Valombre, with the version every element
/// is written with, and its sheet for a PC or an NPC.
fn element(id: &str, kind: &str, name: &str) -> String {
    let sheet = if kind == "lieu" {
        String::new()
    } else {
        format!("INSERT INTO character (entity_id, token_color) VALUES ('{id}', '#888');")
    };
    format!(
        "INSERT INTO entity (id, campaign_id, type, name) VALUES ('{id}', '{VALOMBRE}', '{kind}', '{name}'); \
         {sheet} \
         INSERT INTO entity_version (entity_id, version_number, source, snapshot, diff) \
         VALUES ('{id}', 1, 'dm', '{{}}', '[]');"
    )
}

/// The tavern's instance, as a command writes it.
fn instance() -> Value {
    json!({
        "battlemapInstanceId": INSTANCE,
        "battlemapMediaId": CARTE,
        "sessionId": SEANCE,
        "sceneId": ARRIVEE,
        "createdAt": "2026-10-18T20:00:00Z",
    })
}

/// A token of `character` on the tavern's instance, `hp` its current and
/// maximum HP when tracked.
fn token(id: &str, character: &str, name: &str, visible: bool, hp: Option<(i64, i64)>) -> Value {
    let mut token = json!({
        "tokenId": id,
        "tokenInstanceId": INSTANCE,
        "characterId": character,
        "name": name,
        "q": 2,
        "r": -1,
        "visible": visible,
        "conditions": ["À terre"],
    });
    if let Some((current, max)) = hp {
        token["hpCurrent"] = json!(current);
        token["hpMax"] = json!(max);
        token["number"] = json!(name.rsplit(' ').next().and_then(|n| n.parse::<i64>().ok()));
    }
    token
}

/// The tavern's combat, at its first round, before any entry.
fn combat() -> Value {
    json!({
        "combatId": COMBAT,
        "combatInstanceId": INSTANCE,
        "round": 1,
        "startedAt": "2026-10-18T20:05:00Z",
    })
}

/// An entry of the combat.
fn entry(id: &str, token: &str, roll: Value, initiative: i64, dex: i64, rank: i64) -> Value {
    json!({
        "initiativeEntryId": id,
        "initiativeEntryCombatId": COMBAT,
        "initiativeEntryTokenId": token,
        "roll": roll,
        "initiative": initiative,
        "dexScore": dex,
        "tiebreak": 0.5,
        "rank": rank,
    })
}

/// The tavern's rows, each with the DataCapability that writes it: the
/// instance, Elowen (HP not tracked), Morlox 1 (7 of 7, shown) and Morlox 2
/// (0 of 7, hidden), the combat and its three entries.
fn tavern() -> Vec<(&'static str, Value)> {
    vec![
        ("test.openInstance", instance()),
        (
            "test.dropToken",
            token(JETON_ELOWEN, ELOWEN, "Elowen", true, None),
        ),
        (
            "test.dropToken",
            token(MORLOX_1, MORLOX, "Morlox 1", true, Some((7, 7))),
        ),
        (
            "test.dropToken",
            token(MORLOX_2, MORLOX, "Morlox 2", false, Some((0, 7))),
        ),
        ("test.startCombat", combat()),
        (
            "test.addEntry",
            entry(ENTREE_MORLOX_2, MORLOX_2, json!(18), 19, 12, 1),
        ),
        (
            "test.addEntry",
            entry(ENTREE_ELOWEN, JETON_ELOWEN, Value::Null, 15, 14, 2),
        ),
        (
            "test.addEntry",
            entry(ENTREE_MORLOX_1, MORLOX_1, json!(3), 4, 12, 3),
        ),
    ]
}

/// The engine as the server plants the aggregate — its manifest and its
/// plugin — over the in-memory store, with the test-only DataCapabilities.
async fn engine() -> Engine {
    let mut engine = Engine::new(
        Arc::new(MemoryStore::default()),
        Arc::new(MemoryNotifier::default()),
        Arc::new(SystemClock),
    )
    .with_aggregate(Aggregate::parse(AGGREGATE).expect("contract I"))
    .with_plugins(std::iter::once(aggregate_battlemap_instance::plugin()).collect());
    engine.load().await.expect("the in-memory engine");
    for (name, effect, id) in MANIFESTS {
        let manifest = json!({
            "dataCapability": name,
            "owner": "dataguard",
            "version": 1,
            "effect": effect,
            "target": { "aggregate": "BattlemapInstance", "id": format!("${id}") },
            "mode": "overwrite",
            "callableBy": ["*"],
        });
        engine = engine.with_data_capability(
            DataCapability::parse(&manifest.to_string()).expect("contract F"),
        );
    }
    engine
}

/// What the single applier made of one command.
async fn written(engine: &Engine, data_capability: &str, payload: &Value) -> CommandResult {
    let payload = payload.as_object().expect("a payload is an object");
    engine
        .submit(Submission {
            data_capability: data_capability.to_string(),
            payload: payload.clone().into_iter().collect(),
            target_id: None,
            based_on: None,
            idempotency_key: None,
            caller: Caller {
                by: "dm".to_string(),
                from: "table.battlemap".to_string(),
                permissions: Vec::new(),
                roles: vec!["role:approver".to_string()],
            },
        })
        .await
        .expect("queued");
    let mut applied = engine.run_once().await.expect("the applier ran");
    assert_eq!(applied.len(), 1, "{applied:?}");
    applied.remove(0)
}

/// A database of its own.
struct Scratch {
    client: Client,
    name: String,
}

impl Scratch {
    /// Migrated by the Data layer, holding what the tavern's instance stands
    /// on: Valombre, Elowen and Morlox, the tavern's battlemap and its grid,
    /// the session and its scene.
    async fn new(name: &str) -> Scratch {
        let name = format!("battlemap_instance_test_{name}_{}", std::process::id());
        let config: tokio_postgres::Config = url().parse().expect("a PostgreSQL URL");
        let admin = connect(&config).await;
        for sql in [
            format!("DROP DATABASE IF EXISTS {name} WITH (FORCE)"),
            format!("CREATE DATABASE {name}"),
        ] {
            admin.batch_execute(&sql).await.expect("a scratch database");
        }
        let mut client = connect(config.clone().dbname(&name)).await;
        data::migrate(&mut client, &migrations())
            .await
            .expect("migrated by the Data layer");
        let s = Scratch { client, name };
        s.accepted(&format!(
            "INSERT INTO campaign (id, name, kind) VALUES ('{VALOMBRE}', 'Les Cendres de Valombre', 'campagne'); \
             {}{}{} \
             INSERT INTO media (id, campaign_id, kind, title, file_path, width_px, height_px, place_id) \
             VALUES ('{CARTE}', '{VALOMBRE}', 'battlemap', 'Taverne', 'carte.png', 2048, 1536, '{TAVERNE}'); \
             INSERT INTO battlemap_grid (media_id, hex_size_px, offset_x, offset_y, orientation, color, opacity) \
             VALUES ('{CARTE}', 64, 0, 0, 'pointe', '#202020', 0.4); \
             INSERT INTO session (id, campaign_id, position, title, status, microphone_on) \
             VALUES ('{SEANCE}', '{VALOMBRE}', 1, 'Le Sanglier', 'live', true); \
             INSERT INTO session_present_character (session_id, character_id) VALUES ('{SEANCE}', '{ELOWEN}'); \
             INSERT INTO scene (id, session_id, position, title) VALUES ('{ARRIVEE}', '{SEANCE}', 1, 'Arrivée')",
            element(ELOWEN, "pj", "Elowen"),
            element(MORLOX, "pnj", "Morlox"),
            element(TAVERNE, "lieu", "La Taverne du Sanglier"),
        ))
        .await;
        s
    }

    async fn accepted(&self, sql: &str) {
        self.client
            .batch_execute(sql)
            .await
            .unwrap_or_else(|e| panic!("{sql}: {e:?}"));
    }

    /// `row`, a command's payload, written to its table: its fields are the
    /// columns the statement names.
    async fn insert(&self, row: &Value) -> Result<(), tokio_postgres::Error> {
        let fields: BTreeMap<&str, &Value> = row
            .as_object()
            .expect("a row")
            .iter()
            .map(|(field, value)| (field.as_str(), value))
            .collect();
        let table = column(fields.keys().next().expect("a field")).0;
        let mut record = serde_json::Map::new();
        for (field, value) in &fields {
            let (own, name) = column(field);
            assert_eq!(own, table, "{field} is not {table}'s");
            record.insert(name.to_string(), (*value).clone());
        }
        let names: Vec<&String> = record.keys().collect();
        let list = names
            .iter()
            .map(|name| name.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        let sql = format!(
            "INSERT INTO {table} ({list}) SELECT {list} FROM json_populate_record(NULL::{table}, $1::text::json)"
        );
        self.client
            .execute(&sql, &[&Value::Object(record).to_string()])
            .await
            .map(|_| ())
    }

    /// The tavern's rows written to the tables, then the combat's current
    /// participant — Elowen — which its entry must exist for.
    async fn tavern(&self) {
        for (_, row) in tavern() {
            self.insert(&row)
                .await
                .unwrap_or_else(|e| panic!("{row}: {e:?}"));
        }
        self.accepted(&format!(
            "UPDATE combat SET current_entry_id = '{ENTREE_ELOWEN}' WHERE id = '{COMBAT}'"
        ))
        .await;
    }

    async fn texts(&self, sql: &str) -> Vec<String> {
        self.client
            .query(sql, &[])
            .await
            .unwrap_or_else(|e| panic!("{sql}: {e:?}"))
            .iter()
            .map(|row| {
                row.get::<_, Option<String>>(0)
                    .unwrap_or_else(|| "-".to_string())
            })
            .collect()
    }

    async fn count(&self, sql: &str) -> i64 {
        self.client
            .query_one(sql, &[])
            .await
            .unwrap_or_else(|e| panic!("{sql}: {e:?}"))
            .get(0)
    }

    /// Drops the database, tried again while an autovacuum worker finishes.
    async fn drop(self) {
        drop(self.client);
        let config: tokio_postgres::Config = url().parse().expect("a PostgreSQL URL");
        let admin = connect(&config).await;
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

// AC8: the token's marker, characterType, is a field aggregate.json declares
// and token.character_type the column it is.
#[tokio::test]
async fn every_field_of_the_aggregate_is_a_column_of_its_tables_and_every_column_a_field() {
    let s = Scratch::new("columns").await;
    let mut columns = Vec::new();
    for relation in RELATIONS {
        for name in s
            .texts(&format!(
                "SELECT a.attname::text FROM pg_attribute a \
                 WHERE a.attrelid = '{relation}'::regclass AND a.attnum > 0 \
                 AND NOT a.attisdropped ORDER BY a.attnum"
            ))
            .await
        {
            columns.push(format!("{relation}.{name}"));
        }
    }
    columns.sort_unstable();
    let mut mapped: Vec<String> = COLUMNS.iter().map(|(_, c)| (*c).to_string()).collect();
    mapped.sort_unstable();
    assert_eq!(mapped, columns);
    let aggregate = aggregate();
    let mut fields: Vec<&str> = aggregate["fields"]
        .as_object()
        .expect("fields")
        .keys()
        .map(String::as_str)
        .collect();
    fields.sort_unstable();
    let mut named: Vec<&str> = COLUMNS.iter().map(|(field, _)| *field).collect();
    named.sort_unstable();
    assert_eq!(named, fields);
    // A derived field is a generated column or the view's; an authoritative
    // one, a column the commands write.
    for (field, kind) in aggregate["fields"].as_object().expect("fields") {
        let (table, name) = column(field);
        let generated = s
            .count(&format!(
                "SELECT count(*) FROM pg_attribute WHERE attrelid = '{table}'::regclass \
                 AND attname = '{name}' AND attgenerated = 's'"
            ))
            .await
            == 1;
        let derived = kind["kind"] == "derived";
        assert_eq!(
            derived,
            generated || table == "initiative_order_view",
            "{field}"
        );
    }
    s.drop().await;
}

#[tokio::test]
async fn every_relation_of_the_aggregate_is_its_foreign_key_s_actions() {
    let s = Scratch::new("relations").await;
    for relation in aggregate()["relations"].as_array().expect("relations") {
        let from = relation["from"].as_str().expect("a field");
        let (table, name) = column(from.trim_start_matches("BattlemapInstance."));
        let row = s
            .client
            .query_one(
                "SELECT c.confdeltype::text, c.confupdtype::text, c.condeferrable, c.condeferred \
                 FROM pg_constraint c \
                 JOIN pg_attribute a ON a.attrelid = c.conrelid AND a.attnum = c.conkey[1] \
                 WHERE c.conrelid = $1::text::regclass AND c.contype = 'f' AND a.attname = $2",
                &[&table, &name],
            )
            .await
            .unwrap_or_else(|e| panic!("{from}: {e:?}"));
        let (on_delete, on_update): (String, String) = (row.get(0), row.get(1));
        let (deferrable, deferred): (bool, bool) = (row.get(2), row.get(3));
        // Restrict, and review — a deletion a human approves — keep the row
        // until the commit: what a campaign's whole deletion takes away with
        // it is judged once it has all gone.
        let expected = match relation["onDelete"].as_str().expect("onDelete") {
            "cascade" => ("c", false),
            "nullify" => ("n", false),
            "restrict" | "review" => ("a", true),
            other => panic!("{from}: onDelete {other}"),
        };
        assert_eq!(
            (on_delete.as_str(), deferrable && deferred),
            expected,
            "{from}"
        );
        assert_eq!(relation["onUpdate"], "propagate", "{from}");
        assert_eq!(on_update, "c", "{from}");
    }
    s.drop().await;
}

#[tokio::test]
async fn what_the_dataguard_accepts_of_the_tavern_the_tables_take() {
    let s = Scratch::new("accepted").await;
    let engine = engine().await;
    for (data_capability, row) in tavern() {
        let result = written(&engine, data_capability, &row).await;
        assert_eq!(result.status, State::Applied, "{row}: {result:?}");
    }
    s.tavern().await;
    assert_eq!(s.count("SELECT count(*) FROM token").await, 3);
    assert_eq!(s.count("SELECT count(*) FROM initiative_entry").await, 3);
    s.drop().await;
}

#[tokio::test]
async fn what_the_tables_refuse_of_a_token_the_dataguard_refuses() {
    let s = Scratch::new("refused").await;
    let engine = engine().await;
    assert_eq!(
        written(&engine, "test.openInstance", &instance())
            .await
            .status,
        State::Applied
    );
    s.insert(&instance()).await.expect("the instance");
    let morlox = || token(MORLOX_1, MORLOX, "Morlox 1", true, Some((7, 7)));
    for (row, id) in [
        // Off the axes' integer: single-position-source.
        (
            {
                let mut row = morlox();
                row["q"] = json!(3_000_000_000_i64);
                row
            },
            "single-position-source",
        ),
        // No hex at all.
        (
            {
                let mut row = morlox();
                row.as_object_mut().expect("a token").remove("r");
                row
            },
            "single-position-source",
        ),
        // HP beyond a smallint: pj-hp-not-tracked.
        (
            {
                let mut row = morlox();
                row["hpMax"] = json!(40_000);
                row["hpCurrent"] = json!(40_000);
                row
            },
            "pj-hp-not-tracked",
        ),
        // A number beyond a smallint: multi-drop-numbering.
        (
            {
                let mut row = morlox();
                row["number"] = json!(40_000);
                row
            },
            "multi-drop-numbering",
        ),
        // A token on no character: token-roster.
        (
            {
                let mut row = morlox();
                row.as_object_mut().expect("a token").remove("characterId");
                row
            },
            "token-roster",
        ),
        // What every token carries, missing.
        (
            {
                let mut row = morlox();
                row.as_object_mut().expect("a token").remove("visible");
                row
            },
            "players-see-band-only",
        ),
        (
            {
                let mut row = morlox();
                row.as_object_mut().expect("a token").remove("conditions");
                row
            },
            "state-persisted-reset",
        ),
        (
            {
                let mut row = morlox();
                row.as_object_mut().expect("a token").remove("name");
                row
            },
            "multi-drop-numbering",
        ),
    ] {
        assert!(s.insert(&row).await.is_err(), "the tables took {row}");
        let result = written(&engine, "test.dropToken", &row).await;
        assert_eq!(result.status, State::Rejected, "{row}: {result:?}");
        assert!(
            result.violations.iter().any(|v| v == id),
            "{row}: {result:?}"
        );
    }
    // Death written (dead-derived), a band written (players-see-band-only):
    // the columns are the table's, and the fields derived, which the engine
    // refuses before anything is queued.
    for (field, value) in [("dead", json!(false)), ("healthBand", json!("indemne"))] {
        let mut row = morlox();
        row[field] = value;
        assert!(s.insert(&row).await.is_err(), "the tables took {row}");
        let Value::Object(payload) = row.clone() else {
            panic!("a token is an object");
        };
        let refusal = engine
            .submit(Submission {
                data_capability: "test.dropToken".to_string(),
                payload: payload.into_iter().collect(),
                ..Submission::default()
            })
            .await
            .expect_err("refused before it is queued");
        assert!(refusal.to_string().contains(field), "{row}: {refusal}");
    }
    s.drop().await;
}

#[tokio::test]
async fn dead_derived_and_players_see_band_only_are_the_token_s_generated_columns() {
    let s = Scratch::new("generated").await;
    s.tavern().await;
    // Morlox 1, from 7 to 0 of 7: dead at 0 alone, and a band, never a
    // figure.
    let mut bands = Vec::new();
    for hp in (0..=7).rev() {
        s.accepted(&format!(
            "UPDATE token SET hp_current = {hp} WHERE id = '{MORLOX_1}'"
        ))
        .await;
        let row = s
            .client
            .query_one(
                &format!("SELECT dead, health_band FROM token WHERE id = '{MORLOX_1}'"),
                &[],
            )
            .await
            .expect("the token read");
        let dead: bool = row.get(0);
        assert_eq!(dead, hp == 0, "dead-derived at {hp}");
        bands.push(row.get::<_, String>(1));
    }
    assert_eq!(
        bands,
        [
            "indemne", "blessé", "blessé", "blessé", "en sang", "en sang", "en sang", "mort"
        ]
    );
    // Elowen's HP are not tracked: never dead, no band.
    assert_eq!(
        s.texts(&format!(
            "SELECT dead::text || coalesce(health_band, '-') FROM token WHERE id = '{JETON_ELOWEN}'"
        ))
        .await,
        ["false-"]
    );
    // Neither is written: a command's write could not land.
    for column in ["dead", "health_band", "label_initial"] {
        let error = s
            .client
            .batch_execute(&format!(
                "UPDATE token SET {column} = NULL WHERE id = '{MORLOX_1}'"
            ))
            .await
            .expect_err(column);
        assert_eq!(
            error.code(),
            Some(&SqlState::GENERATED_ALWAYS),
            "{column}: {error:?}"
        );
    }
    s.drop().await;
}

#[tokio::test]
async fn hidden_keeps_rank_and_dead_derived_in_the_order_the_view_gives() {
    let s = Scratch::new("order").await;
    s.tavern().await;
    let order = "SELECT e.token_id::text || ' ' || v.order_index || ' ' || v.shown_to_players || ' ' || v.skipped \
                 FROM initiative_order_view v JOIN initiative_entry e ON e.id = v.entry_id \
                 ORDER BY v.order_index";
    assert_eq!(
        s.texts(order).await,
        [
            format!("{MORLOX_2} 1 false true"),
            format!("{JETON_ELOWEN} 2 true false"),
            format!("{MORLOX_1} 3 true false"),
        ]
    );
    // Revealed, Morlox 2 keeps its place, and so do the others.
    s.accepted(&format!(
        "UPDATE token SET visible = true WHERE id = '{MORLOX_2}'"
    ))
    .await;
    assert_eq!(
        s.texts(order).await,
        [
            format!("{MORLOX_2} 1 true true"),
            format!("{JETON_ELOWEN} 2 true false"),
            format!("{MORLOX_1} 3 true false"),
        ]
    );
    s.drop().await;
}

#[tokio::test]
async fn state_persisted_reset_clears_the_instance_and_instance_isolated_leaves_the_battlemap() {
    let s = Scratch::new("reset").await;
    s.tavern().await;
    let battlemap = format!(
        "SELECT row_to_json(m)::text || row_to_json(g)::text FROM media m \
         JOIN battlemap_grid g ON g.media_id = m.id WHERE m.id = '{CARTE}'"
    );
    let before = s.texts(&battlemap).await;
    // What each action leaves is what a reopening reads.
    s.accepted(&format!(
        "UPDATE token SET q = 5, r = -3, hp_current = 4, conditions = '{{Empoisonné}}' WHERE id = '{MORLOX_1}'"
    ))
    .await;
    assert_eq!(
        s.texts(&format!(
            "SELECT q || ',' || r || ' ' || hp_current || ' ' || array_to_string(conditions, ',') \
             FROM token WHERE id = '{MORLOX_1}'"
        ))
        .await,
        ["5,-3 4 Empoisonné"]
    );
    // « Réinitialiser la carte »: the instance's tokens go, and with them
    // the combat's initiative; its combat goes; the instance stays, reset.
    s.accepted(&format!(
        "BEGIN; \
         DELETE FROM token WHERE instance_id = '{INSTANCE}'; \
         DELETE FROM combat WHERE instance_id = '{INSTANCE}'; \
         UPDATE battlemap_instance SET reset_at = now() WHERE id = '{INSTANCE}'; \
         COMMIT"
    ))
    .await;
    for table in ["token", "combat", "initiative_entry"] {
        assert_eq!(
            s.count(&format!("SELECT count(*) FROM {table}")).await,
            0,
            "{table}"
        );
    }
    assert_eq!(
        s.count("SELECT count(*) FROM battlemap_instance WHERE reset_at IS NOT NULL")
            .await,
        1
    );
    // The battlemap and its grid are as they were.
    assert_eq!(s.texts(&battlemap).await, before);
    s.drop().await;
}
