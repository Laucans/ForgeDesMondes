//! Capability `ai.listProviderRequests` — what was sent to the provider.
//!
//! It reads the `llm` port's request journal through the trait
//! ([`Llm::journal`]) and answers `{ requests: [...] }`, oldest first. It
//! reads no store, writes nothing and holds no secret: a journal record is a
//! sequence, a kind, a provider and a text, so nothing else can be listed.

use ports_llm::{Llm, Provider, RequestKind, RequestRecord};
use serde::Serialize;

/// What a request carries. The port's requests hold a `String` and nothing
/// else, so text is the only value there is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Modality {
    /// Words.
    Text,
}

/// One request the provider received.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ProviderRequest {
    /// Its place in the journal, from 1.
    pub sequence: u64,
    /// What it was.
    pub kind: RequestKind,
    /// Who it went to.
    pub provider: Provider,
    /// The text sent.
    pub text: String,
    /// What it carried — derived, never stored: always `["text"]`.
    pub modalities: Vec<Modality>,
}

impl From<RequestRecord> for ProviderRequest {
    fn from(record: RequestRecord) -> Self {
        Self {
            sequence: record.sequence,
            kind: record.kind,
            provider: record.provider,
            text: record.text,
            modalities: vec![Modality::Text],
        }
    }
}

/// The answer: the requests received, oldest first.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Answer {
    pub requests: Vec<ProviderRequest>,
}

/// Lists the requests `llm` received, in order.
#[must_use]
pub fn list_provider_requests<L: Llm>(llm: &L) -> Answer {
    Answer {
        requests: llm
            .journal()
            .list()
            .into_iter()
            .map(ProviderRequest::from)
            .collect(),
    }
}
