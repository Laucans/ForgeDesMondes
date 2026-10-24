//! The `llm` port's fake, made to answer a character's prompt with a proposal.
//!
//! The proposal's shape (`ai.proposeCharacter`: background, behaviour,
//! statblock, as one JSON object) is that unit's, not the port's: the fake of
//! `ports-llm` keeps its canned sentence and this server dresses it.

use ports_llm::fake::FakeLlm;
use ports_llm::{
    ConnectionReport, GenerateAnswer, GenerateRequest, Llm, LlmError, ProviderConfig,
    RequestJournal, TextKind,
};
use serde_json::Value;

/// A [`FakeLlm`] answering a [`TextKind::Prompt`] request that holds a
/// character's words with the proposal handed for them, as JSON; every other
/// request keeps the fake's answer.
pub struct Proposing {
    fake: FakeLlm,
    /// The words a prompt holds, with the proposal they are answered, in the
    /// order they were handed; empty words match any prompt.
    proposals: Vec<(String, Value)>,
}

impl Proposing {
    /// `fake`, answering no proposal yet.
    #[must_use]
    pub const fn new(fake: FakeLlm) -> Self {
        Self {
            fake,
            proposals: Vec::new(),
        }
    }

    /// Answers a prompt holding `holding` — any prompt, when it is empty —
    /// `proposal`, unless a proposal handed earlier answers it first.
    #[must_use]
    pub fn with_proposal(mut self, holding: impl Into<String>, proposal: Value) -> Self {
        self.proposals.push((holding.into(), proposal));
        self
    }

    /// The proposal handed for the words `prompt` holds, the first one first.
    fn proposal(&self, prompt: &str) -> Option<&Value> {
        self.proposals
            .iter()
            .find(|(holding, _)| prompt.contains(holding.as_str()))
            .map(|(_, proposal)| proposal)
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
        if request.kind == TextKind::Prompt
            && let Some(proposal) = self.proposal(&request.text)
        {
            answer.text = proposal.to_string();
        }
        Ok(answer)
    }

    fn journal(&self) -> &RequestJournal {
        self.fake.journal()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ports_llm::Models;
    use serde_json::json;

    fn config() -> ProviderConfig {
        ProviderConfig::Api {
            url: "https://api.anthropic.com".into(),
            api_type: None,
            key: None,
            models: Models {
                generation: "g".into(),
                summary: "s".into(),
            },
        }
    }

    async fn answer(llm: &Proposing, kind: TextKind, text: &str) -> String {
        llm.generate(&config(), &GenerateRequest::new(kind, text))
            .await
            .expect("an answer")
            .text
    }

    #[tokio::test]
    async fn a_prompt_holding_a_characters_words_is_answered_its_proposal_the_first_first() {
        let morlox = json!({ "background": "Chassé de sa tribu", "ca": 13 });
        let sergente = json!({ "background": "Renvoyée de la garde", "ca": 15 });
        let anyone = json!({ "background": "Quelqu’un", "ca": 10 });
        let llm = Proposing::new(FakeLlm::new())
            .with_proposal("gobelin marchand", morlox.clone())
            .with_proposal("sergente", sergente.clone())
            .with_proposal("", anyone.clone())
            .with_proposal("sergente de la garde", json!({ "never": "answered" }));
        for (prompt, proposal) in [
            ("Prompt initial : un gobelin marchand nerveux", &morlox),
            ("Une ancienne sergente de la garde", &sergente),
            ("Un druide", &anyone),
        ] {
            let text = answer(&llm, TextKind::Prompt, prompt).await;
            let read: Value = serde_json::from_str(&text).expect("one JSON object");
            assert_eq!(&read, proposal, "{prompt}");
        }
        assert_eq!(llm.journal().list().len(), 3);
    }

    #[tokio::test]
    async fn a_proposal_answers_a_prompt_only_and_only_the_words_it_was_handed_for() {
        let llm = Proposing::new(FakeLlm::new())
            .with_proposal("", json!({ "background": "Quelqu’un" }))
            .with_proposal("gué", json!({ "background": "Le gué" }));
        for kind in [TextKind::Brief, TextKind::Journal] {
            let text = answer(&llm, kind, "Une embuscade au gué.").await;
            assert!(text.starts_with("Réponse simulée de "), "{kind:?}: {text}");
        }
        let notes = answer(&llm, TextKind::Notes, "Une embuscade au gué.").await;
        assert!(notes.starts_with("{\"proposals\":"), "{notes}");
        let other = Proposing::new(FakeLlm::new()).with_proposal("sergente", json!({}));
        let text = answer(&other, TextKind::Prompt, "Un druide").await;
        assert!(text.starts_with("Réponse simulée de "), "{text}");
    }
}
