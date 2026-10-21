//! `campaign.createEntitiesFromNote`, as the legacy chain prepares it: until
//! its `Prepare` moves to its crate, this module is its arm of the chain.

use axum::Json;
use axum::http::StatusCode;
use harness_dataguard::Error;
use harness_dataguard::api::{Submission, Submitted};
use ports_llm::Llm;
use ports_speech_to_text::SpeechToText;

use crate::{Reply, Rows, Served, reply, submit_prepared};

/// A validation of a note's proposals (`create_entities_from_note::request`),
/// made against the proposals the engine holds — the accepted ones and every
/// other of their notes, which [`Rows`] knows — and submitted as its
/// commands (`Request::batch`), `now` being when the elements are made. First
/// the element each accepted proposal becomes, on its Entity partition: the
/// DM's save of an element, written as `campaign.saveEntity` writes one —
/// the Entity aggregate's DataCapability, under whose touches the engine
/// judges every Entity invariant. An element the DataGuard judges at risk
/// when it is queued — which it rejects once applied — is not recorded on its
/// proposal, which stays as it was, and the warnings are told, each after
/// the proposal's name; when no proposal is left to write, that element's
/// command is answered. Then each proposal checked or unchecked, in id
/// order: the last, applied after the others, is answered.
/// An element's ids come from its proposal's, so a validation retried after a
/// failure half-way finds the element it queued and does not queue it again. A
/// proposal recording an element the engine holds none of — rejected once
/// applied — is made again; one the DM deleted, never. Each command is
/// the caller's; an idempotency key is the last proposal's alone, as an
/// analysis's is its note's — a validation that writes no proposal keeps
/// none. Validations run one at a time, each against the proposals the one
/// before queued. A validation naming no campaign or no proposal
/// is refused, 422, and one naming a proposal of no note of its campaign,
/// 404: neither is queued.
pub(crate) async fn submit<L: Llm, S: SpeechToText>(
    served: &Served<L, S>,
    body: Submission,
    now: &str,
) -> Reply<Submitted> {
    use create_entities_from_note::Fresh;
    let refused =
        |status: StatusCode, why: String| Err((status, Json(serde_json::json!({ "error": why }))));
    // One validation at a time: the next reads the proposals the last one
    // queued, so a proposal is never made twice by two at once.
    let _validating = VALIDATING.lock().await;
    if let Some(key) = &body.idempotency_key {
        match served.rows.replayed(&body.data_capability, key).await {
            Ok(false) => {}
            // The engine answers the last proposal's command again.
            Ok(true) => return reply(StatusCode::ACCEPTED, submit_prepared(served, body).await),
            Err(e) => return reply(StatusCode::ACCEPTED, Err(e)),
        }
    }
    let request = match create_entities_from_note::request(&body.payload) {
        Ok(request) => request,
        Err(refusal) => return refused(StatusCode::UNPROCESSABLE_ENTITY, refusal.to_string()),
    };
    let proposals = match held(&served.rows, &request.accepted).await {
        Ok(proposals) => proposals,
        Err(e) => return reply(StatusCode::ACCEPTED, Err(e)),
    };
    // The same ids every time: a validation retried after a failure half-way
    // finds the elements it made, and does not make them twice.
    let fresh: Vec<Fresh> = request.accepted.iter().map(|id| Fresh::of(id)).collect();
    let batch = match request.batch(&proposals, fresh, now) {
        Ok(batch) => batch,
        Err(refusal) => return refused(StatusCode::NOT_FOUND, refusal.to_string()),
    };
    let command = |data_capability: &str, command: create_entities_from_note::Command| Submission {
        data_capability: data_capability.to_owned(),
        payload: command.payload,
        target_id: Some(command.target_id),
        based_on: None,
        idempotency_key: None,
        caller: body.caller.clone(),
    };
    let mut warnings = Vec::new();
    let mut rejected = std::collections::BTreeSet::new();
    let mut last_rejected = None;
    for element in batch.elements {
        // Queued by a validation that failed before it stamped its proposal.
        let partition = format!("Entity/{}", element.command.target_id);
        match served.rows.projected(&partition).await {
            Ok(held) if !held.is_empty() => continue,
            Ok(_) => {}
            Err(e) => return reply(StatusCode::ACCEPTED, Err(e)),
        }
        // Made, then deleted by the DM: never made again, still recorded.
        match served.rows.deleted(&partition).await {
            Ok(true) => continue,
            Ok(false) => {}
            Err(e) => return reply(StatusCode::ACCEPTED, Err(e)),
        }
        let submitted = match submit_prepared(
            served,
            command(save_entity::DATA_CAPABILITY, saved(element.command)),
        )
        .await
        {
            Ok(submitted) => submitted,
            Err(e) => return reply(StatusCode::ACCEPTED, Err(e)),
        };
        // What the DataGuard judges at risk now, it rejects once applied; a
        // review it opens, or a value declared ahead, it may still let through.
        let at_risk = submitted
            .warnings
            .iter()
            .any(|warning| warning.starts_with(AT_RISK));
        if !at_risk {
            warnings.extend(submitted.warnings);
            continue;
        }
        let name = proposals
            .iter()
            .find(|proposal| proposal.id == element.proposal)
            .and_then(|proposal| proposal.row.get("name"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or(&element.proposal);
        warnings.extend(submitted.warnings.iter().map(|w| format!("{name} : {w}")));
        rejected.insert(element.proposal);
        last_rejected = Some(submitted);
    }
    let mut stamps: Vec<_> = batch
        .stamps
        .into_iter()
        .filter(|stamp| !rejected.contains(&stamp.target_id))
        .collect();
    let Some(last) = stamps.pop() else {
        // Every element accepted is at risk: the last one is answered.
        return reply(
            StatusCode::ACCEPTED,
            last_rejected.ok_or_else(|| Error::Store("nothing validated".to_string())),
        );
    };
    for stamp in stamps {
        match submit_prepared(served, command(&body.data_capability, stamp)).await {
            Ok(submitted) => warnings.extend(submitted.warnings),
            Err(e) => return reply(StatusCode::ACCEPTED, Err(e)),
        }
    }
    let mut last = command(&body.data_capability, last);
    last.idempotency_key = body.idempotency_key.clone();
    let submitted = submit_prepared(served, last).await.map(|mut submitted| {
        submitted.warnings.splice(0..0, warnings);
        submitted
    });
    reply(StatusCode::ACCEPTED, submitted)
}

/// The element `command` makes, as its save is submitted: the wire payload
/// `campaign.saveEntity`'s `Prepare` maps — its campaign, and its id, type,
/// name and excerpt — which writes it with its first version.
fn saved(command: create_entities_from_note::Command) -> create_entities_from_note::Command {
    let field = |name: &str| command.payload.get(name).cloned();
    let mut entity = serde_json::Map::new();
    entity.insert("id".into(), command.target_id.clone().into());
    for (wire, name) in [
        ("kind", "type"),
        ("name", "name"),
        ("description", "description"),
    ] {
        if let Some(value) = field(name) {
            entity.insert(wire.into(), value);
        }
    }
    let mut payload = std::collections::BTreeMap::new();
    if let Some(campaign) = field("entityCampaignId") {
        payload.insert("campaign".to_owned(), campaign);
    }
    payload.insert("entity".to_owned(), serde_json::Value::Object(entity));
    create_entities_from_note::Command {
        target_id: command.target_id,
        payload,
    }
}

/// The warning the DataGuard gives a command whose state, as queued, breaks
/// an invariant — which it rejects once applied.
const AT_RISK: &str = "invariant(s) at risk";

/// Held while a validation reads the proposals and queues its commands.
static VALIDATING: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// The proposals a validation of `accepted` is made against, as the commands
/// queued will leave them: the accepted ones, and every other of their
/// notes, each with its note's campaign, whether the element it recognises is
/// one the engine still holds, and no element made when the engine holds
/// none of the one it records and did not delete it.
async fn held(
    rows: &Rows,
    accepted: &[String],
) -> Result<Vec<create_entities_from_note::Proposal>, Error> {
    let text = |row: &std::collections::BTreeMap<String, serde_json::Value>, field: &str| {
        row.get(field)
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned)
    };
    let mut ids: std::collections::BTreeSet<String> = accepted.iter().cloned().collect();
    for id in accepted {
        let row = rows.projected(&format!("NoteImport/{id}")).await?;
        if let Some(note) = text(&row, "noteId") {
            ids.extend(rows.proposals(&note));
        }
    }
    let mut campaigns: std::collections::BTreeMap<String, Option<String>> =
        std::collections::BTreeMap::new();
    let mut proposals = Vec::new();
    for id in ids {
        let mut row = rows.projected(&format!("NoteImport/{id}")).await?;
        // An element rejected once applied leaves its proposal recording an
        // element that never existed: the proposal is held as not made, and
        // a validation makes it again. One the DM deleted stays made.
        if let Some(element) = text(&row, "createdEntityId") {
            let partition = format!("Entity/{element}");
            if rows.projected(&partition).await?.is_empty() && !rows.deleted(&partition).await? {
                row.remove("createdEntityId");
            }
        }
        let campaign = match text(&row, "noteId") {
            Some(note) => match campaigns.get(&note) {
                Some(campaign) => campaign.clone(),
                None => {
                    let of = rows.projected(&format!("NoteImport/{note}")).await?;
                    let campaign = text(&of, "campaignId");
                    campaigns.insert(note, campaign.clone());
                    campaign
                }
            },
            None => None,
        };
        // An element deleted is no longer recognised: accepted, the proposal
        // makes a new one.
        let recognised = match text(&row, "existingEntityId") {
            Some(element) => !rows
                .projected(&format!("Entity/{element}"))
                .await?
                .is_empty(),
            None => false,
        };
        proposals.push(create_entities_from_note::Proposal {
            id,
            row,
            campaign,
            recognised,
        });
    }
    Ok(proposals)
}
