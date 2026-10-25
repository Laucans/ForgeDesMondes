//! DataCapability `campaign.importCampaign` (`data-capability.json`): the DM
//! imports a campaign archive — from this machine or another — and always
//! gets a new, separate campaign: the archive's players, elements with their
//! sheets, aliases and versions, media with their grids, sounds, sessions
//! with their present characters, scenes and journal, under new ids, every
//! reference pointing at the copy's rows and never the original's. The
//! original is never read, let alone changed, and importing the same archive
//! twice makes two copies (`import-creates-copy`).
//!
//! The copy is the Concept `CampaignArchive@1` (`concepts/CampaignArchive/v1`):
//! [`copy`] gives every row of the archive the id [`prepare`] drew for it,
//! rewrites the references, drops what the archive does not hold, and stamps
//! the time of the import; the campaign's `last_exported_at` is left out, a
//! copy has never been exported. The engine writes one partition,
//! `Campaign/<the copy's id>`, whose row holds the campaign's fields and the
//! sections of the copy; the Campaign aggregate's projection writes them into
//! the tables.
//!
//! [`request`] reads and checks the wire payload, [`prepare`] turns the
//! request into the row written, or into the [`Refusal`] that says why not —
//! before anything is queued.

use std::collections::{BTreeMap, BTreeSet};

use async_trait::async_trait;
use harness_dataguard::plugins::{self, Context, Plugin, Prepare};
use serde_json::{Map, Value};
use uuid::Uuid;

/// This DataCapability's id, as `data-capability.json` names it.
pub const DATA_CAPABILITY: &str = "campaign.importCampaign";

/// What an archive says it is, and the version of its shape.
pub const FORMAT: &str = "campaign-archive";
pub const FORMAT_VERSION: u64 = 1;

/// One partition's row, as the engine holds it.
pub type Row = BTreeMap<String, Value>;

/// The archive's lists of rows of the campaign, in the order it writes them.
const SECTIONS: [&str; 5] = ["players", "elements", "media", "sounds", "sessions"];

/// The members of a version's snapshot, and the `field` of an entry of its
/// diff, that hold the id of a row.
const VERSION_REFERENCES: [&str; 3] = ["playerId", "portraitMediaId", "tokenMediaId"];

/// The campaign's row as the engine holds it, field by field: its name, then
/// its column in the archive.
const CAMPAIGN_FIELDS: [(&str, &str); 11] = [
    ("id", "id"),
    ("name", "name"),
    ("kind", "kind"),
    ("pitch", "pitch"),
    ("art", "art"),
    ("glyph", "glyph"),
    ("coverMediaId", "cover_media_id"),
    ("createdAt", "created_at"),
    ("importedAt", "imported_at"),
    ("originCampaignId", "origin_campaign_id"),
    ("soundMasterVolume", "sound_master_volume"),
];

/// What the command asks for, read from the wire.
#[derive(Clone, Debug, PartialEq)]
pub struct Request {
    /// The archive, a JSON object whose shape [`request`] has checked.
    pub archive: Map<String, Value>,
    /// Every id the archive holds: the campaign's, and each player's,
    /// element's, version's, medium's, sound's, session's, scene's and
    /// journal line's.
    pub ids: BTreeSet<String>,
}

/// Why the command is refused before it is queued.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// The payload holds no archive: none, a text that is no JSON object,
    /// or something else.
    Unreadable,
    /// The archive's format is not [`FORMAT`].
    NotAnArchive,
    /// The archive's version is not [`FORMAT_VERSION`].
    UnknownVersion,
    /// The archive holds no campaign.
    NoCampaign,
    /// A section, or a member of a row, that must be a list is not one.
    NotAList(&'static str),
    /// A row of the section has no id of its own.
    NotAnId(&'static str),
    /// Two rows of the archive have the same id.
    DuplicateId(String),
    /// The command names the row it writes: a copy takes a new id, it is not
    /// written over a campaign that exists.
    TargetNamed,
    /// The ids the engine draws are no uuids.
    IdsNotUuids,
    /// The ids the engine draws are not all new: two alike, or one the
    /// archive holds.
    IdsNotNew,
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unreadable => f.write_str("archive: l'archive n'est pas un document JSON"),
            Self::NotAnArchive => write!(f, "archive: le format n'est pas {FORMAT}"),
            Self::UnknownVersion => {
                write!(f, "archive: la version n'est pas {FORMAT_VERSION}")
            }
            Self::NoCampaign => f.write_str("archive: elle ne contient aucune campagne"),
            Self::NotAList(what) => write!(f, "archive: {what} n'est pas une liste"),
            Self::NotAnId(what) => write!(f, "archive: une ligne de {what} n'a pas d'identifiant"),
            Self::DuplicateId(id) => {
                write!(f, "archive: l'identifiant {id} apparaît sur deux lignes")
            }
            Self::TargetNamed => f.write_str(
                "une copie prend toujours un nouvel identifiant : l'import ne désigne aucune campagne",
            ),
            Self::IdsNotUuids => f.write_str("les identifiants tirés ne sont pas des uuid"),
            Self::IdsNotNew => f.write_str(
                "les identifiants tirés ne sont pas tous nouveaux : deux se ressemblent, ou l'un est celui de l'archive",
            ),
        }
    }
}

impl From<Refusal> for plugins::Refusal {
    /// Every refusal is of a request that cannot be done: 422.
    fn from(refusal: Refusal) -> Self {
        Self::unprocessable(refusal.to_string())
    }
}

/// What the command becomes before it is submitted: the partition it
/// writes and the fields it writes there — the manifest's `touches`.
#[derive(Clone, Debug, PartialEq)]
pub struct Prepared {
    /// The copy's campaign id: the partition the command lands on.
    pub target_id: String,
    /// The campaign's fields and the sections of the copy.
    pub payload: Row,
}

/// What the DataCapability plugs into the engine: its `Prepare`.
#[must_use]
pub fn plugin() -> Plugin {
    Plugin::new().prepare(DATA_CAPABILITY, Importing)
}

/// A member of `row` that is a list: empty when the row lacks it or holds it
/// as null.
///
/// # Errors
///
/// [`Refusal::NotAList`] named `what` when it holds anything else.
fn section<'a>(row: &'a Value, member: &str, what: &'static str) -> Result<&'a [Value], Refusal> {
    match row.get(member) {
        None | Some(Value::Null) => Ok(&[]),
        Some(Value::Array(rows)) => Ok(rows),
        Some(_) => Err(Refusal::NotAList(what)),
    }
}

/// The same, for a list [`request`] has checked already.
fn list<'a>(row: &'a Value, member: &str) -> &'a [Value] {
    section(row, member, "").unwrap_or_default()
}

/// Notes the id of `row`, a row of `what`.
fn take(ids: &mut BTreeSet<String>, row: &Value, what: &'static str) -> Result<(), Refusal> {
    let id = row
        .get("id")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty())
        .ok_or(Refusal::NotAnId(what))?;
    if ids.insert(id.to_owned()) {
        Ok(())
    } else {
        Err(Refusal::DuplicateId(id.to_owned()))
    }
}

/// Every id the archive holds, and the shape of every list it must hold.
fn held(archive: &Value) -> Result<BTreeSet<String>, Refusal> {
    let mut ids = BTreeSet::new();
    let campaign = archive
        .get("campaign")
        .filter(|campaign| campaign.is_object())
        .ok_or(Refusal::NoCampaign)?;
    take(&mut ids, campaign, "campaign")?;
    for player in section(archive, "players", "players")? {
        take(&mut ids, player, "players")?;
    }
    for element in section(archive, "elements", "elements")? {
        take(&mut ids, element, "elements")?;
        section(element, "aliases", "aliases")?;
        for version in section(element, "versions", "versions")? {
            take(&mut ids, version, "versions")?;
            section(version, "diff", "diff")?;
        }
    }
    for medium in section(archive, "media", "media")? {
        take(&mut ids, medium, "media")?;
    }
    for sound in section(archive, "sounds", "sounds")? {
        take(&mut ids, sound, "sounds")?;
    }
    for session in section(archive, "sessions", "sessions")? {
        take(&mut ids, session, "sessions")?;
        section(
            session,
            "session_present_character",
            "session_present_character",
        )?;
        for scene in section(session, "scenes", "scenes")? {
            take(&mut ids, scene, "scenes")?;
            section(scene, "scene_entity", "scene_entity")?;
            section(scene, "scene_media", "scene_media")?;
        }
        for line in section(session, "journal_lines", "journal_lines")? {
            take(&mut ids, line, "journal_lines")?;
        }
    }
    Ok(ids)
}

/// The command `wire` asks for. `target` is the row the command names, which
/// an import never does.
///
/// # Errors
///
/// [`Refusal::TargetNamed`] for a command that names its row;
/// [`Refusal::Unreadable`] for a payload that holds no archive, whether as a
/// JSON object or as the text of one; [`Refusal::NotAnArchive`] and
/// [`Refusal::UnknownVersion`] for an archive of another format or version;
/// and for one that is not well formed, [`Refusal::NoCampaign`],
/// [`Refusal::NotAList`], [`Refusal::NotAnId`] or [`Refusal::DuplicateId`].
pub fn request(wire: &Row, target: Option<&str>) -> Result<Request, Refusal> {
    if target.is_some() {
        return Err(Refusal::TargetNamed);
    }
    let archive = match wire.get("archive") {
        Some(Value::Object(archive)) => archive.clone(),
        Some(Value::String(text)) => match serde_json::from_str(text) {
            Ok(Value::Object(archive)) => archive,
            _ => return Err(Refusal::Unreadable),
        },
        _ => return Err(Refusal::Unreadable),
    };
    if archive.get("format").and_then(Value::as_str) != Some(FORMAT) {
        return Err(Refusal::NotAnArchive);
    }
    if archive.get("version").and_then(Value::as_u64) != Some(FORMAT_VERSION) {
        return Err(Refusal::UnknownVersion);
    }
    let ids = held(&Value::Object(archive.clone()))?;
    Ok(Request { archive, ids })
}

/// The copy of `request`'s archive at `imported_at` (RFC 3339), and the row
/// the engine holds for it. `fresh` draws the new ids, one for every id the
/// archive holds.
///
/// # Errors
///
/// [`Refusal::IdsNotUuids`] when an id drawn is no uuid;
/// [`Refusal::IdsNotNew`] when two ids drawn are alike or one is the
/// archive's own.
pub fn prepare(
    request: &Request,
    imported_at: &str,
    mut fresh: impl FnMut() -> String,
) -> Result<Prepared, Refusal> {
    let mut new_ids = Map::new();
    let mut drawn = BTreeSet::new();
    for old in &request.ids {
        let new = uuid(&fresh()).ok_or(Refusal::IdsNotUuids)?;
        if request.ids.contains(&new) || !drawn.insert(new.clone()) {
            return Err(Refusal::IdsNotNew);
        }
        new_ids.insert(old.clone(), Value::String(new));
    }
    let copy = copy(
        &Value::Object(request.archive.clone()),
        imported_at,
        &new_ids,
    );
    let payload: Row = CAMPAIGN_FIELDS
        .iter()
        .map(|(field, column)| ((*field).to_owned(), copy["campaign"][*column].clone()))
        .chain(
            SECTIONS
                .iter()
                .map(|section| ((*section).to_owned(), copy[*section].clone())),
        )
        .collect();
    let target_id = copy["campaign"]["id"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    Ok(Prepared { target_id, payload })
}

/// `text` as a uuid, lowercase and hyphenated as the tables write it; `None`
/// when it is no uuid.
fn uuid(text: &str) -> Option<String> {
    Uuid::parse_str(text.trim()).ok().map(|id| id.to_string())
}

/// `row` with `members` set.
fn with(row: &Value, members: &[(&str, Value)]) -> Value {
    let mut row = row.as_object().cloned().unwrap_or_default();
    for (member, value) in members {
        row.insert((*member).to_owned(), value.clone());
    }
    Value::Object(row)
}

/// The copy of an archive, as `CampaignArchive@1` gives it: `{origin_campaign_id,
/// imported_at, campaign, players, elements, media, sounds, sessions}`. Every
/// row takes the id `new_ids` gives it, every reference follows, and a
/// reference to a row the archive does not hold becomes null.
#[must_use]
pub fn copy(archive: &Value, imported_at: &str, new_ids: &Map<String, Value>) -> Value {
    let copy = Copy { new_ids };
    let campaign = copy.reference(&archive["campaign"]["id"]);
    let own = |row: &Value| {
        with(
            row,
            &[
                ("id", copy.reference(&row["id"])),
                ("campaign_id", campaign.clone()),
            ],
        )
    };
    let rows = |section: &str, each: &dyn Fn(&Value) -> Value| {
        Value::Array(list(archive, section).iter().map(each).collect())
    };
    let mut result = Map::new();
    result.insert(
        "origin_campaign_id".into(),
        archive["origin_campaign_id"].clone(),
    );
    result.insert("imported_at".into(), imported_at.into());
    result.insert(
        "campaign".into(),
        with(
            &archive["campaign"],
            &[
                ("id", campaign.clone()),
                (
                    "cover_media_id",
                    copy.reference(&archive["campaign"]["cover_media_id"]),
                ),
                ("last_exported_at", Value::Null),
                ("imported_at", imported_at.into()),
                ("origin_campaign_id", archive["origin_campaign_id"].clone()),
            ],
        ),
    );
    result.insert("players".into(), rows("players", &own));
    result.insert(
        "elements".into(),
        rows("elements", &|e| copy.element(e, &campaign)),
    );
    result.insert(
        "media".into(),
        rows("media", &|m| copy.medium(m, &campaign)),
    );
    result.insert("sounds".into(), rows("sounds", &own));
    result.insert(
        "sessions".into(),
        rows("sessions", &|s| copy.session(s, &campaign)),
    );
    Value::Object(result)
}

/// The ids of the copy, by the id of the row of the archive each replaces.
struct Copy<'a> {
    new_ids: &'a Map<String, Value>,
}

impl Copy<'_> {
    /// A reference: the new id of the row it names, null when it names none
    /// of the archive's.
    fn reference(&self, old: &Value) -> Value {
        old.as_str()
            .and_then(|id| self.new_ids.get(id))
            .cloned()
            .unwrap_or(Value::Null)
    }

    /// A link row, when the row it links to is the archive's.
    fn link(&self, row: &Value, owner: (&str, &Value), other: &str) -> Option<Value> {
        let target = self.reference(&row[other]);
        (!target.is_null()).then(|| with(row, &[(owner.0, owner.1.clone()), (other, target)]))
    }

    /// A version, copied as it is but for the ids it names.
    fn version(&self, version: &Value, element: &Value) -> Value {
        let follow = |member: &str, value: &Value| {
            if VERSION_REFERENCES.contains(&member) {
                self.reference(value)
            } else {
                value.clone()
            }
        };
        let snapshot = match version.get("snapshot") {
            Some(Value::Object(members)) => Value::Object(
                members
                    .iter()
                    .map(|(member, value)| (member.clone(), follow(member, value)))
                    .collect(),
            ),
            other => other.cloned().unwrap_or(Value::Null),
        };
        let diff = list(version, "diff")
            .iter()
            .map(|entry| match entry["field"].as_str() {
                Some(field) => with(
                    entry,
                    &[
                        ("old", follow(field, &entry["old"])),
                        ("new", follow(field, &entry["new"])),
                    ],
                ),
                None => entry.clone(),
            })
            .filter(|entry| entry["old"] != entry["new"])
            .collect();
        with(
            version,
            &[
                ("id", self.reference(&version["id"])),
                ("entity_id", element.clone()),
                (
                    "restored_from_version_id",
                    self.reference(&version["restored_from_version_id"]),
                ),
                ("snapshot", snapshot),
                ("diff", Value::Array(diff)),
            ],
        )
    }

    /// An element with its sheet, aliases and versions.
    fn element(&self, element: &Value, campaign: &Value) -> Value {
        let id = self.reference(&element["id"]);
        let character = match element.get("character") {
            Some(sheet) if !sheet.is_null() => {
                let portrait = self.reference(&sheet["portrait_media_id"]);
                // A crop is a crop of the portrait: it goes with it.
                let crop = if portrait.is_null() {
                    Value::Null
                } else {
                    sheet["token_crop"].clone()
                };
                with(
                    sheet,
                    &[
                        ("entity_id", id.clone()),
                        ("player_id", self.reference(&sheet["player_id"])),
                        ("portrait_media_id", portrait),
                        ("token_media_id", self.reference(&sheet["token_media_id"])),
                        ("token_crop", crop),
                    ],
                )
            }
            _ => Value::Null,
        };
        let versions = list(element, "versions")
            .iter()
            .map(|version| self.version(version, &id))
            .collect();
        with(
            element,
            &[
                ("id", id),
                ("campaign_id", campaign.clone()),
                ("character", character),
                ("aliases", Value::Array(list(element, "aliases").to_vec())),
                ("versions", Value::Array(versions)),
            ],
        )
    }

    /// A medium with its grid.
    fn medium(&self, medium: &Value, campaign: &Value) -> Value {
        let id = self.reference(&medium["id"]);
        let grid = match medium.get("battlemap_grid") {
            Some(grid) if !grid.is_null() => with(grid, &[("media_id", id.clone())]),
            _ => Value::Null,
        };
        with(
            medium,
            &[
                ("id", id),
                ("campaign_id", campaign.clone()),
                ("place_id", self.reference(&medium["place_id"])),
                ("battlemap_grid", grid),
            ],
        )
    }

    /// A scene with its links to the elements and media of the archive.
    fn scene(&self, scene: &Value, session: &Value) -> Value {
        let id = self.reference(&scene["id"]);
        let links = |member: &str, other: &str| {
            Value::Array(
                list(scene, member)
                    .iter()
                    .filter_map(|row| self.link(row, ("scene_id", &id), other))
                    .collect(),
            )
        };
        with(
            scene,
            &[
                ("id", id.clone()),
                ("session_id", session.clone()),
                ("scene_entity", links("scene_entity", "entity_id")),
                ("scene_media", links("scene_media", "media_id")),
            ],
        )
    }

    /// A session with its present characters, scenes and journal.
    fn session(&self, session: &Value, campaign: &Value) -> Value {
        let id = self.reference(&session["id"]);
        let present = list(session, "session_present_character")
            .iter()
            .filter_map(|row| self.link(row, ("session_id", &id), "character_id"))
            .collect();
        let scenes = list(session, "scenes")
            .iter()
            .map(|scene| self.scene(scene, &id))
            .collect();
        let lines = list(session, "journal_lines")
            .iter()
            .map(|line| {
                with(
                    line,
                    &[
                        ("id", self.reference(&line["id"])),
                        ("session_id", id.clone()),
                        ("scene_id", self.reference(&line["scene_id"])),
                    ],
                )
            })
            .collect();
        with(
            session,
            &[
                ("id", id.clone()),
                ("campaign_id", campaign.clone()),
                ("session_present_character", Value::Array(present)),
                ("scenes", Value::Array(scenes)),
                ("journal_lines", Value::Array(lines)),
            ],
        )
    }
}

/// `unix_millis` as RFC 3339 in UTC, to the millisecond
/// (`2026-10-18T09:30:00.123Z`): the time of the import.
#[must_use]
pub fn timestamp(unix_millis: u64) -> String {
    let unix_seconds = unix_millis / 1000;
    // Howard Hinnant's civil_from_days.
    let days = i64::try_from(unix_seconds / 86_400).unwrap_or_default() + 719_468;
    let seconds = unix_seconds % 86_400;
    let era = days.div_euclid(146_097);
    let doe = days.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{:03}Z",
        seconds / 3600,
        seconds % 3600 / 60,
        seconds % 60,
        unix_millis % 1000
    )
}

/// The command made the copy it writes ([`prepare`]), with the ids the
/// engine draws and the time of its clock: refused 422 when the archive is
/// none — nothing queued.
struct Importing;

#[async_trait]
impl Prepare for Importing {
    async fn prepare(
        &self,
        context: &Context<'_>,
        payload: &BTreeMap<String, Value>,
        target_id: Option<&str>,
    ) -> Result<plugins::Prepared, plugins::Refusal> {
        let request = request(payload, target_id)?;
        let imported_at = timestamp(context.now_millis);
        let prepared = prepare(&request, &imported_at, || context.ids.fresh())?;
        Ok(plugins::Prepared {
            target_id: Some(prepared.target_id),
            payload: prepared.payload,
            ..plugins::Prepared::default()
        })
    }
}
