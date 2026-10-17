//! The selector: the saved settings in, the right client out, so a caller
//! never branches on the mode.
//!
//! A [`ProviderConfig`] is an API URL or a binary. The four providers behind
//! them — the cloud API, the home opencode server, the `opencode` program,
//! Claude Code — are told apart where it is cheap: the mode picks [`ApiLlm`]
//! or [`BinaryLlm`], and `ApiLlm` tells the cloud from the home server by the
//! API type of the URL (`ProviderApiType@1`). Both clients write one
//! [`RequestJournal`], the selector's.

use ports_llm::{
    ConnectionReport, GenerateAnswer, GenerateRequest, Llm, LlmError, ProviderConfig,
    RequestJournal,
};

use crate::{ApiLlm, BinaryLlm};

/// The client a configuration is served by.
#[derive(Clone, Copy, Debug)]
pub enum Chosen<'a> {
    /// An API URL: the cloud API or the home opencode server.
    Api(&'a ApiLlm),
    /// A local program: `opencode` or Claude Code.
    Binary(&'a BinaryLlm),
}

impl Llm for Chosen<'_> {
    async fn test_connection(&self, config: &ProviderConfig) -> ConnectionReport {
        match self {
            Self::Api(api) => api.test_connection(config).await,
            Self::Binary(binary) => binary.test_connection(config).await,
        }
    }

    async fn generate(
        &self,
        config: &ProviderConfig,
        request: &GenerateRequest,
    ) -> Result<GenerateAnswer, LlmError> {
        match self {
            Self::Api(api) => api.generate(config, request).await,
            Self::Binary(binary) => binary.generate(config, request).await,
        }
    }

    fn journal(&self) -> &RequestJournal {
        match self {
            Self::Api(api) => api.journal(),
            Self::Binary(binary) => binary.journal(),
        }
    }
}

/// The `llm` port over every provider: a call goes to the client of its
/// configuration's mode.
#[derive(Clone, Debug)]
pub struct Selector {
    api: ApiLlm,
    binary: BinaryLlm,
    journal: RequestJournal,
}

impl Selector {
    /// A selector over `api` and `binary`, which from now on write the
    /// selector's journal, not their own.
    #[must_use]
    pub fn new(api: ApiLlm, binary: BinaryLlm) -> Self {
        let journal = RequestJournal::new();
        Self {
            api: api.with_journal(journal.clone()),
            binary: binary.with_journal(journal.clone()),
            journal,
        }
    }

    /// The selector of the product: the cloud's key from `LLM_API_KEY`, the
    /// programs on the process's `PATH`.
    #[must_use]
    pub fn from_env() -> Self {
        Self::new(ApiLlm::from_env(), BinaryLlm::new())
    }

    /// The client that serves `config`.
    #[must_use]
    pub const fn client(&self, config: &ProviderConfig) -> Chosen<'_> {
        match config {
            ProviderConfig::Api { .. } => Chosen::Api(&self.api),
            ProviderConfig::Binary { .. } => Chosen::Binary(&self.binary),
        }
    }
}

impl Llm for Selector {
    async fn test_connection(&self, config: &ProviderConfig) -> ConnectionReport {
        self.client(config).test_connection(config).await
    }

    async fn generate(
        &self,
        config: &ProviderConfig,
        request: &GenerateRequest,
    ) -> Result<GenerateAnswer, LlmError> {
        self.client(config).generate(config, request).await
    }

    fn journal(&self) -> &RequestJournal {
        &self.journal
    }
}
