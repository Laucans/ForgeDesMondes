//! `media.saveGridSettings`, as the legacy chain prepares it: until its
//! `Prepare` moves to its crate, this module is its arm of the chain.

use axum::Json;
use axum::http::StatusCode;
use harness_dataguard::api::{Submission, Submitted};
use ports_llm::Llm;
use ports_speech_to_text::SpeechToText;

use crate::{Reply, Served, queued};

/// A save of a battlemap's grid, made the six fields it writes on the
/// battlemap's partition (`save_grid_settings::prepare`): one naming no
/// battlemap, carrying another field, lacking one, or holding a value no
/// grid can is refused, 422, and not queued.
pub(crate) async fn submit<L: Llm, S: SpeechToText>(
    served: &Served<L, S>,
    mut body: Submission,
    _now: &str,
) -> Reply<Submitted> {
    match save_grid_settings::prepare(&body.payload, body.target_id.as_deref()) {
        Ok(prepared) => {
            body.payload = prepared.payload;
            body.target_id = Some(prepared.target_id);
        }
        Err(refusal) => {
            return Err((
                StatusCode::UNPROCESSABLE_ENTITY,
                Json(serde_json::json!({ "error": refusal.to_string() })),
            ));
        }
    }
    queued(served, body, None, None).await
}
