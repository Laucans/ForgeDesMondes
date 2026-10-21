//! `media.setMediaPlace`, as the legacy chain prepares it: until its
//! `Prepare` moves to its crate, this module is its arm of the chain.

use axum::Json;
use axum::http::StatusCode;
use harness_dataguard::api::{Submission, Submitted};
use ports_llm::Llm;
use ports_speech_to_text::SpeechToText;

use crate::{Reply, Served, queued, reply};

/// A change of a media's place, made the `placeId` it writes on the media's
/// partition (`set_media_place::prepare`), judged against the media and the
/// place as [`crate::Rows::placed`] reads them: one of a media there is not
/// — or named by anything but a uuid — is refused, 404; one naming a place
/// that is no lieu of the media's campaign (`media-place-is-lieu`), or no
/// media, or a malformed place, 422 — neither is queued. The replay of one,
/// under an idempotency key already submitted, is answered as it was queued.
pub(crate) async fn submit<L: Llm, S: SpeechToText>(
    served: &Served<L, S>,
    mut body: Submission,
    _now: &str,
) -> Reply<Submitted> {
    // A replay is the engine's to answer, as the change was queued: its
    // place, since retyped, is not judged again.
    let replayed = match &body.idempotency_key {
        Some(key) => match served.rows.replayed(&body.data_capability, key).await {
            Ok(replayed) => replayed,
            Err(e) => return reply(StatusCode::ACCEPTED, Err(e)),
        },
        None => false,
    };
    if !replayed {
        // A media there is not is not found; anything else is unprocessable.
        let refused = |refusal: set_media_place::Refusal| {
            let status = if refusal == set_media_place::Refusal::UnknownMedia {
                StatusCode::NOT_FOUND
            } else {
                StatusCode::UNPROCESSABLE_ENTITY
            };
            Err((
                status,
                Json(serde_json::json!({ "error": refusal.to_string() })),
            ))
        };
        let asked = match set_media_place::asked(&body.payload, body.target_id.as_deref()) {
            Ok(asked) => asked,
            Err(refusal) => return refused(refusal),
        };
        let found = match served
            .rows
            .placed(&asked.media, asked.place.as_deref())
            .await
        {
            Ok(found) => found,
            Err(e) => return reply(StatusCode::ACCEPTED, Err(e)),
        };
        let (media, place) = found.map_or((None, None), |(media, place)| (Some(media), place));
        match set_media_place::prepare(&asked, media.as_ref(), place.as_ref()) {
            Ok(prepared) => {
                body.payload = prepared.payload;
                body.target_id = Some(prepared.target_id);
            }
            Err(refusal) => return refused(refusal),
        }
    }
    queued(served, body, None, None).await
}
