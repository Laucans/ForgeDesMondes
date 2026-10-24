//! DataCapability `ai.createFromBrief` (`data-capability.json`): an NPC, a
//! place or an event a brief left « à créer » is made an element of the
//! campaign, from the brief's excerpt and, when the DM refined it, the
//! sentences they added.
//!
//! The DM sends `{ entry }`, « Créer depuis le brief », or `{ entry,
//! refinement }`, « Affiner », the refinement typed or dictated (`audio`,
//! the sound in base64): a dictation is transcribed locally through the
//! `speech-to-text` port and only its transcript goes further
//! (`text-only-no-audio`). [`propose`] sends the excerpt and the refinement
//! — text, never audio — to the `llm` port as a Prompt request
//! ([`request`]), and reads the one JSON object it asks for ([`proposed`]):
//! for an NPC a description, a background, a behaviour and a statblock
//! playable as it is — the six scores, the AC, the HP max and at least one
//! action — and for a place or an event its description. An answer that is
//! no such object, or a failure of the port, is refused before anything is
//! queued: nothing is written and the entry stays « a_creer ».
//!
//! The engine writes one partition per command. A creation is three:
//!
//! - the entry's own command, the one submitted — its idempotency key, its
//!   result — on its `Session/<entry id>` partition: « creee », the element
//!   it became, and the refinement when there is one ([`stamp`]); the
//!   Session aggregate judges it (`to-create-leaves-once-created`);
//! - the element's save (`campaign.saveEntity`, the Entity aggregate's
//!   DataCapability) on its `Entity/<element id>` partition ([`save`]),
//!   which makes it of the entry's campaign with its first version
//!   (EntityVersion@1), an NPC with its sheet and its default token colour;
//! - when the entry names a scene, the scene's row written again with the
//!   element after its last ([`linked`]), which the Session aggregate ranks
//!   at the scene's next position.
//!
//! The two last are the first's follow-ups (`Prepared.then`), the scene's a
//! submission of this DataCapability naming the entry and targeting the
//! scene: its `Prepare` links the element of an entry « creee » to the
//! entry's own scene, the scene read as the queue leaves it then — the
//! element after whatever was linked meanwhile. Only the creation's own
//! follow-up links, once: a link of an element the scene holds, or of an
//! entry no creation just handed over, is refused, so an element the DM
//! removed from the scene is never put back. Which path a submission takes
//! is read from the entry it names: targeting the entry's scene, it is the
//! link; targeting the entry, its session — as the ToCreateList sends it —
//! or nothing, it is the creation; any other target is refused.
//!
//! An entry « creee » whose element exists, or was deleted, is refused: no
//! element is made twice. The element's id is derived from the entry's
//! ([`element_id`]): a creation whose save was rejected is made again under
//! the same id. One entry is created once at a time, and stays held for a
//! while once handed over to the engine, until its stamp is queued
//! ([`Creating`]).
//!
//! The rows are read under the Session aggregate's fields — an entry's
//! `type`, `name`, `excerpt`, `entityToCreate*`, a scene's `scene*`,
//! `notes` and `entityId`, a session's `sessionCampaignId` — the tables'
//! first, the engine's laid over them.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::{Arc, Mutex};

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

/// This DataCapability's id, as `data-capability.json` names it.
pub const DATA_CAPABILITY: &str = "ai.createFromBrief";

/// The DataCapability's manifest (contract F), as the engine parses it.
pub const MANIFEST: &str = include_str!("../data-capability.json");

/// The DataCapability an element is saved through: the Entity aggregate's.
pub const SAVE_ENTITY: &str = "campaign.saveEntity";

/// The payload's fields.
const ENTRY: &str = "entry";
const REFINEMENT: &str = "refinement";
const AUDIO: &str = "audio";

/// An entry's fields, under the Session aggregate's names.
const STATUS: &str = "entityToCreateStatus";
const CREATED: &str = "createdEntityId";
const REFINEMENT_TEXT: &str = "refinementText";
const CAMPAIGN: &str = "entityToCreateCampaignId";
const SESSION: &str = "entityToCreateSessionId";
const SCENE: &str = "entityToCreateSceneId";

/// An entry's statuses: to create, then created.
const A_CREER: &str = "a_creer";
pub const CREEE: &str = "creee";

/// A session's campaign, under the Session aggregate's name.
const SESSION_CAMPAIGN_ID: &str = "sessionCampaignId";

/// The types an entry may have, as `entity_to_create.type` and
/// `entity.type` take them.
const TYPES: [&str; 3] = ["pnj", "lieu", "evenement"];
const PNJ: &str = "pnj";

/// The six scores: the sheet's abbreviation, the SRD's, and the aggregate's
/// field a refusal names.
const ABILITIES: [(&str, &str, &str); 6] = [
    ("FOR", "STR", "strength"),
    ("DEX", "DEX", "dexterity"),
    ("CON", "CON", "constitution"),
    ("INT", "INT", "intelligence"),
    ("SAG", "WIS", "wisdom"),
    ("CHA", "CHA", "charisma"),
];

/// The largest AC or HP max the tables hold (a `smallint`).
const SMALLINT: i64 = 32_767;

/// The largest ability score (SRD 5.2).
const MAX_SCORE: i64 = 30;

/// One partition's row, as the engine holds it.
pub type Row = BTreeMap<String, Value>;

/// Why a creation is refused: nothing is queued, nothing written.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// `entry` is no uuid.
    InvalidEntry,
    /// No entry « à créer » of that id.
    NoEntry,
    /// The entry is « creee », and its element exists or was deleted.
    AlreadyCreated,
    /// The entry is being created by another submission.
    InCreation,
    /// A link names an entry not « creee », another scene than its own, or
    /// an element neither saved nor queued.
    NotLinkable,
    /// A link that is not the creation's own follow-up, or whose scene
    /// already holds the element.
    NotFirstLink,
    /// The target is neither the entry, nor its session, nor its scene.
    ForeignTarget,
    /// The scene to link is gone.
    NoScene,
    /// The entry's session, or its scene's, is of another campaign.
    OtherCampaign,
    /// The dictation is not base64.
    Undecodable,
    /// The `speech-to-text` port could not transcribe the dictation.
    Unheard(SpeechToTextError),
    /// The dictation was transcribed into no word.
    NothingHeard,
    /// The `llm` port gave no answer.
    Unanswered(String),
    /// The answer holds no JSON object.
    NotAProposal,
    /// The answer leaves these fields of an NPC's sheet unfilled or unsound,
    /// in the order of the sheet.
    Incomplete(Vec<&'static str>),
    /// A text holds the character U+0000, which the tables never hold.
    Unwritable,
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidEntry => f.write_str("entry : l'identifiant d'une entrée « à créer »"),
            Self::NoEntry => f.write_str("aucune entrée « à créer » de cet identifiant"),
            Self::AlreadyCreated => f.write_str(
                "to-create-leaves-once-created : l'entrée est déjà créée, aucun second élément",
            ),
            Self::InCreation => f.write_str("l'entrée est déjà en cours de création"),
            Self::NotLinkable => {
                f.write_str("seul l'élément d'une entrée créée rejoint la scène de l'entrée")
            }
            Self::NotFirstLink => {
                f.write_str("seule la création de l'entrée lie son élément à la scène, une fois")
            }
            Self::ForeignTarget => {
                f.write_str("la cible n'est ni l'entrée, ni sa séance, ni sa scène")
            }
            Self::NoScene => f.write_str("scène introuvable"),
            Self::OtherCampaign => f.write_str(
                "scene-links-same-campaign : la séance de l'entrée est d'une autre campagne",
            ),
            Self::Undecodable => f.write_str("la dictée n’est pas un son encodé en base64"),
            Self::Unheard(e) => write!(f, "la dictée n’a pas été transcrite : {e}"),
            Self::NothingHeard => f.write_str("la dictée ne contient aucun mot"),
            Self::Unanswered(why) => write!(f, "le fournisseur n’a pas répondu : {why}"),
            Self::NotAProposal => f.write_str("la réponse de l’IA n’est pas une proposition"),
            Self::Incomplete(fields) => write!(
                f,
                "proposal-fills-sheet-fields : la proposition ne remplit pas {}",
                fields.join(", ")
            ),
            Self::Unwritable => f.write_str("un texte contient le caractère U+0000"),
        }
    }
}

impl std::error::Error for Refusal {}

/// An entry « à créer », as the tables and the engine hold it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    /// Its id: the partition `Session/<id>`.
    pub id: String,
    /// The campaign the element is made in.
    pub campaign: String,
    /// The session whose brief named it, when it is known.
    pub session: Option<String>,
    /// The scene the element is linked to, when it is known.
    pub scene: Option<String>,
    /// One of `pnj`, `lieu`, `evenement`.
    pub kind: &'static str,
    /// Its name, as the brief writes it.
    pub name: String,
    /// The words of the brief that named it.
    pub excerpt: String,
    /// Whether it is « creee ».
    pub created: bool,
    /// The element it became, when it names one.
    pub element: Option<String>,
}

/// `field` of `row` as text, when it is.
fn text<'a>(row: &'a Row, field: &str) -> Option<&'a str> {
    row.get(field).and_then(Value::as_str)
}

/// `field` of `row` as an id: a uuid, in lowercase.
fn id_of(row: &Row, field: &str) -> Option<String> {
    uuid_of(text(row, field))
}

/// A text holding more than blanks, trimmed.
fn filled(text: Option<&str>) -> Option<String> {
    text.map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_owned)
}

/// The entry `row` holds, `id` being its partition's.
///
/// # Errors
///
/// [`Refusal::NoEntry`] when the row is no entry « à créer » — of a
/// campaign, of a known type and status, with a name.
pub fn entry(id: &str, row: &Row) -> Result<Entry, Refusal> {
    let status = text(row, STATUS).ok_or(Refusal::NoEntry)?;
    if ![A_CREER, CREEE].contains(&status) {
        return Err(Refusal::NoEntry);
    }
    let kind = text(row, "type")
        .and_then(|kind| TYPES.into_iter().find(|known| *known == kind))
        .ok_or(Refusal::NoEntry)?;
    Ok(Entry {
        id: id.to_owned(),
        campaign: id_of(row, CAMPAIGN).ok_or(Refusal::NoEntry)?,
        session: id_of(row, SESSION),
        scene: id_of(row, SCENE),
        kind,
        name: filled(text(row, "name")).ok_or(Refusal::NoEntry)?,
        excerpt: filled(text(row, "excerpt")).unwrap_or_default(),
        created: status == CREEE,
        element: id_of(row, CREATED),
    })
}

/// What the DataCapability plugs into the engine: its `Prepare`, which
/// reaches the `llm` and `speech-to-text` ports the server shares.
#[must_use]
pub fn plugin() -> Plugin {
    Plugin::new()
        .prepare(
            DATA_CAPABILITY,
            Create {
                creating: Arc::default(),
            },
        )
        .needs::<SharedLlm>()
        .needs::<SharedSpeechToText>()
}

/// How long an entry stays held once its creation is handed over to the
/// engine, in milliseconds: the engine queues the stamp right after the
/// `Prepare` returns, well within it; past it, a creation whose save was
/// rejected may be made again.
const HOLD_MILLIS: u64 = 60_000;

/// The entries being created, each by one submission at a time: a second
/// submission of the same entry while the first is prepared — a double
/// click, while the provider answers — or just after it was handed over,
/// before the engine queued its stamp, is refused, never prepared twice.
#[derive(Debug, Default)]
pub struct Creating(Mutex<BTreeMap<String, Mark>>);

/// What `Creating` knows of one entry.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Mark {
    /// A submission is preparing it.
    preparing: bool,
    /// When its creation was last handed over, in milliseconds.
    handed: Option<u64>,
    /// The scene's link its last creation owes.
    link_owed: bool,
}

/// Whether a creation handed over at `handed` still holds at `now`.
fn holds(handed: Option<u64>, now: u64) -> bool {
    handed.is_some_and(|at| now < at.saturating_add(HOLD_MILLIS))
}

/// One entry being prepared, until it is dropped.
#[derive(Debug)]
pub struct Flight<'a> {
    creating: &'a Creating,
    entry: String,
    handed: Option<u64>,
}

impl Creating {
    fn marks(&self) -> std::sync::MutexGuard<'_, BTreeMap<String, Mark>> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// The preparation of `entry` begun at `now`: `None` while another one
    /// is. The marks no longer held are forgotten.
    pub fn start(&self, entry: &str, now: u64) -> Option<Flight<'_>> {
        let mut marks = self.marks();
        marks.retain(|_, mark| mark.preparing || holds(mark.handed, now));
        let mark = marks.entry(entry.to_owned()).or_default();
        if mark.preparing {
            return None;
        }
        mark.preparing = true;
        Some(Flight {
            creating: self,
            entry: entry.to_owned(),
            handed: mark.handed,
        })
    }

    /// Whether `entry`'s scene link is owed at `now` by its creation just
    /// handed over: true once, for the follow-up that makes it.
    pub fn link(&self, entry: &str, now: u64) -> bool {
        let mut marks = self.marks();
        let Some(mark) = marks.get_mut(entry) else {
            return false;
        };
        let owed = mark.link_owed && holds(mark.handed, now);
        mark.link_owed = false;
        owed
    }
}

impl Flight<'_> {
    /// Whether a creation of this entry was handed over less than
    /// [`HOLD_MILLIS`] before `now`.
    #[must_use]
    pub fn held(&self, now: u64) -> bool {
        holds(self.handed, now)
    }

    /// The creation handed over to the engine at `now`, owing the scene's
    /// link when `link`: the entry stays held a while.
    pub fn hand_over(self, now: u64, link: bool) {
        if let Some(mark) = self.creating.marks().get_mut(&self.entry) {
            mark.handed = Some(now);
            mark.link_owed = link;
        }
    }
}

impl Drop for Flight<'_> {
    fn drop(&mut self) {
        let mut marks = self.creating.marks();
        if let Some(mark) = marks.get_mut(&self.entry) {
            mark.preparing = false;
            if mark.handed.is_none() {
                marks.remove(&self.entry);
            }
        }
    }
}

/// A creation made what it writes, before anything is queued: on the entry
/// the command targets — else the one `entry` names — as the tables and the
/// queue leave it, the provider the AI settings make active and the
/// element's derived id. A submission that names an entry and targets its
/// scene is the scene's link ([`link`]). An entry the engine and the
/// tables do not hold is refused as not found, any other refusal as
/// unprocessable.
struct Create {
    creating: Arc<Creating>,
}

/// Why a creation is refused when a port is not shared: the engine refuses
/// to load first, so a server never answers it.
const NO_PORT: &str = "le port llm ou speech-to-text n'est pas branché";

fn refused(refusal: Refusal) -> plugins::Refusal {
    match refusal {
        Refusal::NoEntry | Refusal::NoScene => plugins::Refusal::not_found(refusal.to_string()),
        _ => plugins::Refusal::unprocessable(refusal.to_string()),
    }
}

/// `id` as a row's id: a uuid, in lowercase.
fn uuid_of(id: Option<&str>) -> Option<String> {
    id.map(str::trim)
        .filter(|id| uuid(id))
        .map(str::to_ascii_lowercase)
}

/// The scene's link of the entry `entry`: the scene `scene` — the entry's
/// own — as the tables and the queue leave it, written again with the
/// entry's element after its last link. Its follow-up is submitted once the
/// entry « creee » and the element's save are queued, so both read so here;
/// any other link — the scene already holding the element, no creation
/// owing it, or an element neither saved nor queued — is refused. A save
/// queued then rejected leaves a link the Session aggregate refuses whole
/// when it applies (`scene-links-same-campaign`): no row names an element
/// that is not.
async fn link(
    context: &Context<'_>,
    creating: &Creating,
    entry: &Entry,
    scene: &str,
) -> Result<plugins::Prepared, plugins::Refusal> {
    let element = entry
        .element
        .clone()
        .filter(|_| entry.created && entry.scene.as_deref() == Some(scene))
        .ok_or_else(|| refused(Refusal::NotLinkable))?;
    let row = held(context, SCENE_ROW, scene).await?;
    if id_of(&row, SCENE_ID).is_none() {
        return Err(refused(Refusal::NoScene));
    }
    if links(&row).contains(&element) || !creating.link(&entry.id, context.now_millis) {
        return Err(refused(Refusal::NotFirstLink));
    }
    if !exists(context, &element).await? {
        return Err(refused(Refusal::NotLinkable));
    }
    let (scene, row) = linked(&row, &element).ok_or_else(|| refused(Refusal::NoScene))?;
    Ok(plugins::Prepared {
        payload: row,
        target_id: Some(scene),
        ..plugins::Prepared::default()
    })
}

#[async_trait]
impl Prepare for Create {
    async fn prepare(
        &self,
        context: &Context<'_>,
        payload: &Row,
        target_id: Option<&str>,
    ) -> Result<plugins::Prepared, plugins::Refusal> {
        let target = target_id.map(|target| uuid_of(Some(target)));
        let id = match (uuid_of(text(payload, ENTRY)), &target) {
            (Some(named), _) => named,
            (None, Some(Some(target))) => target.clone(),
            _ => return Err(refused(Refusal::InvalidEntry)),
        };
        let now = context.now_millis;
        let flight = self
            .creating
            .start(&id, now)
            .ok_or_else(|| refused(Refusal::InCreation))?;
        let entry = entry(&id, &held(context, ENTRY_ROW, &id).await?).map_err(refused)?;
        match target {
            None => {}
            Some(Some(target)) if target == id || entry.session.as_ref() == Some(&target) => {}
            Some(Some(target)) if entry.scene.as_ref() == Some(&target) => {
                drop(flight);
                return link(context, &self.creating, &entry, &target).await;
            }
            Some(_) => return Err(refused(Refusal::ForeignTarget)),
        }
        // An element whose save was rejected never existed: made again.
        if entry.created {
            let made = match &entry.element {
                Some(element) => exists(context, element).await?,
                None => true,
            };
            if made {
                return Err(refused(Refusal::AlreadyCreated));
            }
        }
        // Handed over, its stamp not yet queued: the same creation.
        if flight.held(now) {
            return Err(refused(Refusal::InCreation));
        }
        let scene = match &entry.scene {
            Some(scene) => held(context, SCENE_ROW, scene).await?,
            None => Row::new(),
        };
        let sessions = entry
            .session
            .iter()
            .cloned()
            .chain(id_of(&scene, SCENE_SESSION_ID));
        for session in sessions {
            let row = held(context, SESSION_ROW, &session).await?;
            if id_of(&row, SESSION_CAMPAIGN_ID).is_some_and(|campaign| campaign != entry.campaign) {
                return Err(refused(Refusal::OtherCampaign));
            }
        }
        let settings = context.row(ai_provider::SETTINGS).await?;
        let (Some(llm), Some(speech)) = (
            context.shared.get::<SharedLlm>(),
            context.shared.get::<SharedSpeechToText>(),
        ) else {
            return Err(plugins::Refusal::unprocessable(NO_PORT));
        };
        let refinement = refinement(speech.as_ref(), payload, &entry)
            .await
            .map_err(refused)?;
        let proposal = propose(
            llm.as_ref(),
            &ai_provider::active(&settings),
            &entry,
            refinement.as_deref(),
        )
        .await
        .map_err(refused)?;
        let element = element_id(&id);
        let saved = save(&entry, &proposal, refinement.as_deref(), &element).map_err(refused)?;
        let mut then = vec![Submission {
            data_capability: SAVE_ENTITY.to_owned(),
            payload: saved,
            ..Submission::default()
        }];
        let mut warnings = Vec::new();
        match (&entry.scene, id_of(&scene, SCENE_ID)) {
            // A creation made again whose element the scene already holds.
            (_, Some(_)) if links(&scene).contains(&element) => {}
            (_, Some(scene)) => then.push(Submission {
                data_capability: DATA_CAPABILITY.to_owned(),
                target_id: Some(scene),
                payload: Row::from([(ENTRY.to_owned(), Value::from(id.clone()))]),
                ..Submission::default()
            }),
            (Some(scene), None) => warnings.push(format!(
                "la scène {scene} n'existe plus : l'élément n'y est pas lié"
            )),
            (None, None) => {}
        }
        flight.hand_over(now, then.len() > 1);
        Ok(plugins::Prepared {
            payload: stamp(&element, refinement.as_deref()),
            target_id: Some(id),
            warnings,
            then,
            attachment: None,
        })
    }
}

/// An entry « à créer », under the aggregate's fields, unless the engine
/// deleted it.
const ENTRY_ROW: &str = "SELECT t.campaign_id::text AS \"entityToCreateCampaignId\",
        t.session_id::text AS \"entityToCreateSessionId\",
        t.scene_id::text AS \"entityToCreateSceneId\",
        t.type AS \"type\", t.name AS \"name\", t.excerpt AS \"excerpt\",
        t.refinement_text AS \"refinementText\", t.status AS \"entityToCreateStatus\",
        t.created_entity_id::text AS \"createdEntityId\"
    FROM entity_to_create t
    WHERE t.id = $1::text::uuid
        AND NOT EXISTS (SELECT 1 FROM dataguard_rows r
            WHERE r.partition = 'Session/' || t.id::text AND r.tombstoned)";

/// A scene, its links in their order, under the aggregate's fields, unless
/// the engine deleted it.
const SCENE_ROW: &str =
    "SELECT c.id::text AS \"sceneId\", c.session_id::text AS \"sceneSessionId\",
        c.title AS \"sceneTitle\", c.notes AS \"notes\",
        coalesce((SELECT jsonb_agg(l.entity_id::text ORDER BY l.position, l.entity_id)
            FROM scene_entity l WHERE l.scene_id = c.id), '[]'::jsonb) AS \"entityId\"
    FROM scene c
    WHERE c.id = $1::text::uuid
        AND NOT EXISTS (SELECT 1 FROM dataguard_rows r
            WHERE r.partition = 'Session/' || c.id::text AND r.tombstoned)";

/// A session's campaign, under the aggregate's field.
const SESSION_ROW: &str = "SELECT s.campaign_id::text AS \"sessionCampaignId\"
    FROM session s WHERE s.id = $1::text::uuid";

/// The element `$1` names, held by the tables or deleted by the engine.
const ELEMENT: &str = "SELECT 1 AS held
    WHERE EXISTS (SELECT 1 FROM entity e WHERE e.id = $1::text::uuid)
        OR EXISTS (SELECT 1 FROM dataguard_rows r
            WHERE r.partition = 'Entity/' || $1::text AND r.tombstoned)";

/// The first row `sql` answers for `id`, empty when it answers none, or
/// over the engine's memory, which has no table.
async fn read(context: &Context<'_>, sql: &str, id: &str) -> Result<Row, plugins::Refusal> {
    let Some(rows) = context.rows else {
        return Ok(Row::new());
    };
    let found = rows
        .rows(sql, &[Value::from(id)])
        .await
        .map_err(|e| plugins::Refusal::unprocessable(e.to_string()))?;
    Ok(found
        .as_array()
        .into_iter()
        .flatten()
        .find_map(|row| match row {
            Value::Object(row) => Some(row.clone().into_iter().collect()),
            _ => None,
        })
        .unwrap_or_default())
}

/// The Session row `id` names: the tables' (`sql`), the engine's laid over.
async fn held(context: &Context<'_>, sql: &str, id: &str) -> Result<Row, plugins::Refusal> {
    let mut row = read(context, sql, id).await?;
    row.extend(context.row(&format!("{PARTITION}{id}")).await?);
    Ok(row)
}

/// Whether the element `id` names exists, or was deleted.
async fn exists(context: &Context<'_>, id: &str) -> Result<bool, plugins::Refusal> {
    Ok(!context.row(&format!("Entity/{id}")).await?.is_empty()
        || !read(context, ELEMENT, id).await?.is_empty())
}

/// The refinement `wire` carries for `entry`: the one typed, else the
/// dictation's transcript, else none.
///
/// # Errors
///
/// [`Refusal::Undecodable`], [`Refusal::Unheard`] or
/// [`Refusal::NothingHeard`] when a dictation cannot be heard;
/// [`Refusal::Unwritable`] when the refinement holds U+0000.
pub async fn refinement<S: SpeechToText>(
    speech: &S,
    wire: &Row,
    entry: &Entry,
) -> Result<Option<String>, Refusal> {
    let refinement = match filled(text(wire, REFINEMENT)) {
        Some(typed) => typed,
        None => {
            let Some(encoded) = filled(text(wire, AUDIO)) else {
                return Ok(None);
            };
            let audio = STANDARD
                .decode(&encoded)
                .map_err(|_| Refusal::Undecodable)?;
            let vocabulary = Vocabulary {
                campaign_names: vec![entry.name.clone()],
                aliases: Vec::new(),
                sound_names: Vec::new(),
            };
            let heard = speech
                .transcribe(&Dictation::new(audio), &vocabulary)
                .await
                .map_err(Refusal::Unheard)?;
            filled(Some(&heard.text)).ok_or(Refusal::NothingHeard)?
        }
    };
    // Kept as the entry's refinement text, whatever the element.
    if refinement.contains('\0') {
        return Err(Refusal::Unwritable);
    }
    Ok(Some(refinement))
}

/// The request a creation sends: what to propose for `entry`, the shape of
/// the answer, then the brief's excerpt and the DM's refinement — text alone.
#[must_use]
pub fn request(entry: &Entry, refinement: Option<&str>) -> GenerateRequest {
    let name = &entry.name;
    let asked = match entry.kind {
        PNJ => format!(
            "Propose le PNJ « {name} » (D&D 2024, SRD 5.2) qu’un brief de séance laisse à créer : \
             une description, un background, un comportement pour le jouer et un statblock \
             jouable tel quel, qui tiennent compte de l’extrait du brief et des précisions du DM. \
             Réponds par un seul objet JSON, sans texte autour : \
             {{\"description\":\"…\",\"background\":\"…\",\"behavior\":\"…\",\
             \"stats\":{{\"FOR\":10,\"DEX\":10,\"CON\":10,\"INT\":10,\"SAG\":10,\"CHA\":10}},\
             \"ca\":10,\"pvMax\":10,\"actions\":[{{\"name\":\"…\",\"description\":\"…\"}}]}} — \
             chaque caractéristique un entier de 1 à 30, la CA et les PV max des entiers \
             positifs, au moins une action."
        ),
        kind => format!(
            "Propose {} « {name} » qu’un brief de séance laisse à créer : sa description, qui \
             tient compte de l’extrait du brief et des précisions du DM. Réponds par un seul \
             objet JSON, sans texte autour : {{\"description\":\"…\"}}",
            if kind == "lieu" {
                "le lieu"
            } else {
                "l’événement"
            }
        ),
    };
    let mut text = format!("{asked}\n\nExtrait du brief : {}", entry.excerpt);
    if let Some(refinement) = refinement {
        text.push_str(&format!("\n\nPrécisions du DM : {refinement}"));
    }
    GenerateRequest::new(TextKind::Prompt, text)
}

/// What a provider proposed for an entry.
#[derive(Clone, Debug, PartialEq)]
pub struct Proposal {
    /// The element's description, when the answer gives one.
    pub description: Option<String>,
    /// An NPC's sheet, by the editor's names — `background`, `behavior`,
    /// `stats` (`FOR`… `CHA`), `ca`, `pvMax`, `actions` (each `{name,
    /// description}`); empty for a place or an event.
    pub sheet: Map<String, Value>,
}

/// The proposal `llm` makes through `provider` for `entry`, refined by
/// `refinement`.
///
/// # Errors
///
/// [`Refusal::Unanswered`] when the `llm` port fails, and [`proposed`]'s
/// refusals when its answer is no proposal.
pub async fn propose<L: Llm>(
    llm: &L,
    provider: &ProviderConfig,
    entry: &Entry,
    refinement: Option<&str>,
) -> Result<Proposal, Refusal> {
    let answer = llm
        .generate(provider, &request(entry, refinement))
        .await
        .map_err(|e| Refusal::Unanswered(e.to_string()))?;
    proposed(&answer.text, entry.kind)
}

/// The JSON object `answer` holds, prose or a code fence around it skipped.
fn object(answer: &str) -> Option<Map<String, Value>> {
    let found = answer.get(answer.find('{')?..=answer.rfind('}')?)?;
    match serde_json::from_str(found).ok()? {
        Value::Object(object) => Some(object),
        _ => None,
    }
}

/// `value` as a whole number from 1 to `max`, written with a fraction of
/// zero or not.
fn whole(value: Option<&Value>, max: i64) -> Option<i64> {
    let value = value?;
    let number = value.as_i64().or_else(|| {
        value
            .as_f64()
            .filter(|n| n.fract() == 0.0 && n.abs() <= 1e9)
            .map(|n| n as i64)
    })?;
    (1..=max).contains(&number).then_some(number)
}

/// The proposal a provider's answer holds for an entry of `kind`: one JSON
/// object, prose or a code fence around it skipped. An NPC's is read
/// whatever the provider: its scores under `stats` or `abilities`, by the
/// sheet's abbreviations or the SRD's; `armorClass` for `ca`, `hpMax` for
/// `pvMax`, `features` for `actions`, an action's `text` for its
/// `description` — at the top of the object or under `statblock`.
///
/// # Errors
///
/// [`Refusal::NotAProposal`] when the answer holds no object;
/// [`Refusal::Incomplete`] naming every field of an NPC's sheet it leaves
/// unfilled or unsound.
pub fn proposed(answer: &str, kind: &str) -> Result<Proposal, Refusal> {
    let found = object(answer).ok_or(Refusal::NotAProposal)?;
    let prose = |field: &str| filled(found.get(field).and_then(Value::as_str));
    let description = prose("description");
    if kind != PNJ {
        return Ok(Proposal {
            description,
            sheet: Map::new(),
        });
    }
    let nested = found.get("statblock").and_then(Value::as_object);
    let any = |names: &[&str]| {
        names
            .iter()
            .find_map(|name| found.get(*name).or_else(|| nested?.get(*name)))
    };
    let mut sheet = Map::new();
    let mut incomplete = Vec::new();
    for field in ["background", "behavior"] {
        match prose(field) {
            Some(text) => {
                sheet.insert(field.into(), text.into());
            }
            None => incomplete.push(field),
        }
    }
    let scores = any(&["stats", "abilities"]).and_then(Value::as_object);
    let mut stats = Map::new();
    for (abbreviation, srd, field) in ABILITIES {
        let given = scores.and_then(|scores| scores.get(abbreviation).or(scores.get(srd)));
        match whole(given, MAX_SCORE) {
            Some(score) => {
                stats.insert(abbreviation.into(), score.into());
            }
            None => incomplete.push(field),
        }
    }
    sheet.insert("stats".into(), stats.into());
    for (names, field, editor) in [
        (["ca", "armorClass"], "armorClass", "ca"),
        (["pvMax", "hpMax"], "hpMax", "pvMax"),
    ] {
        match whole(any(&names), SMALLINT) {
            Some(number) => {
                sheet.insert(editor.into(), number.into());
            }
            None => incomplete.push(field),
        }
    }
    let actions: Vec<Value> = any(&["actions", "features"])
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|action| {
            let action = action.as_object()?;
            let name = filled(action.get("name").and_then(Value::as_str))?;
            let description = filled(
                action
                    .get("description")
                    .or_else(|| action.get("text"))
                    .and_then(Value::as_str),
            )?;
            Some(serde_json::json!({ "name": name, "description": description }))
        })
        .collect();
    if actions.is_empty() {
        incomplete.push("actions");
    }
    sheet.insert("actions".into(), actions.into());
    if !incomplete.is_empty() {
        return Err(Refusal::Incomplete(incomplete));
    }
    Ok(Proposal { description, sheet })
}

/// The id the element `entry` becomes is given: the same one every time, so
/// a creation retried finds the element it made and does not make it twice.
#[must_use]
pub fn element_id(entry: &str) -> String {
    use sha2::{Digest, Sha256};
    let hash = Sha256::new()
        .chain_update(DATA_CAPABILITY)
        .chain_update([0])
        .chain_update("entity")
        .chain_update([0])
        .chain_update(entry)
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

/// Whether `value` holds the character U+0000, in a string or a key.
fn nul(value: &Value) -> bool {
    match value {
        Value::String(text) => text.contains('\0'),
        Value::Array(values) => values.iter().any(nul),
        Value::Object(fields) => fields
            .iter()
            .any(|(key, value)| key.contains('\0') || nul(value)),
        _ => false,
    }
}

/// The element `entry` becomes, `element` being its id: its save as the
/// DM's editor sends it — its campaign, its type, its name, the proposal's
/// description, else the excerpt — which `campaign.saveEntity` makes an
/// element with its first version; an NPC with the proposal's sheet and, for
/// initial prompt, the excerpt and the refinement.
///
/// # Errors
///
/// [`Refusal::Unwritable`] when a text holds U+0000.
pub fn save(
    entry: &Entry,
    proposal: &Proposal,
    refinement: Option<&str>,
    element: &str,
) -> Result<Row, Refusal> {
    let mut entity = Map::new();
    entity.insert("id".into(), element.into());
    entity.insert("kind".into(), entry.kind.into());
    entity.insert("name".into(), entry.name.clone().into());
    let description = proposal
        .description
        .clone()
        .unwrap_or_else(|| entry.excerpt.clone());
    entity.insert("description".into(), description.into());
    if entry.kind == PNJ {
        let prompt = match refinement {
            Some(refinement) => format!("{}\n\n{refinement}", entry.excerpt),
            None => entry.excerpt.clone(),
        };
        entity.insert("prompt".into(), prompt.into());
        entity.extend(proposal.sheet.clone());
    }
    let entity = Value::Object(entity);
    if nul(&entity) {
        return Err(Refusal::Unwritable);
    }
    Ok(Row::from([
        ("campaign".to_owned(), entry.campaign.clone().into()),
        ("entity".to_owned(), entity),
    ]))
}

/// The entry's command: « creee », the element it became, and the
/// refinement when there is one.
#[must_use]
pub fn stamp(element: &str, refinement: Option<&str>) -> Row {
    let mut row = Row::from([
        (STATUS.to_owned(), CREEE.into()),
        (CREATED.to_owned(), element.into()),
    ]);
    if let Some(refinement) = refinement {
        row.insert(REFINEMENT_TEXT.to_owned(), refinement.into());
    }
    row
}

/// The elements the scene `scene` links, in their order, in lowercase.
fn links(scene: &Row) -> Vec<String> {
    scene
        .get(LINKS)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_ascii_lowercase)
        .collect()
}

/// The scene `scene` holds — its id and its row — written again with
/// `element` after its last link, its title and notes kept: `None` when the
/// row is no scene's.
#[must_use]
pub fn linked(scene: &Row, element: &str) -> Option<(String, Row)> {
    let id = id_of(scene, SCENE_ID)?;
    let mut links = links(scene);
    if !links.iter().any(|link| link == element) {
        links.push(element.to_owned());
    }
    let kept = |field: &str| scene.get(field).cloned().unwrap_or(Value::Null);
    let row = Row::from([
        (SCENE_ID.to_owned(), id.clone().into()),
        (SCENE_SESSION_ID.to_owned(), kept(SCENE_SESSION_ID)),
        (SCENE_TITLE.to_owned(), kept(SCENE_TITLE)),
        (NOTES.to_owned(), kept(NOTES)),
        (LINKS.to_owned(), links.into()),
    ]);
    Some((id, row))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn odric() -> Entry {
        Entry {
            id: "7c000000-0000-4000-8000-000000000001".to_owned(),
            campaign: "c1000000-0000-4000-8000-000000000001".to_owned(),
            session: None,
            scene: None,
            kind: PNJ,
            name: "Frère Odric".to_owned(),
            excerpt: "un pèlerin, frère Odric".to_owned(),
            created: false,
            element: None,
        }
    }

    const SHEET: &str = r#"{"description":"Un pèlerin.","background":"Un passé.","behavior":"Bénit.",
        "stats":{"FOR":10,"DEX":10,"CON":11,"INT":11,"SAG":15,"CHA":13},
        "ca":11,"pvMax":18,"actions":[{"name":"Bâton","description":"+2 pour toucher."}]}"#;

    #[test]
    fn an_answer_is_read_through_prose_or_a_code_fence_around_it() {
        let answer = format!("Voici :\n```json\n{SHEET}\n```");
        let proposal = proposed(&answer, PNJ).expect("a proposal");
        assert_eq!(proposal.description.as_deref(), Some("Un pèlerin."));
        assert_eq!(proposal.sheet["background"], "Un passé.");
        assert_eq!(proposal.sheet["behavior"], "Bénit.");
        assert_eq!(
            proposal.sheet["stats"],
            json!({"FOR":10,"DEX":10,"CON":11,"INT":11,"SAG":15,"CHA":13})
        );
        assert_eq!(proposal.sheet["ca"], 11);
        assert_eq!(proposal.sheet["pvMax"], 18);
        assert_eq!(
            proposal.sheet["actions"],
            json!([{"name":"Bâton","description":"+2 pour toucher."}])
        );
    }

    #[test]
    fn an_answer_that_holds_no_object_is_no_proposal() {
        for answer in [
            "Réponse simulée",
            "} {",
            "[1, 2]",
            "{pas du json}",
            "{\"a\": 1",
        ] {
            assert_eq!(
                proposed(answer, "lieu"),
                Err(Refusal::NotAProposal),
                "{answer}"
            );
        }
        assert_eq!(
            object("x [ {\"a\": 1} ]"),
            Some(Map::from_iter([("a".to_owned(), json!(1))]))
        );
    }

    #[test]
    fn a_statblock_is_read_under_its_other_names_and_nested() {
        // The mock-up's shape: abilities by the SRD's names, features with
        // their text, under `statblock`; numbers written with a fraction.
        let answer = r#"{"background":"B","behavior":"C","armorClass":12.0,"hpMax":9,
            "statblock":{"abilities":{"STR":13,"DEX":8,"CON":11,"INT":10,"WIS":14,"CHA":11},
            "features":[{"name":"Meules","text":"Connaît la trappe."},{"name":" ","text":"x"},{"name":"Sans texte"},3]}}"#;
        let proposal = proposed(answer, PNJ).expect("a proposal");
        assert_eq!(proposal.description, None);
        assert_eq!(proposal.sheet["stats"]["FOR"], 13);
        assert_eq!(proposal.sheet["stats"]["SAG"], 14);
        assert_eq!(proposal.sheet["ca"], 12);
        assert_eq!(proposal.sheet["pvMax"], 9);
        assert_eq!(
            proposal.sheet["actions"],
            json!([{"name":"Meules","description":"Connaît la trappe."}])
        );
    }

    #[test]
    fn a_sheet_left_unfilled_or_unsound_names_each_field_in_order() {
        let answer = r#"{"background":" ","stats":{"FOR":0,"DEX":31,"CON":"11","INT":10.5,"SAG":30,"CHA":1},
            "ca":32768,"pvMax":-3,"actions":[]}"#;
        assert_eq!(
            proposed(answer, PNJ),
            Err(Refusal::Incomplete(vec![
                "background",
                "behavior",
                "strength",
                "dexterity",
                "constitution",
                "intelligence",
                "armorClass",
                "hpMax",
                "actions",
            ]))
        );
        // The bounds themselves hold.
        let answer = r#"{"background":"B","behavior":"C","stats":{"FOR":1,"DEX":30,"CON":1,"INT":1,"SAG":1,"CHA":1},
            "ca":32767,"pvMax":1,"actions":[{"name":"N","description":"D"}]}"#;
        assert!(proposed(answer, PNJ).is_ok());
    }

    #[test]
    fn a_place_or_an_event_takes_its_description_alone() {
        let proposal = proposed(SHEET, "lieu").expect("a proposal");
        assert_eq!(proposal.description.as_deref(), Some("Un pèlerin."));
        assert!(proposal.sheet.is_empty());
        let proposal = proposed("{\"description\":\"  \"}", "evenement").expect("a proposal");
        assert_eq!(proposal.description, None);
    }

    #[test]
    fn the_request_asks_for_the_kind_and_carries_the_excerpt_and_the_refinement() {
        let mut entry = odric();
        let request = request(&entry, None);
        assert_eq!(request.kind, TextKind::Prompt);
        assert!(request.text.starts_with("Propose le PNJ « Frère Odric »"));
        assert!(request.text.contains("\"pvMax\""));
        assert!(
            request
                .text
                .ends_with("Extrait du brief : un pèlerin, frère Odric")
        );
        let refined = super::request(&entry, Some("Il boite."));
        assert!(refined.text.ends_with(
            "Extrait du brief : un pèlerin, frère Odric\n\nPrécisions du DM : Il boite."
        ));
        entry.kind = "lieu";
        assert!(
            super::request(&entry, None)
                .text
                .starts_with("Propose le lieu « Frère Odric »")
        );
        entry.kind = "evenement";
        let event = super::request(&entry, None).text;
        assert!(event.starts_with("Propose l’événement"), "{event}");
        assert!(!event.contains("pvMax"), "{event}");
    }

    #[test]
    fn an_element_id_is_a_uuid_derived_from_its_entry_alone() {
        let id = element_id("e1");
        assert!(uuid(&id), "{id}");
        assert_eq!(&id[14..15], "5", "version 5");
        assert!(matches!(&id[19..20], "8" | "9" | "a" | "b"), "variant 10");
        assert_eq!(id, element_id("e1"));
        assert_ne!(id, element_id("e2"));
    }

    #[test]
    fn a_row_is_an_entry_when_its_status_type_campaign_and_name_are() {
        let row = |value| serde_json::from_value::<Row>(value).expect("a row");
        let good = json!({
            "entityToCreateStatus": "creee", "type": "lieu", "name": " Le gué ",
            "entityToCreateCampaignId": "C1000000-0000-4000-8000-000000000001",
            "entityToCreateSceneId": "pas un id", "createdEntityId": "e0000000-0000-4000-8000-000000000001",
        });
        let found = entry("t1", &row(good.clone())).expect("an entry");
        assert_eq!(found.name, "Le gué");
        assert_eq!(found.campaign, "c1000000-0000-4000-8000-000000000001");
        assert_eq!(found.scene, None);
        assert_eq!(found.excerpt, "");
        assert!(found.created);
        assert_eq!(
            found.element.as_deref(),
            Some("e0000000-0000-4000-8000-000000000001")
        );
        for (field, value) in [
            ("entityToCreateStatus", json!("autre")),
            ("entityToCreateStatus", Value::Null),
            ("type", json!("pj")),
            ("entityToCreateCampaignId", json!("c1")),
            ("name", json!("  ")),
        ] {
            let mut bad = good.clone();
            bad[field] = value;
            assert_eq!(entry("t1", &row(bad)), Err(Refusal::NoEntry), "{field}");
        }
        let mut todo = good;
        todo["entityToCreateStatus"] = json!("a_creer");
        assert!(!entry("t1", &row(todo)).expect("an entry").created);
    }

    #[test]
    fn a_save_holds_the_sheet_of_a_pnj_and_refuses_a_nul() {
        let entry = odric();
        let proposal = proposed(SHEET, PNJ).expect("a proposal");
        let saved = save(&entry, &proposal, Some("Il boite."), "e1").expect("a save");
        assert_eq!(saved["campaign"], entry.campaign.as_str());
        let entity = &saved["entity"];
        assert_eq!(entity["id"], "e1");
        assert_eq!(entity["kind"], "pnj");
        assert_eq!(entity["prompt"], "un pèlerin, frère Odric\n\nIl boite.");
        assert_eq!(entity["ca"], 11);
        let bare = Proposal {
            description: None,
            sheet: Map::new(),
        };
        let place = Entry {
            kind: "lieu",
            ..odric()
        };
        let saved = save(&place, &bare, Some("x"), "e2").expect("a save");
        assert_eq!(
            saved["entity"],
            json!({"id":"e2","kind":"lieu","name":"Frère Odric","description":"un pèlerin, frère Odric"})
        );
        assert_eq!(
            save(&entry, &bare, Some("un \0 nul"), "e3"),
            Err(Refusal::Unwritable)
        );
    }

    #[test]
    fn a_nul_is_found_in_a_string_a_list_or_a_key() {
        assert!(nul(&json!("a\0")));
        assert!(nul(&json!(["a", ["b\0"]])));
        assert!(nul(&json!({"k\0": 1})));
        assert!(nul(&json!({"k": {"l": "\0"}})));
        assert!(!nul(&json!({"k": ["a", 1, null, true]})));
    }

    #[test]
    fn a_stamp_is_creee_naming_its_element_and_keeps_a_refinement() {
        assert_eq!(
            stamp("e1", None),
            Row::from([
                (STATUS.to_owned(), json!("creee")),
                (CREATED.to_owned(), json!("e1")),
            ])
        );
        assert_eq!(stamp("e1", Some("Il boite."))[REFINEMENT_TEXT], "Il boite.");
    }

    #[test]
    fn a_scene_links_its_element_once_after_its_last() {
        let scene: Row = serde_json::from_value(json!({
            "sceneId": "5C000000-0000-4000-8000-000000000001", "sceneSessionId": "s4",
            "sceneTitle": "Le bourg", "entityId": ["A", "e1"],
        }))
        .expect("a row");
        let (id, row) = linked(&scene, "e2").expect("a scene");
        assert_eq!(id, "5c000000-0000-4000-8000-000000000001");
        assert_eq!(row[LINKS], json!(["a", "e1", "e2"]));
        assert_eq!(row[NOTES], Value::Null);
        assert_eq!(row[SCENE_TITLE], "Le bourg");
        assert_eq!(
            linked(&scene, "e1").expect("a scene").1[LINKS],
            json!(["a", "e1"])
        );
        assert_eq!(linked(&Row::new(), "e1"), None);
    }

    #[test]
    fn an_entry_is_created_by_one_submission_at_a_time() {
        let creating = Creating::default();
        let first = creating.start("e1", 0).expect("begun");
        assert!(!first.held(0));
        assert!(creating.start("e1", 0).is_none(), "e1 is being created");
        let other = creating.start("e2", 0).expect("another entry begins");
        drop(first);
        let again = creating.start("e1", 0).expect("e1 may begin again");
        assert!(!again.held(0), "a creation refused holds nothing");
        drop((again, other));
        assert!(
            creating.marks().is_empty(),
            "nothing kept: {:?}",
            creating.marks()
        );
    }

    #[test]
    fn a_creation_handed_over_holds_its_entry_for_a_while_and_owes_one_link() {
        let creating = Creating::default();
        creating
            .start("e1", 1_000)
            .expect("begun")
            .hand_over(1_000, true);
        let next = creating.start("e1", 1_500).expect("prepared again");
        assert!(next.held(1_500), "handed over, the stamp not yet queued");
        assert!(next.held(1_000 + HOLD_MILLIS - 1));
        assert!(!next.held(1_000 + HOLD_MILLIS), "the hold is over");
        drop(next);
        assert!(creating.link("e1", 1_500), "the creation's own link");
        assert!(!creating.link("e1", 1_500), "once");
        assert!(!creating.link("e2", 1_500), "no creation of e2");
        creating.start("e2", 0).expect("begun").hand_over(0, true);
        assert!(!creating.link("e2", HOLD_MILLIS), "owed no longer");
        creating.start("e3", 0).expect("begun").hand_over(0, false);
        assert!(!creating.link("e3", 0), "no scene, no link owed");
        // A hold over is forgotten at the next start.
        drop(creating.start("e4", 1_000 + HOLD_MILLIS));
        assert!(creating.marks().is_empty(), "{:?}", creating.marks());
    }

    #[tokio::test]
    async fn a_refinement_is_typed_else_dictated_and_never_holds_a_nul() {
        use ports_speech_to_text::fake::FakeSpeechToText;
        let heard = FakeSpeechToText::new("  Il boite.  ");
        let wire = |value: Value| serde_json::from_value::<Row>(value).expect("a row");
        let entry = odric();
        assert_eq!(
            refinement(
                &heard,
                &wire(json!({"refinement": " Il ment. ", "audio": "AAEC"})),
                &entry
            )
            .await,
            Ok(Some("Il ment.".to_owned()))
        );
        assert_eq!(
            refinement(
                &heard,
                &wire(json!({"refinement": " ", "audio": "AAEC"})),
                &entry
            )
            .await,
            Ok(Some("Il boite.".to_owned()))
        );
        assert_eq!(
            refinement(&heard, &wire(json!({"audio": " "})), &entry).await,
            Ok(None)
        );
        assert_eq!(
            refinement(&heard, &wire(json!({"audio": "%%"})), &entry).await,
            Err(Refusal::Undecodable)
        );
        assert_eq!(
            refinement(&heard, &wire(json!({"refinement": "a\u{0}b"})), &entry).await,
            Err(Refusal::Unwritable)
        );
        let silent = FakeSpeechToText::new(" ");
        assert_eq!(
            refinement(&silent, &wire(json!({"audio": "AAEC"})), &entry).await,
            Err(Refusal::NothingHeard)
        );
        assert_eq!(
            refinement(
                &FakeSpeechToText::new("a\u{0}b"),
                &wire(json!({"audio": "AAEC"})),
                &entry
            )
            .await,
            Err(Refusal::Unwritable)
        );
    }
}
