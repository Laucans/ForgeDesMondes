//! DataCapability `ai.saveSettings` (`data-capability.json`): the DM saves
//! their provider in one action, and it becomes the active one.
//!
//! The engine writes one partition per command, so a save is one command on
//! `AiSettings/1` carrying the whole state — the provider's fields and the
//! settings row's — which the DataGuard's invariants judge and its store
//! projects into `ai_provider` and `ai_settings`. What the DM sends is the
//! wire payload of the screen; [`prepare`] turns it into that state:
//!
//! - `mode: binary` is stored « binaire », `binary: claude` « claude-code »;
//! - in api mode the API type is derived from the URL (`ProviderApiType@1`):
//!   the cloud host by the URL alone, any other by asking the `llm` port.
//!   A type the caller gives is dropped, never stored;
//! - each kind of provider has its one row, the migration's seed: the save
//!   writes that row and makes it active.
//!
//! An unrecognised URL is still submitted, with no type: the invariant
//! `api-type-recognized` rejects it and nothing is applied. A save missing
//! a model is too: `single-settings-row` rejects it.

use std::collections::BTreeMap;

use ports_llm::{
    ApiType, Binary, Llm, Models, Provider, ProviderConfig, Secret, UNRECOGNISED_API_TYPE,
    detect_api_type,
};
use serde_json::Value;

/// This DataCapability's id, as `data-capability.json` names it.
pub const DATA_CAPABILITY: &str = "ai.saveSettings";

/// The partition's id: the one settings row, which the save writes whole.
pub const TARGET_ID: &str = "1";

/// Why a save missing its generation or summary model is refused.
pub const MISSING_MODELS: &str = "modèles de génération et de résumé requis";

/// The row of each kind of provider and its label, as
/// `crates/data/migrations/0001_ai_settings.sql` seeds them.
const fn row_of(provider: Provider) -> (&'static str, &'static str) {
    match provider {
        Provider::Cloud => ("a1000000-0000-4000-8000-000000000001", "API cloud"),
        Provider::OpencodeServer => (
            "a1000000-0000-4000-8000-000000000002",
            "Endpoint opencode du serveur maison",
        ),
        Provider::OpencodeBinary => ("a1000000-0000-4000-8000-000000000003", "Binaire opencode"),
        Provider::ClaudeCodeBinary => (
            "a1000000-0000-4000-8000-000000000004",
            "Binaire Claude Code",
        ),
    }
}

/// What a save becomes before it is submitted.
#[derive(Clone, Debug, PartialEq)]
pub struct Prepared {
    /// The state written on `AiSettings/1`: every field, `null` where empty,
    /// so nothing of the provider saved before is left behind.
    pub payload: BTreeMap<String, Value>,
    /// Why the save will be refused, for the DM: the URL's API type is not
    /// recognised, or a model is missing.
    pub refusal: Option<&'static str>,
}

/// Whether `id` names this DataCapability, with or without its `@version`.
#[must_use]
pub fn names(id: &str) -> bool {
    id.split('@').next() == Some(DATA_CAPABILITY)
}

/// The state a save of `wire` writes, `updated_at` being now (RFC 3339).
pub async fn prepare<L: Llm>(
    llm: &L,
    wire: &BTreeMap<String, Value>,
    updated_at: &str,
) -> Prepared {
    let given = |field: &str| wire.get(field).cloned().unwrap_or(Value::Null);
    let mut payload = BTreeMap::new();
    let mut refusal = None;
    let provider = match wire.get("mode").and_then(Value::as_str) {
        Some("api") => {
            let url = text(wire, "url");
            let key = Some(text(wire, "key")).filter(|key| !key.is_empty());
            let models = Models {
                generation: text(wire, "generationModel"),
                summary: text(wire, "summaryModel"),
            };
            let api_type = detected(llm, &url, key.as_deref(), models).await;
            refusal = api_type.is_none().then_some(UNRECOGNISED_API_TYPE);
            payload.insert("mode".into(), "api".into());
            payload.insert("url".into(), url.into());
            payload.insert(
                "detectedApiType".into(),
                api_type.map_or(Value::Null, |t| api_type_name(t).into()),
            );
            payload.insert("apiKey".into(), key.map_or(Value::Null, Value::from));
            payload.insert("binary".into(), Value::Null);
            // A URL that is not the cloud's is the home server's candidate.
            Some(api_type.map_or(Provider::OpencodeServer, Provider::from))
        }
        Some("binary" | "binaire") => {
            let binary = match wire.get("binary").and_then(Value::as_str) {
                Some("claude" | "claude-code") => Some(Binary::ClaudeCode),
                Some("opencode") => Some(Binary::Opencode),
                _ => None,
            };
            payload.insert("mode".into(), "binaire".into());
            payload.insert("url".into(), Value::Null);
            payload.insert("detectedApiType".into(), Value::Null);
            payload.insert("apiKey".into(), Value::Null);
            // Another binary is kept as given: `mode-fields` refuses it.
            payload.insert(
                "binary".into(),
                binary.map_or_else(|| given("binary"), |b| binary_name(b).into()),
            );
            binary.map(Provider::from)
        }
        _ => {
            payload.insert("mode".into(), given("mode"));
            None
        }
    };
    let (id, label) = provider.map(row_of).unzip();
    let id = id.map_or(Value::Null, Value::from);
    payload.insert("aiProviderId".into(), id.clone());
    payload.insert("activeProviderId".into(), id);
    payload.insert("label".into(), label.map_or(Value::Null, Value::from));
    payload.insert("generationModel".into(), given("generationModel"));
    payload.insert("summaryModel".into(), given("summaryModel"));
    payload.insert("updatedAt".into(), updated_at.into());
    let models_given = ["generationModel", "summaryModel"]
        .iter()
        .all(|field| !text(wire, field).is_empty());
    let refusal = refusal.or_else(|| (!models_given).then_some(MISSING_MODELS));
    Prepared { payload, refusal }
}

/// The API type behind `url`: the cloud host by itself, any other by what
/// the port's connection test reached. No `last_test_*` is written here.
async fn detected<L: Llm>(
    llm: &L,
    url: &str,
    key: Option<&str>,
    models: Models,
) -> Option<ApiType> {
    if url.is_empty() {
        return None;
    }
    if let Some(api_type) = detect_api_type(url, None) {
        return Some(api_type);
    }
    let config = ProviderConfig::Api {
        url: url.to_owned(),
        api_type: None,
        key: key.map(Secret::new),
        models,
    };
    llm.test_connection(&config)
        .await
        .provider
        .and_then(Provider::api_type)
}

fn text(wire: &BTreeMap<String, Value>, field: &str) -> String {
    wire.get(field)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

/// How `ProviderApiType@1` names a type, and `ai_provider` stores it.
const fn api_type_name(api_type: ApiType) -> &'static str {
    match api_type {
        ApiType::Cloud => "cloud",
        ApiType::Opencode => "opencode",
    }
}

/// How `ai_provider.binary` stores a binary.
const fn binary_name(binary: Binary) -> &'static str {
    match binary {
        Binary::Opencode => "opencode",
        Binary::ClaudeCode => "claude-code",
    }
}

/// `unix_seconds` as RFC 3339 in UTC (`2026-10-17T10:00:00Z`): what
/// `ai_settings.updated_at` takes.
#[must_use]
pub fn timestamp(unix_seconds: u64) -> String {
    // Howard Hinnant's civil_from_days.
    let days = (unix_seconds / 86_400) as i64 + 719_468;
    let seconds = unix_seconds % 86_400;
    let era = days.div_euclid(146_097);
    let doe = days.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        seconds / 3600,
        seconds % 3600 / 60,
        seconds % 60
    )
}
