//! DataCapability `dndbeyond.importCharacter` (`data-capability.json`): the
//! DM pastes a player's D&D Beyond URL and the PC's sheet is filled with no
//! typing — its name, its statblock (StatBlock@1: the six scores, the AC, the
//! HP max, the actions and the spells), its link to D&D Beyond and the time it
//! was synchronised — on its `Entity` partition, kept as a version of source
//! `dndbeyond` (EntityVersion@1) the history shows like any other.
//!
//! The character is fetched through the `character-sheets` port the server
//! shares ([`SharedCharacterSheets`]), which reads the URL
//! ([`CharacterId::parse`]: a URL of another host is refused) and answers the
//! sheet, or why it cannot — a character not shared as public, unknown, a
//! service whose answer changed shape or that did not answer. Any failure is a
//! refusal before anything is queued: nothing is written in part. A sheet
//! whose statblock is not complete (StatBlock@1, [`statblock`]) is refused
//! too.
//!
//! The import is an upsert, one PC per D&D Beyond character in a campaign:
//!
//! - the command targets a PC (`entityId`): the import fills that one, of its
//!   own campaign, unless it is linked to another character, or another PC of
//!   its campaign is linked to this one;
//! - else the PC of the campaign the tables hold already linked to that
//!   character is updated;
//! - else the PC whose id the campaign and the character derive ([`derived`])
//!   — one a pending import of the same URL is creating — is updated, or
//!   created when the queue holds none.
//!
//! A PC created is a « pj » (`character-is-pj-or-pnj`), of the player the
//! command names — of the same campaign —, with a token colour to fall back on
//! (`default-token`, TokenAppearance@1: a disc of that colour with its
//! initial). A PC updated takes the statblock, the link and the sync date
//! alone (`dndbeyond-resync-scope`): its name, its player, its aliases, its
//! notes and its token stay the DM's.
//!
//! The engine runs [`plugin`]'s `Prepare`, on the element's row as the queue
//! leaves it: [`prepare`] makes the command and the version it adds, numbered,
//! snapshotted and diffed after the row's, which the Entity aggregate judges,
//! projects into its tables and settles again after the element's last
//! version.

use std::collections::BTreeMap;
use std::fmt;

use aggregate_entity::{Kept, settled};
use async_trait::async_trait;
use harness_dataguard::plugins::{self as engine, Context, Plugin};
use ports_character_sheets::{
    CharacterId, CharacterSheet, CharacterSheets, CharacterSheetsError, SharedCharacterSheets,
};
use serde_json::{Map, Value};
use uuid::Uuid;

/// This DataCapability's id, as `data-capability.json` names it.
pub const DATA_CAPABILITY: &str = "dndbeyond.importCharacter";

/// This DataCapability's manifest (contract F), as the engine parses it.
pub const MANIFEST: &str = include_str!("../data-capability.json");

/// Where an import comes from (EntityVersion@1).
pub const SOURCE: &str = "dndbeyond";

/// The colour a new PC's token falls back on (`default-token`: a sheet has
/// one), the one a new PC saved by the DM takes.
pub const DEFAULT_TOKEN_COLOR: &str = "accent";

/// The type of the element an import creates.
const PJ: &str = "pj";

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

/// The largest AC or HP max the tables hold (a `smallint`).
const SMALLINT: i64 = 32_767;

/// One partition's row, as the engine holds it.
pub type Row = BTreeMap<String, Value>;

/// Why an import is refused: nothing is queued, nothing written.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// The `character-sheets` port's: the URL is not a D&D Beyond
    /// character's, the character is not public or unknown, the service
    /// changed shape or did not answer.
    Sheets(CharacterSheetsError),
    /// A new PC names no campaign.
    NoCampaign,
    /// The campaign named is none the tables hold.
    UnknownCampaign,
    /// The element the command targets does not exist.
    NoElement,
    /// The element the command targets is not a PC.
    NotAPc,
    /// The campaign named is not the one of the PC targeted.
    OtherCampaign,
    /// The PC targeted is linked to another D&D Beyond character.
    LinkedElsewhere,
    /// Another PC of the campaign, of this name, is linked to the character.
    AlreadyImported(String),
    /// A field naming another row (the campaign, the player, the element) is
    /// not a uuid.
    NotAnId(&'static str),
    /// The player named is not of the campaign.
    OtherPlayer,
    /// D&D Beyond gives the character no name.
    NoName,
    /// The statblock leaves these fields unsound (StatBlock@1), in the
    /// Concept's order.
    Incomplete(Vec<&'static str>),
    /// A value the tables could never hold: an AC or HP max beyond a
    /// `smallint`, or the character U+0000.
    Unwritable(&'static str),
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Sheets(e) => e.fmt(f),
            Self::NoCampaign => f.write_str("l'import d'un nouveau PJ nomme sa campagne"),
            Self::UnknownCampaign => f.write_str("aucune campagne de cet identifiant"),
            Self::NoElement => f.write_str("aucun élément de cet identifiant"),
            Self::NotAPc => f.write_str("un import D&D Beyond ne remplit que la fiche d'un PJ"),
            Self::OtherCampaign => f.write_str("ce PJ est d'une autre campagne"),
            Self::LinkedElsewhere => f.write_str("ce PJ est lié à un autre personnage D&D Beyond"),
            Self::AlreadyImported(name) => write!(
                f,
                "ce personnage D&D Beyond est déjà importé dans la campagne : « {name} »"
            ),
            Self::NotAnId(field) => write!(f, "{field} n'est pas un identifiant"),
            Self::OtherPlayer => f.write_str("ce joueur n'est pas de la campagne"),
            Self::NoName => f.write_str("D&D Beyond ne donne pas de nom à ce personnage"),
            Self::Incomplete(fields) => write!(
                f,
                "la fiche D&D Beyond n'a pas de statblock complet : {}",
                fields.join(", ")
            ),
            Self::Unwritable(field) => write!(f, "{field} ne tient pas dans la fiche"),
        }
    }
}

impl std::error::Error for Refusal {}

/// What an import is given besides the row and the sheet: the PC's id, its
/// campaign and player when the import creates it, the new version's id, and
/// the time.
#[derive(Clone, Copy, Debug)]
pub struct Import<'a> {
    /// The PC's id.
    pub entity: &'a str,
    /// Its campaign: read only when the import creates it.
    pub campaign: Option<&'a str>,
    /// Its player: read only when the import creates it.
    pub player: Option<&'a str>,
    /// The id of the version the import adds.
    pub version: &'a str,
    /// Now, RFC 3339.
    pub now: &'a str,
}

/// What the DataCapability plugs into the engine: its `Prepare`, which
/// reaches the `character-sheets` port the server shares.
#[must_use]
pub fn plugin() -> Plugin {
    Plugin::new()
        .prepare(DATA_CAPABILITY, Importing)
        .needs::<SharedCharacterSheets>()
}

/// An import made what it writes, before anything is queued: the PC found
/// (see the crate's documentation), its row as the queue leaves it, the
/// character's sheet fetched last, a fresh version id and now. What names
/// nothing — an element, a campaign, a character — is refused as not found,
/// any other refusal as unprocessable.
struct Importing;

/// Why an import is refused when the port is not shared: the engine refuses
/// to load first, so a server never answers it.
const NO_PORT: &str = "le port character-sheets n'est pas branché";

#[async_trait]
impl engine::Prepare for Importing {
    async fn prepare(
        &self,
        context: &Context<'_>,
        payload: &Row,
        target_id: Option<&str>,
    ) -> Result<engine::Prepared, engine::Refusal> {
        let Some(sheets) = context.shared.get::<SharedCharacterSheets>() else {
            return Err(engine::Refusal::unprocessable(NO_PORT));
        };
        let character = CharacterId::parse(text(payload, "url").unwrap_or_default())
            .map_err(|e| refused(Refusal::Sheets(e)))?;
        let campaign = id_of(payload, "campaign").map_err(refused)?;
        let player = id_of(payload, "player").map_err(refused)?;
        // The engine names the target from the payload's `entityId` too.
        let named = match target_id {
            Some(id) if is_uuid(id.trim()) => Some(id.trim().to_lowercase()),
            Some(_) => return Err(refused(Refusal::NotAnId("entityId"))),
            None => id_of(payload, "entityId").map_err(refused)?,
        };
        let found = found(
            context,
            character,
            campaign.as_deref(),
            player.as_deref(),
            named,
        )
        .await?;
        let sheet = sheets
            .fetch(character)
            .await
            .map_err(|e| refused(Refusal::Sheets(e)))?;
        // The port answers the character it was asked for: the link is the
        // one every later import looks the PC up by.
        if sheet.id != character {
            let answered = format!("le personnage {} répondu pour {character}", sheet.id);
            return Err(refused(Refusal::Sheets(
                CharacterSheetsError::ServiceChanged(answered),
            )));
        }
        let version = context.ids.fresh();
        let now = aggregate_entity::timestamp(context.now_millis);
        let import = Import {
            entity: &found.entity,
            campaign: campaign.as_deref(),
            player: player.as_deref(),
            version: &version,
            now: &now,
        };
        // The partition is the one the payload's `entityId` names.
        Ok(engine::Prepared {
            payload: prepare(&found.base, &sheet, import).map_err(refused)?,
            ..engine::Prepared::default()
        })
    }
}

/// The engine's refusal of `refusal`: what names nothing — an element, a
/// campaign, a character — is not found, anything else unprocessable.
fn refused(refusal: Refusal) -> engine::Refusal {
    match refusal {
        Refusal::NoElement
        | Refusal::UnknownCampaign
        | Refusal::Sheets(CharacterSheetsError::NotFound) => {
            engine::Refusal::not_found(refusal.to_string())
        }
        _ => engine::Refusal::unprocessable(refusal.to_string()),
    }
}

/// The PC an import writes: its id, and its row as the queue leaves it —
/// empty for a PC the import creates.
struct Found {
    entity: String,
    base: Row,
}

/// The PC an import of `character` writes (see the crate's documentation),
/// the element `named` when the command names one. The tables are read when
/// the engine lends them: over its memory alone, any campaign and player are
/// taken, and no PC is found linked but by its derived id.
async fn found(
    context: &Context<'_>,
    character: CharacterId,
    campaign: Option<&str>,
    player: Option<&str>,
    named: Option<String>,
) -> Result<Found, engine::Refusal> {
    let dndbeyond = character.to_string();
    if let Some(entity) = named {
        let base = context.row(&format!("Entity/{entity}")).await?;
        existing(&base, campaign, &dndbeyond).map_err(refused)?;
        let own = text(&base, "entityCampaignId").unwrap_or_default();
        let other = linked(context, own, None, &dndbeyond).await?.other;
        if let Some((_, name)) = other.filter(|(other, _)| *other != entity) {
            return Err(refused(Refusal::AlreadyImported(name)));
        }
        // An import of the character queued and not applied yet creates the
        // PC whose id it derives, which the tables do not hold yet.
        let pending = derived(own, character);
        if pending != entity {
            let row = context.row(&format!("Entity/{pending}")).await?;
            if !row.is_empty() {
                let name = text(&row, "name").unwrap_or_default().to_owned();
                return Err(refused(Refusal::AlreadyImported(name)));
            }
        }
        return Ok(Found { entity, base });
    }
    let campaign = campaign.ok_or_else(|| refused(Refusal::NoCampaign))?;
    let linked = linked(context, campaign, player, &dndbeyond).await?;
    if !linked.campaign {
        return Err(refused(Refusal::UnknownCampaign));
    }
    let entity = linked
        .other
        .map_or_else(|| derived(campaign, character), |(entity, _)| entity);
    let base = context.row(&format!("Entity/{entity}")).await?;
    if base.is_empty() {
        if !linked.player {
            return Err(refused(Refusal::OtherPlayer));
        }
    } else {
        existing(&base, Some(campaign), &dndbeyond).map_err(refused)?;
    }
    Ok(Found { entity, base })
}

/// Whether the element whose row is `base` is one an import of the character
/// `dndbeyond` into `campaign` fills: a PC that exists, of that campaign when
/// one is named, linked to no other character.
fn existing(base: &Row, campaign: Option<&str>, dndbeyond: &str) -> Result<(), Refusal> {
    if base.is_empty() {
        return Err(Refusal::NoElement);
    }
    if text(base, "type") != Some(PJ) {
        return Err(Refusal::NotAPc);
    }
    let own = text(base, "entityCampaignId").unwrap_or_default();
    if campaign.is_some_and(|campaign| !campaign.eq_ignore_ascii_case(own)) {
        return Err(Refusal::OtherCampaign);
    }
    match text(base, "dndbeyondId") {
        Some(linked) if !linked.trim().is_empty() && linked.trim() != dndbeyond => {
            Err(Refusal::LinkedElsewhere)
        }
        _ => Ok(()),
    }
}

/// What the tables say of an import into a campaign: whether they hold the
/// campaign, whether the player — if one is named — is of it, and the PC of
/// it already linked to the character, with its name.
struct Linked {
    campaign: bool,
    player: bool,
    other: Option<(String, String)>,
}

/// `$1` a campaign, `$2` a player or null — both uuids — and `$3` a D&D
/// Beyond id: whether the campaign exists, whether the player is of it, and
/// the element of the campaign linked to that character — the first made,
/// should there be two.
const LINKED: &str = "SELECT
        EXISTS (SELECT 1 FROM campaign WHERE id = $1::text::uuid) AS campaign,
        ($2::text IS NULL OR EXISTS (SELECT 1 FROM player p
            WHERE p.id = $2::text::uuid AND p.campaign_id = $1::text::uuid)) AS player,
        l.id AS linked, l.name AS linked_name
    FROM (SELECT 1) one
    LEFT JOIN LATERAL (SELECT c.entity_id::text AS id, e.name
        FROM character c JOIN entity e ON e.id = c.entity_id
        WHERE e.campaign_id = $1::text::uuid AND btrim(c.dndbeyond_id) = $3::text
        ORDER BY e.created_at, e.id LIMIT 1) l ON true";

/// [`LINKED`], over the tables the engine lends; over its memory, the
/// campaign and the player taken and no PC linked.
async fn linked(
    context: &Context<'_>,
    campaign: &str,
    player: Option<&str>,
    dndbeyond: &str,
) -> Result<Linked, engine::Refusal> {
    let Some(rows) = context.rows else {
        return Ok(Linked {
            campaign: true,
            player: true,
            other: None,
        });
    };
    let found = rows
        .rows(
            LINKED,
            &[
                Value::from(campaign),
                player.map_or(Value::Null, Value::from),
                Value::from(dndbeyond),
            ],
        )
        .await
        .map_err(|e| engine::Refusal::unprocessable(e.to_string()))?;
    let row = &found[0];
    let other = row["linked"].as_str().map(|id| {
        (
            id.to_owned(),
            row["linked_name"].as_str().unwrap_or_default().to_owned(),
        )
    });
    Ok(Linked {
        campaign: row["campaign"] == Value::Bool(true),
        player: row["player"] == Value::Bool(true),
        other,
    })
}

/// The id of the PC an import of `character` creates in `campaign`, the same
/// for every import of it: a uuid v5 of the character's URL and the
/// campaign, so that a second import queued behind the first, before the
/// tables hold it, updates the PC it creates.
#[must_use]
pub fn derived(campaign: &str, character: CharacterId) -> String {
    let name = format!("{}?campaign={}", character.url(), campaign.to_lowercase());
    Uuid::new_v5(&Uuid::NAMESPACE_URL, name.as_bytes()).to_string()
}

/// `field` of `row`, when it is a string.
fn text<'a>(row: &'a Row, field: &str) -> Option<&'a str> {
    row.get(field).and_then(Value::as_str)
}

/// The id `field` of `payload` names, in lowercase as the tables write one:
/// none when absent, null or blank.
fn id_of(payload: &Row, field: &'static str) -> Result<Option<String>, Refusal> {
    match payload.get(field) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(id)) if id.trim().is_empty() => Ok(None),
        Some(Value::String(id)) if is_uuid(id.trim()) => Ok(Some(id.trim().to_lowercase())),
        Some(_) => Err(Refusal::NotAnId(field)),
    }
}

/// Whether `text` is a uuid as PostgreSQL reads one: 8-4-4-4-12 hex digits.
fn is_uuid(text: &str) -> bool {
    text.len() == 36
        && text.char_indices().all(|(i, c)| match i {
            8 | 13 | 18 | 23 => c == '-',
            _ => c.is_ascii_hexdigit(),
        })
}

/// The statblock of `sheet` under the aggregate's names, once StatBlock@1
/// holds it complete and the tables can hold it.
///
/// # Errors
///
/// [`Refusal::Incomplete`] naming every unsound field, [`Refusal::Unwritable`]
/// for an AC or HP max beyond a `smallint`, or an action or a spell holding
/// the character U+0000.
pub fn statblock_of(sheet: &CharacterSheet) -> Result<Row, Refusal> {
    let Ok(Value::Object(fields)) = serde_json::to_value(&sheet.statblock) else {
        return Err(Refusal::Incomplete(STATBLOCK.to_vec()));
    };
    let incomplete = statblock(&fields);
    if !incomplete.is_empty() {
        return Err(Refusal::Incomplete(incomplete));
    }
    for field in ["armorClass", "hpMax"] {
        // StatBlock@1 holds it a positive integer already.
        if fields[field].as_i64().is_some_and(|n| n > SMALLINT) {
            return Err(Refusal::Unwritable(field));
        }
    }
    if ["actions", "spells"].iter().any(|list| nul(&fields[*list])) {
        return Err(Refusal::Unwritable("actions ou spells"));
    }
    Ok(fields.into_iter().collect())
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

/// The command an import of `sheet` submits on the PC `import.entity`, whose
/// row is `base` as the commands queued ahead of it will leave it — empty for
/// a PC the import creates: the statblock, the link and the sync date, and
/// for a new PC its campaign, type, name, player and token colour; then the
/// version it adds after the row's, of source `dndbeyond`.
///
/// # Errors
///
/// [`Refusal`] for a sheet an import cannot write: no name for a new PC, a
/// statblock StatBlock@1 does not hold complete, a value the tables could
/// never hold; [`Refusal::NoCampaign`] for a new PC of no campaign.
pub fn prepare(base: &Row, sheet: &CharacterSheet, import: Import<'_>) -> Result<Row, Refusal> {
    let mut payload = statblock_of(sheet)?;
    let entity = Value::from(import.entity);
    if base.is_empty() {
        let name = sheet.name.trim();
        if name.is_empty() {
            return Err(Refusal::NoName);
        }
        if name.contains('\0') {
            return Err(Refusal::Unwritable("name"));
        }
        let campaign = import.campaign.ok_or(Refusal::NoCampaign)?;
        for (field, value) in [
            ("entityCampaignId", Value::from(campaign)),
            ("type", PJ.into()),
            ("name", name.into()),
            ("entityCreatedAt", import.now.into()),
            ("tokenColor", DEFAULT_TOKEN_COLOR.into()),
        ] {
            payload.insert(field.into(), value);
        }
        if let Some(player) = import.player {
            payload.insert("playerId".into(), player.into());
        }
    }
    for (field, value) in [
        ("entityId", entity.clone()),
        ("characterEntityId", entity.clone()),
        ("dndbeyondId", sheet.id.to_string().into()),
        ("dndbeyondSyncedAt", import.now.into()),
        ("updatedAt", import.now.into()),
        ("entityVersionEntityId", entity),
        ("entityVersionId", import.version.into()),
        ("entityVersionCreatedAt", import.now.into()),
        ("source", SOURCE.into()),
        // An import restores nothing, whatever the last version did.
        ("restoredFromVersionId", Value::Null),
    ] {
        payload.insert(field.into(), value);
    }
    let mut after = base.clone();
    after.extend(payload.iter().map(|(k, v)| (k.clone(), v.clone())));
    Ok(settled(last_version(base).as_ref(), &after)
        .expect("the row after carries the new version's id")
        .stamped(&payload))
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
