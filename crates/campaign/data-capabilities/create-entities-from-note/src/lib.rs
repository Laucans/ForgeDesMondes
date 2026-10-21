//! DataCapability `campaign.createEntitiesFromNote` (`data-capability.json`):
//! the DM validates, in one click, the proposals an analysis of their notes
//! made — the characters, places and events they keep become elements of
//! the campaign.
//!
//! The engine writes one partition per command. A validation is the
//! proposals' commands, on their `NoteImport/<proposal id>` partitions,
//! which the NoteImport aggregate judges (`no-creation-before-validation`),
//! and the elements it creates, each the DM's save of an element
//! (`campaign.saveEntity`, the Entity aggregate's DataCapability) on its
//! `Entity/<element id>` partition, under whose touches the engine judges
//! every Entity invariant and which adds its first version. What the DM sends
//! is `{ campaign, accepted }`; [`Request::batch`] turns it into those
//! commands, against the proposals of the notes the accepted ones are of, as
//! the engine holds them:
//!
//! - an accepted proposal is checked, and becomes an element of its type,
//!   its name and its excerpt for description — unless it already created
//!   one (`createdEntityId`), or recognises an element the campaign still
//!   holds: no element is made twice. The proposal records the element it
//!   became;
//! - every other proposal of those notes is unchecked, unless it created its
//!   element already, which it never forgets;
//! - a validation naming no campaign or no proposal is refused before
//!   anything is queued ([`Refusal`]), as is one naming a proposal of no
//!   note of its campaign.
//!
//! The engine runs it through this crate's [`plugin`]: the last proposal's
//! command is the one submitted — its idempotency key, its result — and the
//! `Prepare` asks the engine for the others after it (`Prepared.then`): the
//! elements' saves first, then the other proposals'.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use aggregate_note_import::{Announced, Notes};
use async_trait::async_trait;
use harness_dataguard::api::Submission;
use harness_dataguard::plugins::{self, Context, Plugin, Prepare};
use serde_json::{Map, Value};

/// This DataCapability's id, as `data-capability.json` names it.
pub const DATA_CAPABILITY: &str = "campaign.createEntitiesFromNote";

/// The DataCapability's manifest (contract F), as the engine parses it.
pub const MANIFEST: &str = include_str!("../data-capability.json");

/// The DataCapability an element is saved through: the Entity aggregate's.
pub const SAVE_ENTITY: &str = "campaign.saveEntity";

/// The types a proposal may have.
const TYPES: [&str; 4] = ["pj", "pnj", "lieu", "evenement"];

/// The proposal's field naming its note.
const NOTE: &str = "noteId";

/// The proposal's field naming the element it created.
const CREATED: &str = "createdEntityId";

/// The proposal's field telling whether the DM kept it.
const CHECKED: &str = "checked";

/// One partition's row, as the engine holds it.
pub type Row = BTreeMap<String, Value>;

/// Why a validation is refused before anything is queued.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// The validation names no campaign.
    NoCampaign,
    /// `accepted` is no list of proposal ids, or an empty one.
    NothingAccepted,
    /// An accepted id names no proposal of a note of the campaign.
    Unknown(String),
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoCampaign => f.write_str("une validation nomme sa campagne"),
            Self::NothingAccepted => f.write_str("une validation nomme les propositions acceptées"),
            Self::Unknown(id) => write!(f, "aucune proposition {id} dans cette campagne"),
        }
    }
}

/// What the DM validated: the campaign, and the proposals kept, each once,
/// in lowercase, in the order given.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Request {
    /// The campaign the notes were imported into.
    pub campaign: String,
    /// The proposals the DM kept.
    pub accepted: Vec<String>,
}

/// A proposal as the engine holds it, with what the server knows of it.
#[derive(Clone, Debug, PartialEq)]
pub struct Proposal {
    /// Its id: the partition `NoteImport/<id>`.
    pub id: String,
    /// Its row.
    pub row: Row,
    /// The campaign its note is of, when the engine holds its note.
    pub campaign: Option<String>,
    /// Whether the element it recognises (`existingEntityId`) is one the
    /// campaign still holds.
    pub recognised: bool,
}

/// The id an element made is given.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fresh {
    /// The element's id.
    pub entity: String,
}

impl Fresh {
    /// The id the element `proposal` becomes is given: the same one every
    /// time, so a validation retried after a failure half-way finds the
    /// element it made and does not make it twice.
    #[must_use]
    pub fn of(proposal: &str) -> Self {
        Self {
            entity: derived_uuid("entity", proposal),
        }
    }
}

/// A uuid (version 5 layout) derived from `what` and the proposal's id.
fn derived_uuid(what: &str, proposal: &str) -> String {
    use sha2::{Digest, Sha256};
    let hash = Sha256::new()
        .chain_update(DATA_CAPABILITY)
        .chain_update([0])
        .chain_update(what)
        .chain_update([0])
        .chain_update(proposal)
        .finalize();
    let mut bytes = [0_u8; 16];
    bytes.copy_from_slice(&hash[..16]);
    // The version (5) and the variant (10), on the bits cleared for them.
    bytes[6] = 0x50 + (bytes[6] & 0x0f);
    bytes[8] = 0x80 + (bytes[8] & 0x3f);
    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &hex[..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..]
    )
}

/// One command: the partition's id and what it writes there.
#[derive(Clone, Debug, PartialEq)]
pub struct Command {
    /// `NoteImport/<target_id>` for a proposal, `Entity/<target_id>` for an
    /// element.
    pub target_id: String,
    /// A proposal's: the fields it writes, by the aggregate's names. An
    /// element's: its save as the DM's editor sends it, which
    /// `campaign.saveEntity` maps.
    pub payload: Row,
}

/// An element a validation makes, and the proposal it is made of.
#[derive(Clone, Debug, PartialEq)]
pub struct Element {
    /// The proposal it is made of.
    pub proposal: String,
    /// Its command, on its Entity partition.
    pub command: Command,
}

/// What a validation submits.
#[derive(Clone, Debug, PartialEq)]
pub struct Batch {
    /// The elements made, in the order accepted.
    pub elements: Vec<Element>,
    /// The proposals checked or unchecked, by id: the last is the one
    /// applied last, its result the validation's.
    pub stamps: Vec<Command>,
}

/// Whether `id` names this DataCapability, with or without its `@version`.
#[must_use]
pub fn names(id: &str) -> bool {
    id.split('@').next() == Some(DATA_CAPABILITY)
}

/// What the DataCapability plugs into the engine: its `Prepare`, which
/// reads the proposals of each note the NoteImport aggregate keeps.
#[must_use]
pub fn plugin() -> Plugin {
    Plugin::new()
        .prepare(
            DATA_CAPABILITY,
            Validate {
                announced: Arc::default(),
                validating: Arc::default(),
            },
        )
        .needs::<Notes>()
}

/// A validation made the commands it submits ([`Request::batch`]), against
/// the proposals as the queue leaves them ([`held`]): the last proposal's
/// command is the one submitted, the elements' saves and the other
/// proposals' its follow-ups, in that order — each follow-up of this
/// DataCapability written as it was announced ([`Announced`]). A validation
/// naming no campaign or no proposal is refused, 422, and one naming a
/// proposal of no note of its campaign, 404: neither is queued.
///
/// Validations are prepared one at a time, each against the proposals the
/// one before queued. Its follow-ups are queued once it is, outside that
/// order: one prepared meanwhile may save an element again — under the same
/// id ([`Fresh::of`]), so never a second element.
struct Validate {
    announced: Arc<Announced>,
    validating: Arc<tokio::sync::Mutex<()>>,
}

/// Why a validation is refused when the NoteImport aggregate shares no
/// proposals: the engine refuses to load first, so a server never answers
/// it.
const NO_NOTES: &str = "les propositions des notes ne sont pas partagées";

#[async_trait]
impl Prepare for Validate {
    async fn prepare(
        &self,
        context: &Context<'_>,
        payload: &Row,
        target_id: Option<&str>,
    ) -> Result<plugins::Prepared, plugins::Refusal> {
        if let Some(stamp) = self.announced.take(target_id, payload) {
            return Ok(plugins::Prepared {
                payload: stamp,
                target_id: target_id.map(str::to_owned),
                ..plugins::Prepared::default()
            });
        }
        let _validating = self.validating.lock().await;
        let request = request(payload)
            .map_err(|refusal| plugins::Refusal::unprocessable(refusal.to_string()))?;
        let notes = context
            .shared
            .get::<Notes>()
            .ok_or_else(|| plugins::Refusal::unprocessable(NO_NOTES))?;
        let proposals = held(context, &notes, &request.accepted).await?;
        let fresh = request.accepted.iter().map(|id| Fresh::of(id)).collect();
        let Batch {
            elements,
            mut stamps,
        } = request
            .batch(&proposals, fresh)
            .map_err(|refusal| plugins::Refusal::not_found(refusal.to_string()))?;
        // An accepted proposal is always stamped: there is a last.
        let last = stamps
            .pop()
            .ok_or_else(|| plugins::Refusal::unprocessable(Refusal::NothingAccepted.to_string()))?;
        let mut saves = Vec::new();
        for Element { command, .. } in elements {
            // Queued by a validation that failed before it stamped its
            // proposal, or made, then deleted by the DM: never made again,
            // still recorded.
            let partition = format!("Entity/{}", command.target_id);
            if !context.row(&partition).await?.is_empty() || deleted(context, &partition).await? {
                continue;
            }
            saves.push(Submission {
                data_capability: SAVE_ENTITY.to_owned(),
                payload: command.payload,
                target_id: Some(command.target_id),
                ..Submission::default()
            });
        }
        let others = stamps.into_iter().map(|stamp| {
            self.announced
                .follow_up(DATA_CAPABILITY, &stamp.target_id, stamp.payload)
        });
        Ok(plugins::Prepared {
            payload: last.payload,
            target_id: Some(last.target_id),
            then: saves.into_iter().chain(others).collect(),
            ..plugins::Prepared::default()
        })
    }
}

/// `field` of `row` as text, when it is.
fn text(row: &Row, field: &str) -> Option<String> {
    row.get(field).and_then(Value::as_str).map(str::to_owned)
}

/// The proposals a validation of `accepted` is made against, as the queue
/// leaves them: the accepted ones, and every other of their notes that the
/// engine applied (`notes`), each with its note's campaign, whether the
/// element it recognises is one the engine still holds, and no element made
/// when the engine holds none of the one it records and did not delete it.
async fn held(
    context: &Context<'_>,
    notes: &Notes,
    accepted: &[String],
) -> Result<Vec<Proposal>, plugins::Refusal> {
    let mut ids: BTreeSet<String> = accepted.iter().cloned().collect();
    for id in accepted {
        let row = context.row(&format!("NoteImport/{id}")).await?;
        if let Some(note) = text(&row, NOTE) {
            ids.extend(notes.of(&note));
        }
    }
    let mut campaigns: BTreeMap<String, Option<String>> = BTreeMap::new();
    let mut proposals = Vec::new();
    for id in ids {
        let mut row = context.row(&format!("NoteImport/{id}")).await?;
        // An element rejected once applied leaves its proposal recording an
        // element that never existed: the proposal is held as not made, and
        // a validation makes it again. One the DM deleted stays made.
        if let Some(element) = text(&row, CREATED) {
            let partition = format!("Entity/{element}");
            if context.row(&partition).await?.is_empty() && !deleted(context, &partition).await? {
                row.remove(CREATED);
            }
        }
        let campaign = match text(&row, NOTE) {
            Some(note) => match campaigns.get(&note) {
                Some(campaign) => campaign.clone(),
                None => {
                    let of = context.row(&format!("NoteImport/{note}")).await?;
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
            Some(element) => !context.row(&format!("Entity/{element}")).await?.is_empty(),
            None => false,
        };
        proposals.push(Proposal {
            id,
            row,
            campaign,
            recognised,
        });
    }
    Ok(proposals)
}

/// Whether the engine deleted `partition`'s row — its tombstone — which it
/// reads as empty, as one never written: over its tables. Over its memory
/// none is: its rows keep a deleted row's values.
async fn deleted(context: &Context<'_>, partition: &str) -> Result<bool, plugins::Refusal> {
    let Some(rows) = context.rows else {
        return Ok(false);
    };
    let found = rows
        .rows(TOMBSTONED, &[Value::from(partition)])
        .await
        .map_err(|e| plugins::Refusal::unprocessable(e.to_string()))?;
    Ok(found.as_array().is_some_and(|found| !found.is_empty()))
}

/// The engine's tombstone of the partition `$1` names, when it holds one.
const TOMBSTONED: &str =
    "SELECT partition FROM dataguard_rows WHERE partition = $1::text AND tombstoned";

/// A string that holds more than blanks.
fn filled(value: &Value) -> Option<&str> {
    value.as_str().filter(|text| !text.trim().is_empty())
}

/// What the DM's validation `wire` asks.
///
/// # Errors
///
/// [`Refusal::NoCampaign`] when it names no campaign,
/// [`Refusal::NothingAccepted`] when `accepted` is no list of ids or an empty
/// one.
pub fn request(wire: &Row) -> Result<Request, Refusal> {
    let campaign = wire
        .get("campaign")
        .and_then(filled)
        .ok_or(Refusal::NoCampaign)?;
    let given = wire
        .get("accepted")
        .and_then(Value::as_array)
        .filter(|given| !given.is_empty())
        .ok_or(Refusal::NothingAccepted)?;
    let mut accepted: Vec<String> = Vec::new();
    for id in given {
        // A proposal's id is a uuid, written in lowercase.
        let id = filled(id)
            .ok_or(Refusal::NothingAccepted)?
            .to_ascii_lowercase();
        if !accepted.contains(&id) {
            accepted.push(id);
        }
    }
    Ok(Request {
        campaign: campaign.to_owned(),
        accepted,
    })
}

/// Whether `a` and `b` name the same row: the same id, whatever its case.
fn same_id(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b)
}

/// `field`'s value in `row`, an absent field and a null being the same.
fn value<'a>(row: &'a Row, field: &str) -> &'a Value {
    row.get(field).unwrap_or(&Value::Null)
}

impl Request {
    /// Whether `proposal` is one of a note of this campaign, of a type an
    /// element has and with a name.
    fn owns(&self, proposal: &Proposal) -> bool {
        proposal
            .campaign
            .as_deref()
            .is_some_and(|campaign| same_id(campaign, &self.campaign))
            && value(&proposal.row, NOTE).is_string()
            && value(&proposal.row, "type")
                .as_str()
                .is_some_and(|kind| TYPES.contains(&kind))
            && filled(value(&proposal.row, "name")).is_some()
    }

    /// The commands this validation submits, `proposals` being every
    /// proposal of the notes the accepted ones are of — those included — and
    /// `fresh` the id of the element each accepted proposal may become, in
    /// the order accepted.
    ///
    /// # Errors
    ///
    /// [`Refusal::Unknown`] when an accepted id names no proposal of a note
    /// of this campaign.
    ///
    /// # Panics
    ///
    /// When `fresh` does not hold one id per accepted proposal.
    pub fn batch(&self, proposals: &[Proposal], fresh: Vec<Fresh>) -> Result<Batch, Refusal> {
        assert_eq!(
            fresh.len(),
            self.accepted.len(),
            "one id per accepted proposal"
        );
        let mut elements = Vec::new();
        let mut stamps = BTreeMap::new();
        for (id, fresh) in self.accepted.iter().zip(fresh) {
            let proposal = proposals
                .iter()
                .find(|proposal| &proposal.id == id && self.owns(proposal))
                .ok_or_else(|| Refusal::Unknown(id.clone()))?;
            let mut stamp = stamp_of(proposal, true);
            if value(&proposal.row, CREATED).is_null() && !proposal.recognised {
                // Of the campaign its note is of, as the engine writes it.
                let campaign = proposal.campaign.as_deref().unwrap_or(&self.campaign);
                let command = element(proposal, campaign, &fresh);
                stamp.insert(CREATED.into(), fresh.entity.into());
                elements.push(Element {
                    proposal: id.clone(),
                    command,
                });
            }
            stamps.insert(id.clone(), stamp);
        }
        let accepted: BTreeSet<&String> = self.accepted.iter().collect();
        for proposal in proposals {
            let unchecked = !accepted.contains(&proposal.id)
                && self.owns(proposal)
                && value(&proposal.row, CREATED).is_null()
                && value(&proposal.row, CHECKED) != &Value::Bool(false);
            if unchecked {
                stamps.insert(proposal.id.clone(), stamp_of(proposal, false));
            }
        }
        Ok(Batch {
            elements,
            stamps: stamps
                .into_iter()
                .map(|(target_id, payload)| Command { target_id, payload })
                .collect(),
        })
    }
}

/// A proposal's command checking it, or unchecking it: the note it is of,
/// which its store projects it on, unchanged.
fn stamp_of(proposal: &Proposal, checked: bool) -> Row {
    Row::from([
        (NOTE.to_owned(), value(&proposal.row, NOTE).clone()),
        (CHECKED.to_owned(), checked.into()),
    ])
}

/// The element `proposal` becomes in `campaign`: its save as the DM's
/// editor sends it — its campaign, its id, its type, its name and its
/// excerpt for description — which `campaign.saveEntity` makes an element
/// with its first version, a PC or an NPC with its sheet and its token
/// colour.
fn element(proposal: &Proposal, campaign: &str, fresh: &Fresh) -> Command {
    let mut entity = Map::new();
    entity.insert("id".into(), fresh.entity.clone().into());
    entity.insert("kind".into(), value(&proposal.row, "type").clone());
    entity.insert("name".into(), value(&proposal.row, "name").clone());
    if let Some(excerpt) = filled(value(&proposal.row, "excerpt")) {
        entity.insert("description".into(), excerpt.trim().into());
    }
    Command {
        target_id: fresh.entity.clone(),
        payload: Row::from([
            ("campaign".to_owned(), campaign.into()),
            ("entity".to_owned(), Value::Object(entity)),
        ]),
    }
}
