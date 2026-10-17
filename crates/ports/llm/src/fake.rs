//! The fake of the `llm` port: it answers deterministically, sends nothing
//! (no network, no process, no key) and journals every request it receives.
//!
//! An API URL is recognised the way `ProviderApiType@1` says: the cloud host
//! by itself, any other host by the model list its endpoint answers. The fake
//! has no endpoint to ask, so the answers are handed to it
//! ([`FakeLlm::with_endpoint`]); a URL it was given none for is, like an
//! endpoint that did not answer, not recognised. A binary always answers.

use std::collections::HashMap;

use serde_json::Value;

use crate::{
    CONNECTION_TEST_PROMPT, ConnectionReport, GenerateAnswer, GenerateRequest, Llm, LlmError,
    Provider, ProviderConfig, RequestJournal, RequestKind, UNRECOGNISED_API_TYPE, detect_api_type,
};

/// The latency every successful fake test reports, in milliseconds.
pub const FAKE_LATENCY_MS: u64 = 12;

/// The recording fake.
#[derive(Clone, Debug, Default)]
pub struct FakeLlm {
    /// What each URL's endpoint answers to its list of models.
    endpoints: HashMap<String, Value>,
    journal: RequestJournal,
}

impl FakeLlm {
    /// A fake whose endpoints answer nothing: only the cloud host and the
    /// binaries are recognised.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Makes the endpoint at `url` — matched as the exact string a
    /// configuration carries — answer `model_list` to its list of models.
    #[must_use]
    pub fn with_endpoint(mut self, url: impl Into<String>, model_list: Value) -> Self {
        self.endpoints.insert(url.into(), model_list);
        self
    }

    /// The provider `config` reaches; `trust_hint` takes the type a saved API
    /// provider already carries instead of detecting it.
    fn resolve(&self, config: &ProviderConfig, trust_hint: bool) -> Option<Provider> {
        match config {
            ProviderConfig::Binary { binary, .. } => Some(Provider::from(*binary)),
            ProviderConfig::Api { url, api_type, .. } => api_type
                .filter(|_| trust_hint)
                .or_else(|| detect_api_type(url, self.endpoints.get(url)))
                .map(Provider::from),
        }
    }
}

impl Llm for FakeLlm {
    async fn test_connection(&self, config: &ProviderConfig) -> ConnectionReport {
        let Some(provider) = self.resolve(config, false) else {
            return ConnectionReport {
                ok: false,
                provider: None,
                latency_ms: 0,
                message: UNRECOGNISED_API_TYPE.to_owned(),
            };
        };
        self.journal.record(
            RequestKind::ConnectionTest,
            provider,
            CONNECTION_TEST_PROMPT,
        );
        let models = config.models();
        ConnectionReport {
            ok: true,
            provider: Some(provider),
            latency_ms: FAKE_LATENCY_MS,
            message: format!(
                "Connexion réussie ({}) — génération : {}, résumé : {}",
                provider.label(),
                models.generation,
                models.summary
            ),
        }
    }

    async fn generate(
        &self,
        config: &ProviderConfig,
        request: &GenerateRequest,
    ) -> Result<GenerateAnswer, LlmError> {
        let provider = self
            .resolve(config, true)
            .ok_or(LlmError::UnrecognisedApiType)?;
        self.journal
            .record(RequestKind::from(request.kind), provider, &request.text);
        let model = config.models().for_kind(request.kind).to_owned();
        Ok(GenerateAnswer {
            text: format!(
                "Réponse simulée de {model} ({}, {} caractères)",
                provider.label(),
                request.text.chars().count()
            ),
            model,
        })
    }

    fn journal(&self) -> &RequestJournal {
        &self.journal
    }
}
