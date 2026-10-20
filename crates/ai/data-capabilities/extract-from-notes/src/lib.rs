//! DataCapability `ai.extractFromNotes` (`data-capability.json`): the DM's
//! free-text notes analysed into proposals, and nothing created.
//!
//! The notes are typed or pasted (`text`), or dictated (`dictation`, the
//! audio in base64): a dictation is transcribed locally through the
//! `speech-to-text` port, listening for the campaign's names and aliases,
//! and only its transcript goes further. [`analyse`] sends the notes — text,
//! never audio — to the `llm` port as a Notes request and reads the answer
//! the port documents, `{"proposals":[{"type","name","excerpt"}]}`:
//!
//! - a proposal's type is one of [`TYPES`] (« événement » is read
//!   `evenement`); a proposal of another type, or with no name, is dropped,
//!   and so is one proposing again a type and a name already proposed;
//! - a proposal naming an element the campaign already knows — its name or
//!   one of its aliases under `AliasMatch@1` ([`normalized`]) — carries that
//!   element's id and starts unchecked: a probable duplicate; any other
//!   starts checked.
//!
//! The engine writes one partition per command, and the `NoteImport`
//! aggregate has a row per imported note and a row per proposal:
//! [`Analysis::commands`] makes one command per proposal and one for the
//! note, which keeps the raw text and is stamped `analyzedAt`. The raw note
//! is always kept: an analysis that failed still writes it, with no
//! proposal and no `analyzedAt`, and says why. No command names an element
//! to create (`no-creation-before-validation`).

use std::collections::BTreeMap;
use std::fmt;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use ports_llm::{GenerateRequest, Llm, ProviderConfig, TextKind};
use ports_speech_to_text::{Dictation, SpeechToText, SpeechToTextError, Vocabulary};
use serde_json::Value;
use unicode_normalization::UnicodeNormalization;

/// This DataCapability's id, as `data-capability.json` names it.
pub const DATA_CAPABILITY: &str = "ai.extractFromNotes";

/// The types a proposal may have, as `note_proposal.type` takes them.
pub const TYPES: [&str; 4] = ["pj", "pnj", "lieu", "evenement"];

/// Whether `id` names this DataCapability, with or without its `@version`.
#[must_use]
pub fn names(id: &str) -> bool {
    id.split('@').next() == Some(DATA_CAPABILITY)
}

/// Why notes are not analysed: nothing is queued.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// `campaign` names no campaign by its uuid.
    NoCampaign,
    /// Neither a text nor a dictation holds a word.
    NoNotes,
    /// The dictation is not base64.
    Undecodable,
    /// The `speech-to-text` port could not transcribe the dictation.
    Unheard(SpeechToTextError),
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoCampaign => f.write_str("aucune campagne : `campaign` la nomme par son id"),
            Self::NoNotes => f.write_str("aucune note à analyser"),
            Self::Undecodable => f.write_str("la dictée n’est pas un son encodé en base64"),
            Self::Unheard(e) => write!(f, "la dictée n’a pas été transcrite : {e}"),
        }
    }
}

impl std::error::Error for Refusal {}

/// An element the campaign already holds: its id, its name and its aliases,
/// as written.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Known {
    /// The element's id.
    pub id: String,
    /// Its name.
    pub name: String,
    /// Its aliases.
    pub aliases: Vec<String>,
}

/// One element the analysis proposes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Proposal {
    /// One of [`TYPES`].
    pub kind: &'static str,
    /// Its name, as the notes write it.
    pub name: String,
    /// The words of the notes that justify it.
    pub excerpt: String,
    /// The element of the campaign it names already, when it does.
    pub existing: Option<String>,
}

impl Proposal {
    /// Whether it starts checked: unless it is a probable duplicate.
    #[must_use]
    pub const fn checked(&self) -> bool {
        self.existing.is_none()
    }
}

/// What the analysis of a campaign's notes found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Analysis {
    /// The campaign's id.
    pub campaign: String,
    /// The notes as text: what was typed or pasted, or the transcript.
    pub text: String,
    /// The proposals, in the order of the answer.
    pub proposals: Vec<Proposal>,
    /// Why the `llm` port gave no proposal, for the DM: the note is kept
    /// unanalysed.
    pub failure: Option<String>,
}

/// One command of the analysis: its partition's id and the row it writes.
#[derive(Clone, Debug, PartialEq)]
pub struct Command {
    /// The id of its `NoteImport/<id>` partition.
    pub target_id: String,
    /// The fields it writes.
    pub payload: BTreeMap<String, Value>,
}

/// The commands an analysis is submitted as.
#[derive(Clone, Debug, PartialEq)]
pub struct Commands {
    /// One per proposal, in its order.
    pub proposals: Vec<Command>,
    /// The note's.
    pub note: Command,
}

impl Analysis {
    /// The analysis as commands, `ids` being fresh uuids — one per proposal
    /// and one for the note — and `now` the time (RFC 3339) the note was
    /// imported and analysed. The note takes the greatest id: the engine's
    /// applier runs the heads of its partitions in the order of their ids,
    /// so the note, submitted after its proposals, is applied after them —
    /// each in a partition of its own, at its head, when it is queued. One
    /// that waits (a review) is applied when it may: the tables take a
    /// proposal whichever of it and its note comes first.
    ///
    /// # Panics
    ///
    /// When `ids` does not hold one id more than there are proposals.
    #[must_use]
    pub fn commands(&self, mut ids: Vec<String>, now: &str) -> Commands {
        assert_eq!(
            ids.len(),
            self.proposals.len() + 1,
            "one id per proposal and one for the note"
        );
        ids.sort();
        let note_id = ids.pop().unwrap_or_default();
        let proposals = self
            .proposals
            .iter()
            .zip(ids)
            .map(|(proposal, id)| Command {
                payload: row([
                    ("noteProposalId", id.clone().into()),
                    ("noteId", note_id.clone().into()),
                    ("type", proposal.kind.into()),
                    ("name", proposal.name.clone().into()),
                    ("excerpt", proposal.excerpt.clone().into()),
                    (
                        "existingEntityId",
                        proposal.existing.clone().map_or(Value::Null, Value::from),
                    ),
                    ("checked", proposal.checked().into()),
                ]),
                target_id: id,
            })
            .collect();
        let analyzed_at = if self.failure.is_some() {
            Value::Null
        } else {
            now.into()
        };
        let note = Command {
            payload: row([
                ("importedNoteId", note_id.clone().into()),
                ("campaignId", self.campaign.clone().into()),
                ("rawText", self.text.clone().into()),
                ("createdAt", now.into()),
                ("analyzedAt", analyzed_at),
            ]),
            target_id: note_id,
        };
        Commands { proposals, note }
    }
}

fn row<const N: usize>(fields: [(&str, Value); N]) -> BTreeMap<String, Value> {
    fields
        .into_iter()
        .map(|(field, value)| (field.to_owned(), value))
        .collect()
}

/// The notes `wire` carries for the campaign it names, analysed by `llm`
/// through `provider` — a dictation first transcribed by `speech`, listening
/// for the names and aliases of `known`, the elements the campaign holds.
///
/// # Errors
///
/// [`Refusal`] when `wire` names no campaign, holds no notes, or holds a
/// dictation that cannot be transcribed. A failure of the `llm` port is no
/// refusal: the analysis keeps the notes and says why in
/// [`Analysis::failure`].
pub async fn analyse<L: Llm, S: SpeechToText>(
    llm: &L,
    provider: &ProviderConfig,
    speech: &S,
    wire: &BTreeMap<String, Value>,
    known: &[Known],
) -> Result<Analysis, Refusal> {
    let campaign = wire
        .get("campaign")
        .and_then(Value::as_str)
        .filter(|id| uuid(id))
        .ok_or(Refusal::NoCampaign)?
        .to_owned();
    let typed = wire.get("text").and_then(Value::as_str).unwrap_or_default();
    let text = if typed.trim().is_empty() {
        transcribed(speech, wire, known).await?
    } else {
        typed.to_owned()
    };
    if text.trim().is_empty() {
        return Err(Refusal::NoNotes);
    }
    let request = GenerateRequest::new(TextKind::Notes, text.clone());
    let (proposals, failure) = match llm.generate(provider, &request).await {
        Ok(answer) => match proposed(&answer.text) {
            Some(found) => (found, None),
            None => (
                Vec::new(),
                Some("la réponse de l’IA n’est pas une liste de propositions".to_owned()),
            ),
        },
        Err(e) => (Vec::new(), Some(e.to_string())),
    };
    let proposals = recognised(proposals, known);
    let failure =
        failure.map(|why| format!("l’analyse des notes a échoué ({why}) : la note est gardée"));
    Ok(Analysis {
        campaign,
        text,
        proposals,
        failure,
    })
}

/// The transcript of `wire`'s dictation; `""` when it holds none.
async fn transcribed<S: SpeechToText>(
    speech: &S,
    wire: &BTreeMap<String, Value>,
    known: &[Known],
) -> Result<String, Refusal> {
    let Some(encoded) = wire.get("dictation").and_then(Value::as_str) else {
        return Ok(String::new());
    };
    if encoded.trim().is_empty() {
        return Ok(String::new());
    }
    let audio = STANDARD
        .decode(encoded.trim())
        .map_err(|_| Refusal::Undecodable)?;
    speech
        .transcribe(&Dictation::new(audio), &vocabulary(known))
        .await
        .map(|transcript| transcript.text)
        .map_err(Refusal::Unheard)
}

/// What a dictation listens for: the campaign's names and aliases, as
/// written.
#[must_use]
pub fn vocabulary(known: &[Known]) -> Vocabulary {
    Vocabulary {
        campaign_names: known.iter().map(|element| element.name.clone()).collect(),
        aliases: known
            .iter()
            .flat_map(|element| element.aliases.iter().cloned())
            .collect(),
        sound_names: Vec::new(),
    }
}

/// The proposals of a Notes answer, `None` when it is not the object the
/// port documents. Prose or a code fence around the object is skipped.
fn proposed(answer: &str) -> Option<Vec<Proposal>> {
    let object = answer.get(answer.find('{')?..=answer.rfind('}')?)?;
    let answer: Value = serde_json::from_str(object).ok()?;
    let found = answer.get("proposals")?.as_array()?;
    let text = |item: &Value, field: &str| {
        item.get(field)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim()
            .to_owned()
    };
    Some(
        found
            .iter()
            .filter_map(|item| {
                let kind = kind(&text(item, "type"))?;
                let name = text(item, "name");
                (!name.is_empty()).then(|| Proposal {
                    kind,
                    name,
                    excerpt: text(item, "excerpt"),
                    existing: None,
                })
            })
            .collect(),
    )
}

/// The type `raw` names, among [`TYPES`], read as `AliasMatch@1` reads a
/// term: « Événement » is `evenement`.
fn kind(raw: &str) -> Option<&'static str> {
    let raw = normalized(raw);
    TYPES.into_iter().find(|kind| *kind == raw)
}

/// `proposals` with each probable duplicate pointing at the element it
/// names, a type and a name proposed twice kept once.
fn recognised(proposals: Vec<Proposal>, known: &[Known]) -> Vec<Proposal> {
    let mut seen = Vec::new();
    proposals
        .into_iter()
        .filter_map(|proposal| {
            let name = normalized(&proposal.name);
            if seen.contains(&(proposal.kind, name.clone())) {
                return None;
            }
            let existing = known
                .iter()
                .find(|element| {
                    std::iter::once(&element.name)
                        .chain(&element.aliases)
                        .any(|term| normalized(term) == name)
                })
                .map(|element| element.id.clone());
            seen.push((proposal.kind, name));
            Some(Proposal {
                existing,
                ..proposal
            })
        })
        .collect()
}

/// A term normalised (AliasMatch@1): no white space around it, in lowercase,
/// with no accent — a combining mark U+0300 to U+036F of the decomposed
/// letter, the rest recomposed. Two terms match when their normalised terms
/// are equal and not empty.
#[must_use]
pub fn normalized(term: &str) -> String {
    term.trim()
        .to_lowercase()
        .nfd()
        .filter(|c| !('\u{300}'..='\u{36f}').contains(c))
        .nfc()
        .collect()
}

/// Whether `id` is a uuid, written 8-4-4-4-12 in hex digits.
fn uuid(id: &str) -> bool {
    id.len() == 36
        && id.char_indices().all(|(i, c)| match i {
            8 | 13 | 18 | 23 => c == '-',
            _ => c.is_ascii_hexdigit(),
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn proposal(kind: &'static str, name: &str, excerpt: &str) -> Proposal {
        Proposal {
            kind,
            name: name.to_owned(),
            excerpt: excerpt.to_owned(),
            existing: None,
        }
    }

    #[test]
    fn the_answer_is_read_through_prose_or_a_code_fence_around_it() {
        let answer = "Voici :\n```json\n{\"proposals\":[{\"type\":\"lieu\",\"name\":\" Le gué \",\"excerpt\":\"le péage illégal au gué\"}]}\n```";
        assert_eq!(
            proposed(answer),
            Some(vec![proposal("lieu", "Le gué", "le péage illégal au gué")])
        );
    }

    #[test]
    fn an_answer_that_is_no_list_of_proposals_is_none() {
        for answer in [
            "Réponse simulée",
            "} {",
            "{\"proposals\": 3}",
            "{\"autre\": []}",
            "{pas du json}",
        ] {
            assert_eq!(proposed(answer), None, "{answer}");
        }
    }

    #[test]
    fn a_type_is_read_as_a_term_and_one_outside_the_four_is_dropped() {
        let answer = r#"{"proposals":[
            {"type":"Événement","name":"L’incendie du moulin","excerpt":"le moulin a brûlé"},
            {"type":"PJ","name":"Elowen"},
            {"type":"objet","name":"La carte"},
            {"type":"pnj","name":"  "},
            {"name":"Sans type"}
        ]}"#;
        assert_eq!(
            proposed(answer),
            Some(vec![
                proposal("evenement", "L’incendie du moulin", "le moulin a brûlé"),
                proposal("pj", "Elowen", ""),
            ])
        );
    }

    #[test]
    fn a_type_and_a_name_proposed_twice_are_kept_once() {
        let proposals = vec![
            proposal("pnj", "Gaspard", "premier"),
            proposal("pnj", "GASPARD ", "second"),
            proposal("lieu", "Gaspard", "un lieu du même nom"),
        ];
        assert_eq!(
            recognised(proposals, &[]),
            vec![
                proposal("pnj", "Gaspard", "premier"),
                proposal("lieu", "Gaspard", "un lieu du même nom"),
            ]
        );
    }

    #[test]
    fn a_blank_alias_recognises_nothing() {
        let known = [Known {
            id: "e1".to_owned(),
            name: "Morlox".to_owned(),
            aliases: vec![" ".to_owned()],
        }];
        let found = recognised(vec![proposal("pnj", "Elowen", "")], &known);
        assert_eq!(found[0].existing, None);
        let found = recognised(vec![proposal("pnj", "morlox", "")], &known);
        assert_eq!(found[0].existing.as_deref(), Some("e1"));
    }
}
