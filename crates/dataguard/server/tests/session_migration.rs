//! The Session aggregate (`crates/dataguard/aggregates/Session/aggregate.json`)
//! on the tables `crates/data/migrations/0006_session.sql` lays, against the
//! harness's PostgreSQL (`DATABASE_URL`), migrated by the Data layer: each
//! relation the aggregate declares is its foreign key's actions; the roster
//! and the media of a session are views, which follow every later presence
//! and every change of place; a scene's links go with their element, their
//! media and their campaign; the statuses and the « à créer » entries take
//! only their values; no column holds audio; a closed session keeps its
//! summary and its journal for the next one. Each test works in a scratch
//! database of its own, dropped when it ends.

use std::path::Path;

use serde_json::Value;
use tokio_postgres::error::SqlState;
use tokio_postgres::{Client, NoTls};

/// The aggregate the tables are held to.
const AGGREGATE: &str = include_str!("../../aggregates/Session/aggregate.json");

/// The aggregate's tables, in the migration's order.
const TABLES: [&str; 8] = [
    "session",
    "session_present_character",
    "scene",
    "scene_entity",
    "scene_media",
    "oral_brief",
    "entity_to_create",
    "session_journal_line",
];

/// Each relation the aggregate declares: the table and column of its
/// foreign key.
const RELATIONS: [(&str, &str, &str); 16] = [
    ("Session.sessionCampaignId", "session", "campaign_id"),
    (
        "Session.sessionPresentCharacterSessionId",
        "session_present_character",
        "session_id",
    ),
    (
        "Session.sessionPresentCharacterCharacterId",
        "session_present_character",
        "character_id",
    ),
    ("Session.sceneSessionId", "scene", "session_id"),
    ("Session.sceneEntitySceneId", "scene_entity", "scene_id"),
    ("Session.entityId", "scene_entity", "entity_id"),
    ("Session.sceneMediaSceneId", "scene_media", "scene_id"),
    ("Session.sceneMediaMediaId", "scene_media", "media_id"),
    ("Session.oralBriefSessionId", "oral_brief", "session_id"),
    (
        "Session.entityToCreateCampaignId",
        "entity_to_create",
        "campaign_id",
    ),
    (
        "Session.entityToCreateSessionId",
        "entity_to_create",
        "session_id",
    ),
    ("Session.briefId", "entity_to_create", "brief_id"),
    (
        "Session.entityToCreateSceneId",
        "entity_to_create",
        "scene_id",
    ),
    (
        "Session.createdEntityId",
        "entity_to_create",
        "created_entity_id",
    ),
    (
        "Session.sessionJournalLineSessionId",
        "session_journal_line",
        "session_id",
    ),
    (
        "Session.sessionJournalLineSceneId",
        "session_journal_line",
        "scene_id",
    ),
];

const VALOMBRE: &str = "c1000000-0000-4000-8000-000000000001";
const BRUME: &str = "c1000000-0000-4000-8000-000000000002";
/// Valombre's PCs: Elowen, present from the start, and Kael.
const ELOWEN: &str = "e1000000-0000-4000-8000-000000000001";
const KAEL: &str = "e1000000-0000-4000-8000-000000000004";
/// Valombre's NPC.
const MORLOX: &str = "e1000000-0000-4000-8000-000000000002";
/// Valombre's places: the tavern, which the arrival scene links, and the
/// mill, which no scene links.
const TAVERNE: &str = "e1000000-0000-4000-8000-000000000003";
const MOULIN: &str = "e1000000-0000-4000-8000-000000000005";
/// Valombre's illustrations: one at the tavern, one at no place, which the
/// ambush scene links.
const ENSEIGNE: &str = "d1000000-0000-4000-8000-000000000001";
const PORTRAIT: &str = "d1000000-0000-4000-8000-000000000002";
/// Valombre's sessions 1, closed, and 2, in preparation.
const SEANCE_1: &str = "51000000-0000-4000-8000-000000000001";
const SEANCE_2: &str = "51000000-0000-4000-8000-000000000002";
/// Brume's session.
const BRUME_SEANCE: &str = "51000000-0000-4000-8000-000000000003";
/// Session 2's scenes: the arrival (the tavern, Morlox), the ambush (Morlox,
/// the portrait).
const ARRIVEE: &str = "5c000000-0000-4000-8000-000000000001";
const EMBUSCADE: &str = "5c000000-0000-4000-8000-000000000002";
const BRIEF: &str = "b0000000-0000-4000-8000-000000000001";
const GRIMBALD: &str = "ac000000-0000-4000-8000-000000000001";

fn url() -> String {
    std::env::var("DATABASE_URL").expect("DATABASE_URL names the harness's PostgreSQL")
}

async fn connect(config: &tokio_postgres::Config) -> Client {
    let (client, connection) = config.connect(NoTls).await.expect("PostgreSQL");
    tokio::spawn(connection);
    client
}

/// The SQL adding element `id` to `campaign`, with the version every element
/// is written with (0004_entity.sql, every-change-versioned), and its sheet
/// for a PC or an NPC.
fn element(id: &str, campaign: &str, kind: &str, name: &str) -> String {
    let sheet = if kind == "lieu" {
        String::new()
    } else {
        format!("INSERT INTO character (entity_id, token_color) VALUES ('{id}', '#888');")
    };
    format!(
        "INSERT INTO entity (id, campaign_id, type, name) VALUES ('{id}', '{campaign}', '{kind}', '{name}'); \
         {sheet} \
         INSERT INTO entity_version (entity_id, version_number, source, snapshot, diff) \
         VALUES ('{id}', 1, 'dm', '{{}}', '[]');"
    )
}

/// A database of its own, migrated by the Data layer. Valombre's session 1
/// is closed, with its summary and two journal lines; session 2 is in
/// preparation, Elowen present, its two scenes linking the tavern, Morlox and
/// the portrait, a brief that left Grimbald « à créer ». Brume has a session
/// in preparation.
struct Scratch {
    client: Client,
    name: String,
}

impl Scratch {
    async fn new(name: &str) -> Scratch {
        let name = format!("session_test_{name}_{}", std::process::id());
        let config: tokio_postgres::Config = url().parse().expect("a PostgreSQL URL");
        let admin = connect(&config).await;
        for sql in [
            format!("DROP DATABASE IF EXISTS {name} WITH (FORCE)"),
            format!("CREATE DATABASE {name}"),
        ] {
            admin.batch_execute(&sql).await.expect("a scratch database");
        }
        let mut client = connect(config.clone().dbname(&name)).await;
        let migrations = Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../data/migrations"
        ));
        data::migrate(&mut client, migrations)
            .await
            .expect("migrated by the Data layer");
        let s = Scratch { client, name };
        s.accepted(&format!(
            "INSERT INTO campaign (id, name, kind) VALUES \
             ('{VALOMBRE}', 'Les Cendres de Valombre', 'campagne'), ('{BRUME}', 'La Brume', 'oneshot'); \
             {}{}{}{}{} \
             INSERT INTO media (id, campaign_id, kind, title, file_path, width_px, height_px, place_id) VALUES \
             ('{ENSEIGNE}', '{VALOMBRE}', 'illustration', 'Enseigne', 'enseigne.png', 800, 600, '{TAVERNE}'), \
             ('{PORTRAIT}', '{VALOMBRE}', 'illustration', 'Morlox', 'morlox.png', 800, 1200, NULL); \
             INSERT INTO session (id, campaign_id, position, title, status, closed_at, microphone_on, summary, summary_model) VALUES \
             ('{SEANCE_1}', '{VALOMBRE}', 1, 'Le départ', 'closed', now(), false, 'Les héros quittent la capitale.', 'claude-haiku-4-5-20251001'), \
             ('{SEANCE_2}', '{VALOMBRE}', 2, 'Le Sanglier', 'prep', NULL, false, NULL, NULL), \
             ('{BRUME_SEANCE}', '{BRUME}', 1, 'Le Phare', 'prep', NULL, false, NULL, NULL); \
             INSERT INTO session_journal_line (session_id, spoken_at, text) VALUES \
             ('{SEANCE_1}', '2026-10-11T21:00:00Z', 'Elowen quitte la capitale.'), \
             ('{SEANCE_1}', '2026-10-11T20:00:00Z', 'Le roi vous convoque.'); \
             INSERT INTO session_present_character (session_id, character_id) VALUES ('{SEANCE_2}', '{ELOWEN}'); \
             INSERT INTO scene (id, session_id, position, title) VALUES \
             ('{ARRIVEE}', '{SEANCE_2}', 1, 'Arrivée'), ('{EMBUSCADE}', '{SEANCE_2}', 2, 'Embuscade'); \
             INSERT INTO scene_entity (scene_id, entity_id, position) VALUES \
             ('{ARRIVEE}', '{TAVERNE}', 1), ('{ARRIVEE}', '{MORLOX}', 2), ('{EMBUSCADE}', '{MORLOX}', 1); \
             INSERT INTO scene_media (scene_id, media_id) VALUES ('{EMBUSCADE}', '{PORTRAIT}'); \
             INSERT INTO oral_brief (id, session_id, transcript, duration_seconds, recorded_at) \
             VALUES ('{BRIEF}', '{SEANCE_2}', 'Grimbald attend à la taverne.', 42, now()); \
             INSERT INTO entity_to_create (id, campaign_id, session_id, brief_id, scene_id, type, name, excerpt, status) \
             VALUES ('{GRIMBALD}', '{VALOMBRE}', '{SEANCE_2}', '{BRIEF}', '{ARRIVEE}', 'pnj', 'Grimbald', 'Grimbald attend', 'a_creer')",
            element(ELOWEN, VALOMBRE, "pj", "Elowen"),
            element(KAEL, VALOMBRE, "pj", "Kael"),
            element(MORLOX, VALOMBRE, "pnj", "Morlox"),
            element(TAVERNE, VALOMBRE, "lieu", "La Taverne du Sanglier"),
            element(MOULIN, VALOMBRE, "lieu", "Le Moulin"),
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

    /// Runs `sql`, which the database refuses with `code` under the
    /// constraint `constraint`.
    async fn refused(&self, sql: &str, code: &SqlState, constraint: &str) {
        let error = self.client.batch_execute(sql).await.expect_err(sql);
        let db = error.as_db_error().expect("a refusal of the database");
        assert_eq!(db.code(), code, "{sql}: {}", db.message());
        assert_eq!(db.constraint(), Some(constraint), "{sql}: {}", db.message());
    }

    /// One text column of the rows `sql` answers, a null as `-`.
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

    /// The roster of `session`: each character offered, and why.
    async fn roster(&self, session: &str) -> Vec<String> {
        self.texts(&format!(
            "SELECT character_id::text || ' ' || via FROM session_roster_view \
             WHERE session_id = '{session}' ORDER BY 1"
        ))
        .await
    }

    /// The media of `session`, each with the place it comes by, if any.
    async fn media(&self, session: &str) -> Vec<String> {
        self.texts(&format!(
            "SELECT media_id::text || ' ' || coalesce(via_place_id::text, '-') FROM session_media_view \
             WHERE session_id = '{session}' ORDER BY 1"
        ))
        .await
    }

    /// Drops the database. An autovacuum worker the writes woke may still be
    /// on it, which `FORCE` cannot end for a role that is not a superuser:
    /// the drop is tried again while the worker finishes.
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

/// The action `pg_constraint` codes for a relation's `onDelete` or
/// `onUpdate`.
fn action(declared: &str) -> &'static str {
    match declared {
        "cascade" | "propagate" => "c",
        "nullify" => "n",
        "restrict" => "r",
        other => panic!("no foreign-key action for {other}"),
    }
}

#[tokio::test]
async fn each_relation_the_aggregate_declares_is_its_foreign_key_s_actions() {
    let s = Scratch::new("relations").await;
    let aggregate: Value = serde_json::from_str(AGGREGATE).expect("contract I");
    let declared = aggregate["relations"].as_array().expect("relations");
    assert_eq!(declared.len(), RELATIONS.len());
    for relation in declared {
        let from = relation["from"].as_str().expect("from");
        let (_, table, column) = RELATIONS
            .iter()
            .find(|(known, _, _)| *known == from)
            .unwrap_or_else(|| panic!("{from} has no foreign key"));
        let actions = s
            .texts(&format!(
                "SELECT c.confdeltype::text || c.confupdtype::text FROM pg_constraint c \
                 JOIN pg_attribute a ON a.attrelid = c.conrelid AND a.attnum = c.conkey[1] \
                 WHERE c.conrelid = '{table}'::regclass AND c.contype = 'f' AND a.attname = '{column}'"
            ))
            .await;
        let expected = format!(
            "{}{}",
            action(relation["onDelete"].as_str().expect("onDelete")),
            action(relation["onUpdate"].as_str().expect("onUpdate"))
        );
        assert_eq!(actions, [expected], "{from}: {table}.{column}");
    }
    s.drop().await;
}

/// `present-only-pj`: the roster is a view of the presences. That a present
/// character is a PC of the session's campaign is not the table's to check:
/// it reads the Entity aggregate's rows (see session.rs).
#[tokio::test]
async fn present_only_pj_offers_exactly_the_present_pcs_those_checked_later_included() {
    let s = Scratch::new("roster").await;
    // A view, never a table: nothing to fall behind the presences.
    assert_eq!(
        s.texts("SELECT relkind::text FROM pg_class WHERE relname = 'session_roster_view'")
            .await,
        ["v"]
    );
    assert_eq!(
        s.roster(SEANCE_2).await,
        [format!("{ELOWEN} present"), format!("{MORLOX} scene")]
    );
    // Kael checked present later is offered at once; Elowen unchecked is no
    // longer offered.
    s.accepted(&format!(
        "INSERT INTO session_present_character (session_id, character_id) VALUES ('{SEANCE_2}', '{KAEL}')"
    ))
    .await;
    assert_eq!(
        s.roster(SEANCE_2).await,
        [
            format!("{ELOWEN} present"),
            format!("{MORLOX} scene"),
            format!("{KAEL} present"),
        ]
    );
    s.accepted(&format!(
        "DELETE FROM session_present_character WHERE session_id = '{SEANCE_2}' AND character_id = '{ELOWEN}'"
    ))
    .await;
    assert_eq!(
        s.roster(SEANCE_2).await,
        [format!("{MORLOX} scene"), format!("{KAEL} present")]
    );
    // A presence is a character's: a place has no sheet to be present with.
    s.refused(
        &format!(
            "INSERT INTO session_present_character (session_id, character_id) VALUES ('{SEANCE_2}', '{TAVERNE}')"
        ),
        &SqlState::FOREIGN_KEY_VIOLATION,
        "session_present_character_character_id_fkey",
    )
    .await;
    assert!(s.roster(BRUME_SEANCE).await.is_empty());
    s.drop().await;
}

/// `session-media-by-place`: the media of a session are a view, never stored.
#[tokio::test]
async fn session_media_by_place_follows_every_change_of_place_and_keeps_a_media_with_none() {
    let s = Scratch::new("media").await;
    assert_eq!(
        s.texts("SELECT relkind::text FROM pg_class WHERE relname = 'session_media_view'")
            .await,
        ["v"]
    );
    // The portrait, at no place, by its scene; the sign, by the tavern its
    // scene links.
    assert_eq!(
        s.media(SEANCE_2).await,
        [format!("{ENSEIGNE} {TAVERNE}"), format!("{PORTRAIT} -")]
    );
    // The sign moved to the mill, which no scene links: it leaves the
    // session; moved back, it returns.
    s.accepted(&format!(
        "UPDATE media SET place_id = '{MOULIN}' WHERE id = '{ENSEIGNE}'"
    ))
    .await;
    assert_eq!(s.media(SEANCE_2).await, [format!("{PORTRAIT} -")]);
    // The portrait given the tavern for a place: still once, by its scene.
    s.accepted(&format!(
        "UPDATE media SET place_id = '{TAVERNE}' WHERE id IN ('{ENSEIGNE}', '{PORTRAIT}')"
    ))
    .await;
    assert_eq!(
        s.media(SEANCE_2).await,
        [format!("{ENSEIGNE} {TAVERNE}"), format!("{PORTRAIT} -")]
    );
    // The tavern deleted: its media are left with no place, valid, and the
    // sign leaves the session with its place.
    s.accepted(&format!("DELETE FROM entity WHERE id = '{TAVERNE}'"))
        .await;
    assert_eq!(
        s.count(&format!(
            "SELECT count(*) FROM media WHERE id IN ('{ENSEIGNE}', '{PORTRAIT}') AND place_id IS NULL"
        ))
        .await,
        2
    );
    assert_eq!(s.media(SEANCE_2).await, [format!("{PORTRAIT} -")]);
    s.drop().await;
}

/// `scene-links-same-campaign`: what the tables hold of it — every link
/// reaches its campaign through its scene's session, and goes with its
/// element, its media or its campaign. A link to another campaign's element
/// or media is not refused by the table: it reads the Entity and Media
/// aggregates' rows (see session.rs).
#[tokio::test]
async fn scene_links_same_campaign_a_scene_s_links_go_with_their_element_their_media_and_their_campaign()
 {
    let s = Scratch::new("links").await;
    let links = format!(
        "SELECT (SELECT count(*) FROM scene_entity l JOIN scene c ON c.id = l.scene_id \
                 WHERE c.session_id = '{SEANCE_2}') \
              + (SELECT count(*) FROM scene_media l JOIN scene c ON c.id = l.scene_id \
                 WHERE c.session_id = '{SEANCE_2}')"
    );
    assert_eq!(s.count(&links).await, 4);
    // Every link reaches the session's campaign through its scene: none is
    // another campaign's.
    assert_eq!(
        s.count(&format!(
            "SELECT count(*) FROM ( \
                 SELECT e.campaign_id FROM scene_entity l JOIN entity e ON e.id = l.entity_id \
                     JOIN scene c ON c.id = l.scene_id WHERE c.session_id = '{SEANCE_2}' \
                 UNION ALL \
                 SELECT m.campaign_id FROM scene_media l JOIN media m ON m.id = l.media_id \
                     JOIN scene c ON c.id = l.scene_id WHERE c.session_id = '{SEANCE_2}' \
             ) linked WHERE campaign_id <> '{VALOMBRE}'"
        ))
        .await,
        0
    );
    // Morlox deleted: his two links go; the portrait deleted: its link goes.
    s.accepted(&format!("DELETE FROM entity WHERE id = '{MORLOX}'"))
        .await;
    assert_eq!(s.count(&links).await, 2);
    s.accepted(&format!("DELETE FROM media WHERE id = '{PORTRAIT}'"))
        .await;
    assert_eq!(s.count(&links).await, 1);
    // The campaign deleted: its sessions, their scenes and every link go,
    // Brume's session stays.
    s.accepted(&format!("DELETE FROM campaign WHERE id = '{VALOMBRE}'"))
        .await;
    for table in TABLES {
        let left = s
            .count(&format!(
                "SELECT count(*) FROM {table} t WHERE row_to_json(t)::text LIKE '%{SEANCE_2}%' \
                 OR row_to_json(t)::text LIKE '%{VALOMBRE}%'"
            ))
            .await;
        assert_eq!(left, 0, "{table}");
    }
    assert_eq!(
        s.texts("SELECT id::text FROM session").await,
        [BRUME_SEANCE]
    );
    s.drop().await;
}

/// `status-lifecycle` and `close-session`: the statuses the table takes, and
/// what a close records.
#[tokio::test]
async fn status_lifecycle_and_close_session_take_only_their_statuses_and_record_the_close() {
    let s = Scratch::new("status").await;
    s.refused(
        &format!("UPDATE session SET status = 'paused' WHERE id = '{SEANCE_2}'"),
        &SqlState::CHECK_VIOLATION,
        "session_status",
    )
    .await;
    // The close's record: the microphone, a boolean always set; the close's
    // date, the summary and its model.
    assert_eq!(
        s.texts(
            "SELECT column_name || ' ' || data_type || ' ' || is_nullable FROM information_schema.columns \
             WHERE table_schema = 'public' AND table_name = 'session' \
             AND column_name IN ('microphone_on', 'closed_at', 'summary', 'summary_model') ORDER BY 1"
        )
        .await,
        [
            "closed_at timestamp with time zone YES",
            "microphone_on boolean NO",
            "summary text YES",
            "summary_model text YES",
        ]
    );
    s.accepted(&format!(
        "UPDATE session SET status = 'live', started_at = now(), microphone_on = true WHERE id = '{SEANCE_2}'; \
         INSERT INTO session_journal_line (session_id, scene_id, spoken_at, text) \
         VALUES ('{SEANCE_2}', '{ARRIVEE}', now(), 'Elowen entre.'); \
         UPDATE session SET status = 'closed', closed_at = now(), microphone_on = false, \
         summary = 'Grimbald attendait.', summary_model = 'claude-haiku-4-5-20251001' WHERE id = '{SEANCE_2}'"
    ))
    .await;
    assert_eq!(
        s.texts(&format!(
            "SELECT status || ' ' || microphone_on::text || ' ' || summary_model FROM session \
             WHERE id = '{SEANCE_2}' AND closed_at IS NOT NULL"
        ))
        .await,
        ["closed false claude-haiku-4-5-20251001"]
    );
    // The journal is kept, timestamped, with the closed session.
    assert_eq!(
        s.count(&format!(
            "SELECT count(*) FROM session_journal_line WHERE session_id = '{SEANCE_2}' AND spoken_at IS NOT NULL"
        ))
        .await,
        1
    );
    s.drop().await;
}

/// `brief-no-auto-creation` and `to-create-leaves-once-created`: the values
/// an entry « à créer » takes, and the element its creation names.
#[tokio::test]
async fn brief_no_auto_creation_and_to_create_leaves_once_created_take_only_their_values() {
    let s = Scratch::new("to_create").await;
    for (column, value, constraint) in [
        ("status", "ignoree", "entity_to_create_status"),
        ("type", "pj", "entity_to_create_type"),
    ] {
        s.refused(
            &format!("UPDATE entity_to_create SET {column} = '{value}' WHERE id = '{GRIMBALD}'"),
            &SqlState::CHECK_VIOLATION,
            constraint,
        )
        .await;
    }
    // The brief left an entry and created nothing: the campaign's elements are
    // the five it had.
    assert_eq!(
        s.count(&format!(
            "SELECT count(*) FROM entity WHERE campaign_id = '{VALOMBRE}'"
        ))
        .await,
        5
    );
    // Created by the DM, the entry names its element.
    s.accepted(&format!(
        "{} UPDATE entity_to_create SET status = 'creee', created_entity_id = 'e1000000-0000-4000-8000-0000000000aa' \
         WHERE id = '{GRIMBALD}'",
        element("e1000000-0000-4000-8000-0000000000aa", VALOMBRE, "pnj", "Grimbald")
    ))
    .await;
    assert_eq!(
        s.texts(&format!(
            "SELECT status || ' ' || created_entity_id::text FROM entity_to_create WHERE id = '{GRIMBALD}'"
        ))
        .await,
        ["creee e1000000-0000-4000-8000-0000000000aa"]
    );
    s.drop().await;
}

/// `text-only-no-audio`: no column of the aggregate's tables holds audio.
#[tokio::test]
async fn text_only_no_audio_no_column_of_the_aggregate_holds_audio() {
    let s = Scratch::new("text_only").await;
    let binary = s
        .texts(&format!(
            "SELECT table_name || '.' || column_name FROM information_schema.columns \
             WHERE table_schema = 'public' AND table_name IN ({}) \
             AND (data_type IN ('bytea', 'oid') OR udt_name IN ('bytea', 'oid', 'lo') \
                  OR column_name ~ '(audio|sound|recording|wav|blob)') ORDER BY 1",
            TABLES.map(|table| format!("'{table}'")).join(", ")
        ))
        .await;
    assert!(binary.is_empty(), "{binary:?}");
    // The brief and the journal are their transcribed text.
    assert_eq!(
        s.texts(
            "SELECT table_name || '.' || column_name || ' ' || data_type FROM information_schema.columns \
             WHERE table_schema = 'public' AND (table_name, column_name) IN \
             (('oral_brief', 'transcript'), ('session_journal_line', 'text')) ORDER BY 1"
        )
        .await,
        ["oral_brief.transcript text", "session_journal_line.text text"]
    );
    s.drop().await;
}

/// `previous-summary-visible`: the next session reads the closed one's
/// summary and journal.
#[tokio::test]
async fn previous_summary_visible_the_next_session_reads_the_closed_one_s_summary_and_journal() {
    let s = Scratch::new("previous").await;
    // From session 2: the closed session before it, its summary and its
    // journal in time order.
    let previous = format!(
        "SELECT p.id FROM session p JOIN session n ON n.campaign_id = p.campaign_id \
         WHERE n.id = '{SEANCE_2}' AND p.status = 'closed' AND p.position < n.position \
         ORDER BY p.position DESC LIMIT 1"
    );
    assert_eq!(
        s.texts(&format!(
            "SELECT summary FROM session WHERE id = ({previous})"
        ))
        .await,
        ["Les héros quittent la capitale."]
    );
    assert_eq!(
        s.texts(&format!(
            "SELECT text FROM session_journal_line WHERE session_id = ({previous}) ORDER BY spoken_at"
        ))
        .await,
        ["Le roi vous convoque.", "Elowen quitte la capitale."]
    );
    // The list of sessions carries each one's summary.
    assert_eq!(
        s.texts(&format!(
            "SELECT position::text || ' ' || coalesce(summary, '-') FROM session \
             WHERE campaign_id = '{VALOMBRE}' ORDER BY position"
        ))
        .await,
        ["1 Les héros quittent la capitale.", "2 -"]
    );
    // A session with a journal keeps its id: its lines stay its own.
    s.refused(
        &format!(
            "UPDATE session SET id = '51000000-0000-4000-8000-0000000000ff' WHERE id = '{SEANCE_1}'"
        ),
        &SqlState::FOREIGN_KEY_VIOLATION,
        "session_journal_line_session_id_fkey",
    )
    .await;
    s.drop().await;
}
