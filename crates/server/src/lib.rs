//! The product's one server: the shell's wire protocol answered by the
//! DataGuard engine as `crates/dataguard/server` plants it — the harness's
//! skeleton, every unit of the registry — and `GET /health`: the binary
//! `scripts/rules-runtime.sh` builds and starts.
//!
//! What the binary reads from its environment ([`Config::read`]):
//!
//! - `PORT`: where it listens, on every interface — 8790 when unset;
//! - `DATABASE_URL`: the product's PostgreSQL, or `memory` for the engine's
//!   in-memory store;
//! - `DATABASE_URL_READONLY`: the read-only connection the Capabilities read
//!   through; when unset, they read through `DATABASE_URL`, every read in a
//!   `READ ONLY` transaction;
//! - `PORTS`: how the ports of ports.json are wired — only `fake`, the
//!   default: every built port on its fake, no secret read, nothing reached
//!   outside — the `llm` port's fake is [`llm`], the `speech-to-text`
//!   port's [`speech_to_text`], the `character-sheets` port's
//!   [`character_sheets`];
//! - `FIXTURES_DIR`: when set and not empty, the folder of the mock-up's
//!   fixtures (`mock/fixtures/*.json`), read as it starts ([`fixtures`]) and
//!   loaded into the store by the Data layer ([`data::insert_missing`]) —
//!   once: a restart loads nothing twice.
//!
//! What it shares with the units ([`shared`]) is the ports their `Prepare`s
//! declare they need: the `llm` port, the `speech-to-text` port and the
//! `character-sheets` port, one of each, which the units and [`app`]'s journal
//! route reach alike.
//!
//! The server adds one read path of its own, and no write path: a read is
//! served when the engine's registry holds its persisted query, a write only
//! through a DataCapability the engine registers. The one exception on the
//! read side is `ai.listProviderRequests`, a Capability with no query that
//! reads the `llm` port's journal: no answer over rows can, so this server
//! answers it on its own route ([`app`]). Both sides are by identifier, so
//! the media system is served the way the ai system is: `media.*` reads
//! through `POST /capabilities/<id>` once a media Capability's query is
//! registered, media writes through `POST /commands` once a media
//! DataCapability is — each slice by its folder, no route to add here. The
//! fixtures are the one exception on the write side: rows the mock-up's data
//! describes, inserted as the tables' own constraints and triggers judge them.
//! Their rows go by their slugs on the wire, as the mock-up names them, and by
//! a uuid in the store: [`renaming`] turns one into the other around the
//! reads and the commands.

mod applier;
mod fixtures;
mod names;
mod proposing;

use std::fmt::Write as _;
use std::net::{Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub use applier::{applier, waking};
use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use dataguard_server::engine::engine::Engine;
use dataguard_server::engine::plugins::Shared;
use dataguard_server::engine::postgres::ReadOnly;
use dataguard_server::engine::reads::Reads;
use dataguard_server::engine::tokio_postgres;
use dataguard_server::{MEMORY, Planted};
pub use fixtures::{Fixtures, NAMESPACE, Skipped, fixtures, id};
pub use names::{Names, renaming};
use ports_character_sheets::fake::FakeCharacterSheets;
use ports_character_sheets::{CharacterId, SharedCharacterSheets};
use ports_llm::fake::FakeLlm;
use ports_llm::{Llm, SharedLlm};
use ports_speech_to_text::SharedSpeechToText;
use ports_speech_to_text::fake::FakeSpeechToText;
pub use proposing::Proposing;
use serde_json::{Value, json};

/// Where the server listens when `PORT` is unset: the rules' runtime's port.
const DEFAULT_PORT: u16 = 8790;

/// Why the server does not start.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// A variable the server needs is not set.
    #[error("{name} is not set: {why}")]
    Unset {
        name: &'static str,
        why: &'static str,
    },
    /// A variable holds a value the server does not take.
    #[error("{name}={value:?} is not supported: {why}")]
    Unsupported {
        name: &'static str,
        value: String,
        why: &'static str,
    },
    /// The Data layer could not reach the database or apply a migration.
    #[error(transparent)]
    Data(#[from] data::Error),
    /// The DataGuard engine could not be planted, or its reads could not
    /// reach the database.
    #[error("{0}")]
    Plant(dataguard_server::Failure),
    /// The server could not listen, or stopped serving.
    #[error("cannot serve on {addr}: {source}")]
    Serve {
        addr: SocketAddr,
        #[source]
        source: std::io::Error,
    },
}

/// How the ports of ports.json are wired.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ports {
    /// Every built port on its fake adapter: nothing reaches outside, and no
    /// secret is read.
    Fake,
}

/// What the server starts with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    /// Where it listens: every interface, on `PORT`.
    pub addr: SocketAddr,
    /// The database `DATABASE_URL` names, or [`dataguard_server::MEMORY`].
    pub database_url: String,
    /// The read-only database `DATABASE_URL_READONLY` names, when set.
    pub read_only_url: Option<String>,
    /// How the ports are wired, from `PORTS`.
    pub ports: Ports,
    /// The fixtures' folder, from `FIXTURES_DIR`; none when unset or empty.
    pub fixtures: Option<PathBuf>,
}

impl Config {
    /// The configuration `var` gives — `std::env::var` in the binary.
    ///
    /// # Errors
    ///
    /// [`Error::Unset`] without `DATABASE_URL`, [`Error::Unsupported`]
    /// naming `PORT` or `PORTS` when either holds a value the server does
    /// not take.
    pub fn read(var: impl Fn(&str) -> Option<String>) -> Result<Config, Error> {
        let ports = match var("PORTS").as_deref() {
            None | Some("fake") => Ports::Fake,
            Some(other) => {
                return Err(Error::Unsupported {
                    name: "PORTS",
                    value: other.to_string(),
                    why: "only `fake` is — no port has a real adapter yet",
                });
            }
        };
        let port = match var("PORT") {
            None => DEFAULT_PORT,
            Some(text) => text.parse().map_err(|_| Error::Unsupported {
                name: "PORT",
                value: text,
                why: "a TCP port is a number from 0 to 65535",
            })?,
        };
        let database_url = var(data::READ_WRITE_URL).ok_or(Error::Unset {
            name: data::READ_WRITE_URL,
            why: "it names the product's PostgreSQL, or `memory`",
        })?;
        let read_only_url = var(data::READ_ONLY_URL).filter(|url| !url.is_empty());
        let fixtures = var("FIXTURES_DIR")
            .filter(|dir| !dir.is_empty())
            .map(PathBuf::from);
        Ok(Config {
            addr: SocketAddr::from((Ipv4Addr::UNSPECIFIED, port)),
            database_url,
            read_only_url,
            ports,
            fixtures,
        })
    }
}

/// The `llm` port as `ports` wires it: its fake, whose home server answers
/// its model list at the opencode provider's URL the fixtures describe
/// (`AiSettings.json` in `fixtures`) — so a save of that URL detects it, as
/// the real endpoint would make it — and which answers a character's prompt
/// with the proposal the fixtures give for it (`EntityEditor.json`,
/// [`characters`]). Without those fixtures, only the cloud host and the
/// binaries are recognised, and a prompt is answered the canned sentence.
pub fn llm(ports: Ports, fixtures: Option<&Path>) -> Proposing {
    match ports {
        Ports::Fake => {
            let proposing = |fake| characters(fake, fixture(fixtures, "EntityEditor.json"));
            let Some(settings) = fixture(fixtures, "AiSettings.json") else {
                return proposing(FakeLlm::new());
            };
            let home = &settings["providers"]["opencode"];
            let Some(url) = home["url"].as_str() else {
                return proposing(FakeLlm::new());
            };
            let models: Vec<Value> = ["generationModel", "summaryModel"]
                .iter()
                .filter_map(|model| home[*model].as_str())
                .map(|id| json!({ "id": id, "object": "model" }))
                .collect();
            proposing(
                FakeLlm::new().with_endpoint(url, json!({ "object": "list", "data": models })),
            )
        }
    }
}

/// The fixture `file` of `fixtures`, when it is there and JSON.
fn fixture(fixtures: Option<&Path>, file: &str) -> Option<Value> {
    fixtures
        .and_then(|dir| std::fs::read_to_string(dir.join(file)).ok())
        .and_then(|text| serde_json::from_str(&text).ok())
}

/// `fake` answering the proposals of `editor` (`EntityEditor.json`): each
/// one of `proposals` to the prompt that holds the dictation of the same
/// name, then the `default` one to any other prompt.
fn characters(fake: FakeLlm, editor: Option<Value>) -> Proposing {
    let fake = Proposing::new(fake);
    let Some(editor) = editor else {
        return fake;
    };
    let Some(proposals) = editor["proposals"].as_object() else {
        return fake;
    };
    let dictated = proposals.iter().filter_map(|(name, proposal)| {
        let dictation = editor["dictations"][name].as_str()?;
        Some((dictation.to_owned(), proposal.clone()))
    });
    let fallback = proposals
        .get("default")
        .map(|proposal| (String::new(), proposal.clone()));
    dictated
        .chain(fallback)
        .fold(fake, |fake, (holding, proposal)| {
            fake.with_proposal(holding, proposal)
        })
}

/// The `speech-to-text` port as `ports` wires it: its fake, which hears in
/// every dictation the draft of notes the fixtures describe
/// (`NoteImport.json` in `fixtures`), as the DM would have dictated it — so
/// a dictated import proposes what a pasted one does. Without that fixture,
/// it hears nothing.
pub fn speech_to_text(ports: Ports, fixtures: Option<&Path>) -> FakeSpeechToText {
    match ports {
        Ports::Fake => {
            let draft = fixture(fixtures, "NoteImport.json")
                .and_then(|notes| notes["draft"].as_str().map(str::to_owned));
            FakeSpeechToText::new(draft.unwrap_or_default())
        }
    }
}

/// The `character-sheets` port as `ports` wires it: its fake, which knows the
/// characters of the mock-up's `entities.json` that have a D&D Beyond link and
/// the level-ups `EntityEditor.json` (`ddbResync`) plans for them — so an
/// import reads the sheet the mock-up shows and a resync finds elowen's
/// level-up. Every level-up is already played when the fake is handed over: a
/// fetch answers D&D Beyond's side after it, while the tables keep the
/// fixtures' sheet as last synchronised. Without either fixture, or when they
/// do not describe a sheet, it knows no character.
pub fn character_sheets(ports: Ports, fixtures: Option<&Path>) -> FakeCharacterSheets {
    match ports {
        Ports::Fake => {
            let json = |name: &str| {
                fixtures
                    .and_then(|dir| std::fs::read_to_string(dir.join(name)).ok())
                    .and_then(|text| serde_json::from_str::<Value>(&text).ok())
            };
            json("entities.json")
                .zip(json("EntityEditor.json"))
                .and_then(|(entities, editor)| {
                    FakeCharacterSheets::from_fixtures(&entities, &editor)
                        .inspect_err(|e| {
                            eprintln!("server: the character-sheets fake knows no character: {e}");
                        })
                        .ok()
                        .inspect(|fake| play_level_ups(fake, &entities))
                })
                .unwrap_or_default()
        }
    }
}

/// Plays on `fake` the level-up of every character of `entities`
/// (`entities.json`) that has a D&D Beyond link: the one `ddbResync` plans for
/// it, or the default, an empty patch.
fn play_level_ups(fake: &FakeCharacterSheets, entities: &Value) {
    let linked = entities["characters"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|character| character["ddb"]["url"].as_str())
        .filter_map(|url| CharacterId::parse(url).ok());
    for id in linked {
        fake.level_up(id);
    }
}

/// What the server shares with its units: the `llm` port, the `speech-to-text`
/// port and the `character-sheets` port, the ones their `Prepare`s declare
/// they need. The engine refuses to start while a need has nobody to fill it.
#[must_use]
pub fn shared(
    llm: &SharedLlm,
    speech: &SharedSpeechToText,
    sheets: &SharedCharacterSheets,
) -> Shared {
    let mut shared = Shared::default();
    shared.insert(Arc::new(llm.clone()));
    shared.insert(Arc::new(speech.clone()));
    shared.insert(Arc::new(sheets.clone()));
    shared
}

/// The engine as `crates/dataguard/server` plants it over `config`'s
/// database, sharing `shared` with the units; its Capabilities read through
/// `DATABASE_URL_READONLY` when `config` has one ([`reads`]), else through
/// the read-only connection the engine opens on `DATABASE_URL`.
///
/// # Errors
///
/// [`Error::Plant`] when the database cannot be reached, a manifest does not
/// parse, or the units do not fit together.
pub async fn plant(config: &Config, shared: Shared) -> Result<Planted, Error> {
    let mut planted = dataguard_server::plant(&config.database_url, shared)
        .await
        .map_err(Error::Plant)?;
    if let Some(url) = &config.read_only_url {
        planted.reads = Arc::new(reads(url).await?);
    }
    Ok(planted)
}

/// Every Capability of the registry, reading the database `url` names over a
/// connection of their own, every read in a `READ ONLY` transaction; over
/// [`MEMORY`], from no row.
///
/// # Errors
///
/// [`Error::Plant`] when the database cannot be reached.
pub async fn reads(url: &str) -> Result<Reads, Error> {
    let reads = Reads::from(&dataguard_server::registry::plugins());
    if url == MEMORY {
        return Ok(reads);
    }
    let (client, connection) = tokio_postgres::connect(url, tokio_postgres::NoTls)
        .await
        .map_err(|e| Error::Plant(e.into()))?;
    tokio::spawn(async move {
        if let Err(e) = connection.await {
            eprintln!("postgres (reads): {e}");
        }
    });
    Ok(reads.with_connection(Arc::new(ReadOnly::new(client))))
}

/// The shell's wire protocol through what `planted` holds, its routes as
/// `crates/dataguard/server` answers them; `ai.listProviderRequests` from
/// `llm`'s journal; and `GET /health`.
pub fn app<L: Llm + 'static>(planted: &Planted, llm: Arc<L>) -> Router {
    let journal = Router::new()
        .route(
            &format!("/capabilities/{}", list_provider_requests::CAPABILITY),
            post(provider_requests::<L>),
        )
        .with_state(Journal {
            engine: planted.engine.clone(),
            llm,
        });
    dataguard_server::router(planted)
        .merge(journal)
        .route("/health", get(health))
}

/// What `ai.listProviderRequests` is answered from: the engine, for the data
/// version an answer is as of, and the `llm` port whose journal it lists.
struct Journal<L> {
    engine: Arc<Engine>,
    llm: Arc<L>,
}

impl<L> Clone for Journal<L> {
    fn clone(&self) -> Self {
        Self {
            engine: self.engine.clone(),
            llm: self.llm.clone(),
        }
    }
}

/// `POST /capabilities/ai.listProviderRequests`: the requests the `llm` port
/// received, oldest first, as any read is answered (`{ asOf, data }`). No
/// persisted query, so no hash to check, and no variable.
async fn provider_requests<L: Llm>(
    State(journal): State<Journal<L>>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    match journal.engine.data_version().await {
        Ok(as_of) => Ok(Json(json!({
            "asOf": as_of,
            "data": list_provider_requests::list_provider_requests(journal.llm.as_ref()),
        }))),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )),
    }
}

/// Every unit the registry holds, one per line: its kind, its folder, and
/// the types it reads from what the server shares — what `server
/// inventory` prints, reading no database.
#[must_use]
pub fn inventory() -> String {
    let mut out = String::new();
    for (kind, units) in [
        ("aggregate", dataguard_registry::AGGREGATES),
        ("data-capability", dataguard_registry::DATA_CAPABILITIES),
        ("capability", dataguard_registry::CAPABILITIES),
    ] {
        for unit in units {
            let needs = unit.needs();
            let needs = if needs.is_empty() {
                "-".to_string()
            } else {
                needs.join(", ")
            };
            let code = if unit.plugin.is_some() {
                ""
            } else {
                " (manifest only)"
            };
            let _ = writeln!(out, "{kind:<16} {}{code}  needs: {needs}", unit.folder);
        }
    }
    out
}

/// Answers 200 once the server serves: migrations applied, engine planted.
async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "status": "ok" }))
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use axum::body::Body;
    use axum::http::{Method, Request, StatusCode};
    use ports_character_sheets::{CharacterSheets, CharacterSheetsError};
    use ports_speech_to_text::SpeechToText;
    use serde_json::{Value, json};
    use tower::ServiceExt;

    use super::*;

    /// The configuration of these variables alone.
    fn config(vars: &[(&str, &str)]) -> Result<Config, Error> {
        let vars: HashMap<String, String> = vars
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        Config::read(|name| vars.get(name).cloned())
    }

    #[test]
    fn the_configuration_reads_port_database_ports_and_fixtures() {
        let read = config(&[
            ("PORT", "8811"),
            ("DATABASE_URL", "postgres://here/app"),
            ("PORTS", "fake"),
            ("FIXTURES_DIR", "mock/fixtures"),
        ])
        .expect("a valid configuration");
        assert_eq!(
            read,
            Config {
                addr: "0.0.0.0:8811".parse().unwrap(),
                database_url: "postgres://here/app".to_string(),
                read_only_url: None,
                ports: Ports::Fake,
                fixtures: Some(PathBuf::from("mock/fixtures")),
            }
        );
    }

    #[test]
    fn the_read_only_url_is_read_when_it_is_set_and_not_empty() {
        let read = config(&[
            ("DATABASE_URL", "postgres://here/app"),
            ("DATABASE_URL_READONLY", "postgres://reader@here/app"),
        ])
        .expect("a valid configuration");
        assert_eq!(
            read.read_only_url.as_deref(),
            Some("postgres://reader@here/app")
        );
        let empty = config(&[("DATABASE_URL", "memory"), ("DATABASE_URL_READONLY", "")])
            .expect("a valid configuration");
        assert_eq!(empty.read_only_url, None);
    }

    #[test]
    fn unset_ports_and_port_default_to_fake_and_8790() {
        let read = config(&[("DATABASE_URL", "memory")]).expect("a valid configuration");
        assert_eq!(read.ports, Ports::Fake);
        assert_eq!(read.addr, "0.0.0.0:8790".parse().unwrap());
        assert_eq!(read.fixtures, None);
    }

    #[test]
    fn an_empty_fixtures_dir_means_no_fixtures() {
        let read = config(&[("DATABASE_URL", "memory"), ("FIXTURES_DIR", "")])
            .expect("a valid configuration");
        assert_eq!(read.fixtures, None);
    }

    #[test]
    fn an_unsupported_ports_value_is_refused_naming_ports() {
        let refused = config(&[("DATABASE_URL", "memory"), ("PORTS", "real")])
            .expect_err("PORTS=real is refused");
        assert!(
            matches!(&refused, Error::Unsupported { name: "PORTS", value, .. } if value == "real"),
            "{refused:?}"
        );
        assert!(
            refused.to_string().starts_with("PORTS=\"real\""),
            "{refused}"
        );
    }

    #[test]
    fn a_port_that_is_not_a_number_is_refused_naming_port() {
        let refused = config(&[("DATABASE_URL", "memory"), ("PORT", "eighty")])
            .expect_err("PORT=eighty is refused");
        assert!(
            matches!(refused, Error::Unsupported { name: "PORT", .. }),
            "{refused:?}"
        );
    }

    #[test]
    fn without_database_url_the_server_does_not_start() {
        let refused = config(&[]).expect_err("DATABASE_URL is required");
        assert!(
            matches!(
                refused,
                Error::Unset {
                    name: "DATABASE_URL",
                    ..
                }
            ),
            "{refused:?}"
        );
    }

    /// A folder of its own under the system's temporary one, holding `files`.
    fn folder(name: &str, files: &[(&str, &str)]) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("server-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("a temporary folder");
        for (file, text) in files {
            std::fs::write(dir.join(file), text).expect("a fixture file");
        }
        dir
    }

    /// The provider the fake's connection test reaches at `url`.
    async fn reached(llm: &impl Llm, url: &str) -> Option<ports_llm::Provider> {
        let config = ports_llm::ProviderConfig::Api {
            url: url.to_string(),
            api_type: None,
            key: None,
            models: ports_llm::Models {
                generation: "g".to_string(),
                summary: "s".to_string(),
            },
        };
        llm.test_connection(&config).await.provider
    }

    #[tokio::test]
    async fn the_fake_llm_answers_the_model_list_of_the_fixtures_home_server() {
        let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../mock/fixtures");
        let llm = llm(Ports::Fake, Some(&fixtures));
        assert_eq!(
            reached(&llm, "http://192.168.1.40:4096").await,
            Some(ports_llm::Provider::OpencodeServer)
        );
        assert_eq!(reached(&llm, "https://inconnu.example.org").await, None);
    }

    /// What the fake answers a character's `prompt`.
    async fn proposed(llm: &impl Llm, prompt: &str) -> String {
        let config = ports_llm::ProviderConfig::Binary {
            binary: ports_llm::Binary::ClaudeCode,
            models: ports_llm::Models {
                generation: "g".to_string(),
                summary: "s".to_string(),
            },
        };
        let request = ports_llm::GenerateRequest::new(ports_llm::TextKind::Prompt, prompt);
        llm.generate(&config, &request)
            .await
            .expect("an answer")
            .text
    }

    #[tokio::test]
    async fn the_fake_llm_answers_a_characters_prompt_with_the_fixtures_proposal() {
        let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../mock/fixtures");
        let editor: Value = serde_json::from_str(
            &std::fs::read_to_string(fixtures.join("EntityEditor.json")).expect("the fixture"),
        )
        .expect("JSON");
        let llm = llm(Ports::Fake, Some(&fixtures));
        for (dictation, proposal) in [("morlox", "morlox"), ("default", "default")] {
            let prompt = format!(
                "Prompt initial : {}",
                editor["dictations"][dictation]
                    .as_str()
                    .expect("a dictation")
            );
            let answer: Value =
                serde_json::from_str(&proposed(&llm, &prompt).await).expect("one JSON object");
            assert_eq!(answer, editor["proposals"][proposal], "{dictation}");
        }
        let anyone: Value =
            serde_json::from_str(&proposed(&llm, "Un druide").await).expect("one JSON object");
        assert_eq!(anyone, editor["proposals"]["default"]);
    }

    #[tokio::test]
    async fn without_the_fixture_the_fake_llm_proposes_no_character() {
        let dir = folder(
            "no-entity-editor",
            &[("EntityEditor.json", r#"{"dictations": {}}"#)],
        );
        let without_proposals = llm(Ports::Fake, Some(&dir));
        std::fs::remove_dir_all(&dir).ok();
        for fake in [without_proposals, llm(Ports::Fake, None)] {
            let answer = proposed(&fake, "Un druide").await;
            assert!(answer.starts_with("Réponse simulée de "), "{answer}");
        }
    }

    #[tokio::test]
    async fn without_the_fixture_the_fake_llm_knows_no_home_server() {
        let dir = folder("no-ai-settings", &[("other.json", "{}")]);
        let llm_of_dir = llm(Ports::Fake, Some(&dir));
        std::fs::remove_dir_all(&dir).ok();
        for fake in [llm_of_dir, llm(Ports::Fake, None)] {
            assert_eq!(reached(&fake, "http://192.168.1.40:4096").await, None);
        }
    }

    #[tokio::test]
    async fn the_fake_speech_to_text_hears_the_fixtures_draft_of_notes() {
        let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../mock/fixtures");
        let notes: Value = serde_json::from_str(
            &std::fs::read_to_string(fixtures.join("NoteImport.json")).expect("the fixture"),
        )
        .expect("JSON");
        let dictation = ports_speech_to_text::Dictation::new(vec![1_u8; 8]);
        let vocabulary = ports_speech_to_text::Vocabulary::default();
        let heard = speech_to_text(Ports::Fake, Some(&fixtures))
            .transcribe(&dictation, &vocabulary)
            .await
            .expect("heard");
        assert_eq!(Some(heard.text.as_str()), notes["draft"].as_str());
        let dir = folder("no-note-import", &[("other.json", "{}")]);
        let without = speech_to_text(Ports::Fake, Some(&dir));
        std::fs::remove_dir_all(&dir).ok();
        for fake in [without, speech_to_text(Ports::Fake, None)] {
            let heard = fake
                .transcribe(&dictation, &vocabulary)
                .await
                .expect("heard");
            assert_eq!(heard.text, "");
        }
    }

    /// The character `elowen` of the mock-up's `entities.json` has on D&D Beyond.
    const ELOWEN: u64 = 112_847_390;

    /// The character `brannoc` has on D&D Beyond, and no `ddbResync` of his own.
    const BRANNOC: u64 = 112_850_214;

    /// The fake over the mock-up's fixtures, as the server starts with
    /// `FIXTURES_DIR=mock/fixtures`.
    fn fixtures_fake() -> FakeCharacterSheets {
        let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../mock/fixtures");
        character_sheets(Ports::Fake, Some(&fixtures))
    }

    #[tokio::test]
    async fn the_fake_character_sheets_answer_elowens_sheet_after_her_level_up() {
        let id = CharacterId::new(ELOWEN).expect("an id");
        let sheet = fixtures_fake().fetch(id).await.expect("elowen is known");
        assert_eq!(sheet.id, id);
        // `ddbResync.elowen.patch`: pvMax 52, SAG 16, two spells more.
        assert_eq!(sheet.statblock.hp_max, 52);
        assert_eq!(sheet.statblock.wisdom, 16);
        let spells: Vec<&str> = sheet
            .statblock
            .spells
            .iter()
            .map(|spell| spell.name.as_str())
            .collect();
        assert!(spells.contains(&"Explorateur né amélioré"), "{spells:?}");
        assert!(
            spells.contains(&"Communication avec les animaux"),
            "{spells:?}"
        );
    }

    #[tokio::test]
    async fn a_character_without_a_resync_of_its_own_carries_its_fixture_values() {
        // brannoc has a D&D Beyond link and no `ddbResync` entry: the default
        // level-up, an empty patch, leaves the sheet as the fixtures draw it.
        let id = CharacterId::new(BRANNOC).expect("an id");
        let sheet = fixtures_fake().fetch(id).await.expect("brannoc is known");
        assert_eq!(sheet.statblock.hp_max, 48);
        assert_eq!(sheet.statblock.wisdom, 17);
        assert!(
            !sheet
                .statblock
                .spells
                .iter()
                .any(|spell| spell.name == "Communication avec les animaux")
        );
    }

    #[tokio::test]
    async fn without_both_fixtures_the_fake_character_sheets_know_no_character() {
        let id = CharacterId::new(ELOWEN).expect("an id");
        let entities = std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../mock/fixtures/entities.json"),
        )
        .expect("the fixture");
        let only_entities = folder("only-entities", &[("entities.json", &entities)]);
        let neither = folder("neither", &[("other.json", "{}")]);
        let broken = folder(
            "broken-editor",
            &[("entities.json", &entities), ("EntityEditor.json", "{")],
        );
        let wiring = |dir: Option<&Path>| character_sheets(Ports::Fake, dir);
        let fakes = [
            wiring(Some(&only_entities)),
            wiring(Some(&neither)),
            wiring(Some(&broken)),
            wiring(None),
        ];
        for dir in [&only_entities, &neither, &broken] {
            std::fs::remove_dir_all(dir).ok();
        }
        for fake in fakes {
            assert_eq!(fake.fetch(id).await, Err(CharacterSheetsError::NotFound));
        }
    }

    #[tokio::test]
    async fn shared_carries_the_character_sheets_port_beside_llm_and_speech_to_text() {
        let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../mock/fixtures");
        let llm = SharedLlm::new(FakeLlm::new());
        let speech = SharedSpeechToText::new(FakeSpeechToText::new(String::new()));
        let fake = character_sheets(Ports::Fake, Some(&fixtures));
        let sheets = SharedCharacterSheets::new(fake.clone());
        let shared = shared(&llm, &speech, &sheets);
        assert!(shared.get::<SharedLlm>().is_some(), "llm");
        assert!(
            shared.get::<SharedSpeechToText>().is_some(),
            "speech-to-text"
        );
        // What a unit's Prepare reads from the map is that very port.
        let handed = shared
            .get::<SharedCharacterSheets>()
            .expect("the character-sheets port");
        let id = CharacterId::new(ELOWEN).expect("an id");
        let sheet = handed.fetch(id).await.expect("elowen is known");
        // A change made on the fake after the handover is what the port answers.
        let mut changed = sheet.clone();
        changed.statblock.hp_max += 1;
        let _ = fake.clone().with_sheet(changed.clone());
        assert_eq!(handed.fetch(id).await, Ok(changed));
    }

    /// The engine over memory, as the binary plants it with the ports on
    /// their fakes.
    async fn memory() -> Planted {
        let config = config(&[("DATABASE_URL", MEMORY)]).expect("a valid configuration");
        let llm = SharedLlm::new(FakeLlm::new());
        let speech = SharedSpeechToText::new(FakeSpeechToText::new(String::new()));
        let sheets = SharedCharacterSheets::new(FakeCharacterSheets::new());
        plant(&config, shared(&llm, &speech, &sheets))
            .await
            .expect("the in-memory engine")
    }

    /// The app over the in-memory engine, answering one request.
    async fn answer(method: Method, uri: &str, body: Option<Value>) -> (StatusCode, Value) {
        let request = Request::builder().method(method).uri(uri);
        let request = match body {
            Some(body) => request
                .header("content-type", "application/json")
                .body(Body::from(body.to_string())),
            None => request.body(Body::empty()),
        }
        .expect("a request");
        let response = app(&memory().await, Arc::new(FakeLlm::new()))
            .oneshot(request)
            .await
            .expect("an answer");
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("a body");
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    #[tokio::test]
    async fn health_answers_200() {
        let (status, body) = answer(Method::GET, "/health", None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body, json!({ "status": "ok" }));
    }

    #[tokio::test]
    async fn data_version_answers_the_engines_version() {
        let (status, body) = answer(Method::GET, "/dataVersion", None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body, json!({ "dataVersion": 0 }));
    }

    #[tokio::test]
    async fn an_unregistered_query_is_404() {
        let (status, body) = answer(
            Method::POST,
            "/capabilities/nothing.registered",
            Some(json!({ "sha256": "0".repeat(64), "variables": {} })),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
        assert!(
            body["error"]
                .as_str()
                .is_some_and(|e| e.contains("not a registered persisted query")),
            "{body}"
        );
    }

    /// The hash `scripts/queries.sh` registered for `ai.getSettings`.
    fn registered_hash() -> String {
        let registry: Value = serde_json::from_str(include_str!("../../../queries/registry.json"))
            .expect("the registry");
        registry["queries"]
            .as_array()
            .and_then(|q| q.iter().find(|q| q["capability"] == "ai.getSettings"))
            .and_then(|q| q["sha256"].as_str())
            .expect("ai.getSettings is registered")
            .to_string()
    }

    #[tokio::test]
    async fn a_registered_get_settings_answers_its_settings() {
        for body in [
            json!({ "sha256": registered_hash(), "variables": {} }),
            json!({ "variables": {} }),
        ] {
            let (status, answer) =
                answer(Method::POST, "/capabilities/ai.getSettings", Some(body)).await;
            assert_eq!(status, StatusCode::OK, "{answer}");
            assert!(answer["data"].is_object(), "{answer}");
            assert!(answer["data"]["providers"].is_object(), "{answer}");
            assert!(answer.get("asOf").is_some(), "{answer}");
        }
    }

    #[tokio::test]
    async fn every_registered_query_is_served_under_its_hash() {
        let registry: Value = serde_json::from_str(include_str!("../../../queries/registry.json"))
            .expect("the registry");
        let queries = registry["queries"].as_array().expect("its queries");
        assert!(queries.len() >= 12, "{registry}");
        for query in queries {
            let capability = query["capability"].as_str().expect("a Capability");
            let (status, answer) = answer(
                Method::POST,
                &format!("/capabilities/{capability}"),
                Some(json!({ "sha256": query["sha256"], "variables": {} })),
            )
            .await;
            assert_eq!(status, StatusCode::OK, "{capability}: {answer}");
            assert!(answer.get("data").is_some(), "{capability}: {answer}");
        }
    }

    #[tokio::test]
    async fn get_settings_under_another_hash_is_404() {
        let (status, body) = answer(
            Method::POST,
            "/capabilities/ai.getSettings",
            Some(json!({ "sha256": "0".repeat(64), "variables": {} })),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    }

    #[tokio::test]
    async fn a_capability_with_no_registered_query_is_404_without_a_hash_too() {
        let (status, body) = answer(
            Method::POST,
            "/capabilities/nothing.registered",
            Some(json!({ "variables": {} })),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    }

    #[tokio::test]
    async fn an_unknown_data_capability_is_404() {
        let (status, body) = answer(
            Method::POST,
            "/commands",
            Some(json!({ "dataCapability": "nothing.registered", "payload": {} })),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
        assert!(
            body["error"]
                .as_str()
                .is_some_and(|e| e.contains("no DataCapability named nothing.registered")),
            "{body}"
        );
    }

    #[tokio::test]
    async fn a_media_read_under_a_hash_nobody_registered_is_404_as_the_ai_routes_say() {
        let unregistered = json!({ "sha256": "0".repeat(64), "variables": {} });
        let (media_status, media) = answer(
            Method::POST,
            "/capabilities/media.anything",
            Some(unregistered.clone()),
        )
        .await;
        let (ai_status, ai) = answer(
            Method::POST,
            "/capabilities/ai.getSettings",
            Some(unregistered),
        )
        .await;
        assert_eq!(media_status, StatusCode::NOT_FOUND, "{media}");
        assert_eq!(media_status, ai_status);
        let said = |body: &Value| body["error"].as_str().unwrap_or_default().to_string();
        assert!(
            said(&media).contains("not a registered persisted query"),
            "{media}"
        );
        assert!(said(&ai).contains("not a registered persisted query"));
    }

    #[tokio::test]
    async fn a_media_data_capability_reaches_the_dataguard_submit_path() {
        let (status, body) = answer(
            Method::POST,
            "/commands",
            Some(json!({ "dataCapability": "media.anything", "payload": {} })),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
        // The engine's own refusal, not the router's: a missing route has no body.
        assert!(
            body["error"]
                .as_str()
                .is_some_and(|e| e.contains("no DataCapability named media.anything")),
            "{body}"
        );
    }

    #[test]
    fn the_mock_ups_media_fixtures_are_in_the_folder_the_server_loads() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../mock/fixtures");
        assert!(fixtures(&dir).is_ok(), "the server lists {dir:?}");
        for media in [
            "media.json",
            "media.listMedia.json",
            "media.getBattlemap.json",
        ] {
            assert!(dir.join(media).is_file(), "{media} in {dir:?}");
        }
    }

    #[test]
    fn the_inventory_lists_every_unit_by_kind_with_what_it_needs() {
        let listed = inventory();
        let units = dataguard_registry::AGGREGATES.len()
            + dataguard_registry::DATA_CAPABILITIES.len()
            + dataguard_registry::CAPABILITIES.len();
        assert_eq!(listed.lines().count(), units, "{listed}");
        for line in [
            "aggregate        crates/campaign/aggregates/Entity  needs: -",
            "data-capability  crates/ai/data-capabilities/save-settings  needs: ports_llm::shared::SharedLlm",
            "capability       crates/ai/capabilities/get-settings  needs: -",
        ] {
            assert!(listed.lines().any(|l| l == line), "{line} in {listed}");
        }
    }

    /// What `POST /capabilities/ai.listProviderRequests` answers over the
    /// in-memory engine, `llm` the port the app is given.
    async fn provider_requests_of(llm: Arc<FakeLlm>, body: Value) -> (StatusCode, Value) {
        let request = Request::post("/capabilities/ai.listProviderRequests")
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .expect("a request");
        let response = app(&memory().await, llm)
            .oneshot(request)
            .await
            .expect("an answer");
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("a body");
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    #[tokio::test]
    async fn the_provider_requests_are_the_journal_of_the_llm_port_the_app_is_given() {
        let llm = Arc::new(FakeLlm::new());
        let (status, empty) = provider_requests_of(llm.clone(), json!({ "variables": {} })).await;
        assert_eq!(status, StatusCode::OK, "{empty}");
        assert_eq!(empty, json!({ "asOf": 0, "data": { "requests": [] } }));

        // The cloud host is recognised by its URL alone: the test reaches it.
        reached(llm.as_ref(), "https://api.anthropic.com/v1").await;
        // No persisted query: a hash, whatever it is, changes nothing.
        let (status, listed) = provider_requests_of(
            llm.clone(),
            json!({ "sha256": "0".repeat(64), "variables": {} }),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{listed}");
        let requests = listed["data"]["requests"].as_array().expect("a list");
        assert_eq!(requests.len(), 1, "{listed}");
        assert_eq!(requests[0]["sequence"], 1);
        assert_eq!(requests[0]["kind"], "connection-test");
        assert_eq!(requests[0]["modalities"], json!(["text"]));
        assert_eq!(requests[0]["text"], llm.journal().list()[0].text.as_str());
    }

    #[tokio::test]
    async fn beside_the_journal_s_route_every_other_read_is_still_the_engine_s() {
        let (status, body) = answer(
            Method::POST,
            "/capabilities/ai.listProviderRequestsAgain",
            Some(json!({ "variables": {} })),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
        assert!(
            body["error"]
                .as_str()
                .is_some_and(|e| e.contains("not a registered persisted query")),
            "{body}"
        );
    }
}
