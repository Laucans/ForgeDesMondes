//! `ai.extractFromNotes`, as the legacy chain prepares it: until its
//! `Prepare` moves to its crate, this module is its arm of the chain.

use aggregate_entity::parsed;
use axum::Json;
use axum::http::StatusCode;
use harness_dataguard::Error;
use harness_dataguard::api::{Submission, Submitted};
use ports_llm::Llm;
use ports_speech_to_text::SpeechToText;

use crate::{Reply, Served, fresh_uuid, reply, submit_prepared};
use ai_provider::{SETTINGS as AI_SETTINGS, active};

/// An analysis of notes (`extract_from_notes::analyse`), made before anything
/// is queued and submitted as its commands, `now` being when the note is
/// imported: one per proposal, then the note's, whose result is answered —
/// the note is applied after its proposals when each is queued
/// (`Analysis::commands`). Each command is the caller's; an idempotency key
/// is the note's alone: a note whose key the engine knows is answered again,
/// and nothing is analysed. An analysis whose commands were not all
/// submitted leaves proposals of a note never written, which the tables
/// never show (`projection::ADD_PROPOSALS`). A failure of the `llm` port is told among
/// the warnings, the note kept alone; the warnings of the proposals' commands
/// are told there too. Notes of a campaign the engine does not hold are
/// refused, 404, and notes that name none or hold no word, 422: neither is
/// queued.
pub(crate) async fn submit<L: Llm, S: SpeechToText>(
    served: &Served<L, S>,
    body: Submission,
    now: &str,
) -> Reply<Submitted> {
    let refused =
        |status: StatusCode, why: String| Err((status, Json(serde_json::json!({ "error": why }))));
    if let Some(key) = &body.idempotency_key {
        match served.rows.replayed(&body.data_capability, key).await {
            Ok(false) => {}
            // The engine answers the note's command again.
            Ok(true) => return reply(StatusCode::ACCEPTED, submit_prepared(served, body).await),
            Err(e) => return reply(StatusCode::ACCEPTED, Err(e)),
        }
    }
    let campaign = body
        .payload
        .get("campaign")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .to_owned();
    if parsed(&campaign).is_some() {
        match served.rows.projected(&format!("Campaign/{campaign}")).await {
            Ok(row) if row.is_empty() => {
                return refused(StatusCode::NOT_FOUND, format!("aucune campagne {campaign}"));
            }
            Ok(_) => {}
            Err(e) => return reply(StatusCode::ACCEPTED, Err(e)),
        }
    }
    let settings = match served.rows.projected(AI_SETTINGS).await {
        Ok(row) => row,
        Err(e) => return reply(StatusCode::ACCEPTED, Err(e)),
    };
    let known = served.rows.known(&campaign);
    let analysis = match extract_from_notes::analyse(
        served.llm.as_ref(),
        &active(&settings),
        served.speech.as_ref(),
        &body.payload,
        &known,
    )
    .await
    {
        Ok(analysis) => analysis,
        Err(refusal) => return refused(StatusCode::UNPROCESSABLE_ENTITY, refusal.to_string()),
    };
    let ids: Result<Vec<String>, _> = (0..=analysis.proposals.len())
        .map(|_| fresh_uuid())
        .collect();
    let Ok(ids) = ids else {
        let e = Error::Store("no randomness for an id".to_string());
        return reply(StatusCode::ACCEPTED, Err(e));
    };
    let commands = analysis.commands(ids, now);
    let command = |command: extract_from_notes::Command, key: Option<String>| Submission {
        data_capability: body.data_capability.clone(),
        payload: command.payload,
        target_id: Some(command.target_id),
        based_on: None,
        idempotency_key: key,
        caller: body.caller.clone(),
    };
    let mut warnings: Vec<String> = analysis.failure.into_iter().collect();
    for proposal in commands.proposals {
        match submit_prepared(served, command(proposal, None)).await {
            Ok(submitted) => warnings.extend(submitted.warnings),
            Err(e) => return reply(StatusCode::ACCEPTED, Err(e)),
        }
    }
    let submitted = submit_prepared(served, command(commands.note, body.idempotency_key.clone()))
        .await
        .map(|mut submitted| {
            submitted.warnings.splice(0..0, warnings);
            submitted
        });
    reply(StatusCode::ACCEPTED, submitted)
}
