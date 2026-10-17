//! The `llm` port (ports.json) — the one seam to every generation provider.
//!
//! The cloud API, the home opencode server's endpoint and the two local
//! binaries sit behind [`Llm`], so a DataCapability or a Capability never
//! knows which one answers. The port is stateless: the provider to reach is
//! handed to every call as a [`ProviderConfig`], and a Capability may call it
//! directly.
//!
//! - **Text only, by construction.** A [`GenerateRequest`] carries a
//!   [`TextKind`] and a `String`: transcribed prompts, briefs, notes, the
//!   journal. There is no field that could hold bytes, so audio cannot reach a
//!   provider.
//! - **Journalled.** Every request a call sends is recorded in the
//!   [`RequestJournal`] of [`Llm::journal`]: its kind, the provider and its
//!   text — never the key.
//! - **[`fake`]** answers deterministically and sends nothing: the tests, the
//!   CI, the rules' runtime and the mock-up run on it.
//! - **[`contract`]** is the suite every adapter passes, run against itself
//!   from its own tests.
//!
//! The adapters (Anthropic Messages API, the opencode endpoint, the binaries)
//! live under `crates/ports/llm/<provider>/`; this crate holds no HTTP client
//! and spawns no process.

use std::fmt;
use std::future::Future;
use std::sync::{Arc, Mutex, PoisonError};

use serde::{Deserialize, Serialize};

pub mod api_type;
pub mod contract;
pub mod fake;

pub use api_type::{UNRECOGNISED_API_TYPE, detect_api_type};

/// What every connection test sends, and the journal records as its text.
pub const CONNECTION_TEST_PROMPT: &str = "Réponds par le mot : ok";

/// The kind of API behind a provider URL (`ProviderApiType@1`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ApiType {
    /// The Anthropic Messages API.
    Cloud,
    /// The OpenAI-compatible endpoint of the DM's home opencode server.
    Opencode,
}

/// A local program that answers a text prompt.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Binary {
    /// The `opencode` program.
    Opencode,
    /// The `claude` program (Claude Code).
    ClaudeCode,
}

/// The provider a request goes to, once the configuration is resolved.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Provider {
    /// The Anthropic Messages API.
    Cloud,
    /// The home opencode server's endpoint.
    OpencodeServer,
    /// The local `opencode` program.
    OpencodeBinary,
    /// The local `claude` program.
    ClaudeCodeBinary,
}

impl Provider {
    /// How a message names it to the DM.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Cloud => "API cloud",
            Self::OpencodeServer => "endpoint opencode du serveur maison",
            Self::OpencodeBinary => "binaire opencode",
            Self::ClaudeCodeBinary => "binaire Claude Code",
        }
    }

    /// The API type behind it; `None` for a binary.
    #[must_use]
    pub const fn api_type(self) -> Option<ApiType> {
        match self {
            Self::Cloud => Some(ApiType::Cloud),
            Self::OpencodeServer => Some(ApiType::Opencode),
            Self::OpencodeBinary | Self::ClaudeCodeBinary => None,
        }
    }
}

impl From<ApiType> for Provider {
    fn from(api_type: ApiType) -> Self {
        match api_type {
            ApiType::Cloud => Self::Cloud,
            ApiType::Opencode => Self::OpencodeServer,
        }
    }
}

impl From<Binary> for Provider {
    fn from(binary: Binary) -> Self {
        match binary {
            Binary::Opencode => Self::OpencodeBinary,
            Binary::ClaudeCode => Self::ClaudeCodeBinary,
        }
    }
}

/// An API key. It names no value in `Debug`, and has no `Serialize`: it
/// cannot reach a log or the journal by accident.
#[derive(Clone, PartialEq, Eq)]
pub struct Secret(String);

impl Secret {
    /// Wraps a key (`LLM_API_KEY`, or the stored `api_key`).
    #[must_use]
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// The key itself, for the one place that sends it to the provider.
    #[must_use]
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret(***)")
    }
}

/// The two models of a provider: one generates, one summarises.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Models {
    /// Generation: extraction, briefs, characters.
    pub generation: String,
    /// The cheaper model of the end-of-session summary.
    pub summary: String,
}

impl Models {
    /// The model that answers a request of `kind`.
    #[must_use]
    pub fn for_kind(&self, kind: TextKind) -> &str {
        match kind {
            TextKind::Journal => &self.summary,
            TextKind::Prompt | TextKind::Brief | TextKind::Notes => &self.generation,
        }
    }
}

/// How to reach a provider: by an API URL, or by a local binary.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProviderConfig {
    /// A provider behind an API URL.
    Api {
        /// The base URL the DM entered.
        url: String,
        /// The type already known for this URL (a saved provider); `None` to
        /// have the port detect it. [`Llm::generate`] speaks that dialect
        /// without asking the endpoint again; [`Llm::test_connection`] always
        /// detects it anew.
        api_type: Option<ApiType>,
        /// The key; the home server may need none.
        key: Option<Secret>,
        /// The generation and summary models.
        models: Models,
    },
    /// A provider run as a local program.
    Binary {
        /// Which program.
        binary: Binary,
        /// The generation and summary models.
        models: Models,
    },
}

impl ProviderConfig {
    /// The models it asks for.
    #[must_use]
    pub const fn models(&self) -> &Models {
        match self {
            Self::Api { models, .. } | Self::Binary { models, .. } => models,
        }
    }
}

/// What a generation request is made of. Each is text the product already
/// holds as text: audio is transcribed before it gets here.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TextKind {
    /// A (transcribed) prompt: a character, a refinement.
    Prompt,
    /// A (transcribed) oral brief.
    Brief,
    /// Imported or dictated notes.
    Notes,
    /// The session journal, to summarise.
    Journal,
}

impl TextKind {
    /// Every kind, for a suite that walks them all.
    pub const ALL: [Self; 4] = [Self::Prompt, Self::Brief, Self::Notes, Self::Journal];
}

/// A text-generation request: text, and nothing else.
///
/// There is no way to hand the port bytes or audio. These do not compile:
///
/// ```compile_fail,E0277
/// use ports_llm::{GenerateRequest, TextKind};
/// let audio: Vec<u8> = vec![0, 1, 2];
/// let _ = GenerateRequest::new(TextKind::Brief, audio);
/// ```
///
/// ```compile_fail,E0560
/// use ports_llm::{GenerateRequest, TextKind};
/// let _ = GenerateRequest { kind: TextKind::Brief, text: String::new(), audio: vec![0u8] };
/// ```
///
/// ```compile_fail,E0308
/// use ports_llm::{GenerateRequest, TextKind};
/// let _ = GenerateRequest { kind: TextKind::Brief, text: vec![0u8] };
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GenerateRequest {
    /// What the text is.
    pub kind: TextKind,
    /// The text sent to the provider.
    pub text: String,
}

impl GenerateRequest {
    /// A request of `kind` carrying `text`.
    #[must_use]
    pub fn new(kind: TextKind, text: impl Into<String>) -> Self {
        Self {
            kind,
            text: text.into(),
        }
    }
}

/// What the provider answered to a [`GenerateRequest`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GenerateAnswer {
    /// The generated text.
    pub text: String,
    /// The model that answered: [`Models::for_kind`] of the request.
    pub model: String,
}

/// How a connection test ended. A refusal is an outcome, not an error: the DM
/// reads `message`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConnectionReport {
    /// Whether the provider answered.
    pub ok: bool,
    /// The provider reached; `None` when the test could not tell which.
    pub provider: Option<Provider>,
    /// How long the provider took to answer, in milliseconds.
    pub latency_ms: u64,
    /// For the DM: the models on success, the reason on failure.
    pub message: String,
}

/// Why a generation failed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LlmError {
    /// The URL's API type is not recognised (`ProviderApiType@1`).
    UnrecognisedApiType,
    /// The provider could not answer; the reason is for the DM.
    Failed(String),
}

impl fmt::Display for LlmError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnrecognisedApiType => f.write_str(UNRECOGNISED_API_TYPE),
            Self::Failed(reason) => f.write_str(reason),
        }
    }
}

impl std::error::Error for LlmError {}

/// What an [`Llm`] call sent: the request kind a record carries.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RequestKind {
    /// A connection test.
    ConnectionTest,
    /// A [`TextKind::Prompt`].
    Prompt,
    /// A [`TextKind::Brief`].
    Brief,
    /// A [`TextKind::Notes`].
    Notes,
    /// A [`TextKind::Journal`].
    Journal,
}

impl From<TextKind> for RequestKind {
    fn from(kind: TextKind) -> Self {
        match kind {
            TextKind::Prompt => Self::Prompt,
            TextKind::Brief => Self::Brief,
            TextKind::Notes => Self::Notes,
            TextKind::Journal => Self::Journal,
        }
    }
}

/// One request the port received: its kind, its provider, its text.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RequestRecord {
    /// Its place in the journal, from 1.
    pub sequence: u64,
    /// What it was.
    pub kind: RequestKind,
    /// Who it went to.
    pub provider: Provider,
    /// The text sent, and nothing else of the request.
    pub text: String,
}

/// The requests a port received, in order. A handle: a clone shares the
/// journal, so the code that lists the requests holds the same one as the
/// port that records them.
#[derive(Clone, Debug, Default)]
pub struct RequestJournal {
    records: Arc<Mutex<Vec<RequestRecord>>>,
}

impl RequestJournal {
    /// An empty journal.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Appends a request; an adapter calls it for every request it sends.
    pub fn record(&self, kind: RequestKind, provider: Provider, text: &str) {
        let mut records = self.records.lock().unwrap_or_else(PoisonError::into_inner);
        let sequence = records.len() as u64 + 1;
        records.push(RequestRecord {
            sequence,
            kind,
            provider,
            text: text.to_owned(),
        });
    }

    /// Every request received, oldest first.
    #[must_use]
    pub fn list(&self) -> Vec<RequestRecord> {
        self.records
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

/// The port: a provider that can be tested and asked for text.
///
/// An implementation journals every request it sends (nothing when the
/// provider cannot be resolved: nothing is sent), never records or returns the
/// key, and passes [`contract::run`].
pub trait Llm: Send + Sync {
    /// Asks the provider a trivial question and reports how it went.
    fn test_connection(
        &self,
        config: &ProviderConfig,
    ) -> impl Future<Output = ConnectionReport> + Send;

    /// Sends `request` to the provider and returns its answer, from the model
    /// of [`Models::for_kind`].
    fn generate(
        &self,
        config: &ProviderConfig,
        request: &GenerateRequest,
    ) -> impl Future<Output = Result<GenerateAnswer, LlmError>> + Send;

    /// The journal of the requests this port received.
    fn journal(&self) -> &RequestJournal;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_journal_numbers_from_one_and_keeps_the_order() {
        let journal = RequestJournal::new();
        journal.record(RequestKind::Brief, Provider::Cloud, "un");
        journal.record(RequestKind::Journal, Provider::ClaudeCodeBinary, "deux");
        let records = journal.list();
        assert_eq!(records.len(), 2);
        assert_eq!(
            (
                records[0].sequence,
                records[0].kind,
                records[0].text.as_str()
            ),
            (1, RequestKind::Brief, "un")
        );
        assert_eq!(
            (records[1].sequence, records[1].provider),
            (2, Provider::ClaudeCodeBinary)
        );
    }

    #[test]
    fn a_clone_of_the_journal_shares_it() {
        let journal = RequestJournal::new();
        let other = journal.clone();
        other.record(RequestKind::Notes, Provider::OpencodeServer, "notes");
        assert_eq!(journal.list().len(), 1);
    }

    #[test]
    fn a_secret_names_no_value() {
        let secret = Secret::new("sk-ant-très-secret");
        assert_eq!(format!("{secret:?}"), "Secret(***)");
        assert_eq!(secret.expose(), "sk-ant-très-secret");
        let config = ProviderConfig::Api {
            url: "https://api.anthropic.com".into(),
            api_type: None,
            key: Some(secret),
            models: Models {
                generation: "g".into(),
                summary: "s".into(),
            },
        };
        assert!(!format!("{config:?}").contains("très-secret"));
    }

    #[test]
    fn only_the_journal_is_summarised_by_the_summary_model() {
        let models = Models {
            generation: "gros".into(),
            summary: "petit".into(),
        };
        for kind in TextKind::ALL {
            let expected = if kind == TextKind::Journal {
                "petit"
            } else {
                "gros"
            };
            assert_eq!(models.for_kind(kind), expected, "{kind:?}");
        }
    }

    #[test]
    fn a_provider_is_told_from_its_api_type_or_its_binary() {
        assert_eq!(Provider::from(ApiType::Cloud), Provider::Cloud);
        assert_eq!(Provider::from(ApiType::Opencode), Provider::OpencodeServer);
        assert_eq!(Provider::from(Binary::Opencode), Provider::OpencodeBinary);
        assert_eq!(
            Provider::from(Binary::ClaudeCode),
            Provider::ClaudeCodeBinary
        );
        assert_eq!(Provider::Cloud.api_type(), Some(ApiType::Cloud));
        assert_eq!(Provider::OpencodeServer.api_type(), Some(ApiType::Opencode));
        assert_eq!(Provider::ClaudeCodeBinary.api_type(), None);
    }

    #[test]
    fn the_unrecognised_error_says_the_concepts_message() {
        assert_eq!(
            LlmError::UnrecognisedApiType.to_string(),
            "type d'API non reconnu"
        );
        assert_eq!(
            LlmError::Failed("hors ligne".into()).to_string(),
            "hors ligne"
        );
    }

    #[test]
    fn a_request_kind_is_the_text_kind_it_carries() {
        for (text, request) in [
            (TextKind::Prompt, RequestKind::Prompt),
            (TextKind::Brief, RequestKind::Brief),
            (TextKind::Notes, RequestKind::Notes),
            (TextKind::Journal, RequestKind::Journal),
        ] {
            assert_eq!(RequestKind::from(text), request);
        }
    }
}
