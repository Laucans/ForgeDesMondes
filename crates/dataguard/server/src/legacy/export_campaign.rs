//! `campaign.exportCampaign`, as the legacy chain prepares it: until its
//! `Prepare` moves to its crate, this module is its arm of the chain.

use axum::Json;
use axum::http::StatusCode;
use harness_dataguard::api::{Submission, Submitted};
use ports_llm::Llm;
use ports_speech_to_text::SpeechToText;

use crate::{Reply, Served, queued, reply};

/// An export, made the stamp it writes on its campaign's partition
/// (`export_campaign::prepare`); its archive is built from what the tables
/// hold of the campaign now and kept until the command is applied
/// ([`crate::status`]): one of a campaign the tables do not hold is refused,
/// 404, and one they cannot make an archive of, 422 — neither is queued, so
/// nothing changes.
pub(crate) async fn submit<L: Llm, S: SpeechToText>(
    served: &Served<L, S>,
    mut body: Submission,
    now: &str,
) -> Reply<Submitted> {
    let id = body
        .target_id
        .clone()
        .or_else(|| export_campaign::campaign_id(&body.payload))
        .unwrap_or_default();
    let tables = match served.rows.archived(&id).await {
        Ok(Some(tables)) => tables,
        Ok(None) => {
            return Err((
                StatusCode::NOT_FOUND,
                Json(
                    serde_json::json!({ "error": export_campaign::Refusal::NoCampaign.to_string() }),
                ),
            ));
        }
        Err(e) => return reply(StatusCode::ACCEPTED, Err(e)),
    };
    match export_campaign::prepare(&tables, now) {
        Ok(prepared) => {
            body.payload = prepared.payload;
            body.target_id = Some(prepared.target_id);
            queued(served, body, None, Some(prepared.archive)).await
        }
        Err(refusal) => Err((
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(serde_json::json!({ "error": refusal.to_string() })),
        )),
    }
}
