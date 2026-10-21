//! `media.uploadMedia`, as the legacy chain prepares it: until its
//! `Prepare` moves to its crate, this module is its arm of the chain.

use axum::Json;
use axum::http::StatusCode;
use harness_dataguard::Error;
use harness_dataguard::api::{Submission, Submitted};
use ports_llm::Llm;
use ports_speech_to_text::SpeechToText;

use crate::{Reply, Rows, Served, fresh_uuid, queued, reply, status_of};

/// An upload ([`uploaded`]), made the media it writes on a new media's
/// partition, a battlemap with its default grid; its place is read first
/// ([`Rows::place`]), and an upload whose place is no lieu of its campaign
/// is refused, 422, naming `media-place-is-lieu`, as is one the tables could
/// never hold — neither is queued. The replay of one, under an idempotency
/// key already submitted, is answered as it was queued.
pub(crate) async fn submit<L: Llm, S: SpeechToText>(
    served: &Served<L, S>,
    mut body: Submission,
    now: &str,
) -> Reply<Submitted> {
    // A replay is the engine's to answer, as the upload was queued: its
    // place, since retyped, is not read again.
    let replayed = match &body.idempotency_key {
        Some(key) => match served.rows.replayed(&body.data_capability, key).await {
            Ok(replayed) => replayed,
            Err(e) => return reply(StatusCode::ACCEPTED, Err(e)),
        },
        None => false,
    };
    if !replayed {
        let prepared = uploaded(&served.rows, &body.payload, now).await?;
        body.payload = prepared.payload;
        body.target_id = Some(prepared.target_id);
    }
    queued(served, body, None, None).await
}

/// An upload made the media it writes on a new media's partition
/// (`upload_media`): checked first, then its campaign and its place read
/// from [`Rows`]. What the tables could never hold, a campaign they do not
/// hold, or a place that is no lieu of the campaign, is refused, 422; rows
/// that cannot be read, 503.
async fn uploaded(
    rows: &Rows,
    wire: &std::collections::BTreeMap<String, serde_json::Value>,
    now: &str,
) -> Result<upload_media::Prepared, (StatusCode, Json<serde_json::Value>)> {
    let refused = |refusal: upload_media::Refusal| {
        (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(serde_json::json!({ "error": refusal.to_string() })),
        )
    };
    let unread = |e: Error| {
        (
            status_of(&e),
            Json(serde_json::json!({ "error": e.to_string() })),
        )
    };
    let upload = upload_media::checked(wire).map_err(refused)?;
    if !rows
        .holds_campaign(upload.campaign())
        .await
        .map_err(unread)?
    {
        return Err(refused(upload_media::Refusal::UnknownCampaign));
    }
    let place = match upload.place() {
        Some(id) => rows.place(id).await.map_err(unread)?,
        None => None,
    };
    let media =
        fresh_uuid().map_err(|_| unread(Error::Store("no randomness for an id".to_string())))?;
    let fresh = upload_media::Fresh { media: &media, now };
    upload.prepare(place.as_ref(), fresh).map_err(refused)
}
