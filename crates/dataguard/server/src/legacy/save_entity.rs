//! `campaign.saveEntity`, as the legacy chain prepares it: until its
//! `Prepare` moves to its crate, this module is its arm of the chain.

use axum::Json;
use axum::http::StatusCode;
use harness_dataguard::Error;
use harness_dataguard::api::{Submission, Submitted};
use ports_llm::Llm;
use ports_speech_to_text::SpeechToText;

use crate::{Reply, Served, fresh_uuid, queued, reply};

/// A save of an element, made the state and the version it writes on the
/// element's partition (`save_entity::prepare`), against the row the
/// commands queued there will leave; a new element is given its id, and a
/// save the tables could never hold is refused, 422, before it is queued.
pub(crate) async fn submit<L: Llm, S: SpeechToText>(
    served: &Served<L, S>,
    mut body: Submission,
    now: &str,
) -> Reply<Submitted> {
    let unrandom = |_| Error::Store("no randomness for an id".to_string());
    let named = save_entity::element_id(&body.payload).or_else(|| body.target_id.clone());
    let ids = match named {
        Some(id) => fresh_uuid().map(|version| (id, version)),
        None => fresh_uuid().and_then(|id| fresh_uuid().map(|version| (id, version))),
    };
    let (id, version) = match ids.map_err(unrandom) {
        Ok(ids) => ids,
        Err(e) => return reply(StatusCode::ACCEPTED, Err(e)),
    };
    let base = match served.rows.projected(&format!("Entity/{id}")).await {
        Ok(base) => base,
        Err(e) => return reply(StatusCode::ACCEPTED, Err(e)),
    };
    let fresh = save_entity::Fresh {
        entity: &id,
        version: &version,
        now,
    };
    match save_entity::prepare(&body.payload, &base, fresh) {
        Ok(prepared) => {
            body.payload = prepared.payload;
            body.target_id = Some(prepared.target_id);
        }
        // What the tables could never hold is not queued.
        Err(refusal) => {
            return Err((
                StatusCode::UNPROCESSABLE_ENTITY,
                Json(serde_json::json!({ "error": refusal.to_string() })),
            ));
        }
    }
    queued(served, body, None, None).await
}
