//! The `llm` port's fake, made to answer a character prompt with a proposal.
//! It lives here, not in the port: the proposal's shape is this unit's
//! (`ai.proposeCharacter`), the port's fake stays as its crate wrote it.

use ports_llm::fake::FakeLlm;
use ports_llm::{
    ConnectionReport, GenerateAnswer, GenerateRequest, Llm, LlmError, ProviderConfig,
    RequestJournal, TextKind,
};
use serde_json::Value;

/// A [`FakeLlm`] whose [`TextKind::Prompt`] answers are the proposal it was
/// handed, as one JSON object; every other request keeps the fake's answer.
/// A clone shares the fake's journal.
#[derive(Clone)]
pub struct Proposing {
    fake: FakeLlm,
    proposal: Option<Value>,
}

impl Proposing {
    /// A fake handed no proposal: it answers its canned sentence.
    // Each test file compiles this module alone, and only `propose.rs` hands none.
    #[allow(dead_code)]
    pub fn new() -> Self {
        Self {
            fake: FakeLlm::new(),
            proposal: None,
        }
    }

    /// A fake answering any prompt `proposal`.
    pub fn answering(proposal: Value) -> Self {
        Self {
            fake: FakeLlm::new(),
            proposal: Some(proposal),
        }
    }
}

impl Llm for Proposing {
    async fn test_connection(&self, config: &ProviderConfig) -> ConnectionReport {
        self.fake.test_connection(config).await
    }

    async fn generate(
        &self,
        config: &ProviderConfig,
        request: &GenerateRequest,
    ) -> Result<GenerateAnswer, LlmError> {
        // The fake journals the request and picks the model; only the text changes.
        let mut answer = self.fake.generate(config, request).await?;
        if let (TextKind::Prompt, Some(proposal)) = (request.kind, &self.proposal) {
            answer.text = proposal.to_string();
        }
        Ok(answer)
    }

    fn journal(&self) -> &RequestJournal {
        self.fake.journal()
    }
}
