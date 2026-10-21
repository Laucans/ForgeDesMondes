//! `ai.saveSettings`, as the legacy chain prepares it: until its `Prepare`
//! moves to its crate, this module is its arm of the chain.

use harness_dataguard::api::{Submission, Submitted};
use ports_llm::Llm;
use ports_speech_to_text::SpeechToText;

use crate::{Reply, Served, queued};

/// A save of the AI settings, first made the state it writes
/// (`save_settings::prepare`) on `AiSettings/1`; its refusal, when its URL
/// is not recognised, is told among the warnings.
pub(crate) async fn submit<L: Llm, S: SpeechToText>(
    served: &Served<L, S>,
    mut body: Submission,
    now: &str,
) -> Reply<Submitted> {
    let prepared = save_settings::prepare(served.llm.as_ref(), &body.payload, now).await;
    body.payload = prepared.payload;
    body.target_id = Some(save_settings::TARGET_ID.to_string());
    queued(served, body, prepared.refusal, None).await
}
