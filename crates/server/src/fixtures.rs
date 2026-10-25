//! The mock-up's fixtures (`mock/fixtures/*.json`) as the rows the store
//! holds.
//!
//! [`fixtures`] reads the folder and plans the load; it touches no database.
//! A fixture a landed aggregate holds (the Campaign, Player, Entity, Media and
//! Session aggregates') becomes the rows of its tables, in the order the foreign keys
//! and the aggregates' triggers need; the others are skipped with their
//! reason. What is planned is loaded whole, in one transaction, by
//! [`data::insert_missing`]: the tables' CHECKs and invariants judge every row
//! as they judge any write.
//!
//! The fixtures name their rows by slug (`cendres-valombre`, `elowen`) and
//! the tables key them by uuid: each id, and each reference to it, becomes
//! the UUIDv5 of `<table>:<slug>` under [`NAMESPACE`], so a restart plans the
//! same ids and loads nothing twice. [`Fixtures::names`] pairs each slug with
//! its uuid. A row with no slug of its own is named by what identifies it:
//! `entity_alias:<element slug>:<alias>`, `entity_version:<element slug>:<n>`,
//! `oral_brief:<session slug>`.
//!
//! What the tables have no column for is not loaded: the portrait and the
//! crop, the speed and the descriptive `type` line of a sheet, the art and
//! illustration of a place, the links between places and characters; of a
//! media, its weight, its terrain and its art. Nor is the campaign's cover:
//! `campaign.cover_media_id` names a media row, and a media row names its
//! campaign, so the one cannot be inserted before the other. An illustration
//! is described by its ratio alone: it is loaded as 1000 pixels high, and as
//! wide as its ratio says. Of a session, the `played` status is the table's
//! `closed`, and no scene, no journal line, no start or close time is loaded:
//! the fixtures of the Session aggregate give the sessions, the PCs present at
//! each, the last brief and the to-create list, and nothing of the brief's
//! scenes. A oneshot those fixtures do not describe gets the one session its
//! creation writes (`oneshot-single-session`), titled after it: closed on the
//! day the campaign list says it was last played, else in preparation for
//! its next day.
//!
//! A PC present at a session is one row of `session_present_character`, keyed
//! by the pair `session_id,character_id`: a row the DM changed since is left
//! alone, and a restart loads none twice. The stamps of `added_at` keep the
//! fixture's order. The fixture gives no time of
//! recording for a brief, only `lastRun`, the time of its analysis: both
//! columns take it, and a time with no zone is UTC.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde_json::{Map, Value, json};
use uuid::Uuid;

/// The namespace of every uuid derived from a fixture's slug. It never
/// changes: a new one would give every fixture row a new id.
pub const NAMESPACE: Uuid = Uuid::from_u128(0x6d6f_636b_2d75_7000_8000_6669_7874_7572);

/// A fixture file not loaded into the store: its name, and why.
pub type Skipped = (String, String);

/// What the fixtures folder plans.
#[derive(Debug, Default, PartialEq)]
pub struct Fixtures {
    /// The rows to insert, table by table, in the order to insert them.
    pub tables: Vec<data::Table>,
    /// The `(slug, uuid)` of every row that has a slug.
    pub names: Vec<(String, String)>,
    /// The files planned.
    pub loaded: Vec<String>,
    /// The files left out, and why.
    pub skipped: Vec<Skipped>,
}

/// The uuid of row `slug` of `table`.
#[must_use]
pub fn id(table: &str, slug: &str) -> String {
    Uuid::new_v5(&NAMESPACE, format!("{table}:{slug}").as_bytes()).to_string()
}

/// The columns of the tables the load fills; the others take their default.
const CAMPAIGN: &[&str] = &["id", "name", "kind", "pitch", "art", "glyph"];
const CAMPAIGN_DATED: &[&str] = &[
    "id",
    "name",
    "kind",
    "pitch",
    "art",
    "glyph",
    "created_at",
    "last_exported_at",
];
const PLAYER: &[&str] = &["id", "campaign_id", "name", "notes"];
const ENTITY: &[&str] = &[
    "id",
    "campaign_id",
    "type",
    "name",
    "description",
    "notes",
    "created_at",
    "updated_at",
];
const CHARACTER: &[&str] = &[
    "entity_id",
    "player_id",
    "initial_prompt",
    "background",
    "behavior",
    "strength",
    "dexterity",
    "constitution",
    "intelligence",
    "wisdom",
    "charisma",
    "armor_class",
    "hp_max",
    "actions",
    "token_color",
    "dndbeyond_id",
    "dndbeyond_synced_at",
];
const ALIAS: &[&str] = &["id", "entity_id", "alias"];
const MEDIA: &[&str] = &[
    "id",
    "campaign_id",
    "kind",
    "title",
    "file_path",
    "width_px",
    "height_px",
    "place_id",
    "uploaded_at",
];
const GRID: &[&str] = &[
    "media_id",
    "hex_size_px",
    "offset_x",
    "offset_y",
    "orientation",
    "color",
    "opacity",
];
const VERSION: &[&str] = &[
    "id",
    "entity_id",
    "version_number",
    "source",
    "snapshot",
    "diff",
    "created_at",
];

const SESSION: &[&str] = &[
    "id",
    "campaign_id",
    "position",
    "title",
    "planned_on",
    "status",
    "microphone_on",
    "summary",
];
const PRESENCE: &[&str] = &["session_id", "character_id", "added_at"];
const BRIEF: &[&str] = &[
    "id",
    "session_id",
    "transcript",
    "duration_seconds",
    "recorded_at",
    "analyzed_at",
];
const TO_CREATE: &[&str] = &[
    "id",
    "campaign_id",
    "session_id",
    "brief_id",
    "type",
    "name",
    "excerpt",
    "status",
    "created_entity_id",
    "created_at",
];

/// The six abilities of a sheet, with the column of each.
const ABILITIES: [(&str, &str); 6] = [
    ("FOR", "strength"),
    ("DEX", "dexterity"),
    ("CON", "constitution"),
    ("INT", "intelligence"),
    ("SAG", "wisdom"),
    ("CHA", "charisma"),
];

/// Reads every `*.json` file directly under `dir`, by name, and plans the
/// load of the ones a landed aggregate holds. The others are skipped, and so
/// is a file that is not JSON or that cannot be mapped to rows — none stops
/// the server, and none is half-planned.
///
/// # Errors
///
/// The folder's, when it cannot be listed.
pub fn fixtures(dir: &Path) -> std::io::Result<Fixtures> {
    let mut files = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.extension().is_some_and(|e| e == "json") && path.is_file() {
            files.push(path);
        }
    }
    files.sort();
    let mut plan = Fixtures::default();
    let mut read = Read::default();
    for path in files {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let text = std::fs::read_to_string(&path);
        let value = match text
            .map_err(|e| format!("cannot be read: {e}"))
            .and_then(|text| {
                serde_json::from_str::<Value>(&text).map_err(|e| format!("not JSON: {e}"))
            }) {
            Ok(value) => value,
            Err(why) => {
                plan.skipped.push((name, why));
                continue;
            }
        };
        let mapped = match name.as_str() {
            "campaign.listCampaigns.json" => read.campaign_list(&value),
            "campaign.getCampaign.json" => read.campaign_details(&value),
            "entities.json" => read.entities(&value),
            "media.json" => read.media(&value),
            SESSIONS => read.sessions(&value),
            BRIEF_FILE => read.brief(&value),
            TO_CREATE_FILE => read.creations(&value),
            _ => {
                plan.skipped
                    .push((name, "no aggregate holds it".to_string()));
                continue;
            }
        };
        match mapped {
            Ok(()) => plan.loaded.push(name),
            Err(why) => plan
                .skipped
                .push((name, format!("cannot be mapped: {why}"))),
        }
    }
    // What depends on another file is checked once every file is read, in the
    // order the foreign keys run: a file that does not fit is left out whole,
    // and so is what hangs on it.
    if let Err(why) = read.library_fits() {
        read.library = None;
        withdraw(&mut plan, LIBRARY, &why);
    }
    if let Err(why) = read.sessions_fit() {
        read.sessions = None;
        withdraw(&mut plan, SESSIONS, &why);
    }
    if let Err(why) = read.brief_fits() {
        read.brief = None;
        withdraw(&mut plan, BRIEF_FILE, &why);
    }
    if let Err(why) = read.to_create_fits() {
        read.to_create = None;
        withdraw(&mut plan, TO_CREATE_FILE, &why);
    }
    read.plan(&mut plan);
    Ok(plan)
}

/// The fixture of a campaign's media.
const LIBRARY: &str = "media.json";
/// The fixtures of the Session aggregate: a campaign's sessions, a session's
/// last brief, a campaign's to-create list.
const SESSIONS: &str = "SessionList.json";
const BRIEF_FILE: &str = "OralBrief.json";
const TO_CREATE_FILE: &str = "ToCreateList.json";

/// Takes `file` back out of the files planned: it is skipped, with `why`.
fn withdraw(plan: &mut Fixtures, file: &str, why: &str) {
    plan.loaded.retain(|name| name != file);
    plan.skipped
        .push((file.to_string(), format!("cannot be mapped: {why}")));
}

/// The first stamp of the fixtures, and the base the rows with no date of
/// their own count from, a microsecond apart.
const FIRST: &str = "2026-08-20T10:00:00";

/// The `index`th stamp after `at` (a time with no zone, as the fixtures
/// write it): fixed and in fixture order, so that a list keeps the fixture's.
fn stamp(at: &str, index: usize) -> String {
    format!("{at}.{index:06}Z")
}

/// What the files that mapped say. A file maps whole or not at all: nothing
/// of one that fails is kept.
#[derive(Default)]
struct Read {
    /// The campaigns by slug: name, kind, pitch, art, glyph, and the dates
    /// when a fixture gives them.
    campaigns: BTreeMap<String, Map<String, Value>>,
    /// The days the campaign list gives each campaign, by slug: the last it
    /// was played, and the next.
    days: BTreeMap<String, (Value, Value)>,
    entities: Option<Entities>,
    library: Option<Library>,
    sessions: Option<Sessions>,
    brief: Option<Brief>,
    to_create: Option<ToCreate>,
}

/// The `SessionList.json` document, mapped: a campaign's sessions.
struct Sessions {
    campaign: String,
    sessions: Vec<Session>,
}

struct Session {
    slug: String,
    position: i32,
    title: String,
    /// The day, or null.
    planned_on: Value,
    status: &'static str,
    summary: Value,
    /// The PCs present, by slug.
    present: Vec<String>,
}

/// The `OralBrief.json` document, mapped: the last brief of a session.
struct Brief {
    session: String,
    transcript: String,
    seconds: i32,
    /// When it was recorded, a time with no zone.
    at: String,
    analyzed: bool,
}

/// The `ToCreateList.json` document, mapped: the elements the briefs
/// mentioned and the campaign does not hold yet.
struct ToCreate {
    entries: Vec<Entry>,
}

struct Entry {
    slug: String,
    kind: &'static str,
    name: String,
    excerpt: String,
    /// The session it came from, by slug.
    session: String,
    /// The element created from it, by slug.
    created: Option<String>,
}

/// The `media.json` document, mapped: a campaign's battlemaps and
/// illustrations.
struct Library {
    campaign: String,
    media: Vec<Medium>,
}

/// A battlemap or an illustration.
struct Medium {
    slug: String,
    kind: &'static str,
    title: String,
    file: String,
    width: u64,
    height: u64,
    uploaded_at: Value,
    /// Its place, by slug.
    place: Option<String>,
    /// A battlemap's grid: the columns of `battlemap_grid` but its media.
    grid: Option<Value>,
}

/// The `entities.json` document, mapped.
struct Entities {
    campaign: String,
    players: Vec<Player>,
    elements: Vec<Element>,
}

struct Player {
    slug: String,
    name: Value,
}

/// An element: a PC, an NPC, a place or an event.
struct Element {
    slug: String,
    kind: &'static str,
    name: Value,
    description: Value,
    notes: Value,
    aliases: Vec<String>,
    /// The player of a PC, by slug.
    player: Option<String>,
    /// A PC's or an NPC's sheet: the columns of `character` it gives.
    sheet: Option<Map<String, Value>>,
}

impl Read {
    fn campaign_list(&mut self, value: &Value) -> Result<(), String> {
        let mut rows = Vec::new();
        for row in array(value, "the list")? {
            rows.push((
                required(row, "id")?.to_string(),
                json!({ "name": required(row, "name")?, "kind": required(row, "type")?,
                        "pitch": row["pitch"], "art": row["art"], "glyph": row["glyph"] }),
                (day(row, "lastPlayed")?, day(row, "next")?),
            ));
        }
        for (slug, row, days) in rows {
            self.campaign(&slug, &row);
            self.days.insert(slug, days);
        }
        Ok(())
    }

    fn campaign_details(&mut self, value: &Value) -> Result<(), String> {
        let row = json!({
            "name": required(value, "name")?, "kind": required(value, "type")?,
            "pitch": value["pitch"], "art": value["art"], "glyph": value["glyph"],
            "created_at": date(value, "createdAt")?,
            "last_exported_at": date(value, "lastExport")?,
        });
        self.campaign(required(value, "id")?, &row);
        Ok(())
    }

    /// Adds what `row` says to the campaign `slug`: what is known already is
    /// kept, a null adds nothing.
    fn campaign(&mut self, slug: &str, row: &Value) {
        let known = self.campaigns.entry(slug.to_string()).or_default();
        for (key, value) in row.as_object().into_iter().flatten() {
            if !value.is_null() {
                known.entry(key.clone()).or_insert_with(|| value.clone());
            }
        }
    }

    fn entities(&mut self, value: &Value) -> Result<(), String> {
        let campaign = required(&value["campaign"], "id")?.to_string();
        let name = required(&value["campaign"], "name")?.to_string();
        let mut players = Vec::new();
        for player in array(&value["players"], "players")? {
            players.push(Player {
                slug: required(player, "id")?.to_string(),
                name: json!(required(player, "name")?),
            });
        }
        let mut elements = Vec::new();
        for (key, kind) in [
            ("characters", None),
            ("places", Some("lieu")),
            ("events", Some("evenement")),
        ] {
            for element in array(&value[key], key)? {
                elements.push(element_of(element, kind)?);
            }
        }
        // A slug names one row, whatever its table: the wire knows no table.
        let mut slugs = BTreeSet::new();
        let own = players
            .iter()
            .map(|p| &p.slug)
            .chain(elements.iter().map(|e| &e.slug))
            .chain(std::iter::once(&campaign));
        for slug in own {
            if !slugs.insert(slug) || (*slug != campaign && self.campaigns.contains_key(slug)) {
                return Err(format!("the slug {slug:?} names two rows"));
            }
        }
        // `entities.json` names its campaign even where no list does.
        self.campaign(&campaign, &json!({ "name": name, "kind": "campagne" }));
        self.entities = Some(Entities {
            campaign,
            players,
            elements,
        });
        Ok(())
    }

    fn media(&mut self, value: &Value) -> Result<(), String> {
        let campaign = required(value, "campaign")?.to_string();
        let mut media = Vec::new();
        for (key, kind) in [
            ("battlemaps", "battlemap"),
            ("illustrations", "illustration"),
        ] {
            for medium in array(&value[key], key)? {
                media.push(medium_of(medium, kind)?);
            }
        }
        let mut slugs = BTreeSet::new();
        if let Some(twice) = media.iter().find(|m| !slugs.insert(&m.slug)) {
            return Err(format!("the slug {:?} names two rows", twice.slug));
        }
        self.library = Some(Library { campaign, media });
        Ok(())
    }

    fn sessions(&mut self, value: &Value) -> Result<(), String> {
        let campaign = required(value, "campaign")?.to_string();
        let mut sessions = Vec::new();
        for session in array(&value["sessions"], "sessions")? {
            sessions.push(session_of(session)?);
        }
        let mut slugs = BTreeSet::new();
        if let Some(twice) = sessions.iter().find(|s| !slugs.insert(&s.slug)) {
            return Err(format!("the slug {:?} names two rows", twice.slug));
        }
        self.sessions = Some(Sessions { campaign, sessions });
        Ok(())
    }

    fn brief(&mut self, value: &Value) -> Result<(), String> {
        let at = required(value, "lastRun")?;
        if !is_time(at) {
            return Err(format!("lastRun {at:?} is not a time"));
        }
        let mut lines = Vec::new();
        for line in array(&value["transcript"], "the transcript")? {
            lines.push(
                line.as_str()
                    .ok_or("a line of the transcript is not a text")?,
            );
        }
        self.brief = Some(Brief {
            session: required(value, "session")?.to_string(),
            transcript: lines.join("\n"),
            seconds: seconds_of(required(value, "duration")?)?,
            at: at.to_string(),
            analyzed: value["result"].is_object(),
        });
        Ok(())
    }

    fn creations(&mut self, value: &Value) -> Result<(), String> {
        let mut entries = Vec::new();
        for item in array(&value["items"], "items")? {
            entries.push(entry_of(item)?);
        }
        let mut slugs = BTreeSet::new();
        if let Some(twice) = entries.iter().find(|e| !slugs.insert(&e.slug)) {
            return Err(format!("the slug {:?} names two rows", twice.slug));
        }
        self.to_create = Some(ToCreate { entries });
        Ok(())
    }

    /// Whether a campaign, a player, an element or a medium of the files
    /// read is named `slug`: the slugs of the rows planned before the
    /// sessions.
    fn named(&self, slug: &str) -> bool {
        self.campaigns.contains_key(slug)
            || self.entities.as_ref().is_some_and(|e| {
                e.players.iter().any(|p| p.slug == slug)
                    || e.elements.iter().any(|element| element.slug == slug)
            })
            || self
                .library
                .as_ref()
                .is_some_and(|l| l.media.iter().any(|m| m.slug == slug))
    }

    /// Whether the element `slug` is of `campaign` and of `kind`.
    fn element(&self, campaign: &str, slug: &str, kind: &str) -> bool {
        self.entities.as_ref().is_some_and(|e| {
            e.campaign == campaign
                && e.elements
                    .iter()
                    .any(|element| element.slug == slug && element.kind == kind)
        })
    }

    /// Whether the sessions read fit what the other files say: their
    /// campaign is one of them, each PC present a « pj » of that campaign,
    /// and no slug of theirs names another row.
    fn sessions_fit(&self) -> Result<(), String> {
        let Some(sessions) = &self.sessions else {
            return Ok(());
        };
        if !self.campaigns.contains_key(&sessions.campaign) {
            return Err(format!(
                "no fixture describes the campaign {:?}",
                sessions.campaign
            ));
        }
        for session in &sessions.sessions {
            if self.named(&session.slug) {
                return Err(format!("the slug {:?} names two rows", session.slug));
            }
            if let Some(pc) = session
                .present
                .iter()
                .find(|pc| !self.element(&sessions.campaign, pc, "pj"))
            {
                return Err(format!(
                    "{pc:?}, present at {:?}, is no pj of its campaign",
                    session.slug
                ));
            }
        }
        Ok(())
    }

    /// Whether the session of the brief is one the sessions read give.
    fn brief_fits(&self) -> Result<(), String> {
        let Some(brief) = &self.brief else {
            return Ok(());
        };
        if self.session(&brief.session).is_none() {
            return Err(format!(
                "no fixture describes the session {:?}",
                brief.session
            ));
        }
        Ok(())
    }

    /// Whether each entry of the to-create list comes from one of the sessions
    /// read — which says its campaign — names no other row, and, once
    /// created, points at an element of that campaign of its own kind.
    fn to_create_fits(&self) -> Result<(), String> {
        let Some(list) = &self.to_create else {
            return Ok(());
        };
        for entry in &list.entries {
            let Some(campaign) = self.session(&entry.session) else {
                return Err(format!(
                    "no fixture describes the session {:?} of {:?}",
                    entry.session, entry.slug
                ));
            };
            let in_sessions = self
                .sessions
                .iter()
                .flat_map(|s| &s.sessions)
                .any(|s| s.slug == entry.slug);
            if in_sessions || self.named(&entry.slug) {
                return Err(format!("the slug {:?} names two rows", entry.slug));
            }
            if let Some(created) = entry
                .created
                .as_deref()
                .filter(|created| !self.element(campaign, created, entry.kind))
            {
                return Err(format!(
                    "the element {created:?} of {:?} is no {:?} of its campaign",
                    entry.slug, entry.kind
                ));
            }
        }
        Ok(())
    }

    /// The campaign of the session `slug`, when the sessions read give it.
    fn session(&self, slug: &str) -> Option<&str> {
        let sessions = self.sessions.as_ref()?;
        sessions
            .sessions
            .iter()
            .any(|s| s.slug == slug)
            .then_some(sessions.campaign.as_str())
    }

    /// Whether the media read fit what the other files say: their campaign
    /// is one of them, each place a « lieu » of that campaign, and no slug
    /// of theirs names another row. Checked once every file is read.
    fn library_fits(&self) -> Result<(), String> {
        let Some(library) = &self.library else {
            return Ok(());
        };
        if !self.campaigns.contains_key(&library.campaign) {
            return Err(format!(
                "no fixture describes the campaign {:?}",
                library.campaign
            ));
        }
        let entities = self.entities.as_ref();
        let lieu = |slug: &str| {
            entities.is_some_and(|e| {
                e.campaign == library.campaign
                    && e.elements
                        .iter()
                        .any(|element| element.slug == slug && element.kind == "lieu")
            })
        };
        let taken = |slug: &String| {
            self.campaigns.contains_key(slug)
                || entities.is_some_and(|e| {
                    e.players.iter().any(|p| &p.slug == slug)
                        || e.elements.iter().any(|element| &element.slug == slug)
                })
        };
        for medium in &library.media {
            if let Some(place) = medium.place.as_deref().filter(|place| !lieu(place)) {
                return Err(format!(
                    "the place {place:?} of {:?} is no lieu of its campaign",
                    medium.slug
                ));
            }
            if taken(&medium.slug) {
                return Err(format!("the slug {:?} names two rows", medium.slug));
            }
        }
        Ok(())
    }

    /// The one session of each oneshot no session fixture describes, as its
    /// creation writes it: the first, the microphone off, titled after the
    /// oneshot. Closed on the day it was last played, else in preparation
    /// for its next day; named `session:<oneshot slug>:1`.
    fn oneshot_sessions(&self) -> Vec<Value> {
        let described = self.sessions.as_ref().map(|s| s.campaign.as_str());
        let mut rows = Vec::new();
        for (slug, row) in &self.campaigns {
            if row.get("kind") != Some(&json!("oneshot")) || described == Some(slug) {
                continue;
            }
            let (played, next) = self.days.get(slug).cloned().unwrap_or_default();
            let (status, planned_on) = if played.is_null() {
                ("prep", next)
            } else {
                ("closed", played)
            };
            rows.push(json!({
                "id": id("session", &format!("{slug}:1")), "campaign_id": id("campaign", slug),
                "position": 1, "title": row.get("name"), "planned_on": planned_on,
                "status": status, "microphone_on": false, "summary": null,
            }));
        }
        rows
    }

    /// The rows of everything read, in the order they are inserted.
    fn plan(self, plan: &mut Fixtures) {
        let oneshots = self.oneshot_sessions();
        let campaign_row = |slug: &String, row: &Map<String, Value>| {
            let mut out = json!({ "id": id("campaign", slug) });
            for column in CAMPAIGN_DATED.iter().skip(1) {
                out[column] = row.get(*column).cloned().unwrap_or(Value::Null);
            }
            out
        };
        let (dated, plain): (Vec<_>, Vec<_>) = self
            .campaigns
            .iter()
            .partition(|(_, row)| row.contains_key("created_at"));
        let mut names: Vec<(String, String)> = self
            .campaigns
            .keys()
            .map(|slug| (slug.clone(), id("campaign", slug)))
            .collect();
        let mut tables = vec![
            table(
                "campaign",
                "id",
                CAMPAIGN_DATED,
                dated.iter().map(|(s, r)| campaign_row(s, r)).collect(),
            ),
            table(
                "campaign",
                "id",
                CAMPAIGN,
                plain.iter().map(|(s, r)| campaign_row(s, r)).collect(),
            ),
        ];
        if let Some(entities) = self.entities {
            let campaign = id("campaign", &entities.campaign);
            let players = entities.players.iter().map(|player| {
                names.push((player.slug.clone(), id("player", &player.slug)));
                json!({ "id": id("player", &player.slug), "campaign_id": campaign,
                        "name": player.name })
            });
            tables.push(table("player", "id", PLAYER, players.collect()));
            let mut rows = Rows::default();
            for (index, element) in entities.elements.iter().enumerate() {
                names.push((element.slug.clone(), id("entity", &element.slug)));
                rows.add(&campaign, index, element);
            }
            tables.extend([
                table("entity", "id", ENTITY, rows.entities),
                table("character", "entity_id", CHARACTER, rows.characters),
                table("entity_alias", "id", ALIAS, rows.aliases),
                table("entity_version", "id", VERSION, rows.versions),
            ]);
        }
        if let Some(library) = self.library {
            let campaign = id("campaign", &library.campaign);
            let mut media = Vec::new();
            let mut grids = Vec::new();
            for medium in library.media {
                let row = id("media", &medium.slug);
                media.push(json!({
                    "id": row, "campaign_id": campaign, "kind": medium.kind,
                    "title": medium.title, "file_path": medium.file,
                    "width_px": medium.width, "height_px": medium.height,
                    "place_id": medium.place.map(|place| id("entity", &place)),
                    "uploaded_at": medium.uploaded_at,
                }));
                if let Some(mut grid) = medium.grid {
                    grid["media_id"] = json!(row);
                    grids.push(grid);
                }
                names.push((medium.slug, row));
            }
            tables.extend([
                table("media", "id", MEDIA, media),
                table("battlemap_grid", "media_id", GRID, grids),
            ]);
        }
        if !oneshots.is_empty() {
            tables.push(table("session", "id", SESSION, oneshots));
        }
        if let Some(sessions) = &self.sessions {
            let campaign = id("campaign", &sessions.campaign);
            let mut rows = Vec::new();
            let mut present = Vec::new();
            for session in &sessions.sessions {
                let row = id("session", &session.slug);
                rows.push(json!({
                    "id": row, "campaign_id": campaign, "position": session.position,
                    "title": session.title, "planned_on": session.planned_on,
                    "status": session.status, "microphone_on": false,
                    "summary": session.summary,
                }));
                for pc in &session.present {
                    // A list reads them by `added_at`: the stamps keep the fixture's order.
                    present.push(json!({
                        "session_id": row, "character_id": id("entity", pc),
                        "added_at": stamp(FIRST, present.len()),
                    }));
                }
                names.push((session.slug.clone(), row));
            }
            tables.extend([
                table("session", "id", SESSION, rows),
                table(
                    "session_present_character",
                    "session_id,character_id",
                    PRESENCE,
                    present,
                ),
            ]);
        }
        let brief = self.brief;
        if let Some(brief) = &brief {
            let at = format!("{}Z", brief.at);
            tables.push(table(
                "oral_brief",
                "id",
                BRIEF,
                vec![json!({
                    "id": id("oral_brief", &brief.session),
                    "session_id": id("session", &brief.session),
                    "transcript": brief.transcript, "duration_seconds": brief.seconds,
                    "recorded_at": at, "analyzed_at": brief.analyzed.then_some(&at),
                })],
            ));
        }
        // Fitted: each entry comes from one of the sessions, which says its campaign.
        if let (Some(list), Some(sessions)) = (self.to_create, &self.sessions) {
            let campaign = id("campaign", &sessions.campaign);
            let mut rows = Vec::new();
            for (index, entry) in list.entries.into_iter().enumerate() {
                // Counted from the brief that listed it, when the fixtures have it.
                let brief = brief.as_ref().filter(|b| b.session == entry.session);
                let row = id("entity_to_create", &entry.slug);
                rows.push(json!({
                    "id": row, "campaign_id": campaign,
                    "session_id": id("session", &entry.session),
                    "brief_id": brief.map(|b| id("oral_brief", &b.session)),
                    "type": entry.kind, "name": entry.name, "excerpt": entry.excerpt,
                    "status": if entry.created.is_some() { "creee" } else { "a_creer" },
                    "created_entity_id": entry.created.map(|created| id("entity", &created)),
                    "created_at": stamp(brief.map_or(FIRST, |b| b.at.as_str()), index),
                }));
                names.push((entry.slug, row));
            }
            tables.push(table("entity_to_create", "id", TO_CREATE, rows));
        }
        names.sort();
        plan.tables = tables;
        plan.names = names;
    }
}

/// The rows of the Entity aggregate's tables.
#[derive(Default)]
struct Rows {
    entities: Vec<Value>,
    characters: Vec<Value>,
    aliases: Vec<Value>,
    versions: Vec<Value>,
}

impl Rows {
    /// The rows of `element`, the `index`th of its campaign: its row, its
    /// sheet, its aliases, and the first version of its history — the Entity
    /// aggregate refuses an element that no version records.
    fn add(&mut self, campaign: &str, index: usize, element: &Element) {
        let entity = id("entity", &element.slug);
        let at = stamp(FIRST, index);
        self.entities.push(json!({
            "id": entity, "campaign_id": campaign, "type": element.kind,
            "name": element.name, "description": element.description,
            "notes": element.notes, "created_at": at, "updated_at": at,
        }));
        for alias in &element.aliases {
            self.aliases.push(json!({
                "id": id("entity_alias", &format!("{}:{alias}", element.slug)),
                "entity_id": entity, "alias": alias,
            }));
        }
        // EntityVersion@1: the snapshot is the whole state, and the first
        // version's diff compares it with nothing.
        let mut snapshot = Map::new();
        snapshot.insert("name".into(), element.name.clone());
        snapshot.insert("type".into(), json!(element.kind));
        snapshot.insert("description".into(), element.description.clone());
        snapshot.insert("notes".into(), element.notes.clone());
        snapshot.insert("aliases".into(), json!(element.aliases));
        if let Some(sheet) = &element.sheet {
            let player = element.player.as_deref().map(|p| id("player", p));
            let mut row = sheet.clone();
            row.insert("entity_id".into(), json!(entity));
            row.insert("player_id".into(), json!(player));
            for (column, value) in &row {
                if column != "entity_id" {
                    snapshot.insert(camel(column), value.clone());
                }
            }
            snapshot.insert("spells".into(), json!([]));
            self.characters.push(Value::Object(row));
        }
        snapshot.retain(|_, value| !value.is_null());
        let diff: Vec<Value> = snapshot
            .iter()
            .map(|(field, new)| json!({ "field": field, "old": null, "new": new }))
            .collect();
        self.versions.push(json!({
            "id": id("entity_version", &format!("{}:1", element.slug)),
            "entity_id": entity, "version_number": 1, "source": "dm",
            "snapshot": snapshot, "diff": diff, "created_at": at,
        }));
    }
}

/// One element of `entities.json`, mapped; `kind` is none for a character,
/// whose own `kind` says what it is.
fn element_of(value: &Value, kind: Option<&'static str>) -> Result<Element, String> {
    let slug = required(value, "id")?.to_string();
    let kind = match (kind, value["kind"].as_str()) {
        (Some(kind), _) => kind,
        (None, Some("pj")) => "pj",
        (None, Some("pnj")) => "pnj",
        (None, other) => return Err(format!("{slug} is of kind {other:?}")),
    };
    // An alias listed twice is one.
    let mut aliases: Vec<String> = Vec::new();
    for alias in value["aliases"].as_array().into_iter().flatten() {
        if let Some(alias) = alias.as_str().filter(|a| !aliases.iter().any(|k| k == a)) {
            aliases.push(alias.to_string());
        }
    }
    let sheet = if matches!(kind, "pj" | "pnj") {
        Some(sheet_of(value)?)
    } else {
        None
    };
    Ok(Element {
        name: json!(required(value, "name")?),
        description: value["description"].clone(),
        notes: value["notes"].clone(),
        player: value["player"].as_str().map(str::to_string),
        slug,
        kind,
        aliases,
        sheet,
    })
}

/// One medium of `media.json`, mapped, of `kind`: a battlemap gives its
/// size in pixels and its grid, an illustration its size or its ratio.
fn medium_of(value: &Value, kind: &'static str) -> Result<Medium, String> {
    let slug = required(value, "id")?.to_string();
    let uploaded_at = date(value, "uploadedAt")?;
    if uploaded_at.is_null() {
        return Err(format!("{slug} has no uploadedAt"));
    }
    let pixels = |key: &str| value[key].as_u64().filter(|n| *n > 0);
    let (width, height) = match (pixels("width"), pixels("height"), kind) {
        (Some(width), Some(height), _) => (width, height),
        (_, _, "illustration") => {
            let ratio = value["ratio"]
                .as_f64()
                .filter(|r| *r > 0.0 && *r < 100.0)
                .ok_or_else(|| format!("{slug} has no size and no ratio"))?;
            // Below 100, a ratio times 1000 is a whole number of pixels.
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let width = (ratio * 1000.0).round() as u64;
            (width.max(1), 1000)
        }
        _ => return Err(format!("{slug} has no size in pixels")),
    };
    let grid = if kind == "battlemap" {
        Some(grid_of(&slug, &value["grid"])?)
    } else {
        None
    };
    Ok(Medium {
        title: required(value, "title")?.to_string(),
        file: required(value, "file")?.to_string(),
        place: value["place"].as_str().map(str::to_string),
        slug,
        kind,
        width,
        height,
        uploaded_at,
        grid,
    })
}

/// One session of `SessionList.json`, mapped: `played` is the table's
/// `closed`, the other two statuses are its own.
fn session_of(value: &Value) -> Result<Session, String> {
    let slug = required(value, "id")?.to_string();
    let status = match value["status"].as_str() {
        Some("prep") => "prep",
        Some("live") => "live",
        Some("played") => "closed",
        other => return Err(format!("{slug} is {other:?}")),
    };
    let position = value["order"]
        .as_i64()
        .and_then(|n| i32::try_from(n).ok())
        .ok_or_else(|| format!("{slug} has no order"))?;
    // The column holds a day, not a time.
    let planned_on = match date(value, "date")? {
        Value::String(at) => json!(&at[..10]),
        other => other,
    };
    // A PC listed twice is present once.
    let mut present: Vec<String> = Vec::new();
    for pc in array(&value["present"], "present")? {
        let pc = pc.as_str().ok_or("a PC present is not a text")?;
        if !present.iter().any(|known| known == pc) {
            present.push(pc.to_string());
        }
    }
    Ok(Session {
        title: required(value, "title")?.to_string(),
        summary: value["summary"].clone(),
        slug,
        position,
        planned_on,
        status,
        present,
    })
}

/// A duration written `mm:ss` (`04:12`), in seconds.
fn seconds_of(text: &str) -> Result<i32, String> {
    let seconds = text.split_once(':').and_then(|(minutes, seconds)| {
        let (minutes, seconds) = (minutes.parse::<i32>().ok()?, seconds.parse::<i32>().ok()?);
        (minutes >= 0 && (0..60).contains(&seconds))
            .then(|| minutes.checked_mul(60)?.checked_add(seconds))
    });
    seconds
        .flatten()
        .ok_or_else(|| format!("the duration {text:?} is not mm:ss"))
}

/// One entry of `ToCreateList.json`, mapped: `todo` is the table's
/// `a_creer`, and `created` its `creee`, which names the element created —
/// an entry is created if and only if it does.
fn entry_of(value: &Value) -> Result<Entry, String> {
    let slug = required(value, "id")?.to_string();
    let kind = match value["type"].as_str() {
        Some("pnj") => "pnj",
        Some("lieu") => "lieu",
        Some("evenement") => "evenement",
        other => return Err(format!("{slug} is of type {other:?}")),
    };
    let created = match (value["status"].as_str(), value["entity"].as_str()) {
        (Some("todo"), None) => None,
        (Some("created"), Some(entity)) => Some(entity.to_string()),
        (status, entity) => {
            return Err(format!("{slug} is {status:?} with the element {entity:?}"));
        }
    };
    Ok(Entry {
        name: required(value, "name")?.to_string(),
        excerpt: required(value, "excerpt")?.to_string(),
        session: required(value, "session")?.to_string(),
        slug,
        kind,
        created,
    })
}

/// A battlemap's grid as the columns of `battlemap_grid`: its orientation
/// as the table names it, its opacity a fraction of 1 where the fixture
/// gives a percentage.
fn grid_of(slug: &str, grid: &Value) -> Result<Value, String> {
    let number = |key: &str| {
        grid[key]
            .as_f64()
            .ok_or_else(|| format!("the grid of {slug} has no {key}"))
    };
    let size = number("size")?;
    let opacity = number("opacity")?;
    if size <= 0.0 || !(0.0..=100.0).contains(&opacity) {
        return Err(format!(
            "the grid of {slug} is out of range: size {size}, opacity {opacity}"
        ));
    }
    let orientation = match grid["orient"].as_str() {
        Some("pointy") => "pointe",
        Some("flat") => "plat",
        other => return Err(format!("the grid of {slug} is oriented {other:?}")),
    };
    Ok(json!({
        "hex_size_px": size, "offset_x": number("ox")?, "offset_y": number("oy")?,
        "orientation": orientation, "color": required(grid, "color")?,
        "opacity": opacity / 100.0,
    }))
}

/// A character's sheet as the columns of `character` that do not point at
/// another row.
fn sheet_of(value: &Value) -> Result<Map<String, Value>, String> {
    let mut sheet = Map::new();
    sheet.insert("initial_prompt".into(), value["prompt"].clone());
    sheet.insert("background".into(), value["background"].clone());
    sheet.insert("behavior".into(), value["behavior"].clone());
    for (label, column) in ABILITIES {
        sheet.insert(column.into(), value["stats"][label].clone());
    }
    sheet.insert("armor_class".into(), value["ca"].clone());
    sheet.insert("hp_max".into(), value["pvMax"].clone());
    sheet.insert(
        "actions".into(),
        value["features"]
            .as_array()
            .map_or_else(|| json!([]), |f| json!(f)),
    );
    sheet.insert("token_color".into(), json!(required(value, "color")?));
    let beyond = value["ddb"]["url"]
        .as_str()
        .and_then(|url| url.rsplit('/').next())
        .filter(|id| !id.is_empty());
    sheet.insert("dndbeyond_id".into(), json!(beyond));
    // The fixture writes a time with no zone: UTC.
    sheet.insert(
        "dndbeyond_synced_at".into(),
        value["ddb"]["syncedAt"]
            .as_str()
            .map_or(Value::Null, |at| json!(format!("{at}Z"))),
    );
    Ok(sheet)
}

fn table(
    name: &'static str,
    key: &'static str,
    columns: &'static [&'static str],
    rows: Vec<Value>,
) -> data::Table {
    data::Table {
        name,
        key,
        columns,
        rows,
    }
}

/// `armor_class` as `armorClass`, the name a snapshot gives the field.
fn camel(column: &str) -> String {
    let mut out = String::new();
    let mut upper = false;
    for c in column.chars() {
        if c == '_' {
            upper = true;
        } else if upper {
            out.extend(c.to_uppercase());
            upper = false;
        } else {
            out.push(c);
        }
    }
    out
}

/// A text the fixture must give.
fn required<'a>(value: &'a Value, key: &str) -> Result<&'a str, String> {
    value[key]
        .as_str()
        .ok_or_else(|| format!("{key} is not a text"))
}

fn array<'a>(value: &'a Value, what: &str) -> Result<&'a Vec<Value>, String> {
    value
        .as_array()
        .ok_or_else(|| format!("{what} is not a list"))
}

/// The whole number the digits of `text` at `range` write.
fn digits(text: &str, range: std::ops::Range<usize>) -> Option<u32> {
    let digits = text.get(range)?;
    if digits.bytes().all(|b| b.is_ascii_digit()) {
        digits.parse().ok()
    } else {
        None
    }
}

/// Whether `text` is a day of the calendar, written `2026-08-20`.
fn is_day(text: &str) -> bool {
    let (Some(year), Some(month), Some(day)) =
        (digits(text, 0..4), digits(text, 5..7), digits(text, 8..10))
    else {
        return false;
    };
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let last = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => 28 + u32::from(leap),
        _ => return false,
    };
    text.len() == 10
        && text.as_bytes()[4] == b'-'
        && text.as_bytes()[7] == b'-'
        && year > 0
        && (1..=last).contains(&day)
}

/// Whether `text` is a time with no zone, written `2026-10-14T19:31:00`.
fn is_time(text: &str) -> bool {
    let (Some(hour), Some(minute), Some(second)) = (
        digits(text, 11..13),
        digits(text, 14..16),
        digits(text, 17..19),
    ) else {
        return false;
    };
    text.len() == 19
        && text.get(..10).is_some_and(is_day)
        && text.as_bytes()[10] == b'T'
        && text.as_bytes()[13] == b':'
        && text.as_bytes()[16] == b':'
        && hour < 24
        && minute < 60
        && second < 60
}

/// The day `key` of `value` (`2026-07-11`), as a `date` column holds it, or
/// null when it has none.
fn day(value: &Value, key: &str) -> Result<Value, String> {
    Ok(match date(value, key)? {
        Value::String(at) => json!(&at[..10]),
        other => other,
    })
}

/// A day (`2026-08-20`) as the midnight UTC the column holds; null stays null.
fn date(value: &Value, key: &str) -> Result<Value, String> {
    match value[key].as_str() {
        None => Ok(Value::Null),
        Some(day) if is_day(day) => Ok(json!(format!("{day}T00:00:00Z"))),
        Some(day) => Err(format!("{key} {day:?} is not a day")),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::path::PathBuf;

    use super::*;

    /// A folder of its own under the system's temporary one, holding `files`.
    fn folder(name: &str, files: &[(&str, &str)]) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("fixtures-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("a temporary folder");
        for (file, text) in files {
            std::fs::write(dir.join(file), text).expect("a fixture file");
        }
        dir
    }

    fn mock() -> Fixtures {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../mock/fixtures");
        fixtures(&dir).expect("the mock-up's fixtures")
    }

    fn rows<'a>(plan: &'a Fixtures, table: &str) -> Vec<&'a Value> {
        plan.tables
            .iter()
            .filter(|t| t.name == table)
            .flat_map(|t| &t.rows)
            .collect()
    }

    #[test]
    fn every_json_fixture_is_read_and_skipped_with_why() {
        let dir = folder(
            "skipped",
            &[
                ("b.json", "{\"items\": []}"),
                ("a.json", "not json"),
                ("notes.txt", "not a fixture"),
            ],
        );
        let plan = fixtures(&dir).expect("the folder is listed");
        std::fs::remove_dir_all(&dir).ok();
        let names: Vec<&str> = plan.skipped.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, ["a.json", "b.json"]);
        assert!(
            plan.skipped[0].1.starts_with("not JSON"),
            "{:?}",
            plan.skipped
        );
        assert_eq!(plan.skipped[1].1, "no aggregate holds it");
        assert!(plan.loaded.is_empty());
        assert!(plan.names.is_empty());
    }

    #[test]
    fn a_missing_fixtures_folder_is_an_error_the_caller_logs() {
        let dir = std::env::temp_dir().join(format!("fixtures-absent-{}", std::process::id()));
        assert!(fixtures(&dir).is_err());
    }

    #[test]
    fn the_namespace_and_the_naming_of_an_id_never_change() {
        // A change gives every fixture row a new id, and a restart loads them twice
        // (the value is `uuid5(NAMESPACE, "campaign:cendres-valombre")`).
        assert_eq!(
            id("campaign", "cendres-valombre"),
            "98dfbfcc-6867-5ab8-a414-f9047309de2f"
        );
        assert_ne!(
            id("entity", "elowen"),
            id("player", "elowen"),
            "the table is in the name"
        );
        assert_eq!(id("entity", "elowen"), id("entity", "elowen"));
    }

    #[test]
    fn the_campaign_fixtures_of_the_mock_up_are_planned_and_the_rest_is_skipped() {
        let plan = mock();
        assert_eq!(
            plan.loaded,
            [
                "OralBrief.json",
                "SessionList.json",
                "ToCreateList.json",
                "campaign.getCampaign.json",
                "campaign.listCampaigns.json",
                "entities.json",
                "media.json"
            ]
        );
        assert!(plan.skipped.len() > 30, "{}", plan.skipped.len());
        assert!(
            plan.skipped
                .iter()
                .all(|(_, why)| why == "no aggregate holds it"),
            "{:?}",
            plan.skipped
        );
        let tables: Vec<&str> = plan.tables.iter().map(|t| t.name).collect();
        assert_eq!(
            tables,
            [
                "campaign",
                "campaign",
                "player",
                "entity",
                "character",
                "entity_alias",
                "entity_version",
                "media",
                "battlemap_grid",
                "session",
                "session",
                "session_present_character",
                "oral_brief",
                "entity_to_create"
            ]
        );
    }

    #[test]
    fn the_media_of_the_mock_up_are_planned_with_their_grids_and_places() {
        let plan = mock();
        let media = rows(&plan, "media");
        assert_eq!(media.len(), 7, "two battlemaps, five illustrations");
        let campaign = id("campaign", "cendres-valombre");
        assert!(media.iter().all(|m| m["campaign_id"] == campaign));
        let medium = |slug: &str| {
            media
                .iter()
                .find(|m| m["id"] == id("media", slug))
                .copied()
                .expect("a planned media")
        };
        let moulin = medium("bm-moulin");
        assert_eq!(moulin["kind"], "battlemap");
        assert_eq!(moulin["file_path"], "ruines-du-moulin.jpg");
        assert_eq!(
            (&moulin["width_px"], &moulin["height_px"]),
            (&json!(900), &json!(600))
        );
        assert_eq!(moulin["place_id"], id("entity", "moulin"));
        assert_eq!(moulin["uploaded_at"], "2026-09-12T00:00:00Z");
        assert_eq!(medium("bm-gue")["place_id"], Value::Null);
        // An illustration has a ratio: 1000 pixels high, as wide as it says.
        let cover = medium("ill-cover");
        assert_eq!(cover["kind"], "illustration");
        assert_eq!(
            (&cover["width_px"], &cover["height_px"]),
            (&json!(1780), &json!(1000))
        );
        assert_eq!(cover["place_id"], id("entity", "valombre"));

        let grids = rows(&plan, "battlemap_grid");
        assert_eq!(grids.len(), 2, "one grid per battlemap");
        let grid = grids
            .iter()
            .find(|g| g["media_id"] == id("media", "bm-moulin"))
            .expect("the mill's grid");
        assert_eq!(
            **grid,
            json!({ "media_id": id("media", "bm-moulin"), "hex_size_px": 34.0,
                "offset_x": 6.0, "offset_y": 10.0, "orientation": "plat",
                "color": "accent", "opacity": 0.55 })
        );
        let gue = grids
            .iter()
            .find(|g| g["media_id"] == id("media", "bm-gue"))
            .expect("the ford's grid");
        assert_eq!(gue["orientation"], "pointe");
        assert!(
            plan.names
                .contains(&("bm-gue".to_string(), id("media", "bm-gue")))
        );
    }

    /// The plan of `entities.json` and `media` (as `media.json`), alone.
    fn library(media: &Value) -> Fixtures {
        let entities = std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../mock/fixtures/entities.json"),
        )
        .expect("the fixture");
        // A folder of its own each time: the tests run side by side.
        static PLANNED: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = PLANNED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = folder(
            &format!("library-{n}"),
            &[
                ("entities.json", &entities),
                ("media.json", &media.to_string()),
            ],
        );
        let plan = fixtures(&dir).expect("the folder is listed");
        std::fs::remove_dir_all(&dir).ok();
        plan
    }

    /// A battlemap of `place`, oriented `orient`.
    fn battlemap(place: Value, orient: &str) -> Value {
        json!({ "id": "bm-x", "title": "X", "file": "x.jpg", "uploadedAt": "2026-10-14",
            "width": 960, "height": 640, "place": place,
            "grid": { "size": 30, "ox": 0, "oy": 0, "orient": orient, "color": "text",
                "opacity": 40 } })
    }

    /// The library of Valombre holding `battlemap` and `illustration`.
    fn holding(battlemap: Value, illustration: Value) -> Value {
        json!({ "campaign": "cendres-valombre", "battlemaps": [battlemap],
            "illustrations": [illustration] })
    }

    /// An illustration, its size or ratio given by `size`.
    fn illustration(size: Value) -> Value {
        let mut illustration =
            json!({ "id": "ill-x", "title": "X", "file": "x.png", "uploadedAt": "2026-10-14" });
        for (key, value) in size.as_object().into_iter().flatten() {
            illustration[key] = value.clone();
        }
        illustration
    }

    #[test]
    fn a_medium_is_sized_in_whole_pixels_or_by_a_ratio_below_one_hundred() {
        let fits = |plan: &Fixtures| plan.loaded.iter().any(|name| name == "media.json");
        let plain = || battlemap(Value::Null, "flat");
        for (size, width) in [
            (json!({ "width": 640, "height": 480 }), 640),
            // A size of no pixel is no size: the ratio says it.
            (json!({ "width": 0, "height": 480, "ratio": 1.5 }), 1500),
            (json!({ "ratio": 99.5 }), 99_500),
        ] {
            let plan = library(&holding(plain(), illustration(size.clone())));
            assert!(fits(&plan), "{size}: {:?}", plan.skipped);
            let media = rows(&plan, "media");
            let planned = media
                .iter()
                .find(|m| m["id"] == id("media", "ill-x"))
                .expect("the illustration");
            assert_eq!(planned["width_px"], width, "{size}");
        }
        for size in [
            json!({ "ratio": 0.0 }),
            json!({ "ratio": 100.0 }),
            json!({}),
        ] {
            let plan = library(&holding(plain(), illustration(size.clone())));
            assert!(!fits(&plan), "{size}");
            assert!(plan.skipped[0].1.contains("no size and no ratio"), "{size}");
        }
        let mut flat = plain();
        flat["width"] = json!(0);
        let plan = library(&holding(flat, illustration(json!({ "ratio": 1.5 }))));
        assert!(!fits(&plan));
        assert!(
            plan.skipped[0].1.contains("no size in pixels"),
            "{:?}",
            plan.skipped
        );
    }

    #[test]
    fn a_grid_has_a_size_and_an_opacity_of_at_most_one_hundred() {
        let plain = || illustration(json!({ "ratio": 1.5 }));
        let grid = |size: f64, opacity: f64| {
            let mut map = battlemap(Value::Null, "flat");
            map["grid"]["size"] = json!(size);
            map["grid"]["opacity"] = json!(opacity);
            library(&holding(map, plain()))
        };
        for (size, opacity) in [(0.0, 40.0), (30.0, 101.0), (30.0, -1.0)] {
            let plan = grid(size, opacity);
            assert!(
                plan.skipped
                    .iter()
                    .any(|(name, why)| name == "media.json" && why.contains("out of range")),
                "{size} {opacity}: {:?}",
                plan.skipped
            );
        }
        let full = grid(30.0, 100.0);
        assert_eq!(rows(&full, "battlemap_grid")[0]["opacity"], 1.0);
    }

    #[test]
    fn a_library_that_does_not_fit_is_skipped_whole_and_the_rest_is_loaded() {
        let cases = [
            (
                json!({ "campaign": "cendres-valombre",
                    "battlemaps": [battlemap(json!("elowen"), "flat")], "illustrations": [] }),
                "no lieu",
            ),
            (
                json!({ "campaign": "cendres-valombre",
                    "battlemaps": [battlemap(json!("moulin"), "sideways")], "illustrations": [] }),
                "oriented",
            ),
            (
                json!({ "campaign": "nowhere",
                    "battlemaps": [battlemap(Value::Null, "flat")], "illustrations": [] }),
                "no fixture describes the campaign",
            ),
            (
                json!({ "campaign": "cendres-valombre", "battlemaps": [],
                    "illustrations": [{ "id": "elowen", "title": "E", "file": "e.png",
                        "uploadedAt": "2026-10-14", "ratio": 1.5 }] }),
                "names two rows",
            ),
            (
                json!({ "campaign": "cendres-valombre", "battlemaps": [],
                    "illustrations": [{ "id": "ill-x", "title": "X", "file": "x.png",
                        "uploadedAt": "2026-10-14" }] }),
                "no size and no ratio",
            ),
        ];
        for (media, why) in cases {
            let plan = library(&media);
            assert_eq!(plan.loaded, ["entities.json"], "{why}");
            assert_eq!(plan.skipped.len(), 1, "{why}");
            assert_eq!(plan.skipped[0].0, "media.json");
            assert!(plan.skipped[0].1.contains(why), "{why}: {:?}", plan.skipped);
            assert!(rows(&plan, "media").is_empty(), "{why}");
            assert!(rows(&plan, "battlemap_grid").is_empty(), "{why}");
            assert!(!plan.names.iter().any(|(slug, _)| slug == "bm-x"), "{why}");
        }
        let fits = library(&json!({ "campaign": "cendres-valombre",
            "battlemaps": [battlemap(json!("moulin"), "pointy")], "illustrations": [] }));
        assert_eq!(fits.loaded, ["entities.json", "media.json"]);
        assert_eq!(rows(&fits, "battlemap_grid")[0]["orientation"], "pointe");
        assert_eq!(rows(&fits, "battlemap_grid")[0]["opacity"], 0.4);
    }

    #[test]
    fn every_id_and_every_reference_is_the_uuid_of_its_slug() {
        let plan = mock();
        assert_eq!(
            rows(&plan, "campaign").len(),
            3,
            "the list's three campaigns"
        );
        let campaign = id("campaign", "cendres-valombre");
        let players = rows(&plan, "player");
        assert_eq!(players.len(), 4);
        assert!(players.iter().all(|p| p["campaign_id"] == campaign));
        let entities = rows(&plan, "entity");
        assert_eq!(
            entities.len(),
            16,
            "nine characters, four places, three events"
        );
        assert!(entities.iter().all(|e| e["campaign_id"] == campaign));
        let known: BTreeSet<&str> = entities.iter().filter_map(|e| e["id"].as_str()).collect();
        assert!(known.contains(id("entity", "elowen").as_str()));

        let player_ids: BTreeSet<&str> = players.iter().filter_map(|p| p["id"].as_str()).collect();
        let sheets = rows(&plan, "character");
        assert_eq!(sheets.len(), 9);
        let player_of = |slug: &str| {
            sheets
                .iter()
                .find(|s| s["entity_id"] == id("entity", slug))
                .map(|s| s["player_id"].clone())
                .expect("a sheet")
        };
        assert_eq!(player_of("elowen"), id("player", "jou-julie"));
        assert_eq!(player_of("morlox"), Value::Null, "an NPC has no player");
        for sheet in &sheets {
            assert!(known.contains(sheet["entity_id"].as_str().unwrap_or_default()));
            if let Some(player) = sheet["player_id"].as_str() {
                assert!(player_ids.contains(player), "{player} is no planned player");
            }
        }
        for alias in rows(&plan, "entity_alias") {
            assert!(known.contains(alias["entity_id"].as_str().unwrap_or_default()));
        }
        // The names pair every slug with its uuid, so a reader keeps its own.
        for (slug, uuid) in [
            ("cendres-valombre", id("campaign", "cendres-valombre")),
            ("jou-julie", id("player", "jou-julie")),
            ("morlox", id("entity", "morlox")),
        ] {
            assert!(plan.names.contains(&(slug.to_string(), uuid)), "{slug}");
        }
    }

    #[test]
    fn a_second_plan_is_the_same_plan_so_a_restart_loads_nothing_twice() {
        assert_eq!(mock(), mock());
    }

    #[test]
    fn every_element_starts_its_history_with_a_complete_first_version() {
        let plan = mock();
        let versions = rows(&plan, "entity_version");
        assert_eq!(
            versions.len(),
            16,
            "the Entity aggregate refuses an element no version records"
        );
        let morlox = versions
            .iter()
            .find(|v| v["entity_id"] == id("entity", "morlox"))
            .expect("a version");
        assert_eq!(morlox["version_number"], 1);
        assert_eq!(morlox["source"], "dm");
        let snapshot = morlox["snapshot"].as_object().expect("a snapshot");
        assert_eq!(snapshot["type"], "pnj");
        assert_eq!(
            snapshot["aliases"],
            json!(["le gobelin", "le marchand", "Morlo"])
        );
        assert_eq!(snapshot["armorClass"], 13);
        // EntityVersion@1: the first diff compares the snapshot with nothing,
        // one entry per field, sorted.
        let diff = morlox["diff"].as_array().expect("a diff");
        let fields: Vec<&str> = diff.iter().filter_map(|c| c["field"].as_str()).collect();
        let mut sorted = fields.clone();
        sorted.sort_unstable();
        assert_eq!(fields, sorted);
        assert_eq!(fields.len(), snapshot.len());
        assert!(
            diff.iter().all(|c| c["old"].is_null()
                && c["new"] == snapshot[c["field"].as_str().unwrap_or_default()])
        );
    }

    #[test]
    fn a_sheet_keeps_what_the_columns_hold() {
        let plan = mock();
        let elowen = rows(&plan, "character")
            .into_iter()
            .find(|s| s["entity_id"] == id("entity", "elowen"))
            .expect("a sheet");
        assert_eq!(elowen["dexterity"], 18);
        assert_eq!(elowen["armor_class"], 16);
        assert_eq!(elowen["hp_max"], 44);
        assert_eq!(elowen["token_color"], "success");
        assert_eq!(elowen["dndbeyond_id"], "112847390");
        assert_eq!(elowen["dndbeyond_synced_at"], "2026-09-26T18:12:00Z");
        assert_eq!(elowen["actions"].as_array().map(Vec::len), Some(4));
        // The portrait waits for the Media aggregate: no column is given for it.
        assert!(!CHARACTER.contains(&"portrait_media_id"));
        assert!(!CHARACTER.contains(&"token_crop"));
    }

    #[test]
    fn a_file_that_does_not_map_is_skipped_whole_and_leaves_no_row() {
        let entities = json!({
            "campaign": { "id": "c", "name": "C" },
            "players": [{ "id": "p", "name": "P" }],
            "characters": [
                { "id": "a", "name": "A", "kind": "pj", "color": "success" },
                { "id": "b", "name": "B", "kind": "boss", "color": "success" },
            ],
            "places": [], "events": [],
        });
        let dir = folder("half", &[("entities.json", &entities.to_string())]);
        let plan = fixtures(&dir).expect("the folder is listed");
        std::fs::remove_dir_all(&dir).ok();
        assert!(plan.loaded.is_empty(), "{:?}", plan.loaded);
        assert_eq!(plan.skipped.len(), 1);
        assert!(
            plan.skipped[0].1.starts_with("cannot be mapped"),
            "{:?}",
            plan.skipped
        );
        assert!(
            plan.tables.iter().all(|t| t.rows.is_empty()),
            "nothing half-planned"
        );
        assert!(plan.names.is_empty());
    }

    #[test]
    fn a_pc_without_a_token_color_does_not_map() {
        let entities = json!({
            "campaign": { "id": "c", "name": "C" }, "players": [],
            "characters": [{ "id": "a", "name": "A", "kind": "pj" }],
            "places": [], "events": [],
        });
        let dir = folder("color", &[("entities.json", &entities.to_string())]);
        let plan = fixtures(&dir).expect("the folder is listed");
        std::fs::remove_dir_all(&dir).ok();
        assert!(plan.skipped[0].1.contains("color"), "{:?}", plan.skipped);
    }

    #[test]
    fn entities_alone_plan_their_campaign_as_a_campagne() {
        let entities = json!({
            "campaign": { "id": "c", "name": "C" }, "players": [],
            "characters": [], "places": [], "events": [
                { "id": "e", "name": "E", "aliases": [], "description": "d" }],
        });
        let dir = folder("alone", &[("entities.json", &entities.to_string())]);
        let plan = fixtures(&dir).expect("the folder is listed");
        std::fs::remove_dir_all(&dir).ok();
        assert_eq!(plan.loaded, ["entities.json"]);
        let campaigns = rows(&plan, "campaign");
        assert_eq!(campaigns.len(), 1);
        assert_eq!(campaigns[0]["kind"], "campagne");
        assert_eq!(campaigns[0]["id"], id("campaign", "c"));
        assert_eq!(rows(&plan, "entity")[0]["type"], "evenement");
    }

    #[test]
    fn a_slug_that_names_two_rows_does_not_map() {
        for (what, entities) in [
            (
                "a player and an element",
                json!({ "campaign": { "id": "c", "name": "C" },
                    "players": [{ "id": "x", "name": "P" }], "characters": [],
                    "places": [{ "id": "x", "name": "L" }], "events": [] }),
            ),
            (
                "an element and its campaign",
                json!({ "campaign": { "id": "x", "name": "C" }, "players": [],
                    "characters": [], "places": [{ "id": "x", "name": "L" }], "events": [] }),
            ),
        ] {
            let dir = folder("twice", &[("entities.json", &entities.to_string())]);
            let plan = fixtures(&dir).expect("the folder is listed");
            std::fs::remove_dir_all(&dir).ok();
            assert!(plan.loaded.is_empty(), "{what}");
            assert!(
                plan.skipped[0].1.contains("names two rows"),
                "{what}: {:?}",
                plan.skipped
            );
        }
    }

    #[test]
    fn a_slug_a_campaign_list_already_names_is_not_an_element_s() {
        let list = json!([{ "id": "x", "name": "X", "type": "campagne" }]);
        let entities = json!({ "campaign": { "id": "c", "name": "C" }, "players": [],
            "characters": [], "places": [{ "id": "x", "name": "L" }], "events": [] });
        let dir = folder(
            "listed",
            &[
                ("campaign.listCampaigns.json", &list.to_string()),
                ("entities.json", &entities.to_string()),
            ],
        );
        let plan = fixtures(&dir).expect("the folder is listed");
        std::fs::remove_dir_all(&dir).ok();
        assert_eq!(plan.loaded, ["campaign.listCampaigns.json"]);
        assert!(
            plan.skipped[0].1.contains("names two rows"),
            "{:?}",
            plan.skipped
        );
    }

    #[test]
    fn however_many_elements_there_are_each_gets_a_valid_stamp() {
        let places: Vec<Value> = (0..3601)
            .map(|n| json!({ "id": format!("p{n}"), "name": format!("Lieu {n}") }))
            .collect();
        let entities = json!({ "campaign": { "id": "c", "name": "C" }, "players": [],
            "characters": [], "places": places, "events": [] });
        let dir = folder("many", &[("entities.json", &entities.to_string())]);
        let plan = fixtures(&dir).expect("the folder is listed");
        std::fs::remove_dir_all(&dir).ok();
        let stamps: Vec<&str> = rows(&plan, "entity")
            .iter()
            .filter_map(|e| e["created_at"].as_str())
            .collect();
        assert_eq!(stamps.len(), 3601);
        assert_eq!(stamps[3600], "2026-08-20T10:00:00.003600Z");
        assert!(stamps.windows(2).all(|w| w[0] < w[1]), "in fixture order");
    }

    #[test]
    fn an_alias_listed_twice_is_one_row() {
        let entities = json!({ "campaign": { "id": "c", "name": "C" }, "players": [],
            "characters": [], "places": [], "events": [
                { "id": "e", "name": "E", "aliases": ["la nuit", "la nuit"] }] });
        let dir = folder("dup", &[("entities.json", &entities.to_string())]);
        let plan = fixtures(&dir).expect("the folder is listed");
        std::fs::remove_dir_all(&dir).ok();
        let aliases = rows(&plan, "entity_alias");
        assert_eq!(aliases.len(), 1, "{aliases:?}");
        let version = rows(&plan, "entity_version");
        assert_eq!(version[0]["snapshot"]["aliases"], json!(["la nuit"]));
    }

    #[test]
    fn the_campaign_a_fixture_describes_keeps_its_dates_and_cover_fields() {
        let plan = mock();
        let cendres = rows(&plan, "campaign")
            .into_iter()
            .find(|c| c["id"] == id("campaign", "cendres-valombre"))
            .expect("a campaign");
        assert_eq!(cendres["kind"], "campagne");
        assert_eq!(cendres["art"], "ember");
        assert_eq!(cendres["glyph"], "V");
        assert_eq!(cendres["created_at"], "2026-08-20T00:00:00Z");
        assert_eq!(cendres["last_exported_at"], "2026-10-04T00:00:00Z");
        // The campaigns no fixture dates take the table's default.
        let oneshot = rows(&plan, "campaign")
            .into_iter()
            .find(|c| c["id"] == id("campaign", "nuit-carnaval"))
            .expect("a campaign");
        assert_eq!(oneshot["kind"], "oneshot");
        assert!(oneshot["created_at"].is_null(), "{oneshot}");
        let undated = plan
            .tables
            .iter()
            .find(|t| t.name == "campaign" && t.rows.contains(oneshot))
            .expect("a table");
        assert!(
            !undated.columns.contains(&"created_at"),
            "the column is left to its default"
        );
    }

    #[test]
    fn a_day_that_is_not_a_day_does_not_map() {
        let details = json!({ "id": "c", "name": "C", "type": "campagne",
            "createdAt": "20 août 2026", "lastExport": null });
        let dir = folder(
            "day",
            &[("campaign.getCampaign.json", &details.to_string())],
        );
        let plan = fixtures(&dir).expect("the folder is listed");
        std::fs::remove_dir_all(&dir).ok();
        assert!(plan.loaded.is_empty(), "{:?}", plan.loaded);
        assert!(
            plan.skipped[0].1.contains("is not a day"),
            "{:?}",
            plan.skipped
        );
    }

    /// The mock-up's fixtures, the files named in `over` replaced by theirs.
    /// The sessions planned for the campaign `slug`.
    fn sessions_of<'a>(plan: &'a Fixtures, slug: &str) -> Vec<&'a Value> {
        let campaign = id("campaign", slug);
        rows(plan, "session")
            .into_iter()
            .filter(|s| s["campaign_id"] == campaign)
            .collect()
    }

    #[test]
    fn the_oneshot_of_the_mock_up_has_its_one_session_closed_on_its_last_day() {
        let plan = mock();
        let oneshot = sessions_of(&plan, "nuit-carnaval");
        assert_eq!(
            oneshot,
            [&json!({ "id": id("session", "nuit-carnaval:1"),
                "campaign_id": id("campaign", "nuit-carnaval"), "position": 1,
                "title": "La Nuit du Carnaval Pourpre", "planned_on": "2026-07-11",
                "status": "closed", "microphone_on": false, "summary": null })]
        );
        assert!(
            sessions_of(&plan, "heritiers-ebene").is_empty(),
            "a campagne no session fixture describes has none"
        );
    }

    #[test]
    fn a_oneshot_never_played_is_in_preparation_for_its_next_day() {
        let list = json!([
            { "id": "o", "name": "O", "type": "oneshot", "lastPlayed": null, "next": "2026-11-02" },
            { "id": "p", "name": "P", "type": "oneshot" },
        ]);
        let plan = replaced(&[("campaign.listCampaigns.json", list)]);
        let next = sessions_of(&plan, "o");
        assert_eq!(next.len(), 1, "{next:?}");
        assert_eq!(next[0]["status"], "prep");
        assert_eq!(next[0]["planned_on"], "2026-11-02");
        assert_eq!(next[0]["title"], "O");
        let undated = sessions_of(&plan, "p");
        assert_eq!(undated.len(), 1, "{undated:?}");
        assert_eq!(undated[0]["status"], "prep");
        assert!(undated[0]["planned_on"].is_null());
    }

    #[test]
    fn a_oneshot_the_session_fixture_describes_gets_no_other_session() {
        let mut list = mock_file("campaign.listCampaigns.json");
        list[0]["type"] = json!("oneshot");
        list[0]["lastPlayed"] = json!("2026-10-03");
        let plan = replaced(&[("campaign.listCampaigns.json", list)]);
        let sessions = sessions_of(&plan, "cendres-valombre");
        assert_eq!(sessions.len(), 4, "the fixture's, and none added");
        assert!(
            sessions
                .iter()
                .all(|s| s["title"] != "Les Cendres de Valombre")
        );
    }

    #[test]
    fn a_campaign_list_with_a_day_that_is_none_is_skipped() {
        let list = json!([{ "id": "o", "name": "O", "type": "oneshot", "lastPlayed": "hier" }]);
        let plan = replaced(&[("campaign.listCampaigns.json", list)]);
        assert!(
            plan.skipped
                .iter()
                .any(|(n, why)| n == "campaign.listCampaigns.json" && why.contains("not a day")),
            "{:?}",
            plan.skipped
        );
        assert!(sessions_of(&plan, "o").is_empty());
    }

    fn replaced(over: &[(&str, Value)]) -> Fixtures {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../mock/fixtures");
        let mut files: Vec<(String, String)> = std::fs::read_dir(&dir)
            .expect("the fixtures folder")
            .filter_map(|entry| {
                let path = entry.ok()?.path();
                let name = path.file_name()?.to_str()?.to_string();
                let json = path.extension().is_some_and(|e| e == "json");
                json.then(|| (name, std::fs::read_to_string(&path).expect("a fixture")))
            })
            .collect();
        for (name, value) in over {
            files.retain(|(file, _)| file != name);
            files.push((name.to_string(), value.to_string()));
        }
        static PLANNED: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = PLANNED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let files: Vec<(&str, &str)> = files
            .iter()
            .map(|(name, text)| (name.as_str(), text.as_str()))
            .collect();
        let folder = folder(&format!("sessions-{n}"), &files);
        let plan = fixtures(&folder).expect("the folder is listed");
        std::fs::remove_dir_all(&folder).ok();
        plan
    }

    /// The files left out for what they say, not for being nobody's.
    fn unmapped(plan: &Fixtures) -> Vec<&Skipped> {
        plan.skipped
            .iter()
            .filter(|(_, why)| why != "no aggregate holds it")
            .collect()
    }

    /// A fixture of the mock-up, as JSON.
    fn mock_file(name: &str) -> Value {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../mock/fixtures");
        serde_json::from_str(&std::fs::read_to_string(path.join(name)).expect("the fixture"))
            .expect("JSON")
    }

    #[test]
    fn the_sessions_of_the_mock_up_are_planned_with_the_pcs_present() {
        let plan = mock();
        let sessions = sessions_of(&plan, "cendres-valombre");
        assert_eq!(sessions.len(), 4);
        assert!(sessions.iter().all(|s| s["microphone_on"] == false));
        let session = |slug: &str| {
            sessions
                .iter()
                .find(|s| s["id"] == id("session", slug))
                .copied()
                .expect("a planned session")
        };
        let second = session("s2");
        assert_eq!(second["position"], 2);
        assert_eq!(second["title"], "Le moulin en flammes");
        assert_eq!(second["planned_on"], "2026-09-19");
        assert_eq!(second["status"], "closed", "played is closed");
        assert!(
            second["summary"]
                .as_str()
                .is_some_and(|s| s.starts_with("Le moulin brûle"))
        );
        assert_eq!(session("s4")["status"], "prep");
        let order: Vec<i64> = sessions
            .iter()
            .filter_map(|s| s["position"].as_i64())
            .collect();
        assert_eq!(order, [1, 2, 3, 4], "in the fixture's order");

        let present = rows(&plan, "session_present_character");
        assert_eq!(present.len(), 4 + 3 + 4 + 4);
        let at = |slug: &str| -> BTreeSet<&str> {
            present
                .iter()
                .filter(|p| p["session_id"] == id("session", slug))
                .filter_map(|p| p["character_id"].as_str())
                .collect()
        };
        let kael = id("entity", "kael");
        let elowen = id("entity", "elowen");
        assert_eq!(at("s2").len(), 3);
        assert!(at("s2").contains(elowen.as_str()) && at("s2").contains(kael.as_str()));
        assert!(!at("s2").contains(id("entity", "sirielle").as_str()));
        let pairs: BTreeSet<(&str, &str)> = present
            .iter()
            .filter_map(|p| Some((p["session_id"].as_str()?, p["character_id"].as_str()?)))
            .collect();
        assert_eq!(
            pairs.len(),
            present.len(),
            "a PC is present once per session"
        );
        let stamps: Vec<&str> = present
            .iter()
            .filter(|p| p["session_id"] == id("session", "s1"))
            .filter_map(|p| p["added_at"].as_str())
            .collect();
        assert_eq!(stamps.len(), 4);
        assert!(
            stamps.windows(2).all(|w| w[0] < w[1]),
            "in the fixture's order"
        );
        assert!(
            plan.tables.iter().any(
                |t| t.name == "session_present_character" && t.key == "session_id,character_id"
            )
        );
        for (slug, uuid) in [("s1", id("session", "s1")), ("s4", id("session", "s4"))] {
            assert!(plan.names.contains(&(slug.to_string(), uuid)), "{slug}");
        }
    }

    #[test]
    fn the_last_brief_of_the_mock_up_is_one_analysed_row_of_text() {
        let plan = mock();
        let briefs = rows(&plan, "oral_brief");
        assert_eq!(briefs.len(), 1);
        let brief = briefs[0];
        assert_eq!(brief["id"], id("oral_brief", "s4"));
        assert_eq!(brief["session_id"], id("session", "s4"));
        assert_eq!(brief["duration_seconds"], 4 * 60 + 12);
        assert_eq!(brief["recorded_at"], "2026-10-14T19:31:00Z");
        assert_eq!(brief["analyzed_at"], "2026-10-14T19:31:00Z");
        let transcript = brief["transcript"].as_str().expect("a text");
        let lines: Vec<&str> = transcript.lines().collect();
        assert_eq!(lines.len(), 7, "one line per line of the fixture");
        assert_eq!(lines[0], "Bon, séance quatre, l’embuscade du gué.");
    }

    #[test]
    fn the_to_create_list_of_the_mock_up_is_planned_with_what_became_of_each_entry() {
        let plan = mock();
        let entries = rows(&plan, "entity_to_create");
        assert_eq!(entries.len(), 3);
        let entry = |slug: &str| {
            entries
                .iter()
                .find(|e| e["id"] == id("entity_to_create", slug))
                .copied()
                .expect("a planned entry")
        };
        let campaign = id("campaign", "cendres-valombre");
        assert!(entries.iter().all(|e| e["campaign_id"] == campaign));
        assert!(
            entries
                .iter()
                .all(|e| e["session_id"] == id("session", "s4"))
        );
        assert!(
            entries
                .iter()
                .all(|e| e["brief_id"] == id("oral_brief", "s4"))
        );
        let odric = entry("tc-odric");
        assert_eq!(odric["status"], "a_creer");
        assert_eq!(odric["type"], "pnj");
        assert_eq!(odric["name"], "Frère Odric");
        assert!(odric["created_entity_id"].is_null());
        assert!(
            odric["excerpt"]
                .as_str()
                .is_some_and(|e| e.contains("frère Odric"))
        );
        let vesna = entry("tc-vesna");
        assert_eq!(vesna["status"], "creee");
        assert_eq!(vesna["created_entity_id"], id("entity", "vesna"));
        assert_eq!(entry("tc-gue")["type"], "lieu");
        // The list keeps the fixture's order, from the brief's time.
        let stamps: Vec<&str> = ["tc-vesna", "tc-odric", "tc-gue"]
            .iter()
            .filter_map(|slug| entry(slug)["created_at"].as_str())
            .collect();
        assert_eq!(stamps[0], "2026-10-14T19:31:00.000000Z");
        assert!(stamps.windows(2).all(|w| w[0] < w[1]), "{stamps:?}");
        assert!(
            plan.names
                .contains(&("tc-odric".to_string(), id("entity_to_create", "tc-odric")))
        );
    }

    #[test]
    fn an_entry_whose_session_has_no_brief_is_not_tied_to_one() {
        let mut list = mock_file(TO_CREATE_FILE);
        list["items"][1]["session"] = json!("s3");
        let plan = replaced(&[(TO_CREATE_FILE, list)]);
        let entries = rows(&plan, "entity_to_create");
        let odric = entries
            .iter()
            .find(|e| e["id"] == id("entity_to_create", "tc-odric"))
            .expect("an entry");
        assert!(odric["brief_id"].is_null(), "{odric}");
        assert_eq!(odric["session_id"], id("session", "s3"));
        assert_eq!(odric["created_at"], "2026-08-20T10:00:00.000001Z");
    }

    #[test]
    fn a_day_a_summary_or_a_pc_a_fixture_leaves_out_is_null_or_not_loaded() {
        let mut list = mock_file(SESSIONS);
        list["sessions"][0]["date"] = Value::Null;
        list["sessions"][0]["summary"] = Value::Null;
        list["sessions"][0]["present"] = json!(["elowen", "elowen"]);
        let plan = replaced(&[(SESSIONS, list)]);
        assert!(
            plan.skipped.iter().all(|(n, _)| n != SESSIONS),
            "{:?}",
            plan.skipped
        );
        let sessions = rows(&plan, "session");
        let first = sessions
            .iter()
            .find(|s| s["id"] == id("session", "s1"))
            .expect("a session");
        assert!(first["planned_on"].is_null() && first["summary"].is_null());
        let present = rows(&plan, "session_present_character");
        let first_present = present
            .iter()
            .filter(|p| p["session_id"] == id("session", "s1"))
            .count();
        assert_eq!(first_present, 1, "a PC listed twice is present once");
    }

    #[test]
    fn a_session_file_that_does_not_fit_is_skipped_with_what_hangs_on_it() {
        let sessions = |edit: &dyn Fn(&mut Value)| {
            let mut list = mock_file(SESSIONS);
            edit(&mut list);
            list
        };
        let cases = [
            (
                "a PC that is an NPC",
                sessions(&|l| l["sessions"][0]["present"] = json!(["morlox"])),
                "is no pj",
            ),
            (
                "a PC nobody describes",
                sessions(&|l| l["sessions"][0]["present"] = json!(["nobody"])),
                "is no pj",
            ),
            (
                "a campaign nobody describes",
                sessions(&|l| l["campaign"] = json!("nowhere")),
                "no fixture describes the campaign",
            ),
            (
                "a slug an element has",
                sessions(&|l| l["sessions"][0]["id"] = json!("elowen")),
                "names two rows",
            ),
            (
                "a slug a medium has",
                sessions(&|l| l["sessions"][0]["id"] = json!("bm-gue")),
                "names two rows",
            ),
            (
                "a slug twice",
                sessions(&|l| l["sessions"][1]["id"] = json!("s1")),
                "names two rows",
            ),
            (
                "a status of nobody's",
                sessions(&|l| l["sessions"][0]["status"] = json!("over")),
                "over",
            ),
            (
                "no order",
                sessions(&|l| l["sessions"][0]["order"] = json!("first")),
                "no order",
            ),
            (
                "a day that is not a day",
                sessions(&|l| l["sessions"][0]["date"] = json!("5 septembre")),
                "is not a day",
            ),
            (
                "a day the calendar does not have",
                sessions(&|l| l["sessions"][0]["date"] = json!("2026-02-30")),
                "is not a day",
            ),
        ];
        for (what, list, why) in cases {
            let plan = replaced(&[(SESSIONS, list)]);
            let left = unmapped(&plan);
            let names: Vec<&str> = left.iter().map(|(n, _)| n.as_str()).collect();
            // The brief and the list name a session: they go with the sessions.
            assert_eq!(names, [SESSIONS, BRIEF_FILE, TO_CREATE_FILE], "{what}");
            assert!(left[0].1.contains(why), "{what}: {:?}", left[0]);
            assert!(left[1].1.contains("no fixture describes the session"));
            for file in [SESSIONS, BRIEF_FILE, TO_CREATE_FILE] {
                assert!(!plan.loaded.iter().any(|n| n == file), "{what}: {file}");
            }
            assert!(plan.loaded.iter().any(|n| n == "entities.json"), "{what}");
            // The oneshot's own session hangs on no file.
            assert!(
                sessions_of(&plan, "cendres-valombre").is_empty(),
                "{what}: session"
            );
            for table in [
                "session_present_character",
                "oral_brief",
                "entity_to_create",
            ] {
                assert!(rows(&plan, table).is_empty(), "{what}: {table}");
            }
            assert!(
                !plan
                    .names
                    .iter()
                    .any(|(slug, _)| slug == "s4" || slug == "tc-odric")
            );
        }
    }

    #[test]
    fn a_brief_or_a_list_that_does_not_fit_is_skipped_alone() {
        let brief = |edit: &dyn Fn(&mut Value)| {
            let mut file = mock_file(BRIEF_FILE);
            edit(&mut file);
            (BRIEF_FILE, file)
        };
        let list = |edit: &dyn Fn(&mut Value)| {
            let mut file = mock_file(TO_CREATE_FILE);
            edit(&mut file);
            (TO_CREATE_FILE, file)
        };
        let cases = [
            (
                "a session nobody describes",
                brief(&|b| b["session"] = json!("s9")),
                "no fixture describes the session",
            ),
            (
                "no zone-less time",
                brief(&|b| b["lastRun"] = json!("yesterday")),
                "is not a time",
            ),
            (
                "a time with a zone",
                brief(&|b| b["lastRun"] = json!("2026-10-14T19:31:00.5Z")),
                "is not a time",
            ),
            (
                "a duration in words",
                brief(&|b| b["duration"] = json!("four")),
                "is not mm:ss",
            ),
            (
                "a line that is no text",
                brief(&|b| b["transcript"][0] = json!(1)),
                "not a text",
            ),
            (
                "a session nobody describes",
                list(&|l| l["items"][0]["session"] = json!("s9")),
                "no fixture describes the session",
            ),
            (
                "a type nobody has",
                list(&|l| l["items"][0]["type"] = json!("monstre")),
                "of type",
            ),
            (
                "created with no element",
                list(&|l| l["items"][0]["entity"] = Value::Null),
                "with the element",
            ),
            (
                "to do with an element",
                list(&|l| l["items"][1]["entity"] = json!("vesna")),
                "with the element",
            ),
            (
                "an element nobody describes",
                list(&|l| l["items"][0]["entity"] = json!("nobody")),
                "is no \"pnj\"",
            ),
            (
                "an element of another kind",
                list(&|l| l["items"][0]["entity"] = json!("valombre")),
                "is no \"pnj\"",
            ),
            (
                "a slug an element has",
                list(&|l| l["items"][0]["id"] = json!("elowen")),
                "names two rows",
            ),
            (
                "a slug a session has",
                list(&|l| l["items"][0]["id"] = json!("s1")),
                "names two rows",
            ),
            (
                "a slug twice",
                list(&|l| l["items"][1]["id"] = json!("tc-vesna")),
                "names two rows",
            ),
        ];
        for (what, (file, value), why) in cases {
            let plan = replaced(&[(file, value)]);
            let left = unmapped(&plan);
            assert_eq!(left.len(), 1, "{what}: {left:?}");
            assert_eq!(left[0].0, file, "{what}");
            assert!(left[0].1.contains(why), "{what}: {:?}", left[0]);
            assert_eq!(
                sessions_of(&plan, "cendres-valombre").len(),
                4,
                "{what}: the sessions are loaded"
            );
            let table = if file == BRIEF_FILE {
                "oral_brief"
            } else {
                "entity_to_create"
            };
            assert!(rows(&plan, table).is_empty(), "{what}");
        }
    }

    #[test]
    fn a_duration_is_minutes_and_seconds() {
        assert_eq!(seconds_of("04:12"), Ok(252));
        assert_eq!(seconds_of("75:00"), Ok(4500));
        assert_eq!(seconds_of("00:00"), Ok(0));
        for text in [
            "4",
            "04:60",
            "-1:00",
            "04:-1",
            "a:b",
            "",
            "99999999999:00",
            "40000000:00",
        ] {
            assert!(seconds_of(text).is_err(), "{text:?}");
        }
    }

    #[test]
    fn an_entry_whose_brief_is_withdrawn_is_still_planned_with_no_brief() {
        let mut brief = mock_file(BRIEF_FILE);
        brief["session"] = json!("s9");
        let plan = replaced(&[(BRIEF_FILE, brief)]);
        let entries = rows(&plan, "entity_to_create");
        assert_eq!(entries.len(), 3);
        assert!(
            entries.iter().all(|e| e["brief_id"].is_null()),
            "{entries:?}"
        );
    }

    #[test]
    fn a_day_and_a_time_are_the_calendar_s_and_the_clock_s() {
        for day in [
            "2026-08-20",
            "2024-02-29",
            "2000-02-29",
            "2026-12-31",
            "0001-01-01",
        ] {
            assert!(is_day(day), "{day}");
        }
        for day in [
            "",
            "2026-8-20",
            "2026-08-2",
            "2026-02-29",
            "1900-02-29",
            "2026-04-31",
            "2026-00-10",
            "2026-13-10",
            "2026-08-00",
            "2026-08-32",
            "0000-01-01",
            "2026/08/20",
            "2026-08-20T",
            "+026-08-20",
            "2026-08-+1",
            "20 août 2026",
        ] {
            assert!(!is_day(day), "{day:?}");
        }
        for time in [
            "2026-10-14T19:31:00",
            "2026-10-14T00:00:00",
            "2026-10-14T23:59:59",
        ] {
            assert!(is_time(time), "{time}");
        }
        for time in [
            "",
            "2026-10-14",
            "2026-10-14 19:31:00",
            "2026-10-14T24:00:00",
            "2026-10-14T19:60:00",
            "2026-10-14T19:31:60",
            "2026-02-30T10:00:00",
            "2026-10-14T19:31:00Z",
            "2026-10-14T19-31-00",
            "2026-10-14T19:31-00",
            "2026-10-14T1:31:000",
            "2026-10-14T19:31:0é",
        ] {
            assert!(!is_time(time), "{time:?}");
        }
    }
}
