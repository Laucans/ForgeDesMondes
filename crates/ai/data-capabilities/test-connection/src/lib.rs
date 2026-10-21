//! DataCapability `ai.testConnection` (`data-capability.json`): the DM
//! clicks « Tester la connexion » and learns whether the provider on screen
//! answers, how fast, and if not, why.
//!
//! The configuration on screen — mode, url or binary, models, key — is the
//! wire payload; it reaches the `llm` port and nothing else. What is written
//! is the test's outcome alone, the four `last_test_*` fields, on the
//! partition of the provider it reached: `AiSettings/<its seeded row>`.
//! [`prepare`] turns the one into the other:
//!
//! - in api mode the port detects the URL's type (`ProviderApiType@1`): the
//!   cloud row or the home server's row is the one written;
//! - in binary mode, `claude` is the Claude Code binary's row;
//! - a URL whose type is not recognised — or a mode, a binary the screen
//!   cannot send — reached no provider: its refusal is written on
//!   [`NO_PROVIDER`], which the store writes on the active provider's row,
//!   its message saying « type d'API non reconnu ».
//!
//! The URL, the models, the mode and the key are never in what is
//! submitted (`test-does-not-save`): the key is the port's, for this test.
//!
//! The engine runs [`prepare`] through this crate's [`plugin`]: the `llm`
//! port is the one the server shares ([`SharedLlm`]), and the test is
//! stamped to the millisecond (`aggregate_ai_settings::timestamp`), so two
//! tests in a second are told apart.

use std::collections::BTreeMap;

pub use aggregate_ai_settings::NO_PROVIDER;
use async_trait::async_trait;
use harness_dataguard::plugins::{self, Context, Plugin, Prepare, Refusal};
use ports_llm::{
    Binary, Llm, Models, Provider, ProviderConfig, Secret, SharedLlm, UNRECOGNISED_API_TYPE,
};
use serde_json::Value;

/// This DataCapability's id, as `data-capability.json` names it.
pub const DATA_CAPABILITY: &str = "ai.testConnection";

/// The row of each kind of provider, as
/// `crates/data/migrations/0001_ai_settings.sql` seeds them.
const fn row_of(provider: Provider) -> &'static str {
    match provider {
        Provider::Cloud => "a1000000-0000-4000-8000-000000000001",
        Provider::OpencodeServer => "a1000000-0000-4000-8000-000000000002",
        Provider::OpencodeBinary => "a1000000-0000-4000-8000-000000000003",
        Provider::ClaudeCodeBinary => "a1000000-0000-4000-8000-000000000004",
    }
}

/// What a test becomes before it is submitted.
#[derive(Clone, Debug, PartialEq)]
pub struct Prepared {
    /// The partition written: the tested provider's row, or [`NO_PROVIDER`].
    pub target_id: String,
    /// The four `last_test_*` fields, and nothing else.
    pub payload: BTreeMap<String, Value>,
}

/// Whether `id` names this DataCapability, with or without its `@version`.
#[must_use]
pub fn names(id: &str) -> bool {
    id.split('@').next() == Some(DATA_CAPABILITY)
}

/// What the DataCapability plugs into the engine: its `Prepare`, which
/// reaches the `llm` port the server shares.
#[must_use]
pub fn plugin() -> Plugin {
    Plugin::new()
        .prepare(DATA_CAPABILITY, TestConnection)
        .needs::<SharedLlm>()
}

/// Why a test is refused when no `llm` port is shared: the engine refuses
/// to load first, so a server never answers it.
const NO_LLM: &str = "le port llm n'est pas branché";

/// The `Prepare` of `ai.testConnection`: [`prepare`], stamped now, on the
/// partition of the provider it reached.
struct TestConnection;

#[async_trait]
impl Prepare for TestConnection {
    async fn prepare(
        &self,
        context: &Context<'_>,
        payload: &BTreeMap<String, Value>,
        _target_id: Option<&str>,
    ) -> Result<plugins::Prepared, Refusal> {
        let llm = context
            .shared
            .get::<SharedLlm>()
            .ok_or_else(|| Refusal::unprocessable(NO_LLM))?;
        let now = aggregate_ai_settings::timestamp(context.now_millis);
        let Prepared { target_id, payload } = prepare(llm.as_ref(), payload, &now).await;
        Ok(plugins::Prepared {
            payload,
            target_id: Some(target_id),
            ..plugins::Prepared::default()
        })
    }
}

/// Tests the configuration `wire` carries through `llm`, at `tested_at`
/// (RFC 3339), and answers the outcome to write.
pub async fn prepare<L: Llm>(llm: &L, wire: &BTreeMap<String, Value>, tested_at: &str) -> Prepared {
    let models = Models {
        generation: text(wire, "generationModel"),
        summary: text(wire, "summaryModel"),
    };
    let config = match wire.get("mode").and_then(Value::as_str) {
        Some("api") => {
            let url = text(wire, "url");
            let key = Some(text(wire, "key")).filter(|key| !key.is_empty());
            (!url.is_empty()).then(|| ProviderConfig::Api {
                url,
                api_type: None,
                key: key.map(Secret::new),
                models,
            })
        }
        Some("binary" | "binaire") => {
            let binary = match wire.get("binary").and_then(Value::as_str) {
                Some("claude" | "claude-code") => Some(Binary::ClaudeCode),
                Some("opencode") => Some(Binary::Opencode),
                _ => None,
            };
            binary.map(|binary| ProviderConfig::Binary { binary, models })
        }
        _ => None,
    };
    let Some(config) = config else {
        // Nothing to test: no request is sent, no latency measured.
        return outcome(
            NO_PROVIDER,
            false,
            UNRECOGNISED_API_TYPE,
            Value::Null,
            tested_at,
        );
    };
    let report = llm.test_connection(&config).await;
    // A failure the port measured nothing of has no latency, not 0 ms.
    let latency = if report.ok || report.latency_ms > 0 {
        report.latency_ms.into()
    } else {
        Value::Null
    };
    match report.provider {
        Some(provider) => outcome(
            row_of(provider),
            report.ok,
            &report.message,
            latency,
            tested_at,
        ),
        None => {
            // No provider recognised: the message says so, whatever else
            // the port told.
            let message = if report.message.contains(UNRECOGNISED_API_TYPE) {
                report.message
            } else {
                format!("{UNRECOGNISED_API_TYPE} : {}", report.message)
            };
            outcome(NO_PROVIDER, false, &message, latency, tested_at)
        }
    }
}

fn outcome(target: &str, ok: bool, message: &str, latency: Value, at: &str) -> Prepared {
    let payload = [
        ("lastTestOk", Value::from(ok)),
        ("lastTestAt", at.into()),
        ("lastTestMessage", message.into()),
        ("lastTestLatencyMs", latency),
    ]
    .into_iter()
    .map(|(field, value)| (field.to_owned(), value))
    .collect();
    Prepared {
        target_id: target.to_owned(),
        payload,
    }
}

fn text(wire: &BTreeMap<String, Value>, field: &str) -> String {
    wire.get(field)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned()
}
