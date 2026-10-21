//! The `llm` port behind one handle a server shares with every unit:
//! [`SharedLlm`].
//!
//! [`Llm`] answers with `impl Future`, so it is not dyn-compatible: a unit
//! cannot read a `dyn Llm` from the server's `Shared`, which holds one value
//! per type. The handle boxes each future once, so one type stands for any
//! port: the fake, an adapter. It implements [`Llm`] itself, so the code a
//! unit calls with a port takes it unchanged, and every clone of it is the
//! same port, with the same journal.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use crate::{
    ConnectionReport, GenerateAnswer, GenerateRequest, Llm, LlmError, ProviderConfig,
    RequestJournal,
};

/// A future of the port, boxed.
type Boxed<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// [`Llm`], with its futures boxed: what a handle holds as `dyn`.
trait Erased: Send + Sync {
    fn test_connection<'a>(&'a self, config: &'a ProviderConfig) -> Boxed<'a, ConnectionReport>;

    fn generate<'a>(
        &'a self,
        config: &'a ProviderConfig,
        request: &'a GenerateRequest,
    ) -> Boxed<'a, Result<GenerateAnswer, LlmError>>;

    fn journal(&self) -> &RequestJournal;
}

impl<L: Llm> Erased for L {
    fn test_connection<'a>(&'a self, config: &'a ProviderConfig) -> Boxed<'a, ConnectionReport> {
        Box::pin(Llm::test_connection(self, config))
    }

    fn generate<'a>(
        &'a self,
        config: &'a ProviderConfig,
        request: &'a GenerateRequest,
    ) -> Boxed<'a, Result<GenerateAnswer, LlmError>> {
        Box::pin(Llm::generate(self, config, request))
    }

    fn journal(&self) -> &RequestJournal {
        Llm::journal(self)
    }
}

/// The `llm` port a server hands its units: one port behind a handle every
/// clone of which is that port.
#[derive(Clone)]
pub struct SharedLlm(Arc<dyn Erased>);

impl SharedLlm {
    /// The handle on `llm`.
    #[must_use]
    pub fn new<L: Llm + 'static>(llm: L) -> Self {
        Self(Arc::new(llm))
    }
}

impl Llm for SharedLlm {
    async fn test_connection(&self, config: &ProviderConfig) -> ConnectionReport {
        self.0.test_connection(config).await
    }

    async fn generate(
        &self,
        config: &ProviderConfig,
        request: &GenerateRequest,
    ) -> Result<GenerateAnswer, LlmError> {
        self.0.generate(config, request).await
    }

    fn journal(&self) -> &RequestJournal {
        self.0.journal()
    }
}
