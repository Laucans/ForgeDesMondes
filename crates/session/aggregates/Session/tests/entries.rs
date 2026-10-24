//! The Session aggregate's entries « à créer » once created, into
//! `entity_to_create`, over PostgreSQL (`DATABASE_URL`): `Entries` writes the
//! status, the element and the refinement a write of an entry leaves, so the
//! to-create list, which reads the table, no longer offers it
//! (`to-create-leaves-once-created`). Only the fields the row holds are
//! written; a row that holds none, a tombstone or an entry the table does not
//! hold writes nothing; a status the table does not know, an element it does
//! not hold, or a field that is no text rejects the command, nothing of it
//! written. The last tests run it as the server's store does, in the
//! transaction that applies a command. Each test works in a scratch database
//! of its own ([`ScratchDb`]), migrated as the server migrates it, dropped
//! when it ends — even on a panic.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use aggregate_session::Entries;
use harness_dataguard::aggregate::Aggregate;
use harness_dataguard::api::Submission;
use harness_dataguard::command::{CommandResult, State};
use harness_dataguard::engine::Engine;
use harness_dataguard::manifest::DataCapability;
use harness_dataguard::memory::MemoryNotifier;
use harness_dataguard::plugins::{Plugins, ProjectPg, ProjectionError};
use harness_dataguard::postgres::{PgStore, SystemClock};
use harness_dataguard::store::{RandomIds, Write};
use harness_dataguard::tokio_postgres::Client;
use platform_scratch_db::ScratchDb;
use serde_json::{Value, json};

/// An entry stamped, test-only: the update `ai.createFromBrief` leaves.
const STAMP: &str = r#"{"dataCapability":"test.stampEntry","owner":"dataguard","version":1,"effect":"update","target":{"aggregate":"Session"},"mode":"overwrite","callableBy":["*"]}"#;

const VALOMBRE: &str = "c1000000-0000-4000-8000-000000000001";
const SEANCE: &str = "51000000-0000-4000-8000-000000000004";
/// The element the campaign holds, and the one it does not.
const ODRIC: &str = "e1000000-0000-4000-8000-000000000007";
const ABSENT: &str = "e1000000-0000-4000-8000-0000000000ff";
/// Three entries « à créer »; the second already holds a refinement.
const GUE: &str = "7c000000-0000-4000-8000-000000000001";
const TOUR: &str = "7c000000-0000-4000-8000-000000000002";
const FETE: &str = "7c000000-0000-4000-8000-000000000003";
const UNKNOWN: &str = "7c000000-0000-4000-8000-0000000000ff";

/// A database of its own, migrated as the server migrates it, holding —
/// as the fixtures write them — Valombre with one element, its session 4,
/// and three entries waiting.
struct Scratch {
    db: ScratchDb,
    client: Client,
}

impl Scratch {
    async fn new(name: &str) -> Scratch {
        let db = ScratchDb::create(&format!("session_entries_{name}"))
            .await
            .expect("DATABASE_URL names the harness's PostgreSQL");
        let mut client = db.connect().await.expect("PostgreSQL");
        let migrations = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../data/migrations");
        data::migrate(&mut client, &migrations)
            .await
            .expect("migrated");
        client
            .batch_execute(&format!(
                "INSERT INTO campaign (id, name, kind) VALUES
                     ('{VALOMBRE}', 'Les Cendres de Valombre', 'campagne');
                 INSERT INTO entity (id, campaign_id, type, name) VALUES
                     ('{ODRIC}', '{VALOMBRE}', 'pnj', 'Odric');
                 INSERT INTO entity_version (entity_id, version_number, source, snapshot, diff)
                     VALUES ('{ODRIC}', 1, 'brief', '{{}}', '[]');
                 INSERT INTO session (id, campaign_id, position, title, status, microphone_on)
                     VALUES ('{SEANCE}', '{VALOMBRE}', 4, 'La séance 4', 'prep', false);
                 INSERT INTO entity_to_create
                     (id, campaign_id, session_id, type, name, excerpt, refinement_text, status) VALUES
                     ('{GUE}', '{VALOMBRE}', '{SEANCE}', 'lieu', 'Le gué', 'un gué', NULL, 'a_creer'),
                     ('{TOUR}', '{VALOMBRE}', '{SEANCE}', 'lieu', 'La tour', 'une tour', 'Elle penche.', 'a_creer'),
                     ('{FETE}', '{VALOMBRE}', '{SEANCE}', 'evenement', 'La fête', 'une fête', NULL, 'a_creer');"
            ))
            .await
            .expect("Valombre, an element, session 4 and three entries");
        Scratch { db, client }
    }

    /// What `Entries` makes of `write`, in a transaction it commits.
    async fn project(&mut self, write: &Write) -> Result<(), ProjectionError> {
        let tx = self.client.transaction().await.expect("a transaction");
        Entries.project(write, &tx).await?;
        tx.commit().await.expect("committed");
        Ok(())
    }

    /// An entry as the table holds it: its status, its element, its refinement.
    async fn entry(&self, id: &str) -> Value {
        let row = self
            .client
            .query_one(
                "SELECT to_jsonb(t)::text FROM (SELECT status, created_entity_id::text AS element,
                        refinement_text AS refinement FROM entity_to_create
                        WHERE id = $1::text::uuid) t",
                &[&id],
            )
            .await
            .expect("the entry");
        serde_json::from_str(row.get::<_, &str>(0)).expect("JSON")
    }

    /// Every entry, every column: what no write of another entry may change.
    async fn table(&self) -> Vec<String> {
        self.client
            .query("SELECT t::text FROM entity_to_create t ORDER BY id", &[])
            .await
            .expect("the table")
            .iter()
            .map(|row| row.get(0))
            .collect()
    }
}

/// A write of `values` on `partition`.
fn write(partition: &str, values: Value) -> Write {
    Write {
        partition: partition.to_owned(),
        values: serde_json::from_value::<BTreeMap<String, Value>>(values).expect("a row"),
        tombstone: false,
    }
}

/// The write `ai.createFromBrief` leaves on its entry.
fn created(entry: &str, element: &str) -> Write {
    write(
        &format!("Session/{entry}"),
        json!({ "entityToCreateStatus": "creee", "createdEntityId": element }),
    )
}

fn waiting() -> Value {
    json!({ "status": "a_creer", "element": null, "refinement": null })
}

#[tokio::test]
async fn to_create_leaves_once_created_an_entry_stamped_creee_names_its_element() {
    let mut scratch = Scratch::new("stamp").await;
    let others = (scratch.entry(TOUR).await, scratch.entry(FETE).await);
    scratch
        .project(&created(GUE, ODRIC))
        .await
        .expect("projected");
    assert_eq!(
        scratch.entry(GUE).await,
        json!({ "status": "creee", "element": ODRIC, "refinement": null })
    );
    // Not the others.
    assert_eq!(
        (scratch.entry(TOUR).await, scratch.entry(FETE).await),
        others
    );
}

#[tokio::test]
async fn a_refinement_sent_with_the_command_is_the_row_s_refinement() {
    let mut scratch = Scratch::new("refinement").await;
    let mut refined = created(TOUR, ODRIC);
    refined.values.insert(
        "refinementText".into(),
        json!("Elle penche, et un corbeau y niche."),
    );
    scratch.project(&refined).await.expect("projected");
    assert_eq!(
        scratch.entry(TOUR).await,
        json!({ "status": "creee", "element": ODRIC, "refinement": "Elle penche, et un corbeau y niche." })
    );
}

#[tokio::test]
async fn a_field_the_row_does_not_hold_is_left_as_it_was() {
    let mut scratch = Scratch::new("kept").await;
    // Created without a refinement: the one the entry holds is kept.
    scratch
        .project(&created(TOUR, ODRIC))
        .await
        .expect("projected");
    assert_eq!(
        scratch.entry(TOUR).await,
        json!({ "status": "creee", "element": ODRIC, "refinement": "Elle penche." })
    );
    // A refinement alone changes neither the status nor the element.
    scratch
        .project(&write(
            &format!("Session/{TOUR}"),
            json!({ "refinementText": "Elle penche vers l'est." }),
        ))
        .await
        .expect("projected");
    assert_eq!(
        scratch.entry(TOUR).await,
        json!({ "status": "creee", "element": ODRIC, "refinement": "Elle penche vers l'est." })
    );
}

#[tokio::test]
async fn a_null_takes_the_element_and_the_refinement_out() {
    let mut scratch = Scratch::new("nulls").await;
    scratch
        .project(&created(TOUR, ODRIC))
        .await
        .expect("projected");
    scratch
        .project(&write(
            &format!("Session/{TOUR}"),
            json!({ "entityToCreateStatus": "a_creer", "createdEntityId": null, "refinementText": null }),
        ))
        .await
        .expect("projected");
    assert_eq!(scratch.entry(TOUR).await, waiting());
}

#[tokio::test]
async fn an_id_in_capitals_names_the_same_entry() {
    let mut scratch = Scratch::new("capitals").await;
    let write = created(&GUE.to_uppercase(), &ODRIC.to_uppercase());
    scratch.project(&write).await.expect("projected");
    assert_eq!(
        scratch.entry(GUE).await,
        json!({ "status": "creee", "element": ODRIC, "refinement": null })
    );
}

#[tokio::test]
async fn a_row_that_holds_none_of_the_fields_a_tombstone_or_a_stranger_writes_nothing() {
    let mut scratch = Scratch::new("nothing").await;
    let table = scratch.table().await;
    let mut tombstone = created(GUE, ODRIC);
    tombstone.tombstone = true;
    for write in [
        // A session's row and a scene's.
        write(
            &format!("Session/{SEANCE}"),
            json!({ "sessionId": SEANCE, "sessionTitle": "x" }),
        ),
        write(
            &format!("Session/{GUE}"),
            json!({ "sceneId": GUE, "sceneTitle": "Le gué" }),
        ),
        // The entry deleted.
        tombstone,
        // An entry the table does not hold: the analysis writes entries.
        created(UNKNOWN, ODRIC),
    ] {
        scratch.project(&write).await.expect("projected");
        assert_eq!(scratch.table().await, table, "{write:?}");
    }
}

#[tokio::test]
async fn the_table_s_checks_reject_the_command_and_nothing_of_it_is_written() {
    let mut scratch = Scratch::new("rejected").await;
    let table = scratch.table().await;
    for bad in [
        // A status the table does not know.
        write(
            &format!("Session/{GUE}"),
            json!({ "entityToCreateStatus": "perdue" }),
        ),
        // An element the table does not hold.
        created(GUE, ABSENT),
        // An id that is no uuid.
        created(GUE, "pas un id"),
        // A field that is no text.
        write(
            &format!("Session/{GUE}"),
            json!({ "entityToCreateStatus": 3 }),
        ),
        write(&format!("Session/{GUE}"), json!({ "createdEntityId": 3 })),
        write(
            &format!("Session/{GUE}"),
            json!({ "refinementText": ["a"] }),
        ),
        // An entry whose partition is no uuid's.
        write(
            "Session/pas-un-id",
            json!({ "entityToCreateStatus": "creee" }),
        ),
    ] {
        let refused = scratch.project(&bad).await;
        assert!(
            matches!(refused, Err(ProjectionError::Refused(_))),
            "{bad:?}: {refused:?}"
        );
        assert_eq!(scratch.table().await, table, "{bad:?}");
    }
}

/// The engine as the server plants the aggregate over this database: its
/// projections in the apply transaction.
async fn serve(scratch: &Scratch) -> Engine {
    let plugins = || -> Plugins { [aggregate_session::plugin()].into_iter().collect() };
    let store = PgStore::open(scratch.db.connect().await.expect("PostgreSQL"))
        .await
        .expect("the store")
        .with_projections(&plugins())
        .await
        .expect("warmed");
    let engine = Engine::new(
        Arc::new(store),
        Arc::<MemoryNotifier>::default(),
        Arc::new(SystemClock),
    )
    .with_ids(Arc::new(RandomIds))
    .with_aggregate(Aggregate::parse(aggregate_session::DOCUMENT).expect("contract I"))
    .with_plugins(plugins())
    .with_data_capability(DataCapability::parse(STAMP).expect("contract F"));
    engine.load().await.expect("the engine loads");
    engine
}

/// The stamp of `entry`, queued and applied.
async fn stamped(engine: &Engine, entry: &str, payload: Value) -> Vec<CommandResult> {
    let submission: Submission = serde_json::from_value(json!({
        "dataCapability": "test.stampEntry",
        "payload": payload,
        "targetId": entry,
        // The critical fields' review is not what these tests judge.
        "caller": { "by": "dm", "from": "ToCreateList", "roles": ["role:approver"] },
    }))
    .expect("a submission");
    engine.submit(submission).await.expect("queued");
    engine.run_once().await.expect("the applier ran")
}

#[tokio::test]
async fn an_applied_command_leaves_its_entry_created_in_the_table() {
    let scratch = Scratch::new("applied").await;
    let engine = serve(&scratch).await;
    let results = stamped(
        &engine,
        GUE,
        json!({ "entityToCreateStatus": "creee", "createdEntityId": ODRIC, "refinementText": "Un gué gardé." }),
    )
    .await;
    assert_eq!(results.len(), 1, "{results:?}");
    assert_eq!(results[0].status, State::Applied, "{results:?}");
    assert_eq!(
        scratch.entry(GUE).await,
        json!({ "status": "creee", "element": ODRIC, "refinement": "Un gué gardé." })
    );
    assert_eq!(scratch.entry(FETE).await, waiting());
}

#[tokio::test]
async fn a_command_the_table_rejects_leaves_its_entry_to_create() {
    let scratch = Scratch::new("rolled_back").await;
    let engine = serve(&scratch).await;
    // The element is not made: the stamp is rejected whole, and made again
    // once it is.
    let results = stamped(
        &engine,
        FETE,
        json!({ "entityToCreateStatus": "creee", "createdEntityId": ABSENT }),
    )
    .await;
    assert_eq!(results.len(), 1, "{results:?}");
    assert_eq!(results[0].status, State::Rejected, "{results:?}");
    assert_eq!(scratch.entry(FETE).await, waiting());
}
