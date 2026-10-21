//! `ai.testConnection`, as the legacy chain prepares it: until its
//! `Prepare` moves to its crate, this module is its arm of the chain.

use harness_dataguard::api::{Submission, Submitted};
use ports_llm::Llm;
use ports_speech_to_text::SpeechToText;

use crate::{Reply, Served, queued};

/// A connection test, first run through the port: only its outcome is
/// submitted (`test_connection::prepare`), the configuration it tested never
/// is.
pub(crate) async fn submit<L: Llm, S: SpeechToText>(
    served: &Served<L, S>,
    mut body: Submission,
    now: &str,
) -> Reply<Submitted> {
    let prepared = test_connection::prepare(served.llm.as_ref(), &body.payload, now).await;
    body.payload = prepared.payload;
    body.target_id = Some(prepared.target_id);
    queued(served, body, None, None).await
}
