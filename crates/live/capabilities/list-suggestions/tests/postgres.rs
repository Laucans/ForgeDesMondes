//! live.listSuggestions over PostgreSQL (`DATABASE_URL`): the real SQL of the
//! Capability, run in a READ ONLY transaction on a scratch database migrated
//! as the server migrates it and the engine plants its tables, and answered by `answer`. The live session's
//! `live_suggestion` rows are the pill's stack (the 5 most recent) and its
//! history (all of them), newest first, each resolved to its element or its
//! sound; a campaign with no live session has none. Each test works in a
//! database of its own ([`ScratchDb`]), dropped when it ends — even on a panic.

use std::collections::BTreeMap;
use std::path::Path;

use harness_dataguard::tokio_postgres::Client;
use harness_dataguard::tokio_postgres::types::Type;
use list_suggestions::{SQL, answer};
use platform_scratch_db::ScratchDb;
use serde_json::{Value, json};

const VALOMBRE: &str = "c1000000-0000-4000-8000-000000000001";
/// A campaign with no live session.
const BRUMES: &str = "c1000000-0000-4000-8000-000000000002";
const SEANCE_1: &str = "51000000-0000-4000-8000-000000000001";
const SEANCE: &str = "51000000-0000-4000-8000-000000000002";
const GASPARD: &str = "e1000000-0000-4000-8000-000000000001";
const INCENDIE: &str = "e1000000-0000-4000-8000-000000000002";
const PORTE: &str = "a1000000-0000-4000-8000-000000000001";

/// A database of its own: Valombre with its closed session 1 and its live
/// session 2, Gaspard (a PNJ), the mill's fire (an event) and a creaking door
/// (a sound); Brumes, with no session.
struct Scratch {
    _db: ScratchDb,
    client: Client,
}

impl Scratch {
    async fn new(label: &str) -> Scratch {
        let db = ScratchDb::create(&format!("list_suggestions_{label}"))
            .await
            .expect("DATABASE_URL names the harness's PostgreSQL");
        let mut client = db.connect().await.expect("PostgreSQL");
        let migrations = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../data/migrations");
        data::migrate(&mut client, &migrations)
            .await
            .expect("migrated by the Data layer");
        // The engine's own tables, where a soft delete leaves its tombstone.
        client
            .batch_execute(harness_dataguard::postgres::MIGRATION)
            .await
            .expect("the engine's tables");
        client
            .batch_execute(&format!(
                "INSERT INTO campaign (id, name, kind) VALUES \
                 ('{VALOMBRE}', 'Les Cendres de Valombre', 'campagne'), \
                 ('{BRUMES}', 'Les Brumes', 'oneshot'); \
                 INSERT INTO session (id, campaign_id, position, title, status, microphone_on) VALUES \
                 ('{SEANCE_1}', '{VALOMBRE}', 1, 'La Route', 'closed', false), \
                 ('{SEANCE}', '{VALOMBRE}', 2, 'Le Sanglier', 'live', true); \
                 INSERT INTO entity (id, campaign_id, type, name) VALUES \
                 ('{GASPARD}', '{VALOMBRE}', 'pnj', 'Gaspard Grosmollet'), \
                 ('{INCENDIE}', '{VALOMBRE}', 'evenement', 'L’incendie du moulin'); \
                 INSERT INTO entity_version (entity_id, version_number, source, snapshot, diff) \
                 SELECT id, 1, 'dm', '{{}}', '[]' FROM entity; \
                 INSERT INTO sound (id, campaign_id, name, file_path) VALUES \
                 ('{PORTE}', '{VALOMBRE}', 'Porte qui grince', 'porte.ogg')"
            ))
            .await
            .expect("Valombre, its sessions, its elements and its sound");
        Scratch { _db: db, client }
    }

    /// A suggestion of `session` naming `target` (an element, else a sound),
    /// heard `seconds` after the session began.
    async fn suggest(&self, session: &str, target: &str, via: &str, seconds: u32) {
        let column = if target == PORTE {
            "sound_id"
        } else {
            "entity_id"
        };
        self.client
            .batch_execute(&format!(
                "INSERT INTO live_suggestion (session_id, {column}, matched_text, detected_at) \
                 VALUES ('{session}', '{target}', '{via}', \
                 timestamptz '2026-03-14 20:00:00+00' + interval '{seconds} seconds')"
            ))
            .await
            .expect("the suggestion");
    }

    /// The rows of [`SQL`] in a READ ONLY transaction, as the Data layer's
    /// read-only connection answers them: the columns of the types it reads,
    /// as JSON.
    async fn rows(&mut self, campaign: &str) -> Value {
        let tx = self
            .client
            .build_transaction()
            .read_only(true)
            .start()
            .await
            .expect("a read only transaction");
        let rows = tx.query(SQL, &[&campaign]).await.expect("the SQL runs");
        tx.rollback().await.expect("rolled back");
        let rows = rows.iter().map(|row| {
            let mut object = serde_json::Map::new();
            for (i, column) in row.columns().iter().enumerate() {
                let value = match *column.type_() {
                    Type::TEXT => json!(row.get::<_, Option<String>>(i)),
                    ref other => panic!(
                        "{} is a {other}, which the Data layer cannot read",
                        column.name()
                    ),
                };
                object.insert(column.name().to_string(), value);
            }
            Value::Object(object)
        });
        Value::Array(rows.collect())
    }

    async fn read(&mut self, campaign: &str) -> Value {
        let variables = BTreeMap::from([("campaign".to_string(), json!(campaign))]);
        answer(&self.rows(campaign).await, &variables)
    }
}

fn vias(items: &Value) -> Vec<&str> {
    items
        .as_array()
        .expect("an array")
        .iter()
        .map(|i| i["via"].as_str().expect("a via"))
        .collect()
}

#[tokio::test]
async fn eight_suggestions_give_a_stack_of_the_five_most_recent_and_a_history_of_all() {
    let mut db = Scratch::new("eight").await;
    // Inserted out of order: the read orders them, the table does not.
    for n in [3, 0, 7, 5, 1, 6, 2, 4] {
        db.suggest(SEANCE, GASPARD, &format!("mot {n}"), n * 10)
            .await;
    }
    let got = db.read(VALOMBRE).await;
    assert_eq!(
        vias(&got["suggestions"]),
        ["mot 7", "mot 6", "mot 5", "mot 4", "mot 3"]
    );
    assert_eq!(
        vias(&got["history"]),
        [
            "mot 7", "mot 6", "mot 5", "mot 4", "mot 3", "mot 2", "mot 1", "mot 0"
        ]
    );
}

#[tokio::test]
async fn an_element_is_named_by_its_id_kind_type_name_and_the_words_heard() {
    let mut db = Scratch::new("entity").await;
    db.suggest(SEANCE, GASPARD, "Gaspard", 12).await;
    let got = db.read(VALOMBRE).await;
    assert_eq!(
        got["suggestions"],
        json!([{
            "id": GASPARD, "kind": "entity", "type": "pnj",
            "name": "Gaspard Grosmollet", "via": "Gaspard",
            "detectedAt": "2026-03-14T20:00:12.000Z",
        }])
    );
}

#[tokio::test]
async fn an_event_reads_with_its_accent() {
    let mut db = Scratch::new("event").await;
    db.suggest(SEANCE, INCENDIE, "le feu du moulin", 1).await;
    let got = db.read(VALOMBRE).await;
    assert_eq!(got["history"][0]["type"], "événement");
}

#[tokio::test]
async fn a_sound_is_named_by_its_id_kind_and_name_with_no_type() {
    let mut db = Scratch::new("sound").await;
    db.suggest(SEANCE, PORTE, "la porte grince", 5).await;
    let got = db.read(VALOMBRE).await;
    let sound = &got["suggestions"][0];
    assert_eq!(
        *sound,
        json!({
            "id": PORTE, "kind": "sound", "name": "Porte qui grince",
            "via": "la porte grince", "detectedAt": "2026-03-14T20:00:05.000Z",
        })
    );
    assert!(sound.get("type").is_none());
}

#[tokio::test]
async fn only_the_live_sessions_suggestions_are_read() {
    let mut db = Scratch::new("live_only").await;
    db.suggest(SEANCE_1, GASPARD, "hier", 100).await;
    db.suggest(SEANCE, GASPARD, "ce soir", 1).await;
    let got = db.read(VALOMBRE).await;
    assert_eq!(vias(&got["history"]), ["ce soir"]);
}

#[tokio::test]
async fn a_campaign_with_no_live_session_has_neither_stack_nor_history() {
    let mut db = Scratch::new("no_live").await;
    db.suggest(SEANCE, GASPARD, "Gaspard", 1).await;
    let got = db.read(BRUMES).await;
    assert_eq!(got, json!({ "suggestions": [], "history": [] }));
}

#[tokio::test]
async fn a_live_session_with_no_suggestion_has_neither() {
    let mut db = Scratch::new("none_yet").await;
    let got = db.read(VALOMBRE).await;
    assert_eq!(got, json!({ "suggestions": [], "history": [] }));
}

#[tokio::test]
async fn no_campaign_reads_the_live_session_and_a_non_uuid_finds_nothing() {
    let mut db = Scratch::new("campaign_text").await;
    db.suggest(SEANCE, GASPARD, "Gaspard", 1).await;
    assert_eq!(vias(&db.read("").await["history"]), ["Gaspard"]);
    assert_eq!(
        db.read("pas-un-uuid").await,
        json!({ "suggestions": [], "history": [] })
    );
}

#[tokio::test]
async fn a_suggestion_naming_not_exactly_one_target_is_left_out() {
    let mut db = Scratch::new("one_target").await;
    db.suggest(SEANCE, GASPARD, "Gaspard", 1).await;
    db.client
        .batch_execute(&format!(
            "INSERT INTO live_suggestion (session_id, matched_text, detected_at) \
             VALUES ('{SEANCE}', 'personne', timestamptz '2026-03-14 20:01:00+00'); \
             INSERT INTO live_suggestion (session_id, entity_id, sound_id, matched_text, detected_at) \
             VALUES ('{SEANCE}', '{GASPARD}', '{PORTE}', 'les deux', timestamptz '2026-03-14 20:02:00+00')"
        ))
        .await
        .expect("the two malformed suggestions");
    let got = db.read(VALOMBRE).await;
    assert_eq!(vias(&got["history"]), ["Gaspard"]);
}

#[tokio::test]
async fn a_live_session_the_engine_deleted_is_none() {
    let mut db = Scratch::new("tombstoned").await;
    db.suggest(SEANCE, GASPARD, "Gaspard", 1).await;
    assert_eq!(vias(&db.read(VALOMBRE).await["history"]), ["Gaspard"]);
    // The engine's soft delete of the live session: its row tombstoned, the
    // table's left as it was.
    db.client
        .batch_execute(&format!(
            "INSERT INTO dataguard_rows (partition, tombstoned) VALUES ('Session/{SEANCE}', true)"
        ))
        .await
        .expect("the live session deleted");
    for campaign in [VALOMBRE, ""] {
        assert_eq!(
            db.read(campaign).await,
            json!({ "suggestions": [], "history": [] }),
            "campaign {campaign:?}"
        );
    }
}

#[tokio::test]
async fn an_element_the_engine_deleted_is_in_neither_stack_nor_history() {
    let mut db = Scratch::new("entity_tombstoned").await;
    db.suggest(SEANCE, GASPARD, "Gaspard", 1).await;
    db.suggest(SEANCE, INCENDIE, "le feu", 2).await;
    db.client
        .batch_execute(&format!(
            "INSERT INTO dataguard_rows (partition, tombstoned) VALUES ('Entity/{GASPARD}', true)"
        ))
        .await
        .expect("Gaspard deleted");
    let got = db.read(VALOMBRE).await;
    assert_eq!(vias(&got["suggestions"]), ["le feu"]);
    assert_eq!(vias(&got["history"]), ["le feu"]);
}

#[tokio::test]
async fn a_sound_the_engine_deleted_is_in_neither_stack_nor_history() {
    let mut db = Scratch::new("sound_tombstoned").await;
    db.suggest(SEANCE, GASPARD, "Gaspard", 1).await;
    db.suggest(SEANCE, PORTE, "la porte grince", 2).await;
    db.client
        .batch_execute(&format!(
            "INSERT INTO dataguard_rows (partition, tombstoned) VALUES ('Sound/{PORTE}', true)"
        ))
        .await
        .expect("the door deleted");
    let got = db.read(VALOMBRE).await;
    assert_eq!(vias(&got["suggestions"]), ["Gaspard"]);
    assert_eq!(vias(&got["history"]), ["Gaspard"]);
}

#[tokio::test]
async fn a_suggestion_the_engine_deleted_is_in_neither_stack_nor_history() {
    let mut db = Scratch::new("suggestion_tombstoned").await;
    db.suggest(SEANCE, GASPARD, "Gaspard", 1).await;
    db.suggest(SEANCE, GASPARD, "le meunier", 2).await;
    db.client
        .batch_execute(
            "INSERT INTO dataguard_rows (partition, tombstoned) \
             SELECT 'Suggestion/' || id::text, true FROM live_suggestion \
             WHERE matched_text = 'le meunier'",
        )
        .await
        .expect("the suggestion deleted");
    let got = db.read(VALOMBRE).await;
    assert_eq!(vias(&got["suggestions"]), ["Gaspard"]);
    assert_eq!(vias(&got["history"]), ["Gaspard"]);
}

#[tokio::test]
async fn an_element_of_another_campaign_is_left_out() {
    let mut db = Scratch::new("other_campaign").await;
    let ailleurs = "e1000000-0000-4000-8000-000000000003";
    db.client
        .batch_execute(&format!(
            "INSERT INTO entity (id, campaign_id, type, name) VALUES \
             ('{ailleurs}', '{BRUMES}', 'pnj', 'Le Passeur'); \
             INSERT INTO entity_version (entity_id, version_number, source, snapshot, diff) \
             VALUES ('{ailleurs}', 1, 'dm', '{{}}', '[]')"
        ))
        .await
        .expect("an element of Brumes");
    db.suggest(SEANCE, GASPARD, "Gaspard", 1).await;
    db.suggest(SEANCE, ailleurs, "le passeur", 2).await;
    let got = db.read(VALOMBRE).await;
    assert_eq!(vias(&got["suggestions"]), ["Gaspard"]);
    assert_eq!(vias(&got["history"]), ["Gaspard"]);
}
