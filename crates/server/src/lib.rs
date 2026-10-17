//! The product's one server: the shell's wire protocol answered by the
//! DataGuard engine of `crates/dataguard/server`, and `GET /health` — the
//! binary `scripts/rules-runtime.sh` builds and starts.
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
//!   outside — the `llm` port's fake is [`llm`];
//! - `FIXTURES_DIR`: when set and not empty, the folder of the mock-up's
//!   fixtures (`mock/fixtures/*.json`), read as it starts ([`fixtures`]).
//!
//! The server adds no read and no write path of its own: a read is served
//! only when the engine's registry holds its persisted query, a write only
//! through a DataCapability the engine registers.

use std::net::{Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::routing::get;
use axum::{Json, Router};
use dataguard_server::{Engine, Reads};
use ports_llm::Llm;
use ports_llm::fake::FakeLlm;
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
    /// The DataGuard engine could not be planted.
    #[error(transparent)]
    Plant(#[from] dataguard_server::PlantError),
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

/// A fixture file not loaded into the store: its name, and why.
pub type Skipped = (String, String);

/// Reads every `*.json` file directly under `dir`, by name, and answers the
/// ones it skipped. None is loaded while no aggregate exists to hold it:
/// each is skipped, and so is a file that is not JSON — neither stops the
/// server.
///
/// # Errors
///
/// The folder's, when it cannot be listed.
pub fn fixtures(dir: &Path) -> std::io::Result<Vec<Skipped>> {
    let mut files = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.extension().is_some_and(|e| e == "json") && path.is_file() {
            files.push(path);
        }
    }
    files.sort();
    let mut skipped = Vec::new();
    for path in files {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let why = match std::fs::read_to_string(&path) {
            Err(e) => format!("cannot be read: {e}"),
            Ok(text) => match serde_json::from_str::<serde_json::Value>(&text) {
                Err(e) => format!("not JSON: {e}"),
                Ok(_) => "no aggregate holds it".to_string(),
            },
        };
        skipped.push((name, why));
    }
    Ok(skipped)
}

/// The `llm` port as `ports` wires it: its fake, whose home server answers
/// its model list at the opencode provider's URL the fixtures describe
/// (`AiSettings.json` in `fixtures`) — so a save of that URL detects it, as
/// the real endpoint would make it. Without that fixture, only the cloud
/// host and the binaries are recognised.
pub fn llm(ports: Ports, fixtures: Option<&Path>) -> FakeLlm {
    match ports {
        Ports::Fake => {
            let fake = FakeLlm::new();
            let settings = fixtures
                .and_then(|dir| std::fs::read_to_string(dir.join("AiSettings.json")).ok())
                .and_then(|text| serde_json::from_str::<Value>(&text).ok());
            let Some(settings) = settings else {
                return fake;
            };
            let home = &settings["providers"]["opencode"];
            let Some(url) = home["url"].as_str() else {
                return fake;
            };
            let models: Vec<Value> = ["generationModel", "summaryModel"]
                .iter()
                .filter_map(|model| home[*model].as_str())
                .map(|id| json!({ "id": id, "object": "model" }))
                .collect();
            fake.with_endpoint(url, json!({ "object": "list", "data": models }))
        }
    }
}

/// The connection the Capabilities read through: `DATABASE_URL_READONLY` when
/// `config` has one, else the database the server writes to — each read then
/// runs in a `READ ONLY` transaction.
///
/// # Errors
///
/// [`Error::Plant`] when the database cannot be reached.
pub async fn reads(config: &Config) -> Result<Reads, Error> {
    let url = config
        .read_only_url
        .as_deref()
        .unwrap_or(&config.database_url);
    Ok(Reads::open(url).await?)
}

/// The shell's wire protocol through `engine` and `llm`, its reads through
/// `reads`, and `GET /health`.
pub fn app<L: Llm + 'static>(engine: Arc<Engine>, llm: Arc<L>, reads: Arc<Reads>) -> Router {
    dataguard_server::router_reading(engine, llm, reads).route("/health", get(health))
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

    #[test]
    fn every_json_fixture_is_read_and_skipped_with_why() {
        let dir = folder(
            "fixtures",
            &[
                ("b.json", "{\"items\": []}"),
                ("a.json", "not json"),
                ("notes.txt", "not a fixture"),
            ],
        );
        let skipped = fixtures(&dir).expect("the folder is listed");
        std::fs::remove_dir_all(&dir).ok();
        let names: Vec<&str> = skipped.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, ["a.json", "b.json"]);
        assert!(skipped[0].1.starts_with("not JSON"), "{skipped:?}");
        assert_eq!(skipped[1].1, "no aggregate holds it");
    }

    #[test]
    fn a_missing_fixtures_folder_is_an_error_the_caller_logs() {
        let dir = std::env::temp_dir().join(format!("server-absent-{}", std::process::id()));
        assert!(fixtures(&dir).is_err());
    }

    /// The provider the fake's connection test reaches at `url`.
    async fn reached(llm: &FakeLlm, url: &str) -> Option<ports_llm::Provider> {
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

    #[tokio::test]
    async fn without_the_fixture_the_fake_llm_knows_no_home_server() {
        let dir = folder("no-ai-settings", &[("other.json", "{}")]);
        let llm_of_dir = llm(Ports::Fake, Some(&dir));
        std::fs::remove_dir_all(&dir).ok();
        for fake in [llm_of_dir, llm(Ports::Fake, None)] {
            assert_eq!(reached(&fake, "http://192.168.1.40:4096").await, None);
        }
    }

    /// The app over the in-memory engine, answering one request.
    async fn answer(method: Method, uri: &str, body: Option<Value>) -> (StatusCode, Value) {
        let engine = dataguard_server::plant(dataguard_server::MEMORY)
            .await
            .expect("the in-memory engine");
        let request = Request::builder().method(method).uri(uri);
        let request = match body {
            Some(body) => request
                .header("content-type", "application/json")
                .body(Body::from(body.to_string())),
            None => request.body(Body::empty()),
        }
        .expect("a request");
        let reads = Reads::open(dataguard_server::MEMORY)
            .await
            .expect("no database");
        let response = app(Arc::new(engine), Arc::new(FakeLlm::new()), Arc::new(reads))
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
            "/capabilities/campaign.listCampaigns",
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
            "/capabilities/campaign.listCampaigns",
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
            Some(json!({ "dataCapability": "campaign.createCampaign", "payload": {} })),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
        assert!(
            body["error"]
                .as_str()
                .is_some_and(|e| e.contains("no DataCapability named campaign.createCampaign")),
            "{body}"
        );
    }
}
