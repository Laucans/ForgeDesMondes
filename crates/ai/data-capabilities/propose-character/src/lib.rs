//! DataCapability `ai.proposeCharacter` (`data-capability.json`): from the
//! initial prompt of a PC or an NPC, the active provider proposes its
//! background, its behaviour and, for an NPC, its statblock, which fill the
//! sheet at once and are kept as a version of source `ia` — the DM sees the
//! diff and undoes it as any other version. A PC's statblock is its own — its
//! player's, or D&D Beyond's — and a proposal leaves it as it is.
//!
//! The prompt is typed (`prompt`), or dictated (`audio`, the sound in
//! base64): a dictation is transcribed locally through the `speech-to-text`
//! port, listening for the element's name and aliases, and only its
//! transcript goes further. With neither, the sheet's own initial prompt is
//! used. [`propose`] sends the prompt — text, never audio — to the `llm` port
//! as a Prompt request ([`request`]), and reads the one JSON object the
//! request asks for ([`proposed`]):
//!
//! ```json
//! {"background":"…","behavior":"…",
//!  "stats":{"FOR":10,"DEX":14,"CON":12,"INT":8,"SAG":13,"CHA":11},
//!  "ca":15,"pvMax":27,"actions":[{"name":"Rapière","description":"…"}]}
//! ```
//!
//! The answer is read whatever the provider: the scores under `stats` by the
//! sheet's abbreviations or the SRD's (`STR`, `WIS`…), or by the aggregate's
//! names; `armorClass` for `ca`, `hpMax` for `pvMax`, `features` for
//! `actions`, an action's `text` for its `description`. A proposal is written
//! only when it fills every field the sheet takes from it
//! (`proposal-fills-sheet-fields`): a background, a behaviour and, for an
//! NPC, a statblock StatBlock@1 holds complete ([`statblock`]) — its spells
//! excepted, which are the sheet's and which a proposal never changes. Any other answer is
//! refused before anything is queued, and nothing is written. A DEX modifier
//! answered is never read (`dex-modifier-derived`).
//!
//! The engine runs [`plugin`]'s `Prepare`, on the element's row as the queue
//! leaves it: [`prepare`] makes the command — the prompt, the proposal, and
//! the version it adds (EntityVersion@1), numbered, snapshotted and diffed
//! after the row's — which the Entity aggregate judges, projects into its
//! tables and settles again after the element's last version.

use std::collections::BTreeMap;
use std::fmt;

use aggregate_entity::{Kept, settled};
use async_trait::async_trait;
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use harness_dataguard::plugins::{self as engine, Context, Plugin};
use ports_llm::{GenerateRequest, Llm, ProviderConfig, SharedLlm, TextKind};
use ports_speech_to_text::{
    Dictation, SharedSpeechToText, SpeechToText, SpeechToTextError, Vocabulary,
};
use serde_json::{Map, Value};

/// This DataCapability's id, as `data-capability.json` names it.
pub const DATA_CAPABILITY: &str = "ai.proposeCharacter";

/// This DataCapability's manifest (contract F), as the engine parses it.
pub const MANIFEST: &str = include_str!("../data-capability.json");

/// Where a proposal comes from (EntityVersion@1).
pub const SOURCE: &str = "ia";

/// The invariant a proposal that does not fill the sheet breaks.
pub const PROPOSAL_FILLS_SHEET_FIELDS: &str = "proposal-fills-sheet-fields";

/// The types of element that have a character sheet.
const CHARACTERS: [&str; 2] = ["pj", "pnj"];

/// The six scores: the sheet's abbreviation, the SRD's, and the aggregate's
/// field.
const ABILITIES: [(&str, &str, &str); 6] = [
    ("FOR", "STR", "strength"),
    ("DEX", "DEX", "dexterity"),
    ("CON", "CON", "constitution"),
    ("INT", "INT", "intelligence"),
    ("SAG", "WIS", "wisdom"),
    ("CHA", "CHA", "charisma"),
];

/// StatBlock@1's ten fields, in the order it names the unsound ones.
const STATBLOCK: [&str; 10] = [
    "strength",
    "dexterity",
    "constitution",
    "intelligence",
    "wisdom",
    "charisma",
    "armorClass",
    "hpMax",
    "actions",
    "spells",
];

/// The statblock's field a proposal leaves to the sheet.
const SPELLS: &str = "spells";

/// The largest AC or HP max the tables hold (a `smallint`).
const SMALLINT: i64 = 32_767;

/// One partition's row, as the engine holds it.
pub type Row = BTreeMap<String, Value>;

/// What a provider proposed, as the sheet takes it.
#[derive(Clone, Debug, PartialEq)]
pub struct Proposal {
    /// The character's background.
    pub background: String,
    /// How to play it.
    pub behavior: String,
    /// An NPC's statblock: the six scores, `armorClass`, `hpMax` and
    /// `actions` (each `{name, description}`), under the aggregate's names;
    /// empty for a PC.
    pub statblock: Row,
}

/// Why a proposal is refused: nothing is queued, nothing written.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// No PC or NPC the engine holds is named.
    NoElement,
    /// The element is a place or an event: it has no sheet.
    NotACharacter,
    /// Neither a typed prompt, a dictation nor the sheet holds a word.
    NoPrompt,
    /// The dictation is not base64.
    Undecodable,
    /// The `speech-to-text` port could not transcribe the dictation.
    Unheard(SpeechToTextError),
    /// The `llm` port gave no answer.
    Unanswered(String),
    /// The answer leaves these fields of the sheet unfilled or unsound
    /// (`proposal-fills-sheet-fields`, StatBlock@1), in the order of the
    /// sheet.
    Incomplete(Vec<&'static str>),
    /// A value the tables could never hold: an AC or HP max beyond a
    /// `smallint`, or the character U+0000.
    Unwritable(&'static str),
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoElement => f.write_str("aucun PJ ni PNJ de ce nom"),
            Self::NotACharacter => f.write_str("seul un PJ ou un PNJ a une fiche à proposer"),
            Self::NoPrompt => f.write_str("aucun prompt initial"),
            Self::Undecodable => f.write_str("la dictée n’est pas un son encodé en base64"),
            Self::Unheard(e) => write!(f, "la dictée n’a pas été transcrite : {e}"),
            Self::Unanswered(why) => write!(f, "le fournisseur n’a pas répondu : {why}"),
            Self::Incomplete(fields) => write!(
                f,
                "{PROPOSAL_FILLS_SHEET_FIELDS} : la proposition ne remplit pas {}",
                fields.join(", ")
            ),
            Self::Unwritable(field) => write!(f, "{field} ne tient pas dans la fiche"),
        }
    }
}

impl std::error::Error for Refusal {}

/// What a proposal is given besides the row and the answer: the new
/// version's id, and the time.
#[derive(Clone, Copy, Debug)]
pub struct Fresh<'a> {
    /// The id of the version the proposal adds.
    pub version: &'a str,
    /// Now, RFC 3339.
    pub now: &'a str,
}

/// What the DataCapability plugs into the engine: its `Prepare`, which
/// reaches the `llm` and `speech-to-text` ports the server shares.
#[must_use]
pub fn plugin() -> Plugin {
    Plugin::new()
        .prepare(DATA_CAPABILITY, Propose)
        .needs::<SharedLlm>()
        .needs::<SharedSpeechToText>()
}

/// A proposal made what it writes, before anything is queued: on the element
/// the command targets — else the one `entityId` names — its row as the
/// queue leaves it, the provider the AI settings make active, a fresh
/// version id and now. An element the engine does not hold is refused as
/// not found, any other refusal as unprocessable.
struct Propose;

/// Why a proposal is refused when a port is not shared: the engine refuses
/// to load first, so a server never answers it.
const NO_PORT: &str = "le port llm ou speech-to-text n'est pas branché";

#[async_trait]
impl engine::Prepare for Propose {
    async fn prepare(
        &self,
        context: &Context<'_>,
        payload: &Row,
        target_id: Option<&str>,
    ) -> Result<engine::Prepared, engine::Refusal> {
        let id = target_id
            .map(str::to_owned)
            .or_else(|| text(payload, "entityId").map(str::to_owned))
            .unwrap_or_default();
        let base = context.row(&format!("Entity/{id}")).await?;
        let settings = context.row(ai_provider::SETTINGS).await?;
        let (Some(llm), Some(speech)) = (
            context.shared.get::<SharedLlm>(),
            context.shared.get::<SharedSpeechToText>(),
        ) else {
            return Err(engine::Refusal::unprocessable(NO_PORT));
        };
        let refused = |refusal: Refusal| match refusal {
            Refusal::NoElement => engine::Refusal::not_found(refusal.to_string()),
            _ => engine::Refusal::unprocessable(refusal.to_string()),
        };
        let (prompt, proposal) = propose(
            llm.as_ref(),
            &ai_provider::active(&settings),
            speech.as_ref(),
            payload,
            &base,
        )
        .await
        .map_err(refused)?;
        let version = context.ids.fresh();
        let now = aggregate_entity::timestamp(context.now_millis);
        let fresh = Fresh {
            version: &version,
            now: &now,
        };
        Ok(engine::Prepared {
            payload: prepare(&base, &prompt, &proposal, fresh),
            target_id: Some(id),
            ..engine::Prepared::default()
        })
    }
}

/// `field` of `row`, when it is a string.
fn text<'a>(row: &'a Row, field: &str) -> Option<&'a str> {
    row.get(field).and_then(Value::as_str)
}

/// The proposal `llm` makes through `provider` for the element whose row is
/// `base`, from the prompt `wire` carries — a dictation first transcribed by
/// `speech` — else from the sheet's: the prompt used, and the proposal.
///
/// # Errors
///
/// [`Refusal`] when `base` is no PC or NPC, when there is no prompt or the
/// dictation cannot be transcribed, when the `llm` port fails, and when its
/// answer does not fill the sheet.
pub async fn propose<L: Llm, S: SpeechToText>(
    llm: &L,
    provider: &ProviderConfig,
    speech: &S,
    wire: &Row,
    base: &Row,
) -> Result<(String, Proposal), Refusal> {
    let kind = text(base, "type").ok_or(Refusal::NoElement)?;
    if !CHARACTERS.contains(&kind) {
        return Err(Refusal::NotACharacter);
    }
    let prompt = prompt(speech, wire, base).await?;
    if prompt.contains('\0') {
        return Err(Refusal::Unwritable("initialPrompt"));
    }
    let name = text(base, "name").unwrap_or_default();
    let answer = llm
        .generate(provider, &request(kind, name, &prompt))
        .await
        .map_err(|e| Refusal::Unanswered(e.to_string()))?;
    Ok((prompt, proposed(&answer.text, kind)?))
}

/// The prompt: the one typed, else the dictation's transcript, else the
/// sheet's own.
async fn prompt<S: SpeechToText>(speech: &S, wire: &Row, base: &Row) -> Result<String, Refusal> {
    let filled = |prompt: Option<&str>| {
        prompt
            .map(str::trim)
            .filter(|prompt| !prompt.is_empty())
            .map(str::to_owned)
    };
    if let Some(typed) = filled(text(wire, "prompt")) {
        return Ok(typed);
    }
    if let Some(encoded) = filled(text(wire, "audio")) {
        let audio = STANDARD
            .decode(encoded.trim())
            .map_err(|_| Refusal::Undecodable)?;
        let heard = speech
            .transcribe(&Dictation::new(audio), &vocabulary(base))
            .await
            .map_err(Refusal::Unheard)?;
        return filled(Some(&heard.text)).ok_or(Refusal::NoPrompt);
    }
    filled(text(base, "initialPrompt")).ok_or(Refusal::NoPrompt)
}

/// What a dictation listens for: the element's name and its aliases.
#[must_use]
pub fn vocabulary(base: &Row) -> Vocabulary {
    Vocabulary {
        campaign_names: text(base, "name").map(str::to_owned).into_iter().collect(),
        aliases: base
            .get("alias")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect(),
        sound_names: Vec::new(),
    }
}

/// The request a proposal sends: what to propose, for a PC (`pj`) or an NPC
/// named `name`, the shape of the answer, then the initial prompt — text
/// alone.
#[must_use]
pub fn request(kind: &str, name: &str, prompt: &str) -> GenerateRequest {
    if kind == "pj" {
        return GenerateRequest::new(
            TextKind::Prompt,
            format!(
                "Propose pour le PJ « {name} » (D&D 2024, SRD 5.2) un background qui suit son \
                 prompt initial et un comportement pour le jouer. Réponds par un seul objet \
                 JSON, sans texte autour : {{\"background\":\"…\",\"behavior\":\"…\"}}\
                 \n\nPrompt initial : {prompt}"
            ),
        );
    }
    GenerateRequest::new(
        TextKind::Prompt,
        format!(
            "Propose pour le PNJ « {name} » (D&D 2024, SRD 5.2) un background qui suit \
             son prompt initial, un comportement pour le jouer et un statblock jouable tel quel. \
             Réponds par un seul objet JSON, sans texte autour : \
             {{\"background\":\"…\",\"behavior\":\"…\",\
             \"stats\":{{\"FOR\":10,\"DEX\":10,\"CON\":10,\"INT\":10,\"SAG\":10,\"CHA\":10}},\
             \"ca\":10,\"pvMax\":10,\"actions\":[{{\"name\":\"…\",\"description\":\"…\"}}]}} — \
             chaque caractéristique un entier de 1 à 30, la CA et les PV max des entiers \
             positifs, au moins une action.\n\nPrompt initial : {prompt}"
        ),
    )
}

/// The proposal a provider's answer holds for a PC (`pj`) or an NPC of
/// `kind`: one JSON object, prose or a code fence around it skipped. A PC's
/// takes its background and its behaviour alone.
///
/// # Errors
///
/// [`Refusal::Incomplete`] naming every field of the sheet the answer leaves
/// unfilled or unsound — all of them when it holds no object —
/// [`Refusal::Unwritable`] for a value the tables could never hold.
pub fn proposed(answer: &str, kind: &str) -> Result<Proposal, Refusal> {
    let npc = kind != "pj";
    let found = object(answer);
    let empty = Map::new();
    let answer = found.as_ref().unwrap_or(&empty);
    let prose = |field: &str| {
        answer
            .get(field)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map(str::to_owned)
    };
    let background = prose("background");
    let behavior = prose("behavior");
    let statblock = if npc {
        statblock_of(answer)
    } else {
        Row::new()
    };
    let mut incomplete: Vec<&'static str> = Vec::new();
    if background.is_none() {
        incomplete.push("background");
    }
    if behavior.is_none() {
        incomplete.push("behavior");
    }
    if npc {
        let fields: Map<String, Value> = statblock
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        incomplete.extend(
            self::statblock(&fields)
                .into_iter()
                .filter(|field| *field != SPELLS),
        );
    }
    if !incomplete.is_empty() {
        return Err(Refusal::Incomplete(incomplete));
    }
    for field in ["armorClass", "hpMax"] {
        if statblock
            .get(field)
            .is_some_and(|n| n.as_i64().is_none_or(|n| n > SMALLINT))
        {
            return Err(Refusal::Unwritable(field));
        }
    }
    let proposal = Proposal {
        background: background.unwrap_or_default(),
        behavior: behavior.unwrap_or_default(),
        statblock,
    };
    let texts = [&proposal.background, &proposal.behavior];
    let actions = proposal.statblock.get("actions").unwrap_or(&Value::Null);
    if texts.iter().any(|text| text.contains('\0')) || nul(actions) {
        return Err(Refusal::Unwritable("background, behavior ou actions"));
    }
    Ok(proposal)
}

/// The first JSON object `answer` holds, read from one of its `{` — prose,
/// a code fence or braces around it skipped.
fn object(answer: &str) -> Option<Map<String, Value>> {
    answer.match_indices('{').find_map(|(start, _)| {
        let mut read = serde_json::Deserializer::from_str(&answer[start..]).into_iter::<Value>();
        match read.next() {
            Some(Ok(Value::Object(object))) => Some(object),
            _ => None,
        }
    })
}

/// `value` with a number written with a fraction of zero (`15.0`) written as
/// the integer it is: providers differ in how they write one.
fn integral(value: &Value) -> Value {
    match value.as_f64() {
        // A cast saturates: a number beyond `i64` is refused as any too large.
        Some(number) if value.as_i64().is_none() && number.fract() == 0.0 => {
            Value::from(number as i64)
        }
        _ => value.clone(),
    }
}

/// Whether `value` holds the character U+0000, which neither `text` nor
/// `jsonb` can hold.
fn nul(value: &Value) -> bool {
    match value {
        Value::String(text) => text.contains('\0'),
        Value::Array(values) => values.iter().any(nul),
        Value::Object(fields) => fields.iter().any(|(k, v)| k.contains('\0') || nul(v)),
        _ => false,
    }
}

/// The statblock an answer gives, under the aggregate's names: the scores
/// under `stats` by either abbreviation, or by their field; `ca` or
/// `armorClass`; `pvMax` or `hpMax`; `actions` or `features`, each as
/// `{name, description}`. A field the answer does not give is absent.
fn statblock_of(answer: &Map<String, Value>) -> Row {
    let stats = answer.get("stats").and_then(Value::as_object);
    let mut statblock = Row::new();
    for (sheet, srd, field) in ABILITIES {
        let score = stats
            .and_then(|stats| stats.get(sheet).or_else(|| stats.get(srd)))
            .or_else(|| answer.get(field));
        if let Some(score) = score {
            statblock.insert(field.into(), integral(score));
        }
    }
    for (field, names) in [
        ("armorClass", ["ca", "armorClass"]),
        ("hpMax", ["pvMax", "hpMax"]),
    ] {
        if let Some(value) = names.iter().find_map(|name| answer.get(*name)) {
            statblock.insert(field.into(), integral(value));
        }
    }
    if let Some(actions) = answer.get("actions").or_else(|| answer.get("features")) {
        let actions = match actions {
            Value::Array(entries) => Value::Array(entries.iter().map(action).collect()),
            other => other.clone(),
        };
        statblock.insert("actions".into(), actions);
    }
    statblock
}

/// An action as the sheet keeps it, `{name, description}` — its `text` read
/// as its description; anything but an object kept as it is, for StatBlock@1
/// to judge.
fn action(entry: &Value) -> Value {
    let Some(fields) = entry.as_object() else {
        return entry.clone();
    };
    let mut kept = Map::new();
    if let Some(name) = fields.get("name") {
        kept.insert("name".into(), name.clone());
    }
    if let Some(description) = fields.get("description").or_else(|| fields.get("text")) {
        kept.insert("description".into(), description.clone());
    }
    Value::Object(kept)
}

/// StatBlock@1: the fields of `fields` a complete statblock lacks — absent,
/// null, of another type or out of range — once each, in the Concept's
/// order; empty when it is complete. A field other than the ten is ignored.
#[must_use]
pub fn statblock(fields: &Map<String, Value>) -> Vec<&'static str> {
    STATBLOCK
        .into_iter()
        .filter(|field| {
            let value = fields.get(*field).unwrap_or(&Value::Null);
            !match *field {
                "armorClass" | "hpMax" => value.as_i64().is_some_and(|n| n > 0),
                "actions" => entries(value).is_some_and(|n| n > 0),
                "spells" => entries(value).is_some(),
                _ => value.as_i64().is_some_and(|n| (1..=30).contains(&n)),
            }
        })
        .collect()
}

/// How many entries `value` holds when it is a list of sound
/// `{name, description}`: both strings, neither blank once trimmed.
fn entries(value: &Value) -> Option<usize> {
    let sound = |entry: &Value| {
        ["name", "description"].iter().all(|key| {
            entry
                .get(key)
                .and_then(Value::as_str)
                .is_some_and(|text| !text.trim().is_empty())
        })
    };
    let list = value.as_array()?;
    list.iter().all(sound).then_some(list.len())
}

/// The command a proposal submits on the element whose row is `base` — as
/// the commands queued ahead of it will leave it: the prompt it was made
/// from, the proposal, and the version it adds after the row's, of source
/// `ia`.
#[must_use]
pub fn prepare(base: &Row, prompt: &str, proposal: &Proposal, fresh: Fresh<'_>) -> Row {
    let mut payload: Row = proposal.statblock.clone();
    for (field, value) in [
        ("initialPrompt", Value::from(prompt)),
        ("background", proposal.background.clone().into()),
        ("behavior", proposal.behavior.clone().into()),
        ("updatedAt", fresh.now.into()),
        (
            "entityVersionEntityId",
            base.get("entityId").cloned().unwrap_or(Value::Null),
        ),
        ("entityVersionId", fresh.version.into()),
        ("entityVersionCreatedAt", fresh.now.into()),
        ("source", SOURCE.into()),
        // A proposal restores nothing, whatever the last version did.
        ("restoredFromVersionId", Value::Null),
    ] {
        payload.insert(field.into(), value);
    }
    let mut after = base.clone();
    after.extend(payload.iter().map(|(k, v)| (k.clone(), v.clone())));
    settled(last_version(base).as_ref(), &after)
        .expect("the row after carries the new version's id")
        .stamped(&payload)
}

/// The version `base` carries: the element's last, as the queue leaves it —
/// an empty snapshot when it holds none.
fn last_version(base: &Row) -> Option<Kept> {
    Some(Kept {
        id: text(base, "entityVersionId").unwrap_or_default().to_owned(),
        number: base.get("versionNumber")?.as_u64()?,
        snapshot: base
            .get("snapshot")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default(),
        diff: Vec::new(),
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn a_value_holding_u0000_anywhere_is_told() {
        assert!(!nul(&json!("rien")));
        assert!(nul(&json!("a\0b")));
        assert!(nul(&json!([{ "name": "x", "description": "\0" }])));
        assert!(nul(&json!({ "a\0": 1 })));
        assert!(!nul(&json!([{ "name": "x", "description": "y" }, 3, null])));
    }

    #[test]
    fn an_action_keeps_its_name_and_its_description_or_its_text() {
        assert_eq!(
            action(&json!({ "name": "Parade", "text": "Réaction", "bonus": 2 })),
            json!({ "name": "Parade", "description": "Réaction" })
        );
        assert_eq!(
            action(&json!({ "name": "Parade", "description": "d", "text": "t" })),
            json!({ "name": "Parade", "description": "d" })
        );
        assert_eq!(action(&json!("Parade")), json!("Parade"));
    }
}
