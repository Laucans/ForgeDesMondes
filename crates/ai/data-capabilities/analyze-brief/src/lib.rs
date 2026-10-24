//! DataCapability `ai.analyzeBrief` (`data-capability.json`): the DM's brief
//! of the next session cut into its scenes, the elements the campaign holds
//! linked to them, and nothing created.
//!
//! The brief is typed (`text`) or dictated (`dictation`, the audio in
//! base64): a dictation is transcribed locally through the `speech-to-text`
//! port, listening for the campaign's names and aliases, and only its
//! transcript goes further (`text-only-no-audio`). [`read_answer`] reads
//! what the `llm` port answers to that text sent as a Brief request — the
//! object the port documents,
//! `{"scenes":[{"title","notes","elements":[{"type","name","excerpt"}]}]}` —
//! and [`analysed`] settles each element named against the campaign:
//!
//! - an element the campaign holds, by its name or one of its aliases under
//!   `AliasMatch@1` ([`normalized`]), whatever type the answer gives it, is
//!   linked to its scene, once; an NPC's excerpts are added to its notes —
//!   those its notes do not hold already — as a save of
//!   `campaign.saveEntity` naming the source `brief`, which adds its version
//!   and its diff (EntityVersion@1, read `ia`): one save per NPC;
//! - else a media of the campaign by its title is linked to the scene;
//! - else an NPC, a place or an event (« événement » is read `evenement`)
//!   goes on the to-create list, `a_creer`, with its type and its excerpt,
//!   once per type and name, and not again when the list awaits it already
//!   (`brief-no-auto-creation`); a PC or a type of no list is dropped.
//!
//! A brief the `llm` port could not analyse writes nothing: neither the
//! brief, nor a scene, an entry or a version — the error is the answer, 422.
//!
//! The engine writes one partition per command, and the `Session` aggregate
//! has a row per brief, per scene, per media linked and per entry:
//! [`Analysis::commands`] makes them, the brief's being the one submitted —
//! its idempotency key, its result — and the others follow-ups the engine
//! submits after it (`Prepared.then`), the saves of the NPCs last. The
//! partitions take sorted ids, so the applier, which runs them in that
//! order, writes the brief, then the scenes in their order — each ranked
//! after the session's last when it is applied (`scene-order-contiguous`) —
//! then what points at them. The scenes are the Session aggregate's
//! projection's; the brief, the entries and the media linked are written
//! into their tables by [`BriefTables`], in the transaction that applies
//! them.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::{Arc, Mutex, PoisonError};

use aggregate_session::{LINKS, NOTES, PARTITION, SCENE_ID, SCENE_SESSION_ID, SCENE_TITLE, uuid};
use async_trait::async_trait;
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use harness_dataguard::api::Submission;
use harness_dataguard::plugins::{self, Context, Plugin, Prepare};
use ports_llm::{GenerateRequest, Llm, ProviderConfig, SharedLlm, TextKind};
use ports_speech_to_text::{
    Dictation, SharedSpeechToText, SpeechToText, SpeechToTextError, Vocabulary,
};
use serde_json::{Map, Value};
use unicode_normalization::UnicodeNormalization;

mod tables;

pub use tables::{
    ANALYZED_AT, BRIEF_ID, BriefTables, CREATED_AT, DURATION_SECONDS, ORAL_BRIEF_ID,
    ORAL_BRIEF_SESSION_ID, RECORDED_AT, SCENE_MEDIA_MEDIA_ID, SCENE_MEDIA_SCENE_ID,
    TO_CREATE_CAMPAIGN_ID, TO_CREATE_ID, TO_CREATE_SCENE_ID, TO_CREATE_SESSION_ID,
    TO_CREATE_STATUS, TRANSCRIPT,
};

/// This DataCapability's id, as `data-capability.json` names it.
pub const DATA_CAPABILITY: &str = "ai.analyzeBrief";

/// The DataCapability's manifest (contract F), as the engine parses it.
pub const MANIFEST: &str = include_str!("../data-capability.json");

/// The DataCapability an existing NPC's update is a save of.
pub const SAVE_ENTITY: &str = "campaign.saveEntity";

/// Where the version an NPC's update adds comes from (EntityVersion@1).
pub const SOURCE: &str = "brief";

/// The types an entry of the to-create list may have, as
/// `entity_to_create.type` takes them.
pub const TYPES: [&str; 3] = ["pnj", "lieu", "evenement"];

/// The status of an entry waiting to be created.
pub const A_CREER: &str = "a_creer";

/// How long a follow-up announced waits to be submitted, in milliseconds:
/// the engine submits it in the same request, so one never taken back — the
/// ones after a refused follow-up — is dropped after it.
const ANNOUNCED_FOR_MS: u64 = 60_000;

/// Whether `id` names this DataCapability, with or without its `@version`.
#[must_use]
pub fn names(id: &str) -> bool {
    id.split('@').next() == Some(DATA_CAPABILITY)
}

/// What the DataCapability plugs into the engine: its `Prepare`, which
/// reaches the `llm` and `speech-to-text` ports the server shares, and the
/// projection of the brief, its entries and its media linked.
#[must_use]
pub fn plugin() -> Plugin {
    Plugin::new()
        .prepare(
            DATA_CAPABILITY,
            Analyse {
                announced: Arc::default(),
            },
        )
        .project_pg(AGGREGATE, BriefTables)
        .needs::<SharedLlm>()
        .needs::<SharedSpeechToText>()
}

const AGGREGATE: &str = "Session";

/// One partition's row, as a command writes it.
pub type Row = BTreeMap<String, Value>;

/// A brief analysed before anything is queued ([`analyse`] then
/// [`analysed`]), on the session the payload names and against the
/// campaign's elements, media and entries awaiting, as the tables hold them.
/// The brief's command is the one submitted, the others its follow-ups,
/// written as they were announced ([`Announced`]). A session the engine and
/// the tables do not hold is refused, 404; a brief that names none, holds no
/// word, or that the `llm` port could not analyse, 422: nothing is queued.
struct Analyse {
    announced: Arc<Announced>,
}

/// Why a brief is refused when a port is not shared: the engine refuses to
/// load first, so a server never answers it.
const NO_PORT: &str = "le port llm ou speech-to-text n'est pas branché";

#[async_trait]
impl Prepare for Analyse {
    async fn prepare(
        &self,
        context: &Context<'_>,
        payload: &Row,
        target_id: Option<&str>,
    ) -> Result<plugins::Prepared, plugins::Refusal> {
        if let Some(row) = self.announced.take(target_id, payload, context.now_millis) {
            return Ok(plugins::Prepared {
                payload: row,
                target_id: target_id.map(str::to_owned),
                ..plugins::Prepared::default()
            });
        }
        let unprocessable = |refusal: Refusal| plugins::Refusal::unprocessable(refusal.to_string());
        let session = session_of(payload).map_err(unprocessable)?;
        let Some(campaign) = campaign_of(context, &session).await? else {
            return Err(plugins::Refusal::not_found(format!(
                "aucune séance {session}"
            )));
        };
        let holdings = holdings(context, &campaign).await?;
        let settings = context.row(ai_provider::SETTINGS).await?;
        let (Some(llm), Some(speech)) = (
            context.shared.get::<SharedLlm>(),
            context.shared.get::<SharedSpeechToText>(),
        ) else {
            return Err(plugins::Refusal::unprocessable(NO_PORT));
        };
        let brief = analyse(
            llm.as_ref(),
            &ai_provider::active(&settings),
            speech.as_ref(),
            payload,
            &holdings.known,
        )
        .await
        .map_err(unprocessable)?;
        let analysis = analysed(&brief.scenes, &holdings);
        let mut warnings = Vec::new();
        let mut saves = Vec::new();
        for update in &analysis.updates {
            let row = context.row(&format!("Entity/{}", update.entity)).await?;
            if row.is_empty() {
                warnings.push(format!(
                    "« {} » est lié à sa scène mais n'est pas mis à jour : sa fiche n'a jamais été enregistrée",
                    update.name
                ));
                continue;
            }
            let current = row.get("notes").and_then(Value::as_str);
            if let Some(notes) = noted(current, &update.excerpts) {
                saves.push(save(&campaign, &update.entity, &notes));
            }
        }
        let ids = (0..analysis.rows()).map(|_| context.ids.fresh()).collect();
        let now = timestamp(context.now_millis);
        let held = Brief {
            session,
            campaign,
            transcript: brief.transcript,
            duration_seconds: brief.duration_seconds,
        };
        let Commands { brief, then } = analysis.commands(&held, ids, &now);
        let then = then
            .into_iter()
            .map(|command| {
                self.announced
                    .follow_up(&command.target_id, command.payload, context.now_millis)
            })
            .chain(saves)
            .collect();
        Ok(plugins::Prepared {
            payload: brief.payload,
            target_id: Some(brief.target_id),
            warnings,
            then,
            attachment: None,
        })
    }
}

/// The save of `campaign.saveEntity` that writes `notes` on the element
/// `entity`, its version of source [`SOURCE`].
fn save(campaign: &str, entity: &str, notes: &str) -> Submission {
    let mut element = Map::new();
    element.insert("id".into(), entity.into());
    element.insert("notes".into(), notes.into());
    Submission {
        data_capability: SAVE_ENTITY.to_owned(),
        payload: row([
            ("campaign", campaign.into()),
            ("entity", Value::Object(element)),
            ("source", SOURCE.into()),
        ]),
        target_id: Some(entity.to_owned()),
        ..Submission::default()
    }
}

/// The session's campaign: its row as the queue leaves it, else the tables'.
async fn campaign_of(
    context: &Context<'_>,
    session: &str,
) -> Result<Option<String>, plugins::Refusal> {
    let row = context.row(&format!("{PARTITION}{session}")).await?;
    if let Some(campaign) = row
        .get(aggregate_session::SESSION_CAMPAIGN_ID)
        .and_then(Value::as_str)
    {
        return Ok(Some(campaign.to_owned()));
    }
    let found = asked(context, SESSION, session).await?;
    Ok(found
        .first()
        .and_then(|row| row["campaign"].as_str())
        .map(str::to_owned))
}

/// The campaign of the session `$1` names.
const SESSION: &str = "SELECT campaign_id::text AS campaign FROM session
    WHERE id::text = lower($1::text)";

/// The elements of the campaign `$1` names, with their type, their name and
/// their aliases as written, in the order of their ids — none the engine
/// tombstoned.
const KNOWN: &str = "SELECT e.id::text AS id, e.type, e.name,
        coalesce((SELECT jsonb_agg(a.alias ORDER BY a.alias, a.id)
            FROM entity_alias a WHERE a.entity_id = e.id), '[]') AS aliases
    FROM entity e WHERE e.campaign_id::text = lower($1::text)
        AND NOT EXISTS (SELECT 1 FROM dataguard_rows r
            WHERE r.partition = 'Entity/' || e.id::text AND r.tombstoned)
    ORDER BY e.id";

/// The media of the campaign `$1` names, with their titles — none the
/// engine tombstoned.
const MEDIA: &str = "SELECT m.id::text AS id, m.title FROM media m
    WHERE m.campaign_id::text = lower($1::text)
        AND NOT EXISTS (SELECT 1 FROM dataguard_rows r
            WHERE r.partition = 'Media/' || m.id::text AND r.tombstoned)
    ORDER BY m.id";

/// The entries of the campaign `$1` names that wait to be created.
const WAITING: &str = "SELECT t.type, t.name FROM entity_to_create t
    WHERE t.campaign_id::text = lower($1::text) AND t.status = 'a_creer'
    ORDER BY t.id";

/// The rows `sql` answers for `id`: none over the engine's memory, which has
/// no table.
async fn asked(context: &Context<'_>, sql: &str, id: &str) -> Result<Vec<Value>, plugins::Refusal> {
    let Some(rows) = context.rows else {
        return Ok(Vec::new());
    };
    let found = rows
        .rows(sql, &[Value::from(id)])
        .await
        .map_err(|e| plugins::Refusal::unprocessable(e.to_string()))?;
    Ok(found.as_array().cloned().unwrap_or_default())
}

/// What the campaign holds, as its tables do.
async fn holdings(context: &Context<'_>, campaign: &str) -> Result<Holdings, plugins::Refusal> {
    let text = |value: &Value| value.as_str().map(str::to_owned);
    let known = asked(context, KNOWN, campaign)
        .await?
        .iter()
        .filter_map(|element| {
            Some(Known {
                id: text(&element["id"])?,
                kind: text(&element["type"])?,
                name: text(&element["name"])?,
                aliases: element["aliases"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(text)
                    .collect(),
            })
        })
        .collect();
    let media = asked(context, MEDIA, campaign)
        .await?
        .iter()
        .filter_map(|media| {
            Some(Media {
                id: text(&media["id"])?,
                title: text(&media["title"])?,
            })
        })
        .collect();
    let waiting = asked(context, WAITING, campaign)
        .await?
        .iter()
        .filter_map(|entry| Some((text(&entry["type"])?, text(&entry["name"])?)))
        .collect();
    Ok(Holdings {
        known,
        media,
        waiting,
    })
}

/// Why a brief is not analysed: nothing is queued.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// `session` names no session by its uuid.
    NoSession,
    /// Neither a text nor a dictation holds a word.
    NoBrief,
    /// The dictation is not base64.
    Undecodable,
    /// The `speech-to-text` port could not transcribe the dictation.
    Unheard(SpeechToTextError),
    /// `durationSeconds` is no whole number of seconds a column holds.
    NotADuration,
    /// The `llm` port failed, or answered no list of scenes.
    Unanalysed(String),
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoSession => f.write_str("aucune séance : `session` la nomme par son id"),
            Self::NoBrief => f.write_str("aucun brief à analyser"),
            Self::Undecodable => f.write_str("la dictée n’est pas un son encodé en base64"),
            Self::Unheard(e) => write!(f, "la dictée n’a pas été transcrite : {e}"),
            Self::NotADuration => {
                f.write_str("`durationSeconds` est un nombre entier de secondes, positif")
            }
            Self::Unanalysed(why) => {
                write!(f, "l’analyse du brief a échoué ({why}) : rien n’est écrit")
            }
        }
    }
}

impl std::error::Error for Refusal {}

/// The session `wire` names by its uuid, lowercase as its partition and the
/// tables write it.
fn session_of(wire: &Row) -> Result<String, Refusal> {
    wire.get("session")
        .and_then(Value::as_str)
        .filter(|id| uuid(id))
        .map(str::to_lowercase)
        .ok_or(Refusal::NoSession)
}

/// An element the campaign already holds: its id, its type, its name and its
/// aliases, as written.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Known {
    /// The element's id.
    pub id: String,
    /// Its type: `pj`, `pnj`, `lieu` or `evenement`.
    pub kind: String,
    /// Its name.
    pub name: String,
    /// Its aliases.
    pub aliases: Vec<String>,
}

/// A media of the campaign: its id and its title.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Media {
    /// The media's id.
    pub id: String,
    /// Its title.
    pub title: String,
}

/// What the campaign holds that a brief is settled against.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Holdings {
    /// Its elements.
    pub known: Vec<Known>,
    /// Its media.
    pub media: Vec<Media>,
    /// The type and name of each entry of its to-create list still waiting.
    pub waiting: Vec<(String, String)>,
}

/// One element a scene of the answer names.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Named {
    /// Its type, as the answer gives it.
    pub kind: String,
    /// Its name, as the brief writes it.
    pub name: String,
    /// The words of the brief that speak of it.
    pub excerpt: String,
}

/// One scene of the answer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Scene {
    /// Its title.
    pub title: String,
    /// The words of the brief it holds.
    pub notes: String,
    /// The elements it names, in order.
    pub elements: Vec<Named>,
}

/// The brief as text, and what the `llm` port cut it into.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Read {
    /// The brief: what was typed, or the transcript.
    pub transcript: String,
    /// How long it was dictated, in seconds.
    pub duration_seconds: u32,
    /// Its scenes, in order.
    pub scenes: Vec<Scene>,
}

/// The brief `wire` carries, read by `llm` through `provider` — a dictation
/// first transcribed by `speech`, listening for the names and aliases of
/// `known`, the elements the campaign holds.
///
/// # Errors
///
/// [`Refusal`] when `wire` holds no brief, a dictation that cannot be
/// transcribed or a duration no column holds, and when the `llm` port fails
/// or answers no list of scenes.
pub async fn analyse<L: Llm, S: SpeechToText>(
    llm: &L,
    provider: &ProviderConfig,
    speech: &S,
    wire: &Row,
    known: &[Known],
) -> Result<Read, Refusal> {
    let duration_seconds = match wire.get("durationSeconds").unwrap_or(&Value::Null) {
        Value::Null => 0,
        given => given
            .as_u64()
            .and_then(|seconds| i32::try_from(seconds).ok())
            .and_then(|seconds| u32::try_from(seconds).ok())
            .ok_or(Refusal::NotADuration)?,
    };
    let typed = wire.get("text").and_then(Value::as_str).unwrap_or_default();
    let transcript = if typed.trim().is_empty() {
        transcribed(speech, wire, known).await?
    } else {
        typed.to_owned()
    };
    if transcript.trim().is_empty() {
        return Err(Refusal::NoBrief);
    }
    let request = GenerateRequest::new(TextKind::Brief, transcript.clone());
    let answer = llm
        .generate(provider, &request)
        .await
        .map_err(|e| Refusal::Unanalysed(e.to_string()))?;
    let scenes = read_answer(&answer.text).ok_or_else(|| {
        Refusal::Unanalysed("la réponse de l’IA n’est pas une liste de scènes".to_owned())
    })?;
    Ok(Read {
        transcript,
        duration_seconds,
        scenes,
    })
}

/// The transcript of `wire`'s dictation; `""` when it holds none.
async fn transcribed<S: SpeechToText>(
    speech: &S,
    wire: &Row,
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

/// The scenes of a Brief answer, `None` when it holds no object the port
/// documents. Prose or a code fence around the object is skipped, braces in
/// that prose too: the object is the first one, from a `{`, that holds a
/// list of scenes. A scene that is no object, and an element with no name,
/// are dropped.
#[must_use]
pub fn read_answer(answer: &str) -> Option<Vec<Scene>> {
    let answer = answer
        .match_indices('{')
        .filter_map(|(at, _)| {
            serde_json::Deserializer::from_str(&answer[at..])
                .into_iter::<Value>()
                .next()?
                .ok()
        })
        .find(|object| object.get("scenes").is_some_and(Value::is_array))?;
    let text = |item: &Value, field: &str| {
        item.get(field)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim()
            .to_owned()
    };
    let scenes = answer.get("scenes")?.as_array()?;
    Some(
        scenes
            .iter()
            .filter(|scene| scene.is_object())
            .map(|scene| Scene {
                title: text(scene, "title"),
                notes: text(scene, "notes"),
                elements: scene
                    .get("elements")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .map(|element| Named {
                        kind: text(element, "type"),
                        name: text(element, "name"),
                        excerpt: text(element, "excerpt"),
                    })
                    .filter(|element| !element.name.is_empty())
                    .collect(),
            })
            .collect(),
    )
}

/// A scene as the brief writes it: what it links, in order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Planned {
    /// Its title: the answer's, else `Scène <its rank in the brief>`.
    pub title: String,
    /// The words of the brief it holds; `None` when there are none.
    pub notes: Option<String>,
    /// The elements of the campaign it links, by id, each once.
    pub links: Vec<String>,
    /// The media of the campaign it links, by id, each once.
    pub media: Vec<String>,
}

/// An entry the brief puts on the to-create list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToCreate {
    /// The scene, by its index among [`Analysis::scenes`], that named it
    /// first.
    pub scene: usize,
    /// One of [`TYPES`].
    pub kind: &'static str,
    /// Its name, as the brief writes it.
    pub name: String,
    /// The words of the brief that named it: never blank.
    pub excerpt: String,
}

/// An existing NPC the brief speaks of.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Update {
    /// The NPC's id.
    pub entity: String,
    /// Its name, as the campaign writes it.
    pub name: String,
    /// What the brief says of it, in order, each once.
    pub excerpts: Vec<String>,
}

/// What a brief's scenes, settled against the campaign, write.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Analysis {
    /// The scenes, in order.
    pub scenes: Vec<Planned>,
    /// The entries to create, in the order met.
    pub entries: Vec<ToCreate>,
    /// The existing NPCs it speaks of, in the order met.
    pub updates: Vec<Update>,
}

/// `scenes` settled against what the campaign holds (the module says how).
/// A scene with no title, no notes and no element is no scene.
#[must_use]
pub fn analysed(scenes: &[Scene], holdings: &Holdings) -> Analysis {
    let mut analysis = Analysis {
        scenes: Vec::new(),
        entries: Vec::new(),
        updates: Vec::new(),
    };
    let mut entered: Vec<(&str, String)> = holdings
        .waiting
        .iter()
        .filter_map(|(kind, name)| Some((type_of(kind)?, normalized(name))))
        .collect();
    let scenes = scenes
        .iter()
        .filter(|s| !(s.title.is_empty() && s.notes.is_empty() && s.elements.is_empty()));
    for scene in scenes {
        let index = analysis.scenes.len();
        let mut planned = Planned {
            title: if scene.title.is_empty() {
                format!("Scène {}", index + 1)
            } else {
                scene.title.clone()
            },
            notes: (!scene.notes.is_empty()).then(|| scene.notes.clone()),
            links: Vec::new(),
            media: Vec::new(),
        };
        for element in &scene.elements {
            let name = normalized(&element.name);
            let excerpt = [&element.excerpt, &scene.notes, &element.name]
                .into_iter()
                .find(|text| !text.is_empty())
                .cloned()
                .unwrap_or_default();
            if let Some(known) = holdings.known.iter().find(|known| answers_to(known, &name)) {
                if !planned.links.contains(&known.id) {
                    planned.links.push(known.id.clone());
                }
                if known.kind == "pnj" && !element.excerpt.is_empty() {
                    spoken_of(&mut analysis.updates, known, &element.excerpt);
                }
            } else if let Some(media) = holdings
                .media
                .iter()
                .find(|media| !name.is_empty() && normalized(&media.title) == name)
            {
                if !planned.media.contains(&media.id) {
                    planned.media.push(media.id.clone());
                }
            } else if let Some(kind) = type_of(&element.kind)
                && !entered.contains(&(kind, name.clone()))
            {
                entered.push((kind, name));
                analysis.entries.push(ToCreate {
                    scene: index,
                    kind,
                    name: element.name.clone(),
                    excerpt,
                });
            }
        }
        analysis.scenes.push(planned);
    }
    analysis
}

/// Whether `known` answers to the normalised term `name`: its name or one of
/// its aliases (AliasMatch@1), a blank term answering to none.
fn answers_to(known: &Known, name: &str) -> bool {
    !name.is_empty()
        && std::iter::once(&known.name)
            .chain(&known.aliases)
            .any(|term| normalized(term) == name)
}

/// `excerpt` added to what the brief says of `known`, once.
fn spoken_of(updates: &mut Vec<Update>, known: &Known, excerpt: &str) {
    let update = match updates.iter().position(|update| update.entity == known.id) {
        Some(at) => &mut updates[at],
        None => {
            updates.push(Update {
                entity: known.id.clone(),
                name: known.name.clone(),
                excerpts: Vec::new(),
            });
            updates.last_mut().expect("just pushed")
        }
    };
    if !update.excerpts.iter().any(|said| said == excerpt) {
        update.excerpts.push(excerpt.to_owned());
    }
}

/// The type of the to-create list `raw` names, read as `AliasMatch@1` reads
/// a term: « Événement » is `evenement`; `None` for a PC or any other.
fn type_of(raw: &str) -> Option<&'static str> {
    let raw = normalized(raw);
    TYPES.into_iter().find(|kind| *kind == raw)
}

/// The notes an NPC whose notes are `current` has once `excerpts` are added
/// to them, each on a line of its own: `None` when its notes hold every one
/// already, and nothing changes.
#[must_use]
pub fn noted(current: Option<&str>, excerpts: &[String]) -> Option<String> {
    let current = current.unwrap_or_default().trim_end();
    let new: Vec<&str> = excerpts
        .iter()
        .map(|excerpt| excerpt.trim())
        .filter(|excerpt| !excerpt.is_empty() && !current.contains(excerpt))
        .collect();
    if new.is_empty() {
        return None;
    }
    let added = new.join("\n");
    Some(if current.is_empty() {
        added
    } else {
        format!("{current}\n{added}")
    })
}

/// The brief as it is kept.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Brief {
    /// The session it prepares.
    pub session: String,
    /// The session's campaign.
    pub campaign: String,
    /// The text: what was typed, or the transcript.
    pub transcript: String,
    /// How long it was dictated, in seconds.
    pub duration_seconds: u32,
}

/// One command of the analysis: its partition's id and the row it writes.
#[derive(Clone, Debug, PartialEq)]
pub struct Command {
    /// The id of its `Session/<id>` partition.
    pub target_id: String,
    /// The fields it writes.
    pub payload: Row,
}

/// The commands a brief is submitted as.
#[derive(Clone, Debug, PartialEq)]
pub struct Commands {
    /// The brief's: the one submitted.
    pub brief: Command,
    /// The scenes', in order, then the media linked, then the entries: its
    /// follow-ups.
    pub then: Vec<Command>,
}

impl Analysis {
    /// How many rows the analysis writes in the Session aggregate: the
    /// brief, and one per scene, media linked and entry.
    #[must_use]
    pub fn rows(&self) -> usize {
        1 + self.scenes.len()
            + self
                .scenes
                .iter()
                .map(|scene| scene.media.len())
                .sum::<usize>()
            + self.entries.len()
    }

    /// The analysis of `brief` as commands, `ids` being fresh uuids — as many
    /// as [`Analysis::rows`] — and `now` the time (RFC 3339) the brief was
    /// recorded and analysed. The ids are sorted and handed out in the order
    /// the rows are applied: the brief, the scenes, the media linked, the
    /// entries.
    ///
    /// # Panics
    ///
    /// When `ids` does not hold [`Analysis::rows`] ids.
    #[must_use]
    pub fn commands(&self, brief: &Brief, mut ids: Vec<String>, now: &str) -> Commands {
        assert_eq!(ids.len(), self.rows(), "one id per row");
        ids.sort();
        let mut ids = ids.into_iter();
        let mut next = || ids.next().expect("one id per row");
        let brief_id = next();
        let brief_command = Command {
            payload: row([
                (ORAL_BRIEF_ID, brief_id.clone().into()),
                (ORAL_BRIEF_SESSION_ID, brief.session.clone().into()),
                (TRANSCRIPT, brief.transcript.clone().into()),
                (DURATION_SECONDS, brief.duration_seconds.into()),
                (RECORDED_AT, now.into()),
                (ANALYZED_AT, now.into()),
            ]),
            target_id: brief_id.clone(),
        };
        let scene_ids: Vec<String> = self.scenes.iter().map(|_| next()).collect();
        let mut then: Vec<Command> = self
            .scenes
            .iter()
            .zip(&scene_ids)
            .map(|(scene, id)| Command {
                payload: row([
                    (SCENE_ID, id.clone().into()),
                    (SCENE_SESSION_ID, brief.session.clone().into()),
                    (SCENE_TITLE, scene.title.clone().into()),
                    (NOTES, scene.notes.clone().map_or(Value::Null, Value::from)),
                    (LINKS, scene.links.clone().into()),
                ]),
                target_id: id.clone(),
            })
            .collect();
        for (scene, scene_id) in self.scenes.iter().zip(&scene_ids) {
            for media in &scene.media {
                let id = next();
                then.push(Command {
                    payload: row([
                        (SCENE_MEDIA_SCENE_ID, scene_id.clone().into()),
                        (SCENE_MEDIA_MEDIA_ID, media.clone().into()),
                    ]),
                    target_id: id,
                });
            }
        }
        for entry in &self.entries {
            let id = next();
            then.push(Command {
                payload: row([
                    (TO_CREATE_ID, id.clone().into()),
                    (TO_CREATE_CAMPAIGN_ID, brief.campaign.clone().into()),
                    (TO_CREATE_SESSION_ID, brief.session.clone().into()),
                    (BRIEF_ID, brief_id.clone().into()),
                    (TO_CREATE_SCENE_ID, scene_ids[entry.scene].clone().into()),
                    ("type", entry.kind.into()),
                    ("name", entry.name.clone().into()),
                    ("excerpt", entry.excerpt.clone().into()),
                    (TO_CREATE_STATUS, A_CREER.into()),
                    (CREATED_AT, now.into()),
                ]),
                target_id: id,
            });
        }
        Commands {
            brief: brief_command,
            then,
        }
    }
}

fn row<const N: usize>(fields: [(&str, Value); N]) -> Row {
    fields
        .into_iter()
        .map(|(field, value)| (field.to_owned(), value))
        .collect()
}

/// The follow-ups announced and not yet taken back, by the id of the
/// partition each writes, with the time they are dropped at: a follow-up
/// re-enters this DataCapability's `Prepare` with no payload, and takes back
/// the row it was announced with.
#[derive(Debug, Default)]
struct Announced {
    commands: Mutex<BTreeMap<String, (Row, u64)>>,
}

impl Announced {
    /// The follow-up writing `payload` on the partition `target_id` names,
    /// announced at `now` (Unix milliseconds) for [`ANNOUNCED_FOR_MS`]: the
    /// submission the `Prepare` hands the engine. The ones announced and
    /// expired by `now` are dropped.
    fn follow_up(&self, target_id: &str, payload: Row, now: u64) -> Submission {
        let mut commands = self.commands.lock().unwrap_or_else(PoisonError::into_inner);
        commands.retain(|_, (_, until)| *until > now);
        commands.insert(target_id.to_owned(), (payload, now + ANNOUNCED_FOR_MS));
        Submission {
            data_capability: DATA_CAPABILITY.to_owned(),
            target_id: Some(target_id.to_owned()),
            ..Submission::default()
        }
    }

    /// What the follow-up submitted with `payload` on the partition
    /// `target_id` names writes, taken back at `now`: `None` for a
    /// submission that is no follow-up announced, or one that expired.
    fn take(&self, target_id: Option<&str>, payload: &Row, now: u64) -> Option<Row> {
        let target_id = target_id.filter(|_| payload.is_empty())?;
        let (row, until) = self
            .commands
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(target_id)?;
        (until > now).then_some(row)
    }
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

/// `millis` since the Unix epoch, written RFC 3339 in UTC to the
/// millisecond (`2026-10-24T09:30:00.000Z`).
#[must_use]
pub fn timestamp(millis: u64) -> String {
    let (days, rest) = (millis / 86_400_000, millis % 86_400_000);
    // Days to a civil date (H. Hinnant's `civil_from_days`).
    let z = days + 719_468;
    let era = z / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + u64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{:03}Z",
        rest / 3_600_000,
        rest / 60_000 % 60,
        rest / 1_000 % 60,
        rest % 1_000
    )
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn wire(value: Value) -> Row {
        serde_json::from_value(value).expect("an object")
    }

    #[test]
    fn a_follow_up_names_its_row_carries_no_payload_and_is_taken_back_once() {
        let announced = Announced::default();
        let submission = announced.follow_up("p1", wire(json!({ "sceneId": "p1" })), 1_000);
        assert_eq!(submission.data_capability, DATA_CAPABILITY);
        assert_eq!(submission.target_id.as_deref(), Some("p1"));
        assert!(submission.payload.is_empty());
        // The author's own submission, with a payload, takes nothing.
        assert_eq!(
            announced.take(Some("p1"), &wire(json!({ "session": "s" })), 1_000),
            None
        );
        assert_eq!(announced.take(None, &Row::new(), 1_000), None);
        assert_eq!(announced.take(Some("p2"), &Row::new(), 1_000), None);
        assert_eq!(
            announced.take(Some("p1"), &Row::new(), 60_999),
            Some(wire(json!({ "sceneId": "p1" })))
        );
        assert_eq!(announced.take(Some("p1"), &Row::new(), 1_000), None, "once");
    }

    #[test]
    fn a_follow_up_never_taken_back_expires() {
        let announced = Announced::default();
        announced.follow_up("p1", Row::new(), 1_000);
        assert_eq!(announced.take(Some("p1"), &Row::new(), 61_000), None);
        // One announced later drops the ones expired by then, and only those.
        announced.follow_up("p2", Row::new(), 1_000);
        announced.follow_up("p3", Row::new(), 30_000);
        announced.follow_up("p4", Row::new(), 61_000);
        let left: Vec<String> = announced.commands.lock().unwrap().keys().cloned().collect();
        assert_eq!(left, ["p3", "p4"]);
    }

    #[test]
    fn a_session_is_named_by_its_uuid() {
        let id = "0a1b2c3d-4e5f-6a7b-8c9d-0e1f2a3b4c5d";
        assert_eq!(
            session_of(&wire(json!({ "session": id }))),
            Ok(id.to_owned())
        );
        // In uppercase, the session is named as its partition writes it.
        assert_eq!(
            session_of(&wire(json!({ "session": id.to_uppercase() }))),
            Ok(id.to_owned())
        );
        for given in [json!("s4"), json!(7), json!(null)] {
            assert_eq!(
                session_of(&wire(json!({ "session": given }))),
                Err(Refusal::NoSession)
            );
        }
        assert_eq!(session_of(&Row::new()), Err(Refusal::NoSession));
    }

    #[test]
    fn a_save_writes_the_notes_of_its_element_versioned_by_the_brief() {
        let submission = save("c1", "e1", "Doit 40 po.\nIl a trahi.");
        assert_eq!(submission.data_capability, "campaign.saveEntity");
        assert_eq!(submission.target_id.as_deref(), Some("e1"));
        assert_eq!(
            Value::Object(submission.payload.into_iter().collect()),
            json!({
                "campaign": "c1",
                "entity": { "id": "e1", "notes": "Doit 40 po.\nIl a trahi." },
                "source": "brief",
            })
        );
        assert_eq!(submission.idempotency_key, None);
    }
}
