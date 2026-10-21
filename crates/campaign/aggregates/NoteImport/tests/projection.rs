//! The NoteImport aggregate over PostgreSQL (`DATABASE_URL`), as the server
//! plants it with ai.extractFromNotes and campaign.createEntitiesFromNote —
//! and the Entity aggregate and campaign.saveEntity, which a validation
//! saves its elements through: what the engine applies on `NoteImport/<id>`
//! reaches `imported_note` and `note_proposal` in the apply transaction —
//! the raw note kept, one proposal per element the notes name, the ones the
//! campaign's tables know by a name or an alias unchecked — and a validation
//! checks or unchecks each proposal, the accepted ones recording the element
//! they became; a start projects what the tables lost and reads the
//! proposals the engine holds. Each test works in a scratch database of its
//! own, migrated as the server migrates it, dropped when it ends.

use std::path::Path;
use std::sync::Arc;

use harness_dataguard::aggregate::Aggregate;
use harness_dataguard::api::{FollowUp, Submission, Submitted};
use harness_dataguard::command::State;
use harness_dataguard::engine::Engine;
use harness_dataguard::manifest::DataCapability;
use harness_dataguard::memory::MemoryNotifier;
use harness_dataguard::plugins::{Plugins, Shared};
use harness_dataguard::postgres::{PgStore, ReadOnly, SystemClock};
use harness_dataguard::store::RandomIds;
use harness_dataguard::tokio_postgres::{self, Client, NoTls};
use ports_llm::SharedLlm;
use ports_llm::fake::FakeLlm;
use ports_speech_to_text::SharedSpeechToText;
use ports_speech_to_text::fake::FakeSpeechToText;
use serde_json::{Value, json};

const VALOMBRE: &str = "c1000000-0000-4000-8000-000000000001";
const GASPARD: &str = "e1000000-0000-4000-8000-000000000001";

/// A campaign written on its partition, test-only: the fixtures' campaign,
/// as the engine holds it.
const ADD_CAMPAIGN: &str = r#"{"dataCapability":"test.addCampaign","owner":"dataguard","version":1,"effect":"insert","target":{"aggregate":"Campaign","id":"$id"},"mode":"overwrite","callableBy":["*"]}"#;

/// A proposal written on its own partition, test-only: one a review let
/// through after its note.
const LATE_PROPOSAL: &str = r#"{"dataCapability":"test.lateProposal","owner":"dataguard","version":1,"effect":"insert","target":{"aggregate":"NoteImport"},"mode":"overwrite","callableBy":["*"]}"#;

/// The draft of notes the mock-up pastes, as mock/fixtures/NoteImport.json
/// holds it: a copy, the crate reading nothing outside its folder.
const DRAFT: &str = "Valombre — notes en vrac après la séance 2.\n\
Le moulin a brûlé dans la nuit, le meunier a disparu. Gaspard (le tavernier du Sanglier) dit avoir vu des cavaliers en cape grise.\n\
Mère Ombeline, l’herboriste aveugle à l’orée du bois de Corbière, a soigné les apprentis. Elle sent la fumée sur les gens.\n\
Aerin, le frère d’Elowen, aurait été vu avec ces cavaliers.\n\
La Tour d’Ébène : ruine au nord du mont Pourpre, son sceau est un œil fermé.\n\
À creuser : le péage illégal au gué, 5 po par tête.";

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

fn caller() -> Value {
    json!({ "by": "dm", "from": "NoteImport", "roles": ["role:approver"] })
}

/// A database of its own.
struct Scratch {
    name: String,
    url: String,
    client: Client,
}

impl Scratch {
    /// Migrated as the server migrates it, holding — as the fixtures write
    /// them — Valombre and Gaspard Grosmollet, known as « Gaspard ».
    async fn new(name: &str) -> Scratch {
        let mut s = Self::bare(name).await;
        let migrations = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../data/migrations");
        data::migrate(&mut s.client, &migrations)
            .await
            .expect("migrated");
        s.client
            .batch_execute(&format!(
                "INSERT INTO campaign (id, name, kind) VALUES ('{VALOMBRE}', 'Les Cendres de Valombre', 'campagne');
                 INSERT INTO entity (id, campaign_id, type, name) VALUES ('{GASPARD}', '{VALOMBRE}', 'pnj', 'Gaspard Grosmollet');
                 INSERT INTO entity_alias (entity_id, alias) VALUES ('{GASPARD}', 'Gaspard');
                 INSERT INTO entity_version (entity_id, version_number, source, snapshot, diff)
                     VALUES ('{GASPARD}', 1, 'dm', '{{}}', '[]');"
            ))
            .await
            .expect("Valombre and Gaspard");
        s
    }

    /// Not migrated: none of the product's tables.
    async fn bare(name: &str) -> Scratch {
        let name = format!("note_import_projection_test_{name}_{}", std::process::id());
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

    /// The engine as the server plants the units of a note's import over
    /// this database: its store projecting them, its Prepares reading the
    /// tables, the ports on their fakes. Valombre is written on its
    /// partition, as the server hydrates it.
    async fn serve(&self) -> Engine {
        let plugins: Plugins = [
            aggregate_note_import::plugin(),
            aggregate_entity::plugin(),
            extract_from_notes::plugin(),
            create_entities_from_note::plugin(),
            save_entity::plugin(),
        ]
        .into_iter()
        .collect();
        let store = PgStore::open(connect(&self.url).await)
            .await
            .expect("the engine's tables")
            .with_projections(&plugins)
            .await
            .expect("the projections warmed");
        let mut shared = Shared::default();
        shared.insert(Arc::new(SharedLlm::new(FakeLlm::new())));
        shared.insert(Arc::new(SharedSpeechToText::new(FakeSpeechToText::new(""))));
        let mut engine = Engine::new(
            Arc::new(store),
            Arc::new(MemoryNotifier::default()),
            Arc::new(SystemClock),
        )
        .with_ids(Arc::new(RandomIds))
        .with_rows(Arc::new(ReadOnly::new(connect(&self.url).await)));
        for aggregate in [aggregate_note_import::MANIFEST, aggregate_entity::MANIFEST] {
            engine = engine.with_aggregate(Aggregate::parse(aggregate).expect("contract I"));
        }
        for manifest in [
            extract_from_notes::MANIFEST,
            create_entities_from_note::MANIFEST,
            save_entity::MANIFEST,
            ADD_CAMPAIGN,
            LATE_PROPOSAL,
        ] {
            engine =
                engine.with_data_capability(DataCapability::parse(manifest).expect("contract F"));
        }
        let engine = engine.with_plugins(plugins).with_shared(shared);
        engine.load().await.expect("the units fit");
        sent(
            &engine,
            json!({
                "dataCapability": "test.addCampaign@1",
                "payload": { "id": VALOMBRE, "name": "Les Cendres de Valombre" },
                "targetId": VALOMBRE,
            }),
        )
        .await;
        engine
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

    /// How many rows `table` holds.
    async fn count(&self, table: &str) -> i64 {
        self.client
            .query_one(&format!("SELECT count(*) FROM {table}"), &[])
            .await
            .expect("a count")
            .get(0)
    }

    /// The id of the note's proposal named `name`.
    async fn proposal(&self, note: &str, name: &str) -> String {
        let found = self
            .rows(&format!(
                "SELECT id FROM note_proposal WHERE note_id = '{note}' AND name = '{name}'"
            ))
            .await;
        found[0]["id"]
            .as_str()
            .unwrap_or_else(|| panic!("{name} is not proposed"))
            .to_owned()
    }

    /// The note's proposals: name, checked, created element.
    async fn proposals(&self, note: &str) -> Vec<Value> {
        self.rows(&format!(
            "SELECT name, checked, created_entity_id FROM note_proposal \
             WHERE note_id = '{note}' ORDER BY name"
        ))
        .await
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

/// `body` submitted by the DM, approver of their own campaign — unless it
/// names its caller — every command applied: the answer.
async fn sent(engine: &Engine, mut body: Value) -> Submitted {
    if body.get("caller").is_none() {
        body["caller"] = caller();
    }
    let submission: Submission = serde_json::from_value(body).expect("a submission");
    let answer = engine.submit(submission).await.expect("queued");
    let applied = engine.run_once().await.expect("the applier ran");
    assert!(
        applied.iter().all(|r| r.status == State::Applied),
        "{applied:?}"
    );
    answer
}

/// The draft pasted for Valombre, analysed: the note's id.
async fn extract(engine: &Engine) -> String {
    let body = json!({
        "dataCapability": "ai.extractFromNotes@1",
        "payload": { "campaign": VALOMBRE, "text": DRAFT },
    });
    sent(engine, body)
        .await
        .partition
        .strip_prefix("NoteImport/")
        .expect("a NoteImport partition")
        .to_owned()
}

/// `accepted` validated for Valombre.
async fn validate(engine: &Engine, accepted: &[&str]) {
    let body = json!({
        "dataCapability": "campaign.createEntitiesFromNote@1",
        "payload": { "campaign": VALOMBRE, "accepted": accepted },
    });
    sent(engine, body).await;
}

#[tokio::test]
async fn the_raw_note_and_its_proposals_reach_the_tables_and_no_element_is_created() {
    let s = Scratch::new("tables").await;
    let before = [
        s.count("entity").await,
        s.count("character").await,
        s.count("entity_version").await,
    ];
    let engine = s.serve().await;
    let note = extract(&engine).await;

    let notes = s
        .rows("SELECT id, campaign_id, raw_text, analyzed_at IS NOT NULL AS analyzed FROM imported_note")
        .await;
    assert_eq!(
        notes,
        [json!({ "id": note, "campaign_id": VALOMBRE, "raw_text": DRAFT, "analyzed": true })]
    );
    let proposals = s
        .rows(&format!(
            "SELECT type, name, excerpt, existing_entity_id, checked, created_entity_id \
             FROM note_proposal WHERE note_id = '{note}' ORDER BY name"
        ))
        .await;
    assert!(proposals.len() >= 2, "{proposals:?}");
    for proposal in &proposals {
        assert!(
            proposal["excerpt"].as_str().is_some_and(|e| !e.is_empty()),
            "{proposal}"
        );
        assert_eq!(proposal["created_entity_id"], Value::Null, "{proposal}");
    }
    let named = |name: &str| {
        proposals
            .iter()
            .find(|p| p["name"] == name)
            .unwrap_or_else(|| panic!("{name} is not proposed: {proposals:?}"))
            .clone()
    };
    // Gaspard, whom only the tables hold, is known by his alias: a probable
    // duplicate, unchecked.
    let gaspard = named("Gaspard");
    assert_eq!(gaspard["existing_entity_id"], json!(GASPARD));
    assert_eq!(gaspard["checked"], json!(false));
    let ombeline = named("Mère Ombeline");
    assert_eq!(ombeline["type"], json!("pnj"));
    assert_eq!(ombeline["existing_entity_id"], Value::Null);
    assert_eq!(ombeline["checked"], json!(true));

    let after = [
        s.count("entity").await,
        s.count("character").await,
        s.count("entity_version").await,
    ];
    assert_eq!(after, before, "no-creation-before-validation");
    s.drop().await;
}

#[tokio::test]
async fn a_note_the_tables_lost_is_written_again_at_the_next_start() {
    let s = Scratch::new("repair").await;
    let engine = s.serve().await;
    let note = extract(&engine).await;
    let proposed = s.count("note_proposal").await;
    assert!(proposed >= 2);
    drop(engine);
    // Lost: the proposals go with their note.
    s.client
        .batch_execute("DELETE FROM imported_note")
        .await
        .expect("deleted");
    assert_eq!(s.count("note_proposal").await, 0);

    let _restarted = s.serve().await;
    let notes = s.rows("SELECT id, raw_text FROM imported_note").await;
    assert_eq!(notes, [json!({ "id": note, "raw_text": DRAFT })]);
    assert_eq!(s.count("note_proposal").await, proposed);
    s.drop().await;
}

#[tokio::test]
async fn a_proposal_applied_after_its_note_reaches_the_tables_too() {
    let s = Scratch::new("late").await;
    let engine = s.serve().await;
    let note = extract(&engine).await;
    let proposed = s.count("note_proposal").await;
    let late = "f1000000-0000-4000-8000-0000000000aa";
    sent(
        &engine,
        json!({
            "dataCapability": "test.lateProposal@1",
            "payload": {
                "noteProposalId": late,
                "noteId": note,
                "type": "lieu",
                "name": "Le gué",
                "excerpt": "le péage illégal au gué, 5 po par tête.",
                "existingEntityId": null,
                "checked": true,
            },
            "targetId": late,
        }),
    )
    .await;
    assert_eq!(s.count("note_proposal").await, proposed + 1);
    let gue = s
        .rows(&format!(
            "SELECT note_id, name, checked FROM note_proposal WHERE id = '{late}'"
        ))
        .await;
    assert_eq!(
        gue,
        [json!({ "note_id": note, "name": "Le gué", "checked": true })]
    );
    s.drop().await;
}

#[tokio::test]
async fn a_proposal_the_tables_refuse_is_rejected_and_rolled_back() {
    let s = Scratch::new("refused").await;
    let engine = s.serve().await;
    let note = extract(&engine).await;
    let proposed = s.count("note_proposal").await;
    // A proposal written with the element it created: the table's trigger
    // refuses what the engine's judge, on an insert, refuses too — here the
    // tables are the judge, the row being written by a test DataCapability
    // that declares no invariant.
    let late = "f1000000-0000-4000-8000-0000000000bb";
    let submission: Submission = serde_json::from_value(json!({
        "dataCapability": "test.lateProposal@1",
        "payload": {
            "noteProposalId": late,
            "noteId": note,
            "type": "pnj",
            "name": "Le meunier",
            "excerpt": "le meunier a disparu",
            "checked": true,
            "createdEntityId": GASPARD,
        },
        "targetId": late,
        "caller": caller(),
    }))
    .expect("a submission");
    engine.submit(submission).await.expect("queued");
    let ran = engine.run_once().await.expect("the applier ran");
    assert_eq!(ran.len(), 1, "{ran:?}");
    assert_eq!(ran[0].status, State::Rejected, "{ran:?}");
    assert_eq!(s.count("note_proposal").await, proposed);
    let held = s
        .count(&format!(
            "dataguard_rows WHERE partition = 'NoteImport/{late}'"
        ))
        .await;
    assert_eq!(held, 0, "the engine's row rolled back with the tables'");
    s.drop().await;
}

#[tokio::test]
async fn without_the_tables_a_start_reads_nothing_and_a_note_fails_the_step_unrefused() {
    let s = Scratch::bare("bare").await;
    let plugins: Plugins = std::iter::once(aggregate_note_import::plugin()).collect();
    let store = PgStore::open(connect(&s.url).await)
        .await
        .expect("the engine's tables")
        .with_projections(&plugins)
        .await
        .expect("a start without the product's tables");
    let engine = Engine::new(
        Arc::new(store),
        Arc::new(MemoryNotifier::default()),
        Arc::new(SystemClock),
    )
    .with_aggregate(Aggregate::parse(aggregate_note_import::MANIFEST).expect("contract I"))
    .with_data_capability(DataCapability::parse(LATE_PROPOSAL).expect("contract F"))
    .with_plugins(plugins);
    engine.load().await.expect("the units fit");
    let submission: Submission = serde_json::from_value(json!({
        "dataCapability": "test.lateProposal@1",
        "payload": { "importedNoteId": "a1000000-0000-4000-8000-000000000001", "rawText": "x" },
        "targetId": "a1000000-0000-4000-8000-000000000001",
        "caller": caller(),
    }))
    .expect("a submission");
    engine.submit(submission).await.expect("queued");
    let failed = engine.run_once().await;
    assert!(
        matches!(&failed, Err(harness_dataguard::Error::Store(why)) if why.contains("imported_note")),
        "{failed:?}"
    );
    s.drop().await;
}

#[tokio::test]
async fn the_accepted_proposals_reach_the_tables_as_versioned_elements_and_the_others_unchecked() {
    let s = Scratch::new("validated").await;
    let before = s.count("entity").await;
    let engine = s.serve().await;
    let note = extract(&engine).await;
    let ombeline = s.proposal(&note, "Mère Ombeline").await;
    let lieu = s
        .rows(&format!(
            "SELECT id, name, excerpt FROM note_proposal WHERE note_id = '{note}' \
             AND type = 'lieu' AND existing_entity_id IS NULL ORDER BY name LIMIT 1"
        ))
        .await
        .remove(0);
    let lieu_id = lieu["id"].as_str().expect("a place proposed");
    validate(&engine, &[&ombeline, lieu_id]).await;
    assert!(engine.run_once().await.expect("the applier ran").is_empty());

    assert_eq!(s.count("entity").await, before + 2);
    let proposals = s.proposals(&note).await;
    for proposal in &proposals {
        let accepted = proposal["name"] == "Mère Ombeline" || proposal["name"] == lieu["name"];
        assert_eq!(proposal["checked"], json!(accepted), "{proposal}");
        assert_eq!(
            proposal["created_entity_id"].is_string(),
            accepted,
            "{proposal}"
        );
    }
    let elements = s
        .rows(&format!(
            "SELECT e.campaign_id, e.type, e.name, e.description, c.token_color, \
                 v.version_number, v.source, v.snapshot \
             FROM note_proposal p JOIN entity e ON e.id = p.created_entity_id \
             LEFT JOIN character c ON c.entity_id = e.id \
             JOIN entity_version v ON v.entity_id = e.id \
             WHERE p.note_id = '{note}' ORDER BY e.name"
        ))
        .await;
    let ombeline = elements
        .iter()
        .find(|e| e["name"] == "Mère Ombeline")
        .expect("Mère Ombeline is an element");
    assert_eq!(ombeline["campaign_id"], VALOMBRE);
    assert_eq!(ombeline["type"], "pnj");
    assert_eq!(ombeline["token_color"], "accent");
    assert_eq!(ombeline["version_number"], 1);
    assert_eq!(ombeline["source"], "dm");
    assert_eq!(ombeline["snapshot"]["name"], "Mère Ombeline");
    assert_eq!(ombeline["snapshot"]["tokenColor"], "accent");
    let place = elements
        .iter()
        .find(|e| e["name"] == lieu["name"])
        .expect("the place is an element");
    assert_eq!(place["type"], "lieu");
    assert_eq!(place["description"], lieu["excerpt"]);
    assert_eq!(place["token_color"], Value::Null, "a place has no sheet");
    assert_eq!(place["version_number"], 1);
    assert_eq!(elements.len(), 2, "one version each: {elements:?}");

    // Validated again, nothing is made twice.
    let ombeline = s.proposal(&note, "Mère Ombeline").await;
    validate(&engine, &[&ombeline]).await;
    assert_eq!(s.count("entity").await, before + 2);
    assert_eq!(s.proposals(&note).await, proposals);
    s.drop().await;
}

#[tokio::test]
async fn the_proposals_held_before_a_start_are_unchecked_by_a_validation_after_it() {
    let s = Scratch::new("restart").await;
    let engine = s.serve().await;
    let note = extract(&engine).await;
    drop(engine);

    let engine = s.serve().await;
    let ombeline = s.proposal(&note, "Mère Ombeline").await;
    validate(&engine, &[&ombeline]).await;
    let proposals = s.proposals(&note).await;
    assert!(proposals.len() >= 3, "{proposals:?}");
    for proposal in &proposals {
        let accepted = proposal["name"] == "Mère Ombeline";
        assert_eq!(proposal["checked"], json!(accepted), "{proposal}");
    }
    s.drop().await;
}

/// The reviews `answer` and its follow-ups wait for, in the order queued.
fn reviews(answer: &Submitted) -> Vec<String> {
    let follow_ups = answer.then.iter().filter_map(|follow_up| match follow_up {
        FollowUp::Submitted(submitted) => Some(submitted.as_ref()),
        FollowUp::Refused { .. } => None,
    });
    std::iter::once(answer)
        .chain(follow_ups)
        .flat_map(|submitted| submitted.warnings.iter())
        .filter_map(|w| w.strip_prefix("review ")?.strip_suffix(" required"))
        .map(str::to_owned)
        .collect()
}

#[tokio::test]
async fn an_element_applied_after_its_proposal_is_recorded_on_it_once_the_tables_hold_it() {
    let s = Scratch::new("late_element").await;
    let engine = s.serve().await;
    let note = extract(&engine).await;
    let ombeline = s.proposal(&note, "Mère Ombeline").await;
    // A DM who is no approver: the proposals and the element wait for a
    // review each — the proposal answered first, then the element's.
    let submission: Submission = serde_json::from_value(json!({
        "dataCapability": "campaign.createEntitiesFromNote@1",
        "payload": { "campaign": VALOMBRE, "accepted": [ombeline] },
        "caller": { "by": "dm", "from": "NoteImport", "roles": [] },
    }))
    .expect("a submission");
    let answer = engine.submit(submission).await.expect("queued");
    let element = answer
        .then
        .iter()
        .find_map(|follow_up| match follow_up {
            FollowUp::Submitted(save) if save.partition.starts_with("Entity/") => {
                Some(reviews(save))
            }
            _ => None,
        })
        .expect("the element's save");
    let reviews = reviews(&answer);
    assert!(reviews.len() >= 2, "{answer:?}");
    let engine = &engine;
    let approve = |review: String| async move {
        engine
            .decide(&review, "mj", &["role:approver".to_string()], true)
            .await
            .expect("approved");
        let applied = engine.run_once().await.expect("the applier ran");
        assert!(
            applied.iter().all(|r| r.status == State::Applied),
            "{applied:?}"
        );
    };
    // The proposals first: the element they name is none the tables hold.
    for review in reviews.iter().filter(|r| !element.contains(r)) {
        approve(review.clone()).await;
    }
    let stamped = s
        .rows(&format!(
            "SELECT checked, created_entity_id FROM note_proposal WHERE id = '{ombeline}'"
        ))
        .await;
    assert_eq!(
        stamped,
        [json!({ "checked": true, "created_entity_id": null })]
    );

    for review in element {
        approve(review).await;
    }
    let created = s
        .rows(&format!(
            "SELECT e.name FROM note_proposal p JOIN entity e ON e.id = p.created_entity_id \
             WHERE p.id = '{ombeline}'"
        ))
        .await;
    assert_eq!(created, [json!({ "name": "Mère Ombeline" })]);
    s.drop().await;
}

#[tokio::test]
async fn an_element_the_dm_deleted_is_never_made_again_and_its_proposal_keeps_it() {
    let s = Scratch::new("deleted").await;
    let engine = s.serve().await;
    let note = extract(&engine).await;
    let ombeline = s.proposal(&note, "Mère Ombeline").await;
    validate(&engine, &[&ombeline]).await;
    let proposals = s.proposals(&note).await;
    let element = proposals
        .iter()
        .find(|p| p["name"] == "Mère Ombeline")
        .and_then(|p| p["created_entity_id"].as_str())
        .expect("Mère Ombeline made")
        .to_owned();
    // The engine's tombstone, as a deletion of the element leaves it.
    let deleted = s
        .client
        .execute(
            "UPDATE dataguard_rows SET tombstoned = true WHERE lower(partition) = lower($1)",
            &[&format!("Entity/{element}")],
        )
        .await
        .expect("the element deleted");
    assert_eq!(deleted, 1);
    let tombstoned = format!(
        "SELECT tombstoned FROM dataguard_rows WHERE lower(partition) = lower('Entity/{element}')"
    );
    let recorded = format!(
        "SELECT fields -> 'checked' AS checked, fields -> 'createdEntityId' AS created \
         FROM dataguard_rows WHERE lower(partition) = lower('NoteImport/{ombeline}')"
    );
    let live = "dataguard_rows WHERE partition LIKE 'Entity/%' AND NOT tombstoned";
    let entities = s.count("entity").await;
    let held = s.count(live).await;
    let stamp = json!({ "checked": true, "created": element });

    // Validated again: nothing is made, the proposal still records it.
    validate(&engine, &[&ombeline]).await;
    assert_eq!(s.rows(&tombstoned).await, [json!({ "tombstoned": true })]);
    assert_eq!(s.count("entity").await, entities);
    assert_eq!(s.count(live).await, held);
    assert_eq!(s.rows(&recorded).await, std::slice::from_ref(&stamp));
    assert_eq!(s.proposals(&note).await, proposals);

    // Its proposal's stamp lost, as a validation failing half-way leaves it:
    // the element found deleted is recorded again, not made.
    s.client
        .execute(
            "UPDATE dataguard_rows SET fields = fields - 'createdEntityId' \
             WHERE lower(partition) = lower($1)",
            &[&format!("NoteImport/{ombeline}")],
        )
        .await
        .expect("the stamp lost");
    validate(&engine, &[&ombeline]).await;
    assert_eq!(s.rows(&tombstoned).await, [json!({ "tombstoned": true })]);
    assert_eq!(s.count("entity").await, entities);
    assert_eq!(s.count(live).await, held);
    assert_eq!(s.rows(&recorded).await, [stamp]);
    s.drop().await;
}
