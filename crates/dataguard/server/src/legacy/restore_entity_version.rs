//! `campaign.restoreEntityVersion`, as the legacy chain prepares it: until
//! its `Prepare` moves to its crate, this module is its arm of the chain.

use axum::Json;
use axum::http::StatusCode;
use harness_dataguard::Error;
use harness_dataguard::api::{Submission, Submitted};
use ports_llm::Llm;
use ports_speech_to_text::SpeechToText;

use crate::{Reply, Served, fresh_uuid, queued, reply};

/// A restoration, made the state and the version it writes
/// (`restore_entity_version::prepare`), against the element's row and its
/// versions: one naming a version the element does not have — or any of an
/// element the engine does not hold — is refused, 404, and one naming no
/// element, no version or a change there is not, 422.
pub(crate) async fn submit<L: Llm, S: SpeechToText>(
    served: &Served<L, S>,
    mut body: Submission,
    now: &str,
) -> Reply<Submitted> {
    let id = body
        .target_id
        .clone()
        .or_else(|| restore_entity_version::element_id(&body.payload))
        .unwrap_or_default();
    let version = match fresh_uuid() {
        Ok(version) => version,
        Err(_) => {
            let e = Error::Store("no randomness for an id".to_string());
            return reply(StatusCode::ACCEPTED, Err(e));
        }
    };
    let base = match served.rows.projected(&format!("Entity/{id}")).await {
        Ok(base) => base,
        Err(e) => return reply(StatusCode::ACCEPTED, Err(e)),
    };
    let fresh = restore_entity_version::Fresh {
        version: &version,
        now,
    };
    let history = served.rows.versions(&id);
    match restore_entity_version::prepare(&body.payload, &base, &history, &id, fresh) {
        Ok(prepared) => {
            body.payload = prepared.payload;
            body.target_id = Some(id);
        }
        Err(refusal) => {
            let status = if refusal == restore_entity_version::Refusal::UnknownVersion {
                StatusCode::NOT_FOUND
            } else {
                StatusCode::UNPROCESSABLE_ENTITY
            };
            return Err((
                status,
                Json(serde_json::json!({ "error": refusal.to_string() })),
            ));
        }
    }
    queued(served, body, None, None).await
}
