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
//! The history of an element (`campaign.listEntityVersions.json`) is its
//! versions, from the first: the last is the element as `entities.json` has
//! it, and each earlier one the state before the changes of the next, put
//! back field by field from the labels of the screen (`Notes`, `Alias`,
//! `Statblock`…). A label the tables hold no column for (`Niveau`, `Token`)
//! moves nothing, and a version's date is the fixture's, in UTC. Its origin
//! comes to a `source`: an import and the DM's hand are `dm`, a brief is
//! `brief`, the rest of the AI's work `ia`, a resync `dndbeyond` — and the
//! first version is always the DM's, a creation being no resync. The elements
//! with no history have the one version they always had. A version whose
//! changes go beyond what its source may change (a resync's stats, a
//! proposal's sheet fields: the version trigger's scopes) skips the file
//! whole. An element the store already versioned otherwise (the app's own
//! versions, or the lone first version of an earlier load) keeps what it has:
//! [`Fixtures::leave_out_taken_versions`] drops the fixture's versions of it
//! before the load, the trigger taking only the next number. The versions go in
//! by round, every first version before any second: a version must be the
//! next of its element, and one statement inserts its rows in no order.
//! `campaign.getEntity.json` and the three lists repeat `entities.json`.
//!
//! What the tables have no column for is not loaded: the `seen` flag and the
//! free text of a version's `source`; the portrait and the
//! crop, the speed and the descriptive `type` line of a sheet, the art and
//! illustration of a place, the links between places and characters; of a
//! media, its weight, its terrain and its art. Nor is the campaign's cover:
//! `campaign.cover_media_id` names a media row, and a media row names its
//! campaign, so the one cannot be inserted before the other. An illustration
//! is described by its ratio alone: it is loaded as 1000 pixels high, and as
//! wide as its ratio says. Of a session, the `played` status is the table's
//! `closed`, and no start or close time is loaded: the fixtures of the Session
//! aggregate give the sessions, the PCs present at each, the scenes of
//! `SceneBoard.json`, the closed session's journal and summary of
//! `PreviousSummary.json`, the last brief and the to-create list.
//! A oneshot those fixtures do not describe gets the one session its
//! creation writes (`oneshot-single-session`), titled after it: closed on the
//! day the campaign list says it was last played, else in preparation for
//! its next day.
//!
//! The session under way is the exception, and `session.getLiveSession.json`
//! names it: that session of `SessionList.json` (which still says `prep`) is
//! loaded `live`, started on its day at the file's `startedAt` (`HH:MM`, UTC),
//! so the DM view finds the running session. It is the one session that is
//! live, a session going from `prep` to `live` once (`status-lifecycle`); a
//! file that names a session already played, a day other than the session's,
//! or no session at all is skipped, and the session stays as the list has it.
//!
//! A scene is one row of `scene` (its order is its rank, from 1 to n), and
//! each place, character, event and medium it lists one row of `scene_entity`
//! (ranked from 1 in the order of the fixture: places, characters, events) or
//! `scene_media`. What a scene lists is of the session's campaign. The scene's
//! `toCreate` is another row's link: not loaded here. Its `battlemap` is the
//! instance below, which names the scene.
//!
//! The instance of a scene's battlemap (`table.getBattlemapInstance.json`; the
//! same one is in `Battlemap.json`, not read) is one `battlemap_instance` row of
//! the scene and the battlemap medium it is a copy of, and one `token` row per
//! token, at its hex: the scene names it as its `battlemap`, and
//! `session.listScenes` answers the row. Its roster belongs to a combat
//! starting; one already running is not loaded, and a file that has one is
//! skipped. The initiative each PC was last given (`pjInitiative`) is the
//! seat of one combat that ended on the instance — round 1, its two stamps
//! those of the session's start (the first stamp of the fixtures when the
//! instance is of another session), a microsecond apart —, one `initiative_entry`
//! per PC, ranked from the highest: the next combat starts from them. A figure
//! outside 1 to 40, or a key that is no PC's character with a token, skips the
//! file. The tokens obey the aggregate's invariants as
//! a command would (`token-roster`, `multi-drop-numbering`, `pj-hp-not-tracked`,
//! `dead-derived`): a file that breaks one is skipped, not loaded.
//! A scene the store already gave an instance, under another id (the DM's), is
//! left that one alone: [`Fixtures::leave_out_taken_scenes`] drops the
//! fixture's instance, its tokens, its combat and the combat's seats before
//! the load.
//!
//! The session under way is loaded live only if the store holds no other live
//! session, of any campaign: [`Fixtures::leave_out_second_live`] leaves it in
//! preparation, with its diffusion, before the load.
//!
//! What the session under way has shown its players (`live.getDiffusion.json`)
//! is one `diffusion` row of that session: the cover, no window, the framing
//! at rest (zoom 1, pan 0). It is what `live.showWindowOnPlayerView` finds to
//! open a window on; a file that shows more than the cover, a cover other than
//! the one `campaign.getCampaign.json` gives the session's campaign, or that no
//! session under way receives, is skipped. A restart leaves the row the players' view
//! has since changed alone.
//!
//! The summary of the closed session, its model and the day it was edited
//! (read from `summaryMeta`, « Généré à la clôture par <model>, retouché le
//! 4 oct. 2026 ») are `session.summary`, `summary_model` and
//! `summary_edited_at`; `PreviousSummary.json`'s summary wins over
//! `SessionList.json`'s for that one session, and the two files carry the
//! same text for it (a test holds them), so the panel and the list agree. Its journal is one
//! `session_journal_line` per line, named `session_journal_line:<session
//! slug>:<n>`, spoken on the session's day at the line's `HH:MM` (UTC), a
//! microsecond apart so that two lines of one minute keep their order. A journal
//! that runs past midnight does not map: its lines would sort before the evening's.
//!
//! The campaign's sounds (`sounds.json`) are one row of `sound` each, named
//! by their slug (`snd-porte`): the name, the tags (normalised as
//! `tags-normalized` asks, by the add-sound unit's own function), the file, and the
//! volume as a fraction of 1 (the file says percents), stamped in the order of
//! the file so that the soundbox lists them as it does. The campaign's
//! `masterVolume` is its `sound_master_volume`, and every other campaign takes
//! the column's default, 1. A sound's duration has no column and is not
//! loaded, nor is the `soundbox.listSounds.json` that repeats the file. The
//! master volume is the campaign row's, inserted once: a campaign the store
//! already holds keeps the volume it has.
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
const CAMPAIGN: &[&str] = &[
    "id",
    "name",
    "kind",
    "pitch",
    "art",
    "glyph",
    "sound_master_volume",
];
const CAMPAIGN_DATED: &[&str] = &[
    "id",
    "name",
    "kind",
    "pitch",
    "art",
    "glyph",
    "sound_master_volume",
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
const SOUND: &[&str] = &[
    "id",
    "campaign_id",
    "name",
    "file_path",
    "tags",
    "volume",
    "created_at",
];
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
    "started_at",
    "microphone_on",
    "summary",
    "summary_model",
    "summary_edited_at",
];
const PRESENCE: &[&str] = &["session_id", "character_id", "added_at"];
const SCENE: &[&str] = &["id", "session_id", "position", "title", "notes"];
const SCENE_ENTITY: &[&str] = &["scene_id", "entity_id", "position"];
const SCENE_MEDIA: &[&str] = &["scene_id", "media_id"];
const JOURNAL: &[&str] = &["id", "session_id", "spoken_at", "text"];
const BRIEF: &[&str] = &[
    "id",
    "session_id",
    "transcript",
    "duration_seconds",
    "recorded_at",
    "analyzed_at",
];
const INSTANCE: &[&str] = &["id", "battlemap_media_id", "session_id", "scene_id"];
const TOKEN: &[&str] = &[
    "id",
    "instance_id",
    "character_id",
    "number",
    "name",
    "q",
    "r",
    "visible",
    "hp_current",
    "hp_max",
    "conditions",
];
const COMBAT: &[&str] = &["id", "instance_id", "round", "started_at", "ended_at"];
const INITIATIVE_ENTRY: &[&str] = &[
    "id",
    "combat_id",
    "token_id",
    "initiative",
    "tiebreak",
    "rank",
];
const DIFFUSION: &[&str] = &[
    "session_id",
    "base_kind",
    "view_zoom",
    "view_pan_x",
    "view_pan_y",
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
            SOUNDS_FILE => read.sounds(&value),
            HISTORY_FILE => read.history(&value),
            SESSIONS => read.sessions(&value),
            BOARD_FILE => read.board(&value),
            CLOSING_FILE => read.closing(&value),
            BRIEF_FILE => read.brief(&value),
            TO_CREATE_FILE => read.creations(&value),
            INSTANCE_FILE => read.instance(&value),
            LIVE_FILE => read.running(&value),
            DIFFUSION_FILE => read.shown(&value),
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
    if let Err(why) = read.soundbox_fits() {
        read.soundbox = None;
        withdraw(&mut plan, SOUNDS_FILE, &why);
    }
    if let Err(why) = read.history_fits() {
        read.history = None;
        read.versions.clear();
        withdraw(&mut plan, HISTORY_FILE, &why);
    }
    if let Err(why) = read.sessions_fit() {
        read.sessions = None;
        withdraw(&mut plan, SESSIONS, &why);
    }
    if let Err(why) = read.board_fits() {
        read.board = None;
        withdraw(&mut plan, BOARD_FILE, &why);
    }
    if let Err(why) = read.closing_fits() {
        read.closing = None;
        withdraw(&mut plan, CLOSING_FILE, &why);
    }
    if let Err(why) = read.brief_fits() {
        read.brief = None;
        withdraw(&mut plan, BRIEF_FILE, &why);
    }
    if let Err(why) = read.to_create_fits() {
        read.to_create = None;
        withdraw(&mut plan, TO_CREATE_FILE, &why);
    }
    if let Err(why) = read.running_fits() {
        read.running = None;
        withdraw(&mut plan, LIVE_FILE, &why);
    }
    if let Err(why) = read.shown_fits() {
        read.diffusion = None;
        withdraw(&mut plan, DIFFUSION_FILE, &why);
    }
    if let Err(why) = read.instance_fits() {
        read.instance = None;
        withdraw(&mut plan, INSTANCE_FILE, &why);
    }
    read.plan(&mut plan);
    Ok(plan)
}

/// The fixture of a campaign's media.
const LIBRARY: &str = "media.json";
/// The fixture of the Sound aggregate: a campaign's sounds and its master
/// volume. `soundbox.listSounds.json` repeats it and is not read.
const SOUNDS_FILE: &str = "sounds.json";
/// The fixture of the Entity aggregate's version history: the versions of
/// some elements, newest first. `campaign.getEntity.json` and the three lists
/// repeat `entities.json`, which holds them, and are not read.
const HISTORY_FILE: &str = "campaign.listEntityVersions.json";
/// The fixtures of the Session aggregate: a campaign's sessions, a session's
/// scenes, a closed session's summary and journal, a session's last brief, a
/// campaign's to-create list.
const SESSIONS: &str = "SessionList.json";
const BOARD_FILE: &str = "SceneBoard.json";
const CLOSING_FILE: &str = "PreviousSummary.json";
const BRIEF_FILE: &str = "OralBrief.json";
const TO_CREATE_FILE: &str = "ToCreateList.json";
/// The fixture of the BattlemapInstance aggregate: the instance of a scene's
/// battlemap, with its tokens. `Battlemap.json` holds the same instance for
/// the Micro-UI, and is not read: it would name the same rows twice.
const INSTANCE_FILE: &str = "table.getBattlemapInstance.json";
/// The fixture of the session under way: which session of the list is live,
/// and when it started. `LiveSessionBar.json` holds the same header for the
/// Micro-UI, and is not read.
const LIVE_FILE: &str = "session.getLiveSession.json";
/// The fixture of the Diffusion aggregate: what the session under way has
/// shown its players. `PlayerStage.json` holds the same for the Micro-UI, and
/// is not read.
const DIFFUSION_FILE: &str = "live.getDiffusion.json";

impl Fixtures {
    /// Leaves out the instance planned for a scene where the store already
    /// holds an instance under another id (`one-instance-per-scene`): the
    /// DM's own would be hidden by a second, more recent one. `held` is the
    /// `(instance id, scene id)` of every instance of the store, see
    /// [`held_instances`]; its tokens, its combat and the seats of the combat
    /// go with it.
    pub fn leave_out_taken_scenes(&mut self, held: &[(String, String)]) {
        let Some(instances) = self
            .tables
            .iter_mut()
            .find(|t| t.name == "battlemap_instance")
        else {
            return;
        };
        let mut left = BTreeSet::new();
        instances.rows.retain(|row| {
            let (row_id, scene) = (row["id"].as_str(), row["scene_id"].as_str());
            let taken = held.iter().any(|(held_id, held_scene)| {
                Some(held_scene.as_str()) == scene && Some(held_id.as_str()) != row_id
            });
            if taken && let Some(row_id) = row_id {
                left.insert(row_id.to_string());
            }
            !taken
        });
        if left.is_empty() {
            return;
        }
        for table in self.tables.iter_mut().filter(|t| t.name == "token") {
            let (gone, kept): (Vec<Value>, Vec<Value>) = std::mem::take(&mut table.rows)
                .into_iter()
                .partition(|row| {
                    row["instance_id"]
                        .as_str()
                        .is_some_and(|o| left.contains(o))
                });
            left.extend(
                gone.iter()
                    .filter_map(|row| row["id"].as_str().map(str::to_string)),
            );
            table.rows = kept;
        }
        // The combat of an instance goes with it, and the seats of the combat.
        let mut ended = BTreeSet::new();
        for table in self.tables.iter_mut().filter(|t| t.name == "combat") {
            table.rows.retain(|row| {
                let gone = row["instance_id"]
                    .as_str()
                    .is_some_and(|o| left.contains(o));
                if gone && let Some(combat) = row["id"].as_str() {
                    ended.insert(combat.to_string());
                }
                !gone
            });
        }
        for table in self
            .tables
            .iter_mut()
            .filter(|t| t.name == "initiative_entry")
        {
            table.rows.retain(|row| {
                !row["combat_id"]
                    .as_str()
                    .is_some_and(|combat| ended.contains(combat))
            });
        }
        self.names.retain(|(_, row)| !left.contains(row));
        self.withdraw_with(
            INSTANCE_FILE,
            "its scene already has an instance in the store",
        );
    }

    /// Leaves out the versions planned for an element whose store already
    /// holds versions other than the planned ones (the app's own, under
    /// random ids, or the lone first version of an earlier load): the version
    /// trigger takes only the next number of an element, so a planned second
    /// version would be refused and roll the whole load back. An element the
    /// store holds none of, or exactly the planned ones (a restart), is left
    /// alone. `held` is the `(element id, version id)` of every version of the
    /// store, see [`held_versions`].
    pub fn leave_out_taken_versions(&mut self, held: &[(String, String)]) {
        let mut planned: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        for table in self.tables.iter().filter(|t| t.name == "entity_version") {
            for row in &table.rows {
                if let (Some(entity), Some(version)) =
                    (row["entity_id"].as_str(), row["id"].as_str())
                {
                    planned
                        .entry(entity.to_string())
                        .or_default()
                        .insert(version.to_string());
                }
            }
        }
        let mut stored: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
        for (entity, version) in held {
            stored.entry(entity).or_default().insert(version);
        }
        let taken: BTreeSet<String> = planned
            .iter()
            .filter(|(entity, versions)| {
                stored.get(entity.as_str()).is_some_and(|held| {
                    held.len() != versions.len()
                        || !versions.iter().all(|v| held.contains(v.as_str()))
                })
            })
            .map(|(entity, _)| entity.clone())
            .collect();
        if taken.is_empty() {
            return;
        }
        let mut left = BTreeSet::new();
        for table in self
            .tables
            .iter_mut()
            .filter(|t| t.name == "entity_version")
        {
            table.rows.retain(|row| {
                let gone = row["entity_id"]
                    .as_str()
                    .is_some_and(|entity| taken.contains(entity));
                if gone && let Some(version) = row["id"].as_str() {
                    left.insert(version.to_string());
                }
                !gone
            });
        }
        self.skipped.push((
            HISTORY_FILE.to_string(),
            format!(
                "{} element(s) keep the versions the store already holds",
                taken.len()
            ),
        ));
    }

    /// Leaves the session under way in preparation when the store already
    /// holds a live session under another id, in any campaign: two sessions
    /// live at once is not a state the app reaches. `held` is the id of every
    /// live session of the store, see [`held_live_sessions`]; the session's
    /// diffusion goes with its start.
    pub fn leave_out_second_live(&mut self, held: &[String]) {
        let mut left = BTreeSet::new();
        for table in self.tables.iter().filter(|t| t.name == "session") {
            for row in &table.rows {
                if let Some(row_id) = row["id"].as_str()
                    && row["status"] == "live"
                    && held.iter().any(|live| live != row_id)
                {
                    left.insert(row_id.to_string());
                }
            }
        }
        if left.is_empty() {
            return;
        }
        for table in self.tables.iter_mut().filter(|t| t.name == "session") {
            for row in &mut table.rows {
                if row["id"].as_str().is_some_and(|id| left.contains(id)) {
                    row["status"] = json!("prep");
                    row["started_at"] = Value::Null;
                }
            }
        }
        for table in self.tables.iter_mut().filter(|t| t.name == "diffusion") {
            table.rows.retain(|row| {
                !row["session_id"]
                    .as_str()
                    .is_some_and(|id| left.contains(id))
            });
        }
        self.withdraw_with(LIVE_FILE, "another session is live in the store");
        self.withdraw_with(DIFFUSION_FILE, "no session under way receives it");
    }

    fn withdraw_with(&mut self, file: &str, why: &str) {
        self.loaded.retain(|name| name != file);
        self.skipped.push((file.to_string(), why.to_string()));
    }
}

/// The `(instance id, scene id)` of every battlemap instance the store holds.
///
/// # Errors
///
/// [`data::Error::Database`] when the query fails.
pub async fn held_instances(client: &data::Client) -> Result<Vec<(String, String)>, data::Error> {
    let rows = client
        .query(
            "SELECT id::text, scene_id::text FROM battlemap_instance",
            &[],
        )
        .await
        .map_err(data::Error::Database)?;
    Ok(rows.iter().map(|row| (row.get(0), row.get(1))).collect())
}

/// The `(element id, version id)` of every entity version the store holds.
///
/// # Errors
///
/// [`data::Error::Database`] when the query fails.
pub async fn held_versions(client: &data::Client) -> Result<Vec<(String, String)>, data::Error> {
    let rows = client
        .query("SELECT entity_id::text, id::text FROM entity_version", &[])
        .await
        .map_err(data::Error::Database)?;
    Ok(rows.iter().map(|row| (row.get(0), row.get(1))).collect())
}

/// The id of every session the store holds live.
///
/// # Errors
///
/// [`data::Error::Database`] when the query fails.
pub async fn held_live_sessions(client: &data::Client) -> Result<Vec<String>, data::Error> {
    let rows = client
        .query("SELECT id::text FROM session WHERE status = 'live'", &[])
        .await
        .map_err(data::Error::Database)?;
    Ok(rows.iter().map(|row| row.get(0)).collect())
}

/// Takes `file` back out of the files planned: it is skipped, with `why`.
fn withdraw(plan: &mut Fixtures, file: &str, why: &str) {
    plan.loaded.retain(|name| name != file);
    plan.skipped
        .push((file.to_string(), format!("cannot be mapped: {why}")));
}

/// The first stamp of the fixtures, and the base the rows with no date of
/// their own count from, a microsecond apart.
const FIRST: &str = "2026-08-20T10:00:00";

/// How many stamps follow `at` before the microseconds run out.
const STAMPS: usize = 1_000_000;

/// Whether `count` rows can each have a stamp of their own after [`FIRST`].
fn stampable(count: usize) -> Result<(), String> {
    if count > STAMPS {
        return Err(format!("{count} rows are more than {STAMPS} stamps"));
    }
    Ok(())
}

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
    /// The cover a campaign's details name, by slug (the media id it shows).
    covers: BTreeMap<String, String>,
    entities: Option<Entities>,
    history: Option<History>,
    /// The version rows of each element `history` gives a history, by slug,
    /// made once the elements are known ([`Read::history_fits`]).
    versions: BTreeMap<String, Vec<(u64, Value)>>,
    library: Option<Library>,
    soundbox: Option<Soundbox>,
    sessions: Option<Sessions>,
    board: Option<Board>,
    closing: Option<Closing>,
    brief: Option<Brief>,
    to_create: Option<ToCreate>,
    instance: Option<Instance>,
    running: Option<Running>,
    /// The cover `live.getDiffusion.json` shows alone, which the session
    /// under way has received: the id the file gives it.
    diffusion: Option<String>,
}

/// The `session.getLiveSession.json` document, mapped: the session under way.
struct Running {
    /// The session, by slug.
    session: String,
    /// The campaign it belongs to, by slug.
    campaign: String,
    /// Its title.
    title: String,
    /// Its rank among the campaign's sessions.
    position: i32,
    /// The day it was played.
    day: String,
    /// The time of that day it started, `HH:MM`.
    started: String,
}

/// The `sounds.json` document, mapped: a campaign's soundbox.
struct Soundbox {
    campaign: String,
    /// The campaign's master volume, a fraction of 1.
    master: f64,
    sounds: Vec<Sound>,
}

struct Sound {
    slug: String,
    name: String,
    file: String,
    tags: Vec<String>,
    /// Its own volume, a fraction of 1.
    volume: f64,
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

/// The `SceneBoard.json` document, mapped: the scenes of a session.
struct Board {
    session: String,
    scenes: Vec<Scene>,
}

struct Scene {
    slug: String,
    /// Its rank, from 1.
    position: i32,
    title: String,
    /// A text, or null.
    notes: Value,
    /// What it links, by slug, each once.
    places: Vec<String>,
    characters: Vec<String>,
    events: Vec<String>,
    media: Vec<String>,
    /// Its battlemap instance, by slug.
    battlemap: Option<String>,
}

/// The `PreviousSummary.json` document, mapped: how a closed session ended.
struct Closing {
    session: String,
    /// The day it was played.
    day: String,
    summary: String,
    model: String,
    /// The day the DM last edited the summary.
    edited_on: Option<String>,
    /// The journal's lines: the `HH:MM` they were spoken at, and their text.
    journal: Vec<(String, String)>,
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

/// The `table.getBattlemapInstance.json` document, mapped: a scene's working
/// copy of a battlemap.
struct Instance {
    slug: String,
    /// The battlemap's medium, the session and the scene, by slug.
    battlemap: String,
    session: String,
    scene: String,
    tokens: Vec<Token>,
    /// The initiative each PC was last given, by character slug, the highest
    /// first (ties by slug): the seats of the combat that ended.
    initiative: Vec<(String, i64)>,
}

struct Token {
    slug: String,
    /// Its character, by slug, and whether that is a « pj » or a « pnj ».
    character: String,
    kind: &'static str,
    name: String,
    /// Its number among the tokens of its character; null for a PC.
    number: Value,
    q: i32,
    r: i32,
    visible: bool,
    /// Hit points, or null: a PC's are not tracked.
    hp: Value,
    hp_max: Value,
    conditions: Value,
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

/// The `campaign.listEntityVersions.json` document, mapped: the history of
/// some elements, by slug.
type History = BTreeMap<String, Account>;

/// One element's history, from its first version to its last.
struct Account {
    name: String,
    revisions: Vec<Revision>,
}

/// One version of an element, as the fixture tells it.
struct Revision {
    number: u64,
    /// When it was made: a time with no zone, which is UTC.
    at: String,
    /// The table's `source` the fixture's origin comes to.
    source: &'static str,
    /// What it changed: the screen's label of the field, and what it held
    /// before.
    changes: Vec<(String, Value)>,
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
        let slug = required(value, "id")?;
        if let Some(cover) = value["cover"].as_str() {
            self.covers.insert(slug.to_string(), cover.to_string());
        }
        self.campaign(slug, &row);
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

    fn history(&mut self, value: &Value) -> Result<(), String> {
        let mut history = History::new();
        for (slug, entry) in value.as_object().ok_or("the histories are not an object")? {
            let mut revisions = Vec::new();
            for version in array(&entry["versions"], "versions")? {
                revisions.push(revision_of(slug, version)?);
            }
            if revisions.is_empty() {
                return Err(format!("{slug} has no version"));
            }
            revisions.sort_by_key(|revision| revision.number);
            if !revisions.iter().zip(1..).all(|(r, n)| r.number == n) {
                return Err(format!(
                    "the versions of {slug} do not run from 1 without a gap or a repeat"
                ));
            }
            history.insert(
                slug.clone(),
                Account {
                    name: required(entry, "name")?.to_string(),
                    revisions,
                },
            );
        }
        self.history = Some(history);
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

    fn sounds(&mut self, value: &Value) -> Result<(), String> {
        let campaign = required(value, "campaign")?.to_string();
        let master = fraction(&value["masterVolume"], "masterVolume")?;
        let mut sounds = Vec::new();
        for sound in array(&value["sounds"], "sounds")? {
            sounds.push(sound_of(sound)?);
        }
        stampable(sounds.len())?;
        let mut slugs = BTreeSet::new();
        if let Some(twice) = sounds.iter().find(|s| !slugs.insert(&s.slug)) {
            return Err(format!("the slug {:?} names two rows", twice.slug));
        }
        self.soundbox = Some(Soundbox {
            campaign,
            master,
            sounds,
        });
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

    fn board(&mut self, value: &Value) -> Result<(), String> {
        let session = required(&value["session"], "id")?.to_string();
        let mut scenes = Vec::new();
        for (index, scene) in array(&value["scenes"], "scenes")?.iter().enumerate() {
            scenes.push(scene_of(scene, index)?);
        }
        let mut slugs = BTreeSet::new();
        if let Some(twice) = scenes.iter().find(|s| !slugs.insert(&s.slug)) {
            return Err(format!("the slug {:?} names two rows", twice.slug));
        }
        self.board = Some(Board { session, scenes });
        Ok(())
    }

    fn closing(&mut self, value: &Value) -> Result<(), String> {
        let day = required(&value["session"], "date")?;
        if !is_day(day) {
            return Err(format!("the date {day:?} is not a day"));
        }
        let (model, edited_on) = meta_of(required(value, "summaryMeta")?)?;
        let mut journal = Vec::new();
        let mut last = "";
        for line in array(&value["journal"], "the journal")? {
            let at = required(line, "t")?;
            if !is_clock(at) {
                return Err(format!("the time {at:?} of a line is not an HH:MM"));
            }
            if at < last {
                return Err(format!("the line at {at} is out of time order"));
            }
            last = at;
            journal.push((at.to_string(), required(line, "text")?.to_string()));
        }
        self.closing = Some(Closing {
            session: required(&value["session"], "id")?.to_string(),
            day: day.to_string(),
            summary: required(value, "summary")?.to_string(),
            model,
            edited_on,
            journal,
        });
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

    fn instance(&mut self, value: &Value) -> Result<(), String> {
        self.instance = Some(instance_of(value)?);
        Ok(())
    }

    /// `live.getDiffusion.json`: only a player view that has the cover and
    /// nothing else — no window, no framing — is loaded; the row is the
    /// session's, with the framing at rest (zoom 1, pan 0).
    fn shown(&mut self, value: &Value) -> Result<(), String> {
        if value["base"]["kind"] != "cover" {
            return Err("only the cover as a background is loaded".to_string());
        }
        if !value["window"].is_null() || !value["view"].is_null() {
            return Err("a window or a framing is not loaded".to_string());
        }
        let cover = required(&value["base"], "id")?;
        self.diffusion = Some(cover.to_string());
        Ok(())
    }

    /// Whether the cover shown is the one of the campaign of the session
    /// under way: the row names no media, so another cover would be dropped.
    fn shown_fits(&self) -> Result<(), String> {
        let Some(shown) = &self.diffusion else {
            return Ok(());
        };
        let Some(running) = &self.running else {
            return Err("no fixture describes the session under way".to_string());
        };
        match self.covers.get(&running.campaign) {
            Some(cover) if cover == shown => Ok(()),
            Some(cover) => Err(format!(
                "the cover {shown:?} is not {cover:?}, the one of the campaign {:?}",
                running.campaign
            )),
            None => Err(format!(
                "no fixture gives the cover of the campaign {:?}",
                running.campaign
            )),
        }
    }

    fn running(&mut self, value: &Value) -> Result<(), String> {
        let session = required(value, "id")?.to_string();
        let campaign = required(value, "campaign")?.to_string();
        let title = required(value, "title")?.to_string();
        let position = value["order"]
            .as_i64()
            .and_then(|n| i32::try_from(n).ok())
            .ok_or_else(|| format!("{session} has no order"))?;
        let day = required(value, "date")?.to_string();
        let started = required(value, "startedAt")?.to_string();
        if !is_day(&day) {
            return Err(format!("the day {day:?} is not a day"));
        }
        if !is_clock(&started) {
            return Err(format!("{started:?} is not a time of the day"));
        }
        self.running = Some(Running {
            session,
            campaign,
            title,
            position,
            day,
            started,
        });
        Ok(())
    }

    /// Whether a campaign, a player, an element, a medium or a sound of the
    /// files read is named `slug`: the slugs of the rows planned before the
    /// sessions.
    fn named(&self, slug: &str) -> bool {
        self.named_but_sounds(slug)
            || self
                .soundbox
                .as_ref()
                .is_some_and(|s| s.sounds.iter().any(|sound| sound.slug == slug))
    }

    /// [`Read::named`], the sounds left out: the slugs planned before them.
    fn named_but_sounds(&self, slug: &str) -> bool {
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

    /// Whether the scenes read fit what the other files say: their session is
    /// one the sessions read give, each place a « lieu », each character a
    /// « pj » or a « pnj », each event an « evenement » and each medium one of
    /// that session's campaign, and no slug of theirs names another row.
    fn board_fits(&self) -> Result<(), String> {
        let Some(board) = &self.board else {
            return Ok(());
        };
        let Some(campaign) = self.session(&board.session) else {
            return Err(format!(
                "no fixture describes the session {:?}",
                board.session
            ));
        };
        let in_sessions = |slug: &str| {
            self.sessions
                .iter()
                .flat_map(|s| &s.sessions)
                .any(|s| s.slug == slug)
        };
        let in_list = |slug: &str| {
            self.to_create
                .iter()
                .flat_map(|l| &l.entries)
                .any(|e| e.slug == slug)
        };
        for scene in &board.scenes {
            if self.named(&scene.slug) || in_sessions(&scene.slug) || in_list(&scene.slug) {
                return Err(format!("the slug {:?} names two rows", scene.slug));
            }
            let links = [
                (&scene.places, &["lieu"][..]),
                (&scene.characters, &["pj", "pnj"][..]),
                (&scene.events, &["evenement"][..]),
            ];
            for (slugs, kinds) in links {
                if let Some(slug) = slugs
                    .iter()
                    .find(|slug| !kinds.iter().any(|kind| self.element(campaign, slug, kind)))
                {
                    return Err(format!(
                        "{slug:?}, linked to {:?}, is none of {kinds:?} of its campaign",
                        scene.slug
                    ));
                }
            }
            let holds = |slug: &str| {
                self.library.as_ref().is_some_and(|l| {
                    l.campaign == campaign && l.media.iter().any(|m| m.slug == slug)
                })
            };
            if let Some(slug) = scene.media.iter().find(|slug| !holds(slug)) {
                return Err(format!(
                    "the medium {slug:?}, linked to {:?}, is none of its campaign",
                    scene.slug
                ));
            }
        }
        Ok(())
    }

    /// Whether `slug` names a row of the sessions, the scenes or the to-create
    /// list as well as one `named` does.
    fn claimed(&self, slug: &str) -> bool {
        self.named(slug) || self.claimed_but_sounds(slug)
    }

    /// [`Read::claimed`], the sounds left out: the sounds are checked against
    /// these, as they are read first.
    fn claimed_but_sounds(&self, slug: &str) -> bool {
        self.named_but_sounds(slug)
            || self
                .sessions
                .iter()
                .flat_map(|s| &s.sessions)
                .any(|s| s.slug == slug)
            || self
                .board
                .iter()
                .flat_map(|b| &b.scenes)
                .any(|s| s.slug == slug)
            || self
                .to_create
                .iter()
                .flat_map(|l| &l.entries)
                .any(|e| e.slug == slug)
    }

    /// Whether the session under way fits the list of sessions: one the list
    /// gives, still in preparation (a session goes from `prep` to `live`, and
    /// never back: `status-lifecycle`), of the list's campaign, with its title
    /// and rank, on the day the list plans it (a session with no day is not
    /// on any), and with no other session already live.
    fn running_fits(&self) -> Result<(), String> {
        let Some(running) = &self.running else {
            return Ok(());
        };
        let Some(sessions) = &self.sessions else {
            return Err(format!(
                "no fixture describes the session {:?}",
                running.session
            ));
        };
        let Some(session) = sessions.sessions.iter().find(|s| s.slug == running.session) else {
            return Err(format!(
                "no fixture describes the session {:?}",
                running.session
            ));
        };
        if session.status != "prep" {
            return Err(format!(
                "the session {:?} is {}, not in preparation",
                running.session, session.status
            ));
        }
        if sessions.campaign != running.campaign {
            return Err(format!(
                "the session {:?} is of the campaign {:?}, not of {:?}",
                running.session, sessions.campaign, running.campaign
            ));
        }
        if session.title != running.title || session.position != running.position {
            return Err(format!(
                "the session {:?} is not titled {:?}, number {}, in the list",
                running.session, running.title, running.position
            ));
        }
        if session.planned_on.as_str() != Some(running.day.as_str()) {
            return Err(format!(
                "the session {:?} is of {}, not of {}",
                running.session, session.planned_on, running.day
            ));
        }
        if let Some(other) = sessions
            .sessions
            .iter()
            .find(|s| s.status == "live" && s.slug != running.session)
        {
            return Err(format!("the session {:?} is live already", other.slug));
        }
        Ok(())
    }

    /// Whether the instance read fits what the other files say: its scene is
    /// one the scenes read give, which names it as its battlemap and links
    /// the battlemap it is the copy of, a battlemap of the session's
    /// campaign (`one-instance-per-scene`); each token stands for a « pj »
    /// present at the session or for a « pnj » of that campaign, whose hit
    /// points are capped by its sheet's (`token-roster`, `pj-hp-not-tracked`);
    /// and no slug of its own names another row.
    fn instance_fits(&self) -> Result<(), String> {
        let Some(instance) = &self.instance else {
            return Ok(());
        };
        let Some(board) = self
            .board
            .as_ref()
            .filter(|b| b.session == instance.session)
        else {
            return Err(format!(
                "no fixture describes the session {:?} and its scenes",
                instance.session
            ));
        };
        let scene = board.scenes.iter().find(|s| s.slug == instance.scene);
        let Some(scene) = scene else {
            return Err(format!(
                "{:?} is no scene of the session {:?}",
                instance.scene, instance.session
            ));
        };
        if scene.battlemap.as_deref() != Some(instance.slug.as_str()) {
            return Err(format!(
                "the scene {:?} does not name {:?} as its battlemap",
                scene.slug, instance.slug
            ));
        }
        if !scene.media.contains(&instance.battlemap) {
            return Err(format!(
                "the battlemap {:?} is not linked to {:?}",
                instance.battlemap, scene.slug
            ));
        }
        let campaign = self.session(&instance.session).unwrap_or_default();
        let holds = self.library.as_ref().is_some_and(|l| {
            l.campaign == campaign
                && l.media
                    .iter()
                    .any(|m| m.slug == instance.battlemap && m.kind == "battlemap")
        });
        if !holds {
            return Err(format!(
                "{:?} is no battlemap of its campaign",
                instance.battlemap
            ));
        }
        let present = self
            .sessions
            .iter()
            .flat_map(|s| &s.sessions)
            .find(|s| s.slug == instance.session)
            .map(|s| &s.present);
        for slug in std::iter::once(&instance.slug).chain(instance.tokens.iter().map(|t| &t.slug)) {
            if self.claimed(slug) {
                return Err(format!("the slug {slug:?} names two rows"));
            }
        }
        for token in &instance.tokens {
            let sheet = self
                .entities
                .iter()
                .filter(|e| e.campaign == campaign)
                .flat_map(|e| &e.elements)
                .find(|e| e.slug == token.character && e.kind == token.kind);
            let Some(sheet) = sheet else {
                return Err(format!(
                    "{:?}, the character of {:?}, is no {:?} of its campaign",
                    token.character, token.slug, token.kind
                ));
            };
            if token.kind == "pj" && !present.is_some_and(|p| p.contains(&token.character)) {
                return Err(format!(
                    "{:?}, with a token, is not present at {:?}",
                    token.character, instance.session
                ));
            }
            let capped = sheet.sheet.as_ref().and_then(|s| s.get("hp_max"));
            if token.kind == "pnj" && capped != Some(&token.hp_max) {
                return Err(format!(
                    "the hit points of {:?} are not those of the sheet of {:?}",
                    token.slug, token.character
                ));
            }
        }
        for (character, _) in &instance.initiative {
            if !instance
                .tokens
                .iter()
                .any(|t| t.kind == "pj" && &t.character == character)
            {
                return Err(format!("pjInitiative names {character:?}, no PC's token"));
            }
        }
        Ok(())
    }

    /// Whether the closing read fits what the sessions say: its session is a
    /// closed one of theirs, played on the day of its journal.
    fn closing_fits(&self) -> Result<(), String> {
        let Some(closing) = &self.closing else {
            return Ok(());
        };
        let Some(session) = self
            .sessions
            .iter()
            .flat_map(|s| &s.sessions)
            .find(|s| s.slug == closing.session)
        else {
            return Err(format!(
                "no fixture describes the session {:?}",
                closing.session
            ));
        };
        if session.status != "closed" {
            return Err(format!(
                "the session {:?} is {}, not closed",
                closing.session, session.status
            ));
        }
        // A session with no day of its own takes the journal's.
        if session
            .planned_on
            .as_str()
            .is_some_and(|day| day != closing.day)
        {
            return Err(format!(
                "the journal of {:?} is of {}, not of its day",
                closing.session, closing.day
            ));
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

    /// The history of each element it names is made into the version rows of
    /// that element: the last version is the element as `entities.json` has
    /// it, each earlier one the state before the changes of the next.
    fn history_fits(&mut self) -> Result<(), String> {
        let Some(history) = &self.history else {
            return Ok(());
        };
        let entities = self
            .entities
            .as_ref()
            .ok_or("no fixture holds the elements")?;
        let mut versions = BTreeMap::new();
        for (slug, account) in history {
            let element = entities
                .elements
                .iter()
                .find(|element| &element.slug == slug)
                .ok_or_else(|| format!("{slug} is no element of the campaign"))?;
            if element.name != json!(account.name) {
                return Err(format!(
                    "{slug} is called {} here and {:?} there",
                    element.name, account.name
                ));
            }
            versions.insert(slug.clone(), history_rows(element, &account.revisions)?);
        }
        self.versions = versions;
        Ok(())
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

    /// Whether the sounds read fit what the other files say: their campaign
    /// is one of them, and no slug of theirs names another row, a session, a
    /// scene or a to-create entry included: the clash leaves the soundbox
    /// out, not the sessions and what hangs on them. Checked once every file
    /// is read.
    fn soundbox_fits(&self) -> Result<(), String> {
        let Some(soundbox) = &self.soundbox else {
            return Ok(());
        };
        if !self.campaigns.contains_key(&soundbox.campaign) {
            return Err(format!(
                "no fixture describes the campaign {:?}",
                soundbox.campaign
            ));
        }
        match soundbox
            .sounds
            .iter()
            .find(|sound| self.claimed_but_sounds(&sound.slug))
        {
            Some(sound) => Err(format!("the slug {:?} names two rows", sound.slug)),
            None => Ok(()),
        }
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
        // A campaign with no soundbox takes the column's default, 1.
        let master = |slug: &String| {
            self.soundbox
                .as_ref()
                .filter(|soundbox| &soundbox.campaign == slug)
                .map_or(1.0, |soundbox| soundbox.master)
        };
        let campaign_row = |slug: &String, row: &Map<String, Value>| {
            let mut out = json!({ "id": id("campaign", slug) });
            for column in CAMPAIGN_DATED.iter().skip(1) {
                out[column] = row.get(*column).cloned().unwrap_or(Value::Null);
            }
            out["sound_master_volume"] = json!(master(slug));
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
                rows.add(&campaign, index, element, self.versions.get(&element.slug));
            }
            tables.extend([
                table("entity", "id", ENTITY, rows.entities),
                table("character", "entity_id", CHARACTER, rows.characters),
                table("entity_alias", "id", ALIAS, rows.aliases),
            ]);
            // A version must be the next of its element, and a table goes in
            // by one statement, whose rows come in no order: the Nth versions
            // of the elements go in together, the first before the second.
            tables.extend(
                rows.versions
                    .into_values()
                    .map(|round| table("entity_version", "id", VERSION, round)),
            );
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
        if let Some(soundbox) = self.soundbox {
            let campaign = id("campaign", &soundbox.campaign);
            let mut sounds = Vec::new();
            for (index, sound) in soundbox.sounds.into_iter().enumerate() {
                let row = id("sound", &sound.slug);
                sounds.push(json!({
                    "id": row, "campaign_id": campaign, "name": sound.name,
                    "file_path": sound.file, "tags": sound.tags, "volume": sound.volume,
                    "created_at": stamp(FIRST, index),
                }));
                names.push((sound.slug, row));
            }
            tables.push(table("sound", "id", SOUND, sounds));
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
                // How a closed session ended is the closing's, when the fixtures have it.
                let closing = self.closing.as_ref().filter(|c| c.session == session.slug);
                // The session under way is live, since the time of its day it started.
                let running = self.running.as_ref().filter(|r| r.session == session.slug);
                rows.push(json!({
                    "id": row, "campaign_id": campaign, "position": session.position,
                    "title": session.title, "planned_on": session.planned_on,
                    "status": if running.is_some() { "live" } else { session.status },
                    "started_at": running.map(|r| format!("{}T{}:00Z", r.day, r.started)),
                    "microphone_on": false,
                    "summary": closing.map_or(session.summary.clone(), |c| json!(c.summary)),
                    "summary_model": closing.map(|c| &c.model),
                    "summary_edited_at": closing
                        .and_then(|c| c.edited_on.as_ref())
                        .map(|day| format!("{day}T00:00:00Z")),
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
            // What the session under way has shown its players: the cover, no
            // window, the framing at rest.
            if let (Some(running), Some(_)) = (&self.running, &self.diffusion) {
                tables.push(table(
                    "diffusion",
                    "session_id",
                    DIFFUSION,
                    vec![json!({
                        "session_id": id("session", &running.session), "base_kind": "cover",
                        "view_zoom": 1, "view_pan_x": 0, "view_pan_y": 0,
                    })],
                ));
            }
            if let Some(board) = &self.board {
                let session = id("session", &board.session);
                let (mut scenes, mut entities, mut media) = (Vec::new(), Vec::new(), Vec::new());
                for scene in &board.scenes {
                    let row = id("scene", &scene.slug);
                    scenes.push(json!({
                        "id": row, "session_id": session, "position": scene.position,
                        "title": scene.title, "notes": scene.notes,
                    }));
                    let linked = [&scene.places, &scene.characters, &scene.events];
                    for (rank, slug) in linked.into_iter().flatten().enumerate() {
                        entities.push(json!({
                            "scene_id": row, "entity_id": id("entity", slug), "position": rank + 1,
                        }));
                    }
                    for slug in &scene.media {
                        media.push(json!({ "scene_id": row, "media_id": id("media", slug) }));
                    }
                    names.push((scene.slug.clone(), row));
                }
                tables.extend([
                    table("scene", "id", SCENE, scenes),
                    table("scene_entity", "scene_id,entity_id", SCENE_ENTITY, entities),
                    table("scene_media", "scene_id,media_id", SCENE_MEDIA, media),
                ]);
                // Fitted: its scene, its battlemap and its characters are planned above.
                if let Some(instance) = &self.instance {
                    let row = id("battlemap_instance", &instance.slug);
                    let tokens = instance.tokens.iter().map(|token| {
                        let token_row = id("token", &token.slug);
                        names.push((token.slug.clone(), token_row.clone()));
                        json!({
                            "id": token_row, "instance_id": row,
                            "character_id": id("entity", &token.character),
                            "number": token.number, "name": token.name,
                            "q": token.q, "r": token.r, "visible": token.visible,
                            "hp_current": token.hp, "hp_max": token.hp_max,
                            "conditions": token.conditions,
                        })
                    });
                    let tokens: Vec<Value> = tokens.collect();
                    tables.extend([
                        table(
                            "battlemap_instance",
                            "id",
                            INSTANCE,
                            vec![json!({
                                "id": row, "battlemap_media_id": id("media", &instance.battlemap),
                                "session_id": id("session", &instance.session),
                                "scene_id": id("scene", &instance.scene),
                            })],
                        ),
                        table("token", "id", TOKEN, tokens),
                    ]);
                    // The PCs' last initiative is the seats of a combat that ended.
                    if !instance.initiative.is_empty() {
                        let combat = id("combat", &instance.slug);
                        let moment = self
                            .running
                            .as_ref()
                            .filter(|r| r.session == instance.session)
                            .map_or_else(
                                || FIRST.to_string(),
                                |r| format!("{}T{}:00", r.day, r.started),
                            );
                        let mut seats = Vec::new();
                        for (rank, (character, initiative)) in
                            instance.initiative.iter().enumerate()
                        {
                            let Some(token) = instance
                                .tokens
                                .iter()
                                .find(|t| t.kind == "pj" && &t.character == character)
                            else {
                                continue;
                            };
                            seats.push(json!({
                                "id": id("initiative_entry", &format!("{}:{}", instance.slug, token.slug)),
                                "combat_id": combat, "token_id": id("token", &token.slug),
                                "initiative": initiative, "tiebreak": 0, "rank": rank + 1,
                            }));
                        }
                        tables.extend([
                            table(
                                "combat",
                                "id",
                                COMBAT,
                                vec![json!({
                                    "id": combat, "instance_id": row, "round": 1,
                                    "started_at": stamp(&moment, 0), "ended_at": stamp(&moment, 1),
                                })],
                            ),
                            table("initiative_entry", "id", INITIATIVE_ENTRY, seats),
                        ]);
                    }
                    names.push((instance.slug.clone(), row));
                }
            }
            if let Some(closing) = &self.closing {
                let lines = closing.journal.iter().enumerate().map(|(index, (at, text))| {
                    json!({
                        "id": id("session_journal_line", &format!("{}:{}", closing.session, index + 1)),
                        "session_id": id("session", &closing.session),
                        "spoken_at": stamp(&format!("{}T{at}:00", closing.day), index),
                        "text": text,
                    })
                });
                tables.push(table(
                    "session_journal_line",
                    "id",
                    JOURNAL,
                    lines.collect(),
                ));
            }
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
    /// The version rows by number: the rounds they go in.
    versions: BTreeMap<u64, Vec<Value>>,
}

impl Rows {
    /// The rows of `element`, the `index`th of its campaign: its row, its
    /// sheet, its aliases, and its versions — those of its `history` when it
    /// has one, else the first of them: the Entity aggregate refuses an
    /// element that no version records.
    fn add(
        &mut self,
        campaign: &str,
        index: usize,
        element: &Element,
        history: Option<&Vec<(u64, Value)>>,
    ) {
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
        let (snapshot, sheet) = state_of(element, &entity);
        self.characters.extend(sheet.map(Value::Object));
        match history {
            Some(versions) => {
                for (number, row) in versions {
                    self.versions.entry(*number).or_default().push(row.clone());
                }
            }
            None => self.versions.entry(1).or_default().push(version_row(
                &element.slug,
                &entity,
                1,
                "dm",
                &at,
                &Map::new(),
                &snapshot,
            )),
        }
    }
}

/// The state of `element` now, as a version's snapshot (EntityVersion@1: the
/// whole state), and the row of its sheet, if it has one.
fn state_of(element: &Element, entity: &str) -> (Map<String, Value>, Option<Map<String, Value>>) {
    let mut snapshot = Map::new();
    snapshot.insert("name".into(), element.name.clone());
    snapshot.insert("type".into(), json!(element.kind));
    snapshot.insert("description".into(), element.description.clone());
    snapshot.insert("notes".into(), element.notes.clone());
    snapshot.insert("aliases".into(), json!(element.aliases));
    let sheet = element.sheet.as_ref().map(|sheet| {
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
        row
    });
    snapshot.retain(|_, value| !value.is_null());
    (snapshot, sheet)
}

/// One row of `entity_version`: version `number` of the element `slug`, whose
/// snapshot is `snapshot` and whose diff is what moved since `before`.
fn version_row(
    slug: &str,
    entity: &str,
    number: u64,
    source: &str,
    at: &str,
    before: &Map<String, Value>,
    snapshot: &Map<String, Value>,
) -> Value {
    json!({
        "id": id("entity_version", &format!("{slug}:{number}")),
        "entity_id": entity, "version_number": number, "source": source,
        "snapshot": snapshot, "diff": diff(before, snapshot), "created_at": at,
    })
}

/// EntityVersion@1: one `{ field, old, new }` per field whose value differs
/// between the two snapshots (an absent field is null), sorted by field.
fn diff(before: &Map<String, Value>, after: &Map<String, Value>) -> Vec<Value> {
    let fields: BTreeSet<&String> = before.keys().chain(after.keys()).collect();
    fields
        .into_iter()
        .filter_map(|field| {
            let old = before.get(field).unwrap_or(&Value::Null);
            let new = after.get(field).unwrap_or(&Value::Null);
            (old != new).then(|| json!({ "field": field, "old": old, "new": new }))
        })
        .collect()
}

/// One version of `slug`'s history, mapped.
fn revision_of(slug: &str, value: &Value) -> Result<Revision, String> {
    let number = value["v"]
        .as_u64()
        .ok_or_else(|| format!("a version of {slug} has no number"))?;
    let at = required(value, "date")?;
    if !is_time(at) {
        return Err(format!("version {number} of {slug} is dated {at:?}"));
    }
    // The table has no `import` source: an import is the DM's. A proposal is
    // the AI's, and a brief's is the brief's, which the table lets change what
    // an AI proposal may not (the sheet alone).
    let source = match (required(value, "origin")?, required(value, "source")?) {
        ("ia", label) if label.starts_with("Brief") => "brief",
        ("ia", _) => "ia",
        ("dndbeyond", _) => "dndbeyond",
        ("manuelle" | "import", _) => "dm",
        (other, _) => return Err(format!("version {number} of {slug} is of origin {other:?}")),
    };
    // A creation is no resync: a D&D Beyond import is the first version of
    // the element, which the DM made, whole.
    let source = if number == 1 { "dm" } else { source };
    let mut changes = Vec::new();
    for change in array(&value["changes"], "changes")? {
        changes.push((
            required(change, "field")?.to_string(),
            change["old"].clone(),
        ));
    }
    Ok(Revision {
        number,
        at: format!("{at}Z"),
        source,
        changes,
    })
}

/// The version rows of `element`, one per revision, oldest first.
fn history_rows(element: &Element, revisions: &[Revision]) -> Result<Vec<(u64, Value)>, String> {
    let entity = id("entity", &element.slug);
    let (current, _) = state_of(element, &entity);
    // Newest first: each state is the one after it with its changes put back.
    let mut states = vec![current.clone()];
    let mut state = current;
    for revision in revisions.iter().skip(1).rev() {
        for (label, old) in &revision.changes {
            undo(element, label, old, &mut state)
                .map_err(|why| format!("version {} of {}: {why}", revision.number, element.slug))?;
        }
        state.retain(|_, value| !value.is_null());
        states.push(state.clone());
    }
    states.reverse();
    let mut before = Map::new();
    let mut rows = Vec::new();
    for (revision, snapshot) in revisions.iter().zip(&states) {
        let row = version_row(
            &element.slug,
            &entity,
            revision.number,
            revision.source,
            &revision.at,
            &before,
            snapshot,
        );
        in_scope(revision, &row)
            .map_err(|why| format!("version {} of {}: {why}", revision.number, element.slug))?;
        rows.push((revision.number, row));
        before = snapshot.clone();
    }
    Ok(rows)
}

/// The fields a D&D Beyond resync (not the first version) may change, as the
/// version trigger of `0013_entity_dndbeyond_creation.sql` allows them.
const RESYNC_SCOPE: &[&str] = &[
    "strength",
    "dexterity",
    "constitution",
    "intelligence",
    "wisdom",
    "charisma",
    "armorClass",
    "hpMax",
    "spells",
    "actions",
    "dndbeyondId",
    "dndbeyondSyncedAt",
    "updatedAt",
];

/// The fields an AI proposal may change, as the same trigger allows them.
const PROPOSAL_SCOPE: &[&str] = &[
    "background",
    "behavior",
    "strength",
    "dexterity",
    "constitution",
    "intelligence",
    "wisdom",
    "charisma",
    "armorClass",
    "hpMax",
    "actions",
    "initialPrompt",
    "updatedAt",
];

/// Whether the diff of `row` stays within what its `source` may change: the
/// database refuses a version that does not, and the whole load with it. A
/// `dndbeyond` revision is always a resync: a creation is made `dm` upstream.
fn in_scope(revision: &Revision, row: &Value) -> Result<(), String> {
    let scope = match revision.source {
        "dndbeyond" => RESYNC_SCOPE,
        "ia" => PROPOSAL_SCOPE,
        _ => return Ok(()),
    };
    let changes = row["diff"].as_array().into_iter().flatten();
    match changes
        .filter_map(|change| change["field"].as_str())
        .find(|field| !scope.contains(field))
    {
        Some(field) => Err(format!(
            "a version of origin {:?} cannot change {field}",
            revision.source
        )),
        None => Ok(()),
    }
}

/// Puts back in `state` what the screen's field `label` held before a
/// change, `old` in the words of the screen. A label the tables hold no
/// column for (a level, a token's look) or the creation moves nothing: the
/// version has one change less.
fn undo(
    element: &Element,
    label: &str,
    old: &Value,
    state: &mut Map<String, Value>,
) -> Result<(), String> {
    let sheet = matches!(
        label,
        "Comportement" | "Background" | "PV max" | "Statblock" | "Sorts et capacités"
    );
    if sheet && element.sheet.is_none() {
        return Err(format!("{label:?} changed an element with no sheet"));
    }
    let text = || {
        old.as_str()
            .ok_or_else(|| format!("{label:?} held {old}, not a text"))
    };
    match label {
        "Notes" => state.insert("notes".into(), json!(text()?)),
        "Comportement" => state.insert("behavior".into(), json!(text()?)),
        "Background" => state.insert("background".into(), json!(text()?)),
        "Alias" => {
            let aliases: Vec<&str> = text()?.split(", ").filter(|a| !a.is_empty()).collect();
            state.insert("aliases".into(), json!(aliases))
        }
        "PV max" => {
            let hp = old
                .as_i64()
                .or_else(|| text().ok()?.trim().parse().ok())
                .ok_or_else(|| format!("{label:?} held {old}, not a number"))?;
            state.insert("hpMax".into(), json!(hp))
        }
        "Statblock" => {
            if !text()?.is_empty() {
                return Err(format!(
                    "{label:?} held {old}: only a first one is put back"
                ));
            }
            for field in ["armorClass", "hpMax", "dexterity"] {
                state.remove(field);
            }
            state.insert("actions".into(), json!([]))
        }
        "Sorts et capacités" => {
            let kept: Vec<&str> = text()?.split(", ").filter(|a| !a.is_empty()).collect();
            let actions: Vec<Value> = state
                .get("actions")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter(|action| {
                    // « Ennemi juré » is « Ennemi juré (morts-vivants) », not « Ennemi juré-né ».
                    let name = action["name"].as_str().unwrap_or_default();
                    kept.iter().any(|kept| {
                        name == *kept
                            || name
                                .strip_prefix(kept)
                                .is_some_and(|rest| rest.starts_with(" ("))
                    })
                })
                .cloned()
                .collect();
            state.insert("actions".into(), json!(actions))
        }
        _ => None,
    };
    Ok(())
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

/// A volume the fixture writes as a whole percent from 0 to 100, as the
/// fraction of 1 with the two decimals the column keeps.
fn fraction(value: &Value, what: &str) -> Result<f64, String> {
    value
        .as_f64()
        .filter(|percent| (0.0..=100.0).contains(percent) && percent.fract() == 0.0)
        .map(|percent| percent / 100.0)
        .ok_or_else(|| format!("{what} is not a whole percent from 0 to 100"))
}

/// One sound of `sounds.json`, mapped. Its tags are normalised as
/// `tags-normalized` asks: trimmed, lowercase, none empty, none twice.
fn sound_of(value: &Value) -> Result<Sound, String> {
    let slug = required(value, "id")?.to_string();
    let mut given = Vec::new();
    for tag in array(&value["tags"], "tags")? {
        given.push(
            tag.as_str()
                .ok_or_else(|| format!("a tag of {slug} is not a text"))?,
        );
    }
    let tags = add_sound::normalized(given);
    Ok(Sound {
        name: required(value, "name")?.to_string(),
        file: required(value, "file")?.to_string(),
        volume: fraction(&value["volume"], "volume")?,
        slug,
        tags,
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

/// One scene of `SceneBoard.json`, mapped: the `index`th (from 0), which says
/// its order (`scene-order-contiguous`: from 1 to n, no hole).
fn scene_of(value: &Value, index: usize) -> Result<Scene, String> {
    let slug = required(value, "id")?.to_string();
    let position = i32::try_from(index + 1).map_err(|_| format!("{slug} is too far"))?;
    if value["order"] != position {
        return Err(format!("{slug} is not the scene {position}"));
    }
    if !(value["notes"].is_null() || value["notes"].is_string()) {
        return Err(format!("the notes of {slug} are not a text"));
    }
    // An element or a medium is linked once per scene: one listed twice is linked once.
    let links = |key: &str| -> Result<Vec<String>, String> {
        let mut linked: Vec<String> = Vec::new();
        for link in array(&value[key], key)? {
            let link = link
                .as_str()
                .ok_or_else(|| format!("a {key} of {slug} is not a text"))?;
            if !linked.iter().any(|known| known == link) {
                linked.push(link.to_string());
            }
        }
        Ok(linked)
    };
    Ok(Scene {
        title: required(value, "title")?.to_string(),
        notes: value["notes"].clone(),
        places: links("places")?,
        characters: links("characters")?,
        events: links("events")?,
        media: links("media")?,
        battlemap: match &value["battlemap"] {
            Value::Null => None,
            Value::String(slug) => Some(slug.clone()),
            _ => return Err(format!("the battlemap of {slug} is not a text")),
        },
        slug,
        position,
    })
}

/// The instance of `table.getBattlemapInstance.json`, mapped. Its roster is
/// derived from the PCs present and the tokens, and the initiative of its PCs
/// is what a combat starts from: no table holds either before one runs, and a
/// combat running is not loaded, the fixture giving no moment it started.
fn instance_of(value: &Value) -> Result<Instance, String> {
    let slug = required(value, "id")?.to_string();
    if !value["state"]["combat"].is_null() {
        return Err(format!("{slug} has a combat running, which is not loaded"));
    }
    let mut tokens: Vec<Token> = Vec::new();
    for token in array(&value["state"]["tokens"], "tokens")? {
        let token = token_of(token)?;
        if token.slug == slug || tokens.iter().any(|known| known.slug == token.slug) {
            return Err(format!("the slug {:?} names two rows", token.slug));
        }
        // One token per PC (`token-roster`), numbers unique per character
        // (`multi-drop-numbering`).
        if tokens
            .iter()
            .any(|k| k.character == token.character && k.number == token.number)
        {
            return Err(format!(
                "{:?} stands twice for {:?} with the number {}",
                token.slug, token.character, token.number
            ));
        }
        tokens.push(token);
    }
    let initiative = initiative_of(&value["pjInitiative"])?;
    Ok(Instance {
        battlemap: required(value, "battlemap")?.to_string(),
        session: required(value, "session")?.to_string(),
        scene: required(value, "scene")?.to_string(),
        slug,
        tokens,
        initiative,
    })
}

/// The bounds of an initiative the DM enters.
const INITIATIVE: std::ops::RangeInclusive<i64> = 1..=40;

/// The `pjInitiative` of an instance — each PC's last initiative, by
/// character — as seats, the highest first and the ties by character: every
/// figure a whole number of [`INITIATIVE`]. Absent or null, no PC was ever
/// seated. The keys name PCs with a token ([`Read::instance_fits`]).
fn initiative_of(value: &Value) -> Result<Vec<(String, i64)>, String> {
    let seats = match value {
        Value::Null => return Ok(Vec::new()),
        Value::Object(seats) => seats,
        _ => return Err("pjInitiative is not an object".to_string()),
    };
    let mut initiative = Vec::with_capacity(seats.len());
    for (character, figure) in seats {
        let figure = figure
            .as_i64()
            .filter(|n| INITIATIVE.contains(n))
            .ok_or_else(|| {
                format!(
                    "the initiative of {character:?} is not a whole number from {} to {}",
                    INITIATIVE.start(),
                    INITIATIVE.end()
                )
            })?;
        initiative.push((character.clone(), figure));
    }
    initiative.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    Ok(initiative)
}

/// One token of an instance, mapped, in the bounds of the columns and of the
/// aggregate's invariants: a PC has no number and no hit points
/// (`pj-hp-not-tracked`), an NPC a number from 1 and hit points from 0 to its
/// maximum, and is dead if and only if it has none left (`dead-derived`).
fn token_of(value: &Value) -> Result<Token, String> {
    let slug = required(value, "id")?.to_string();
    let kind = match value["kind"].as_str() {
        Some("pj") => "pj",
        Some("pnj") => "pnj",
        other => return Err(format!("{slug} is of kind {other:?}")),
    };
    let whole = |key: &str, low: i64, high: i64| -> Result<Option<i64>, String> {
        match &value[key] {
            Value::Null => Ok(None),
            number => number
                .as_i64()
                .filter(|n| (low..=high).contains(n))
                .map(Some)
                .ok_or_else(|| {
                    format!("{key} of {slug} is not a whole number from {low} to {high}")
                }),
        }
    };
    let (number, hp, hp_max) = (
        whole("num", 1, i64::from(i16::MAX))?,
        whole("hp", 0, i64::from(i16::MAX))?,
        whole("hpMax", 1, i64::from(i16::MAX))?,
    );
    if kind == "pj" && (number, hp, hp_max) != (None, None, None) {
        return Err(format!("{slug}, a PC's, has a number or hit points"));
    }
    if kind == "pnj"
        && (number.is_none() || hp_max.is_none() || hp.is_none_or(|h| Some(h) > hp_max))
    {
        return Err(format!(
            "{slug}, an NPC's, has no number or no hit points within its maximum"
        ));
    }
    if value["dead"] != json!(hp == Some(0)) {
        return Err(format!(
            "{slug} is dead if and only if it has no hit point left"
        ));
    }
    let wide = |key: &str| -> Result<i32, String> {
        value[key]
            .as_i64()
            .and_then(|n| i32::try_from(n).ok())
            .ok_or_else(|| format!("{key} of {slug} is not a whole number"))
    };
    let mut conditions = Vec::new();
    for state in array(&value["states"], "states")? {
        conditions.push(
            state
                .as_str()
                .ok_or_else(|| format!("a state of {slug} is not a text"))?,
        );
    }
    let name = required(value, "name")?;
    if name.is_empty() {
        return Err(format!("{slug} has no name"));
    }
    Ok(Token {
        character: required(value, "char")?.to_string(),
        name: name.to_string(),
        number: json!(number),
        q: wide("q")?,
        r: wide("r")?,
        visible: value["visible"]
            .as_bool()
            .ok_or_else(|| format!("visible of {slug} is not a boolean"))?,
        hp: json!(hp),
        hp_max: json!(hp_max),
        conditions: json!(conditions),
        slug,
        kind,
    })
}

/// The model and the day of the last edit a `summaryMeta` says: « Généré à la
/// clôture par <model>, retouché le 4 oct. 2026 » — the edit is optional.
fn meta_of(text: &str) -> Result<(String, Option<String>), String> {
    let rest = text
        .strip_prefix("Généré à la clôture par ")
        .ok_or_else(|| format!("summaryMeta {text:?} names no model"))?;
    let (model, edited) = match rest.split_once(", retouché le ") {
        Some((model, day)) => (
            model,
            Some(french_day(day).ok_or_else(|| format!("{day:?} is not a day"))?),
        ),
        None => (rest, None),
    };
    match model.trim() {
        "" => Err(format!("summaryMeta {text:?} names no model")),
        model => Ok((model.to_string(), edited)),
    }
}

/// `4 oct. 2026` (`1er janv. 2026`) as `2026-10-04`; none for anything else
/// than a day of the calendar.
fn french_day(text: &str) -> Option<String> {
    const MONTHS: [&str; 12] = [
        "janv.", "févr.", "mars", "avr.", "mai", "juin", "juil.", "août", "sept.", "oct.", "nov.",
        "déc.",
    ];
    let mut parts = text.split(' ');
    let (day, month, year) = (parts.next()?, parts.next()?, parts.next()?);
    if parts.next().is_some() {
        return None;
    }
    // `1er` is the only ordinal; the rest is digits.
    let day: u32 = match day {
        "1er" => 1,
        digits if digits.bytes().all(|b| b.is_ascii_digit()) => digits.parse().ok()?,
        _ => return None,
    };
    let month = MONTHS.iter().position(|known| *known == month)? + 1;
    let day = format!("{year}-{month:02}-{day:02}");
    is_day(&day).then_some(day)
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

/// Whether `text` is a time of the day, written `20:04`.
fn is_clock(text: &str) -> bool {
    let (Some(hour), Some(minute)) = (digits(text, 0..2), digits(text, 3..5)) else {
        return false;
    };
    text.len() == 5 && text.as_bytes()[2] == b':' && hour < 24 && minute < 60
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
                "PreviousSummary.json",
                "SceneBoard.json",
                "SessionList.json",
                "ToCreateList.json",
                "campaign.getCampaign.json",
                "campaign.listCampaigns.json",
                "campaign.listEntityVersions.json",
                "entities.json",
                "live.getDiffusion.json",
                "media.json",
                "session.getLiveSession.json",
                "sounds.json",
                "table.getBattlemapInstance.json"
            ]
        );
        assert!(plan.skipped.len() > 29, "{}", plan.skipped.len());
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
                "entity_version",
                "entity_version",
                "entity_version",
                "media",
                "battlemap_grid",
                "sound",
                "session",
                "session",
                "session_present_character",
                "diffusion",
                "scene",
                "scene_entity",
                "scene_media",
                "battlemap_instance",
                "token",
                "combat",
                "initiative_entry",
                "session_journal_line",
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

    #[test]
    fn the_sounds_of_the_mock_up_are_planned_with_their_tags_volumes_and_master() {
        let plan = mock();
        assert!(plan.loaded.contains(&"sounds.json".to_string()));
        assert!(
            !plan
                .loaded
                .contains(&"soundbox.listSounds.json".to_string()),
            "it repeats sounds.json"
        );
        let file = std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../mock/fixtures/sounds.json"),
        )
        .expect("the fixture");
        let told: Value = serde_json::from_str(&file).expect("JSON");
        let told = told["sounds"].as_array().expect("sounds");
        let planted = rows(&plan, "sound");
        assert_eq!(planted.len(), told.len());
        let campaign = id("campaign", "cendres-valombre");
        let mut stamps = Vec::new();
        for (row, sound) in planted.iter().zip(told) {
            let slug = sound["id"].as_str().expect("a slug");
            assert_eq!(row["id"], id("sound", slug), "{slug}: its uuid by slug");
            assert_eq!(row["campaign_id"], campaign, "{slug}");
            assert_eq!(row["name"], sound["name"], "{slug}");
            assert_eq!(row["file_path"], sound["file"], "{slug}");
            assert_eq!(row["tags"], sound["tags"], "{slug}");
            let percent = sound["volume"].as_f64().expect("a percent");
            let fraction = row["volume"].as_f64().expect("a fraction");
            assert!((fraction * 100.0 - percent).abs() < 1e-9, "{slug}: {row}");
            assert!(
                plan.names.contains(&(slug.to_string(), id("sound", slug))),
                "{slug} is named"
            );
            stamps.push(row["created_at"].as_str().expect("a stamp").to_string());
        }
        let mut sorted = stamps.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted, stamps, "stamped in the order of the file");
        // The master volume is the campaign's, and the others keep the default.
        for row in rows(&plan, "campaign") {
            let want = if row["id"] == campaign { 0.8 } else { 1.0 };
            assert_eq!(row["sound_master_volume"], want, "{row}");
        }
        let door = planted[0];
        assert_eq!(door["name"], "Porte qui grince");
        assert_eq!(door["volume"], 0.8);
        assert_eq!(planted[1]["volume"], 0.55);
    }

    #[test]
    fn a_soundbox_that_does_not_fit_is_skipped_whole_and_the_rest_is_loaded() {
        let sound = |tags: Value, volume: Value| json!({ "id": "snd-x", "name": "X", "tags": tags, "volume": volume, "file": "x.ogg" });
        let cases = [
            (
                json!({ "campaign": "nowhere", "masterVolume": 80,
                    "sounds": [sound(json!(["a"]), json!(50))] }),
                "no fixture describes the campaign",
            ),
            (
                json!({ "campaign": "cendres-valombre", "masterVolume": 80,
                    "sounds": [{ "id": "elowen", "name": "E", "tags": [], "volume": 50,
                        "file": "e.ogg" }] }),
                "names two rows",
            ),
            (
                json!({ "campaign": "cendres-valombre", "masterVolume": 80,
                    "sounds": [sound(json!(["a"]), json!(50)), sound(json!(["b"]), json!(50))] }),
                "names two rows",
            ),
            (
                json!({ "campaign": "cendres-valombre", "masterVolume": 80,
                    "sounds": [sound(json!(["a"]), json!(100.5))] }),
                "volume is not a whole percent",
            ),
            (
                json!({ "campaign": "cendres-valombre", "masterVolume": 80,
                    "sounds": [sound(json!(["a"]), json!(-1))] }),
                "volume is not a whole percent",
            ),
            (
                json!({ "campaign": "cendres-valombre", "masterVolume": 80,
                    "sounds": [sound(json!(["a"]), json!(12.5))] }),
                "volume is not a whole percent",
            ),
            (
                json!({ "campaign": "cendres-valombre", "masterVolume": 80.5, "sounds": [] }),
                "masterVolume is not a whole percent",
            ),
            (
                json!({ "campaign": "cendres-valombre", "masterVolume": 101, "sounds": [] }),
                "masterVolume is not a whole percent",
            ),
            (
                json!({ "campaign": "cendres-valombre", "sounds": [] }),
                "masterVolume is not a whole percent",
            ),
        ];
        for (soundbox, why) in cases {
            let plan = soundbox_of(&soundbox);
            assert_eq!(plan.loaded, ["entities.json"], "{why}");
            assert_eq!(plan.skipped.len(), 1, "{why}");
            assert_eq!(plan.skipped[0].0, "sounds.json");
            assert!(plan.skipped[0].1.contains(why), "{why}: {:?}", plan.skipped);
            assert!(rows(&plan, "sound").is_empty(), "{why}");
            assert!(
                rows(&plan, "campaign")
                    .iter()
                    .all(|c| c["sound_master_volume"] == 1.0),
                "{why}: a soundbox left out leaves the master volume at its default"
            );
        }
        let fits = soundbox_of(&json!({ "campaign": "cendres-valombre", "masterVolume": 0,
            "sounds": [sound(json!([]), json!(0)), sound(json!(["é", "b"]), json!(100))] }));
        // Two sounds of one slug are no soundbox; with distinct slugs they are.
        assert!(
            fits.skipped
                .iter()
                .any(|(_, why)| why.contains("names two rows"))
        );
        let fits = soundbox_of(&json!({ "campaign": "cendres-valombre", "masterVolume": 0,
            "sounds": [
                { "id": "a", "name": "A", "tags": [], "volume": 0, "file": "a.ogg" },
                { "id": "b", "name": "B", "tags": ["é", "b"], "volume": 100, "file": "b.ogg" },
            ] }));
        assert_eq!(fits.loaded, ["entities.json", "sounds.json"]);
        let planted = rows(&fits, "sound");
        assert_eq!(planted[0]["volume"], 0.0);
        assert_eq!(planted[1]["volume"], 1.0);
        assert_eq!(planted[1]["tags"], json!(["é", "b"]));
        let campaign = id("campaign", "cendres-valombre");
        let master = rows(&fits, "campaign")
            .into_iter()
            .find(|c| c["id"] == campaign)
            .map(|c| c["sound_master_volume"].clone());
        assert_eq!(master, Some(json!(0.0)), "a master of 0 is a master");
    }

    #[test]
    fn the_tags_of_a_sound_are_normalised_not_refused() {
        let plan = soundbox_of(&json!({ "campaign": "cendres-valombre", "masterVolume": 80,
            "sounds": [{ "id": "t", "name": "T", "tags": ["Taverne", " Bois ", "", "taverne"],
                "volume": 50, "file": "t.ogg" }] }));
        assert_eq!(
            plan.loaded,
            ["entities.json", "sounds.json"],
            "{:?}",
            plan.skipped
        );
        assert_eq!(rows(&plan, "sound")[0]["tags"], json!(["taverne", "bois"]));
        let plan = soundbox_of(&json!({ "campaign": "cendres-valombre", "masterVolume": 80,
            "sounds": [{ "id": "t", "name": "T", "tags": [1], "volume": 50, "file": "t.ogg" }] }));
        assert!(
            plan.skipped[0].1.contains("is not a text"),
            "{:?}",
            plan.skipped
        );
    }

    #[test]
    fn a_soundbox_past_the_stamps_there_are_is_refused() {
        assert!(stampable(STAMPS).is_ok());
        assert!(stampable(STAMPS + 1).is_err());
        assert_eq!(stamp(FIRST, STAMPS - 1), "2026-08-20T10:00:00.999999Z");
    }

    #[test]
    fn a_sound_named_as_a_session_leaves_the_soundbox_skipped() {
        let read = |name: &str| {
            std::fs::read_to_string(
                Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../../mock/fixtures")
                    .join(name),
            )
            .expect("the fixture")
        };
        let soundbox = json!({ "campaign": "cendres-valombre", "masterVolume": 80,
            "sounds": [{ "id": "s1", "name": "S", "tags": [], "volume": 50, "file": "s.ogg" }] });
        let dir = folder(
            "sound-and-session",
            &[
                ("entities.json", &read("entities.json")),
                ("SessionList.json", &read("SessionList.json")),
                ("sounds.json", &soundbox.to_string()),
            ],
        );
        let plan = fixtures(&dir).expect("the folder is listed");
        std::fs::remove_dir_all(&dir).ok();
        // The clash is the soundbox's: the sessions are kept.
        let mut loaded = plan.loaded.clone();
        loaded.sort();
        assert_eq!(loaded, ["SessionList.json", "entities.json"]);
        assert_eq!(plan.skipped.len(), 1, "{:?}", plan.skipped);
        assert_eq!(plan.skipped[0].0, "sounds.json");
        assert!(plan.skipped[0].1.contains("names two rows"));
        assert!(rows(&plan, "sound").is_empty());
        assert!(!rows(&plan, "session").is_empty());
    }

    /// The plan of `entities.json` and `soundbox` (as `sounds.json`), alone.
    fn soundbox_of(soundbox: &Value) -> Fixtures {
        let entities = std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../mock/fixtures/entities.json"),
        )
        .expect("the fixture");
        // A folder of its own each time: the tests run side by side.
        static PLANNED: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = PLANNED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = folder(
            &format!("soundbox-{n}"),
            &[
                ("entities.json", &entities),
                ("sounds.json", &soundbox.to_string()),
            ],
        );
        let plan = fixtures(&dir).expect("the folder is listed");
        std::fs::remove_dir_all(&dir).ok();
        plan
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
        // Sixteen elements; the history adds three versions to Morlox, two to
        // Elowen and one to Gaspard.
        assert_eq!(
            versions.len(),
            16 + 3 + 2 + 1,
            "the Entity aggregate refuses an element no version records"
        );
        let of = |slug: &str| -> Vec<&&Value> {
            versions
                .iter()
                .filter(|v| v["entity_id"] == id("entity", slug))
                .collect()
        };
        // Kael has no history of his own: his one version is his state now.
        let kael = of("kael");
        assert_eq!(kael.len(), 1);
        let first = kael[0];
        assert_eq!(first["version_number"], 1);
        assert_eq!(first["source"], "dm");
        let snapshot = first["snapshot"].as_object().expect("a snapshot");
        assert_eq!(snapshot["type"], "pj");
        assert!(snapshot.contains_key("armorClass"));
        // EntityVersion@1: the first diff compares the snapshot with nothing,
        // one entry per field, sorted.
        let diff = first["diff"].as_array().expect("a diff");
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
    fn the_versions_go_in_by_round_so_that_each_is_the_next_of_its_element() {
        let plan = mock();
        let rounds: Vec<&data::Table> = plan
            .tables
            .iter()
            .filter(|t| t.name == "entity_version")
            .collect();
        // Morlox has four versions.
        assert_eq!(rounds.len(), 4);
        for (round, table) in rounds.iter().zip(1..) {
            let numbers: BTreeSet<u64> = round
                .rows
                .iter()
                .filter_map(|v| v["version_number"].as_u64())
                .collect();
            assert_eq!(numbers, BTreeSet::from([table]));
        }
        // The first round holds every element, the last Morlox alone.
        assert_eq!(rounds[0].rows.len(), 16);
        assert_eq!(rounds[3].rows.len(), 1);
        assert_eq!(rounds[3].rows[0]["entity_id"], id("entity", "morlox"));
    }

    /// The `(number, source, created_at, snapshot, diff)` of the versions of
    /// `slug` in `plan`, oldest first.
    fn history_of<'a>(plan: &'a Fixtures, slug: &str) -> Vec<&'a Value> {
        rows(plan, "entity_version")
            .into_iter()
            .filter(|v| v["entity_id"] == id("entity", slug))
            .collect()
    }

    fn fields(version: &Value) -> Vec<&str> {
        version["diff"]
            .as_array()
            .expect("a diff")
            .iter()
            .filter_map(|c| c["field"].as_str())
            .collect()
    }

    fn change<'a>(version: &'a Value, field: &str) -> &'a Value {
        version["diff"]
            .as_array()
            .expect("a diff")
            .iter()
            .find(|c| c["field"] == field)
            .expect("a change")
    }

    #[test]
    fn the_history_of_the_mock_up_is_planned_version_by_version() {
        let plan = mock();
        let described = mock_file("campaign.listEntityVersions.json");
        for (slug, account) in described.as_object().expect("histories") {
            let planned = history_of(&plan, slug);
            // The versions of the fixture, from the first, dated as it dates them.
            let numbers: Vec<&Value> = planned.iter().map(|v| &v["version_number"]).collect();
            let expected: Vec<Value> = (1..=planned.len()).map(|n| json!(n)).collect();
            assert_eq!(numbers, expected.iter().collect::<Vec<_>>(), "{slug}");
            let mut told: Vec<&Value> = account["versions"]
                .as_array()
                .expect("versions")
                .iter()
                .collect();
            told.reverse();
            assert_eq!(planned.len(), told.len(), "{slug}");
            for (version, fixture) in planned.iter().zip(told) {
                let date = format!("{}Z", fixture["date"].as_str().expect("a date"));
                assert_eq!(version["created_at"], date, "{slug} {fixture}");
            }
            // Each version's diff is what moved from the one before it: the
            // snapshots chain.
            let mut state = Map::new();
            for version in &planned {
                for c in version["diff"].as_array().expect("a diff") {
                    let field = c["field"].as_str().expect("a field").to_string();
                    assert_eq!(state.get(&field).unwrap_or(&Value::Null), &c["old"]);
                    state.insert(field, c["new"].clone());
                }
                state.retain(|_, value| !value.is_null());
                assert_eq!(
                    &Value::Object(state.clone()),
                    &version["snapshot"],
                    "{slug}"
                );
            }
        }
        // The last version is the element as `entities.json` has it.
        let morlox = history_of(&plan, "morlox");
        let current = mock_file("entities.json");
        let fixture = current["characters"]
            .as_array()
            .expect("characters")
            .iter()
            .find(|c| c["id"] == "morlox")
            .expect("morlox");
        let last = morlox[3]["snapshot"].as_object().expect("a snapshot");
        assert_eq!(last["notes"], fixture["notes"]);
        assert_eq!(last["aliases"], fixture["aliases"]);
        assert_eq!(last["background"], fixture["background"]);
        assert_eq!(last["armorClass"], fixture["ca"]);
    }

    #[test]
    fn what_a_version_changed_is_what_the_fixture_says_it_changed() {
        let plan = mock();
        let morlox = history_of(&plan, "morlox");
        let sources: Vec<&Value> = morlox.iter().map(|v| &v["source"]).collect();
        // Imported, then dictated to the AI, edited by hand, found in a brief.
        assert_eq!(sources, ["dm", "ia", "dm", "brief"]);
        assert_eq!(fields(morlox[3]), ["aliases", "behavior", "notes"]);
        assert_eq!(
            change(morlox[3], "aliases")["old"],
            json!(["le gobelin", "le marchand"])
        );
        assert_eq!(change(morlox[2], "notes")["old"], "Doit 40 po à Vesna.");
        assert_eq!(fields(morlox[2]), ["notes"]);
        assert_eq!(
            fields(morlox[1]),
            ["actions", "armorClass", "background", "dexterity", "hpMax"]
        );
        assert_eq!(change(morlox[1], "background")["old"], "");
        assert_eq!(change(morlox[1], "hpMax")["old"], Value::Null);
        // Before the statblock the element had none.
        assert!(morlox[0]["snapshot"].get("armorClass").is_none());
        assert_eq!(morlox[0]["snapshot"]["actions"], json!([]));

        let elowen = history_of(&plan, "elowen");
        let sources: Vec<&Value> = elowen.iter().map(|v| &v["source"]).collect();
        assert_eq!(sources, ["dm", "dm", "dndbeyond"]);
        // The level and the token's look have no column: the resync changed two fields.
        assert_eq!(fields(elowen[2]), ["actions", "hpMax"]);
        assert_eq!(change(elowen[2], "hpMax")["old"], 36);
        assert_eq!(change(elowen[2], "hpMax")["new"], 44);
        assert_eq!(
            elowen[1]["snapshot"]["actions"].as_array().map(Vec::len),
            Some(2)
        );
        assert_eq!(fields(elowen[1]), ["aliases"]);

        let gaspard = history_of(&plan, "gaspard");
        assert_eq!(gaspard[1]["source"], "ia");
        assert_eq!(fields(gaspard[1]), ["background"]);
    }

    /// An `entities.json` of a PNJ and a place, and `history` as the history
    /// fixture; what is planned.
    fn small(history: &Value) -> Fixtures {
        static MADE: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let entities = json!({
            "campaign": { "id": "c", "name": "C" }, "players": [],
            "characters": [{
                "id": "pc", "name": "Pc", "kind": "pnj", "aliases": ["a", "b"],
                "description": "d", "prompt": "", "background": "bg", "behavior": "agité",
                "stats": { "FOR": 10, "DEX": 11, "CON": 12, "INT": 13, "SAG": 14, "CHA": 15 },
                "ca": 12, "pvMax": 10, "color": "success", "notes": "n",
                "features": [{ "name": "Rage", "text": "x" }, { "name": "Rageur", "text": "z" },
                    { "name": "Rune (rare)", "text": "y" }]
            }],
            "places": [{ "id": "lieu", "name": "Lieu", "aliases": [], "description": "d" }],
            "events": []
        });
        let n = MADE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = folder(
            &format!("small-{n}"),
            &[
                ("entities.json", &entities.to_string()),
                (HISTORY_FILE, &history.to_string()),
            ],
        );
        let plan = fixtures(&dir).expect("the folder is listed");
        std::fs::remove_dir_all(&dir).ok();
        plan
    }

    fn version(v: u64, origin: &str, source: &str, changes: &Value) -> Value {
        json!({ "v": v, "date": format!("2026-09-{:02}T10:00:00", v + 10), "origin": origin,
                "source": source, "seen": true, "changes": changes })
    }

    fn account(name: &str, versions: &[Value]) -> Value {
        json!({ "name": name, "versions": versions })
    }

    fn created() -> Value {
        version(
            1,
            "import",
            "Import",
            &json!([{ "field": "Création", "old": "", "new": "x" }]),
        )
    }

    #[test]
    fn each_label_of_the_screen_is_put_back_in_the_state_before_its_change() {
        let history = json!({ "pc": account("Pc", &[
            version(5, "ia", "Brief oral — séance 4", &json!([
                { "field": "Comportement", "old": "calme", "new": "agité" }])),
            version(4, "dndbeyond", "Resynchronisation", &json!([
                { "field": "PV max", "old": "8", "new": "10" },
                { "field": "Sorts et capacités", "old": "Rage", "new": "Rage, Rune" }])),
            version(3, "manuelle", "À la main", &json!([
                { "field": "Notes", "old": "avant", "new": "n" },
                { "field": "Alias", "old": "a", "new": "a, b" },
                { "field": "Niveau", "old": "4", "new": "5" }])),
            version(2, "ia", "Prompt initial dicté", &json!([
                { "field": "Statblock", "old": "", "new": "CA 12" },
                { "field": "Background", "old": "", "new": "bg" }])),
            created(),
        ]) });
        let plan = small(&history);
        assert!(unmapped(&plan).is_empty(), "{:?}", plan.skipped);
        assert!(plan.loaded.contains(&HISTORY_FILE.to_string()));
        let pc = history_of(&plan, "pc");
        let numbers: Vec<&Value> = pc.iter().map(|v| &v["version_number"]).collect();
        assert_eq!(numbers, [1, 2, 3, 4, 5]);
        let sources: Vec<&Value> = pc.iter().map(|v| &v["source"]).collect();
        assert_eq!(sources, ["dm", "ia", "dm", "dndbeyond", "brief"]);
        assert_eq!(pc[1]["created_at"], "2026-09-12T10:00:00Z");
        // The last version is the element now.
        assert_eq!(pc[4]["snapshot"]["behavior"], "agité");
        assert_eq!(fields(pc[4]), ["behavior"]);
        assert_eq!(change(pc[4], "behavior")["old"], "calme");
        // A resync: the hit points and the capabilities, the Rune not yet known.
        assert_eq!(fields(pc[3]), ["actions", "hpMax"]);
        assert_eq!(change(pc[3], "hpMax")["old"], 8);
        assert_eq!(
            change(pc[3], "actions")["old"],
            json!([{ "name": "Rage", "text": "x" }])
        );
        // By hand: the level moves no column.
        assert_eq!(fields(pc[2]), ["aliases", "notes"]);
        assert_eq!(change(pc[2], "aliases")["old"], json!(["a"]));
        assert_eq!(change(pc[2], "notes")["old"], "avant");
        // The statblock and the background were new.
        assert_eq!(
            fields(pc[1]),
            ["actions", "armorClass", "background", "dexterity", "hpMax"]
        );
        assert_eq!(change(pc[1], "actions")["old"], json!([]));
        assert_eq!(change(pc[1], "armorClass")["old"], Value::Null);
        assert_eq!(change(pc[1], "background")["old"], "");
        // And the place, with no history of its own, has the one version.
        assert_eq!(history_of(&plan, "lieu").len(), 1);
    }

    #[test]
    fn a_history_that_does_not_fit_the_elements_is_skipped_whole() {
        let hurt = |history: Value, why: &str| {
            let plan = small(&history);
            assert!(!plan.loaded.contains(&HISTORY_FILE.to_string()), "{why}");
            let (_, skipped) = plan
                .skipped
                .iter()
                .find(|(file, _)| file == HISTORY_FILE)
                .unwrap_or_else(|| panic!("{why}: {:?}", plan.skipped));
            assert!(skipped.contains(why), "{why}: {skipped}");
            // The elements keep their one version each, and the rest is loaded.
            assert_eq!(rows(&plan, "entity_version").len(), 2, "{why}");
            assert!(plan.loaded.contains(&"entities.json".to_string()), "{why}");
        };
        let one = |changes: Value| json!({ "pc": account("Pc", &[version(2, "manuelle", "x", &changes), created()]) });
        hurt(
            json!({ "ghost": account("Pc", &[created()]) }),
            "is no element of the campaign",
        );
        hurt(json!({ "pc": account("Autre", &[created()]) }), "is called");
        hurt(
            json!({ "pc": account("Pc", &[version(3, "manuelle", "x", &json!([])), created()]) }),
            "do not run from 1 without a gap or a repeat",
        );
        hurt(
            json!({ "pc": account("Pc", &[version(2, "manuelle", "x", &json!([])),
                version(2, "manuelle", "x", &json!([])), created()]) }),
            "or a repeat",
        );
        hurt(json!({ "pc": account("Pc", &[]) }), "has no version");
        hurt(
            json!({ "pc": account("Pc", &[json!({ "v": 1, "date": "2026-13-01T10:00:00",
                "origin": "import", "source": "x", "changes": [] })]) }),
            "is dated",
        );
        hurt(
            json!({ "pc": account("Pc", &[version(1, "magie", "x", &json!([]))]) }),
            "is of origin",
        );
        hurt(
            json!({ "pc": account("Pc", &[json!({ "date": "2026-09-11T10:00:00", "origin": "import",
                "source": "x", "changes": [] })]) }),
            "has no number",
        );
        hurt(
            one(json!([{ "field": "Statblock", "old": "CA 9", "new": "CA 12" }])),
            "only a first one",
        );
        hurt(
            one(json!([{ "field": "PV max", "old": "beaucoup", "new": "10" }])),
            "not a number",
        );
        hurt(
            one(json!([{ "field": "Notes", "old": 4, "new": "n" }])),
            "not a text",
        );
        hurt(
            json!({ "lieu": account("Lieu", &[
                version(2, "manuelle", "x", &json!([{ "field": "Background", "old": "", "new": "d" }])),
                created()]) }),
            "no sheet",
        );
        hurt(json!(["pc"]), "not an object");
    }

    #[test]
    fn a_version_beyond_the_scope_of_its_source_skips_the_history_whole() {
        // The database refuses it, and the whole load with it: it is skipped here.
        for (origin, field, change) in [
            ("ia", "Notes", "notes"),
            ("dndbeyond", "Background", "background"),
        ] {
            let history = json!({ "pc": account("Pc", &[
                version(2, origin, "x", &json!([{ "field": field, "old": "", "new": "d" }])),
                created()]) });
            let plan = small(&history);
            assert!(!plan.loaded.contains(&HISTORY_FILE.to_string()), "{origin}");
            assert!(
                plan.skipped
                    .iter()
                    .any(|(file, why)| file == HISTORY_FILE && why.contains(change)),
                "{origin}: {:?}",
                plan.skipped
            );
            assert_eq!(rows(&plan, "entity_version").len(), 2, "{origin}");
        }
    }

    #[test]
    fn an_element_the_store_versioned_itself_keeps_its_versions_only() {
        let (morlox, elowen) = (id("entity", "morlox"), id("entity", "elowen"));
        let version_of = |slug: &str, n: u64| id("entity_version", &format!("{slug}:{n}"));
        let planned = |plan: &Fixtures| -> usize { rows(plan, "entity_version").len() };
        let all = |slug: &str, n: u64| -> Vec<(String, String)> {
            (1..=n)
                .map(|v| (id("entity", slug), version_of(slug, v)))
                .collect()
        };
        // Nothing held, or exactly the planned versions (a restart): planned as is.
        for held in [vec![], [all("morlox", 4), all("elowen", 3)].concat()] {
            let mut plan = mock();
            plan.leave_out_taken_versions(&held);
            assert_eq!(plan, mock(), "{held:?}");
        }
        // The app's own version under a random id, or only the first of an
        // earlier load: the element's versions are left out, the others' stay.
        for held in [
            vec![(morlox.clone(), "app-written".to_string())],
            vec![(morlox.clone(), version_of("morlox", 1))],
            // Every planned version, plus one of the app's: more than planned.
            [
                all("morlox", 4),
                vec![(morlox.clone(), "app-written".to_string())],
            ]
            .concat(),
            // As many as planned, none of them the planned ones.
            (1..=4)
                .map(|n| (morlox.clone(), format!("app-{n}")))
                .collect(),
        ] {
            let mut plan = mock();
            plan.leave_out_taken_versions(&held);
            assert_eq!(planned(&plan), 16 + 2 + 1 - 1, "{held:?}");
            assert!(
                history_of(&plan, "morlox").is_empty(),
                "{held:?}: Morlox keeps the store's"
            );
            assert_eq!(history_of(&plan, "elowen").len(), 3);
            assert!(
                plan.skipped
                    .iter()
                    .any(|(file, why)| file == HISTORY_FILE && why.contains("1 element(s)")),
                "{:?}",
                plan.skipped
            );
        }
        assert_ne!(morlox, elowen);
    }

    #[test]
    fn a_history_with_no_elements_is_skipped() {
        let history = json!({ "pc": account("Pc", &[created()]) });
        let dir = folder("alone", &[(HISTORY_FILE, &history.to_string())]);
        let plan = fixtures(&dir).expect("the folder is listed");
        std::fs::remove_dir_all(&dir).ok();
        assert!(plan.loaded.is_empty());
        assert!(
            plan.skipped[0].1.contains("no fixture holds the elements"),
            "{:?}",
            plan.skipped
        );
        assert!(
            plan.tables
                .iter()
                .all(|t| t.name != "entity_version" || t.rows.is_empty())
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
            "a campaign no session fixture describes has none"
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
        replaced_but(&[], over)
    }

    /// [`replaced`], the files named in `without` taken out first.
    fn replaced_but(without: &[&str], over: &[(&str, Value)]) -> Fixtures {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../mock/fixtures");
        let mut files: Vec<(String, String)> = std::fs::read_dir(&dir)
            .expect("the fixtures folder")
            .filter_map(|entry| {
                let path = entry.ok()?.path();
                let name = path.file_name()?.to_str()?.to_string();
                let json = path.extension().is_some_and(|e| e == "json");
                (json && !without.contains(&name.as_str()))
                    .then(|| (name, std::fs::read_to_string(&path).expect("a fixture")))
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
        // AC1: the session under way is loaded live, started on its day.
        assert_eq!(session("s4")["status"], "live");
        assert_eq!(session("s4")["started_at"], "2026-10-17T20:02:00Z");
        assert!(
            sessions
                .iter()
                .filter(|s| s["id"] != id("session", "s4"))
                .all(|s| s["started_at"].is_null() && s["status"] != "live"),
            "no other session is live or started"
        );
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
    fn the_scenes_of_the_mock_up_are_planned_with_their_links() {
        let plan = mock();
        let board = mock_file(BOARD_FILE);
        let scenes = rows(&plan, "scene");
        assert_eq!(scenes.len(), 3);
        for (index, (row, fixture)) in scenes
            .iter()
            .zip(board["scenes"].as_array().expect("scenes"))
            .enumerate()
        {
            let slug = fixture["id"].as_str().expect("an id");
            assert_eq!(row["id"], id("scene", slug));
            assert_eq!(row["session_id"], id("session", "s4"));
            assert_eq!(row["position"], index + 1, "the rank is the order");
            assert_eq!(row["title"], fixture["title"]);
            assert_eq!(row["notes"], fixture["notes"]);
            assert!(plan.names.contains(&(slug.to_string(), id("scene", slug))));
        }
        let linked = |table: &str, scene: &str, column: &str| -> Vec<Value> {
            let mut rows: Vec<&Value> = rows(&plan, table)
                .into_iter()
                .filter(|r| r["scene_id"] == id("scene", scene))
                .collect();
            rows.sort_by_key(|r| r["position"].as_i64());
            rows.iter().map(|r| r[column].clone()).collect()
        };
        // Places, then characters, then events, each in the fixture's order.
        assert_eq!(
            linked("scene_entity", "sc3", "entity_id"),
            [
                id("entity", "bandit"),
                id("entity", "vesna"),
                id("entity", "morlox"),
                id("entity", "embuscade-gue"),
            ]
        );
        assert_eq!(
            linked("scene_entity", "sc1", "entity_id"),
            [
                id("entity", "taverne-sanglier"),
                id("entity", "gaspard"),
                id("entity", "morlox"),
                id("entity", "marche-noir"),
            ]
        );
        assert_eq!(
            linked("scene_media", "sc1", "media_id").len(),
            2,
            "both illustrations"
        );
        assert_eq!(
            linked("scene_media", "sc3", "media_id"),
            [id("media", "bm-gue")]
        );
        assert_eq!(rows(&plan, "scene_entity").len(), 4 + 2 + 4);
        let first: Vec<_> = rows(&plan, "scene_entity")
            .into_iter()
            .filter(|r| r["scene_id"] == id("scene", "sc2"))
            .filter_map(|r| r["position"].as_i64())
            .collect();
        assert_eq!(first, [1, 2], "ranked from 1");
        let keyed = |table: &str| plan.tables.iter().find(|t| t.name == table).map(|t| t.key);
        assert_eq!(keyed("scene_entity"), Some("scene_id,entity_id"));
        assert_eq!(keyed("scene_media"), Some("scene_id,media_id"));
    }

    #[test]
    fn the_journal_and_the_summary_of_the_closed_session_are_planned() {
        let plan = mock();
        let closing = mock_file(CLOSING_FILE);
        let lines = rows(&plan, "session_journal_line");
        let fixture = closing["journal"].as_array().expect("a journal");
        assert_eq!(lines.len(), 14);
        assert_eq!(lines.len(), fixture.len());
        for (index, (row, line)) in lines.iter().zip(fixture).enumerate() {
            assert_eq!(
                row["id"],
                id("session_journal_line", &format!("s3:{}", index + 1))
            );
            assert_eq!(row["session_id"], id("session", "s3"));
            assert_eq!(row["text"], line["text"]);
            let at = row["spoken_at"].as_str().expect("a time");
            let clock = line["t"].as_str().expect("a time");
            assert_eq!(&at[..10], "2026-10-03", "the session's day");
            assert_eq!(&at[11..16], clock, "the line's HH:MM");
            assert_eq!(&at[at.len() - 1..], "Z", "UTC");
        }
        let stamps: Vec<&str> = lines
            .iter()
            .filter_map(|l| l["spoken_at"].as_str())
            .collect();
        assert!(stamps.windows(2).all(|w| w[0] < w[1]), "in spoken order");

        let sessions = rows(&plan, "session");
        let third = sessions
            .iter()
            .find(|s| s["id"] == id("session", "s3"))
            .expect("a session");
        assert_eq!(
            third["summary"], closing["summary"],
            "PreviousSummary's wins"
        );
        assert_eq!(
            third["summary_model"],
            "qwen2.5-14b-instruct (opencode maison)"
        );
        assert_eq!(third["summary_edited_at"], "2026-10-04T00:00:00Z");
        for other in sessions.iter().filter(|s| s["id"] != id("session", "s3")) {
            assert!(other["summary_model"].is_null() && other["summary_edited_at"].is_null());
        }
        let list = mock_file(SESSIONS);
        let first = sessions
            .iter()
            .find(|s| s["id"] == id("session", "s1"))
            .expect("a session");
        assert_eq!(
            first["summary"], list["sessions"][0]["summary"],
            "the others keep theirs"
        );
    }

    #[test]
    fn the_closed_session_has_one_summary_in_the_list_and_the_panel() {
        let closing = mock_file(CLOSING_FILE);
        let slug = closing["session"]["id"].as_str().expect("a session");
        let summary = closing["summary"].as_str().expect("a summary");
        let in_list = |file: &str, sessions: &Value| -> Value {
            sessions
                .as_array()
                .expect("sessions")
                .iter()
                .find(|s| s["id"] == slug)
                .unwrap_or_else(|| panic!("{slug} in {file}"))["summary"]
                .clone()
        };
        let list = mock_file(SESSIONS);
        assert_eq!(in_list(SESSIONS, &list["sessions"]), summary);
        // The Capability's own fixture, which the campaign's list replays.
        let answer = mock_file("session.listSessions.json");
        assert_eq!(in_list("session.listSessions.json", &answer), summary);
        // And the store holds that one text, whichever file wins the load.
        let plan = mock();
        let row = rows(&plan, "session")
            .into_iter()
            .find(|s| s["id"] == id("session", slug))
            .expect("the closed session");
        assert_eq!(row["summary"], summary);
    }

    #[test]
    fn a_summary_meta_says_a_model_and_maybe_the_day_of_an_edit() {
        assert_eq!(
            meta_of(
                "Généré à la clôture par qwen2.5-14b-instruct (opencode maison), retouché le 4 oct. 2026"
            ),
            Ok((
                "qwen2.5-14b-instruct (opencode maison)".to_string(),
                Some("2026-10-04".to_string())
            ))
        );
        assert_eq!(
            meta_of("Généré à la clôture par claude"),
            Ok(("claude".to_string(), None))
        );
        assert_eq!(
            meta_of("Généré à la clôture par claude, retouché le 1er janv. 2026"),
            Ok(("claude".to_string(), Some("2026-01-01".to_string())))
        );
        for bad in [
            "Écrit par un humain",
            "Généré à la clôture par ",
            "Généré à la clôture par  , retouché le 4 oct. 2026",
            "Généré à la clôture par x, retouché le 4 octobre 2026",
            "Généré à la clôture par x, retouché le 31 févr. 2026",
            "Généré à la clôture par x, retouché le 4 oct. 2026 à 10 h",
            "Généré à la clôture par x, retouché le 4 oct.",
        ] {
            assert!(meta_of(bad).is_err(), "{bad}");
        }
        assert_eq!(french_day("30 déc. 2025").as_deref(), Some("2025-12-30"));
        assert_eq!(french_day("29 févr. 2028").as_deref(), Some("2028-02-29"));
        assert_eq!(french_day("29 févr. 2027"), None);
        assert_eq!(french_day("0 mars 2026"), None);
        for bad in [
            "+4 oct. 2026",
            "2er oct. 2026",
            "4er oct. 2026",
            "-4 oct. 2026",
            " oct. 2026",
        ] {
            assert_eq!(french_day(bad), None, "{bad}");
        }
        assert_eq!(french_day("04 oct. 2026").as_deref(), Some("2026-10-04"));
    }

    #[test]
    fn a_time_of_the_day_is_hours_and_minutes() {
        for good in ["00:00", "20:04", "23:59"] {
            assert!(is_clock(good), "{good}");
        }
        for bad in [
            "24:00", "20:60", "2:04", "20-04", "20:4", "20:04:00", "ab:cd", "20:0é", "",
        ] {
            assert!(!is_clock(bad), "{bad}");
        }
    }

    #[test]
    fn a_scene_file_that_does_not_fit_is_skipped_alone() {
        let board = |edit: &dyn Fn(&mut Value)| {
            let mut board = mock_file(BOARD_FILE);
            edit(&mut board);
            board
        };
        let cases = [
            (
                "a session nobody describes",
                board(&|b| b["session"]["id"] = json!("s9")),
                "no fixture describes the session",
            ),
            (
                "a place that is a character",
                board(&|b| b["scenes"][0]["places"] = json!(["morlox"])),
                "is none of",
            ),
            (
                "a character that is a place",
                board(&|b| b["scenes"][0]["characters"] = json!(["valombre"])),
                "is none of",
            ),
            (
                "an event that is a character",
                board(&|b| b["scenes"][0]["events"] = json!(["gaspard"])),
                "is none of",
            ),
            (
                "an element nobody describes",
                board(&|b| b["scenes"][0]["characters"] = json!(["nobody"])),
                "is none of",
            ),
            (
                "a medium nobody describes",
                board(&|b| b["scenes"][0]["media"] = json!(["nothing"])),
                "is none of its campaign",
            ),
            (
                "a slug an element has",
                board(&|b| b["scenes"][0]["id"] = json!("elowen")),
                "names two rows",
            ),
            (
                "a slug a session has",
                board(&|b| b["scenes"][0]["id"] = json!("s1")),
                "names two rows",
            ),
            (
                "a slug an entry has",
                board(&|b| b["scenes"][0]["id"] = json!("tc-gue")),
                "names two rows",
            ),
            (
                "a slug twice",
                board(&|b| b["scenes"][1]["id"] = json!("sc1")),
                "names two rows",
            ),
            (
                "a hole in the orders",
                board(&|b| b["scenes"][1]["order"] = json!(3)),
                "is not the scene 2",
            ),
            (
                "notes that are no text",
                board(&|b| b["scenes"][0]["notes"] = json!(7)),
                "are not a text",
            ),
            (
                "a link that is no text",
                board(&|b| b["scenes"][0]["media"] = json!([7])),
                "is not a text",
            ),
            (
                "no title",
                board(&|b| b["scenes"][0]["title"] = json!(null)),
                "title is not a text",
            ),
        ];
        for (what, scenes, why) in cases {
            let plan = replaced(&[(BOARD_FILE, scenes)]);
            let left = unmapped(&plan);
            // The instance hangs on its scene: it goes with the board.
            let files: Vec<&str> = left.iter().map(|(n, _)| n.as_str()).collect();
            assert_eq!(files, [BOARD_FILE, INSTANCE_FILE], "{what}: {left:?}");
            assert!(left[0].1.contains(why), "{what}: {:?}", left[0]);
            for table in [
                "scene",
                "scene_entity",
                "scene_media",
                "battlemap_instance",
                "token",
            ] {
                assert!(rows(&plan, table).is_empty(), "{what}: {table}");
            }
            assert!(plan.names.iter().all(|(slug, _)| slug != "sc1"), "{what}");
            assert!(plan.loaded.iter().any(|n| n == SESSIONS), "{what}");
            assert_eq!(rows(&plan, "session_journal_line").len(), 14, "{what}");
        }
    }

    #[test]
    fn a_link_listed_twice_in_a_scene_is_linked_once() {
        let mut board = mock_file(BOARD_FILE);
        board["scenes"][0]["characters"] = json!(["gaspard", "morlox", "gaspard"]);
        board["scenes"][0]["media"] = json!(["ill-gaspard", "ill-gaspard", "ill-salle-sanglier"]);
        let plan = replaced(&[(BOARD_FILE, board)]);
        assert!(unmapped(&plan).is_empty(), "{:?}", plan.skipped);
        let of_scene = |table: &str| {
            rows(&plan, table)
                .into_iter()
                .filter(|r| r["scene_id"] == id("scene", "sc1"))
                .count()
        };
        assert_eq!(of_scene("scene_entity"), 4);
        assert_eq!(of_scene("scene_media"), 2);
    }

    #[test]
    fn a_closing_file_that_does_not_fit_is_skipped_and_the_sessions_summary_stays() {
        let closing = |edit: &dyn Fn(&mut Value)| {
            let mut closing = mock_file(CLOSING_FILE);
            edit(&mut closing);
            closing
        };
        let cases = [
            (
                "a session nobody describes",
                closing(&|c| c["session"]["id"] = json!("s9")),
                "no fixture describes the session",
            ),
            (
                "a session that is not closed",
                closing(&|c| c["session"]["id"] = json!("s4")),
                "is prep, not closed",
            ),
            (
                "a day that is not a day",
                closing(&|c| c["session"]["date"] = json!("3 octobre")),
                "is not a day",
            ),
            (
                "a meta with no model",
                closing(&|c| c["summaryMeta"] = json!("retouché le 4 oct. 2026")),
                "names no model",
            ),
            (
                "no summary",
                closing(&|c| c["summary"] = json!(null)),
                "summary is not a text",
            ),
            (
                "another day than the session's",
                closing(&|c| c["session"]["date"] = json!("2026-10-04")),
                "not of its day",
            ),
            (
                "a time that is not a time",
                closing(&|c| c["journal"][2]["t"] = json!("25:00")),
                "is not an HH:MM",
            ),
            (
                "a line out of time order",
                closing(&|c| c["journal"][3]["t"] = json!("19:59")),
                "out of time order",
            ),
            (
                "a line with no text",
                closing(&|c| c["journal"][3]["text"] = json!(null)),
                "text is not a text",
            ),
            (
                "a journal that is no list",
                closing(&|c| c["journal"] = json!("none")),
                "is not a list",
            ),
        ];
        let list = mock_file(SESSIONS);
        for (what, document, why) in cases {
            let plan = replaced(&[(CLOSING_FILE, document)]);
            let left = unmapped(&plan);
            assert_eq!(left.len(), 1, "{what}: {left:?}");
            assert_eq!(left[0].0, CLOSING_FILE, "{what}");
            assert!(left[0].1.contains(why), "{what}: {:?}", left[0]);
            assert!(rows(&plan, "session_journal_line").is_empty(), "{what}");
            let sessions = rows(&plan, "session");
            let third = sessions
                .iter()
                .find(|s| s["id"] == id("session", "s3"))
                .expect("a session");
            assert_eq!(third["summary"], list["sessions"][2]["summary"], "{what}");
            assert!(third["summary_model"].is_null(), "{what}");
            assert!(plan.loaded.iter().any(|n| n == BOARD_FILE), "{what}");
        }
    }

    #[test]
    fn two_lines_of_one_minute_keep_their_order() {
        let mut closing = mock_file(CLOSING_FILE);
        closing["journal"] = json!([
            { "t": "20:04", "text": "a" },
            { "t": "20:04", "text": "b" },
            { "t": "20:04", "text": "c" },
        ]);
        let plan = replaced(&[(CLOSING_FILE, closing)]);
        let stamps: Vec<&str> = rows(&plan, "session_journal_line")
            .iter()
            .filter_map(|l| l["spoken_at"].as_str())
            .collect();
        assert_eq!(stamps.len(), 3);
        assert!(stamps.windows(2).all(|w| w[0] < w[1]), "{stamps:?}");
    }

    #[test]
    fn a_session_with_no_day_takes_the_day_of_its_journal() {
        let mut list = mock_file(SESSIONS);
        list["sessions"][2]["date"] = Value::Null;
        let plan = replaced(&[(SESSIONS, list)]);
        assert!(unmapped(&plan).is_empty(), "{:?}", plan.skipped);
        let lines = rows(&plan, "session_journal_line");
        assert_eq!(lines.len(), 14);
        assert!(lines.iter().all(|l| {
            l["spoken_at"]
                .as_str()
                .is_some_and(|at| at.starts_with("2026-10-03"))
        }));
    }

    #[test]
    fn a_closing_without_an_edit_leaves_the_edit_day_null() {
        let mut closing = mock_file(CLOSING_FILE);
        closing["summaryMeta"] = json!("Généré à la clôture par claude");
        let plan = replaced(&[(CLOSING_FILE, closing)]);
        assert!(unmapped(&plan).is_empty(), "{:?}", plan.skipped);
        let sessions = rows(&plan, "session");
        let third = sessions
            .iter()
            .find(|s| s["id"] == id("session", "s3"))
            .expect("a session");
        assert_eq!(third["summary_model"], "claude");
        assert!(third["summary_edited_at"].is_null());
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
    fn the_running_session_is_the_one_the_live_fixture_names_and_nothing_else_changes() {
        // AC1: the session s4 is loaded live, from the time the fixture says it started.
        let live = mock_file(LIVE_FILE);
        assert_eq!(
            live["startedAt"],
            mock_file("LiveSessionBar.json")["session"]["startedAt"],
            "the Micro-UI's fixture is the same session"
        );
        let plan = mock();
        assert!(
            plan.loaded.iter().any(|f| f == LIVE_FILE),
            "{:?}",
            plan.skipped
        );
        assert!(!plan.skipped.iter().any(|(f, _)| f == LIVE_FILE));
        let with = stated(&plan);
        assert_eq!(
            with.iter()
                .filter(|(_, status, _)| status == "live")
                .count(),
            1
        );
        // Without the file, s4 is the list's own (in preparation, never started).
        let without = stated(&replaced_but(&[LIVE_FILE], &[]));
        let s4 = id("session", "s4");
        let state_of = |all: &[(Value, Value, Value)]| {
            all.iter()
                .find(|(row, _, _)| *row == s4)
                .map(|(_, status, started)| (status.clone(), started.clone()))
        };
        assert_eq!(state_of(&without), Some((json!("prep"), Value::Null)));
        assert_eq!(
            state_of(&with).map(|(status, _)| status),
            Some(json!("live"))
        );
        let others = |all: &[(Value, Value, Value)]| {
            all.iter()
                .filter(|(row, _, _)| *row != s4)
                .cloned()
                .collect::<Vec<_>>()
        };
        assert_eq!(
            others(&with),
            others(&without),
            "the other sessions are untouched"
        );
    }

    /// The `(id, status, started_at)` of every session planned.
    fn stated(plan: &Fixtures) -> Vec<(Value, Value, Value)> {
        rows(plan, "session")
            .iter()
            .map(|s| {
                (
                    s["id"].clone(),
                    s["status"].clone(),
                    s["started_at"].clone(),
                )
            })
            .collect()
    }

    #[test]
    fn a_live_fixture_that_does_not_fit_is_skipped_alone_and_the_session_stays_in_preparation() {
        let live = |edit: &dyn Fn(&mut Value)| {
            let mut file = mock_file(LIVE_FILE);
            edit(&mut file);
            file
        };
        let cases = [
            (
                "a session nobody lists",
                live(&|l| l["id"] = json!("s9")),
                "no fixture describes",
            ),
            (
                "a session already played",
                live(&|l| l["id"] = json!("s3")),
                "not in preparation",
            ),
            (
                "another day than the list's",
                live(&|l| l["date"] = json!("2026-10-18")),
                "not of 2026-10-18",
            ),
            (
                "another campaign than the list's",
                live(&|l| l["campaign"] = json!("autre")),
                "not of \"autre\"",
            ),
            (
                "another title than the list's",
                live(&|l| l["title"] = json!("Autre")),
                "is not titled",
            ),
            (
                "another order than the list's",
                live(&|l| l["order"] = json!(9)),
                "is not titled",
            ),
            (
                "a day that is not a day",
                live(&|l| l["date"] = json!("17 octobre")),
                "is not a day",
            ),
            (
                "a time that is not a time",
                live(&|l| l["startedAt"] = json!("25:61")),
                "not a time of the day",
            ),
            (
                "no start",
                live(&|l| l["startedAt"] = Value::Null),
                "startedAt",
            ),
        ];
        for (what, file, why) in cases {
            let plan = replaced(&[(LIVE_FILE, file)]);
            let skipped: Vec<&Skipped> = plan
                .skipped
                .iter()
                .filter(|(f, _)| f == LIVE_FILE)
                .collect();
            assert_eq!(skipped.len(), 1, "{what}: {:?}", plan.skipped);
            assert!(skipped[0].1.contains(why), "{what}: {skipped:?}");
            let session = rows(&plan, "session")
                .into_iter()
                .find(|s| s["id"] == id("session", "s4"))
                .expect("s4 is planned");
            assert_eq!(session["status"], "prep", "{what}");
            assert!(session["started_at"].is_null(), "{what}");
            assert!(
                plan.loaded.iter().any(|f| f == SESSIONS)
                    && plan.loaded.iter().any(|f| f == BOARD_FILE),
                "{what}: the rest is loaded"
            );
        }
        // A list that is withdrawn leaves the live fixture with no session to start.
        let mut list = mock_file(SESSIONS);
        list["campaign"] = json!("nowhere");
        let plan = replaced(&[(SESSIONS, list)]);
        assert!(
            plan.skipped
                .iter()
                .any(|(f, why)| f == LIVE_FILE && why.contains("no fixture describes the session")),
            "{:?}",
            plan.skipped
        );
        // A session the list leaves undated is on no day: it does not go live.
        let mut list = mock_file(SESSIONS);
        list["sessions"][3]["date"] = Value::Null;
        let plan = replaced(&[(SESSIONS, list)]);
        assert!(
            plan.skipped
                .iter()
                .any(|(f, why)| f == LIVE_FILE && why.contains("not of 2026-10-17")),
            "{:?}",
            plan.skipped
        );
        assert!(
            stated(&plan).iter().all(|(_, status, _)| status != "live"),
            "{:?}",
            plan.skipped
        );
        // Two sessions live: the list's own live one is not displaced.
        let mut list = mock_file(SESSIONS);
        list["sessions"][1]["status"] = json!("live");
        let plan = replaced(&[(SESSIONS, list)]);
        assert!(
            plan.skipped
                .iter()
                .any(|(f, why)| f == LIVE_FILE && why.contains("live already")),
            "{:?}",
            plan.skipped
        );
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
            // The scenes, the closing, the brief and the list name a session: they go with the sessions.
            let hanging = [
                BOARD_FILE,
                CLOSING_FILE,
                BRIEF_FILE,
                TO_CREATE_FILE,
                LIVE_FILE,
                DIFFUSION_FILE,
                INSTANCE_FILE,
            ];
            assert_eq!(
                names,
                [
                    SESSIONS,
                    BOARD_FILE,
                    CLOSING_FILE,
                    BRIEF_FILE,
                    TO_CREATE_FILE,
                    LIVE_FILE,
                    DIFFUSION_FILE,
                    INSTANCE_FILE
                ],
                "{what}"
            );
            assert!(left[0].1.contains(why), "{what}: {:?}", left[0]);
            for hung in &left[1..] {
                assert!(
                    hung.1.contains("no fixture describes the session"),
                    "{hung:?}"
                );
            }
            for file in [SESSIONS].iter().chain(&hanging) {
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
                "scene",
                "scene_entity",
                "scene_media",
                "battlemap_instance",
                "token",
                "session_journal_line",
                "oral_brief",
                "entity_to_create",
            ] {
                assert!(rows(&plan, table).is_empty(), "{what}: {table}");
            }
            assert!(
                !plan
                    .names
                    .iter()
                    .any(|(slug, _)| slug == "s4" || slug == "tc-odric" || slug == "sc1")
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

    #[test]
    fn the_instance_of_the_mock_up_is_planned_with_its_tokens_at_their_hexes() {
        let plan = mock();
        let fixture = mock_file(INSTANCE_FILE);
        assert_eq!(
            fixture,
            mock_file("Battlemap.json")["instances"][0],
            "the Micro-UI's fixture is the same instance"
        );
        let instances = rows(&plan, "battlemap_instance");
        assert_eq!(instances.len(), 1);
        let instance = id("battlemap_instance", "bmi-gue-s4");
        assert_eq!(
            instances[0],
            &json!({
                "id": instance,
                "battlemap_media_id": id("media", "bm-gue"),
                "session_id": id("session", "s4"),
                "scene_id": id("scene", "sc3"),
            })
        );
        let tokens = rows(&plan, "token");
        let fixture = fixture["state"]["tokens"].as_array().expect("tokens");
        assert_eq!(tokens.len(), 8);
        assert_eq!(tokens.len(), fixture.len());
        for (row, token) in tokens.iter().zip(fixture) {
            let slug = token["id"].as_str().expect("an id");
            assert_eq!(row["id"], id("token", slug));
            assert_eq!(row["instance_id"], instance);
            assert_eq!(
                row["character_id"],
                id("entity", token["char"].as_str().unwrap())
            );
            for (column, key) in [
                ("number", "num"),
                ("name", "name"),
                ("q", "q"),
                ("r", "r"),
                ("visible", "visible"),
                ("hp_current", "hp"),
                ("hp_max", "hpMax"),
                ("conditions", "states"),
            ] {
                assert_eq!(row[column], token[key], "{slug}.{column}");
            }
            assert!(plan.names.contains(&(slug.to_string(), id("token", slug))));
        }
        assert!(plan.names.contains(&("bmi-gue-s4".to_string(), instance)));
        let tables: Vec<&str> = plan.tables.iter().map(|t| t.name).collect();
        let at = |name: &str| tables.iter().position(|t| *t == name).expect(name);
        assert!(at("scene_media") < at("battlemap_instance"), "{tables:?}");
        assert_eq!(at("battlemap_instance") + 1, at("token"));
        let keyed = |table: &str| plan.tables.iter().find(|t| t.name == table).map(|t| t.key);
        assert_eq!(keyed("battlemap_instance"), Some("id"));
        assert_eq!(keyed("token"), Some("id"));
        // The scene's battlemap is not a column: the instance row names the scene.
        assert!(
            rows(&plan, "scene")
                .iter()
                .all(|s| s.get("battlemap").is_none())
        );
    }

    // AC1: the PCs' last initiative is planned as the seats of one ended combat
    #[test]
    fn the_pcs_last_initiative_is_planned_as_the_seats_of_a_combat_that_ended() {
        let plan = mock();
        let instance = id("battlemap_instance", "bmi-gue-s4");
        let combats = rows(&plan, "combat");
        assert_eq!(
            combats,
            [&json!({
                "id": id("combat", "bmi-gue-s4"), "instance_id": instance, "round": 1,
                "started_at": "2026-10-17T20:02:00.000000Z",
                "ended_at": "2026-10-17T20:02:00.000001Z",
            })],
            "round 1, over, at the start of the session under way"
        );
        // elowen 18, sirielle 14, kael 12, brannoc 9: rank 1 to 4, one entry per PC token.
        let wanted = [
            ("t-elowen", 18),
            ("t-sirielle", 14),
            ("t-kael", 12),
            ("t-brannoc", 9),
        ];
        let seats = rows(&plan, "initiative_entry");
        assert_eq!(seats.len(), wanted.len());
        for (index, (seat, (token, initiative))) in seats.iter().zip(wanted).enumerate() {
            assert_eq!(
                seat,
                &&json!({
                    "id": id("initiative_entry", &format!("bmi-gue-s4:{token}")),
                    "combat_id": id("combat", "bmi-gue-s4"), "token_id": id("token", token),
                    "initiative": initiative, "tiebreak": 0, "rank": index + 1,
                })
            );
        }
        let tables: Vec<&str> = plan.tables.iter().map(|t| t.name).collect();
        let at = |name: &str| tables.iter().position(|t| *t == name).expect(name);
        assert_eq!(at("token") + 1, at("combat"));
        assert_eq!(at("combat") + 1, at("initiative_entry"));
        let keyed = |table: &str| plan.tables.iter().find(|t| t.name == table).map(|t| t.key);
        assert_eq!(keyed("combat"), Some("id"));
        assert_eq!(keyed("initiative_entry"), Some("id"));
    }

    #[test]
    fn equal_initiatives_are_ranked_by_character_and_no_initiative_plans_no_combat() {
        let edited = |edit: &dyn Fn(&mut Value)| {
            let mut value = mock_file(INSTANCE_FILE);
            edit(&mut value);
            replaced(&[(INSTANCE_FILE, value)])
        };
        let plan = edited(&|v| v["pjInitiative"] = json!({ "kael": 12, "elowen": 12 }));
        let seats = rows(&plan, "initiative_entry");
        let order: Vec<_> = seats.iter().map(|s| s["token_id"].clone()).collect();
        assert_eq!(
            order,
            [id("token", "t-elowen"), id("token", "t-kael")],
            "ties by character"
        );
        // The bounds are inclusive.
        let plan = edited(&|v| v["pjInitiative"] = json!({ "kael": 1, "elowen": 40 }));
        let figures: Vec<_> = rows(&plan, "initiative_entry")
            .iter()
            .map(|s| s["initiative"].clone())
            .collect();
        assert_eq!(figures, [json!(40), json!(1)]);
        // None entered, or none given: the instance loads with no combat.
        for none in [json!({}), json!(null)] {
            let plan = edited(&|v| v["pjInitiative"] = none.clone());
            assert!(unmapped(&plan).is_empty(), "{:?}", plan.skipped);
            assert_eq!(rows(&plan, "token").len(), 8);
            assert!(rows(&plan, "combat").is_empty());
            assert!(rows(&plan, "initiative_entry").is_empty());
        }
    }

    // AC2: the session under way has received the cover, as its fixture says
    #[test]
    fn the_session_under_way_has_received_the_cover_and_nothing_else() {
        let plan = mock();
        assert_eq!(
            rows(&plan, "diffusion"),
            [&json!({
                "session_id": id("session", "s4"), "base_kind": "cover",
                "view_zoom": 1, "view_pan_x": 0, "view_pan_y": 0,
            })]
        );
        let keyed = plan.tables.iter().find(|t| t.name == "diffusion");
        assert_eq!(keyed.map(|t| t.key), Some("session_id"));
        let tables: Vec<&str> = plan.tables.iter().map(|t| t.name).collect();
        let at = |name: &str| tables.iter().position(|t| *t == name).expect(name);
        assert!(at("session") < at("diffusion"), "{tables:?}");
    }

    #[test]
    fn a_diffusion_that_shows_more_than_the_cover_or_that_nobody_receives_is_skipped_alone() {
        let edited = |edit: &dyn Fn(&mut Value)| {
            let mut value = mock_file(DIFFUSION_FILE);
            edit(&mut value);
            (DIFFUSION_FILE, value)
        };
        let cases = [
            (
                "a background that is an illustration",
                vec![edited(&|v| {
                    v["base"] = json!({ "kind": "illustration", "id": "ill-moulin" })
                })],
                "only the cover",
            ),
            (
                "a cover that is not the campaign's",
                vec![edited(&|v| v["base"]["id"] = json!("ill-moulin"))],
                "is not \"ill-cover\"",
            ),
            (
                "a window",
                vec![edited(
                    &|v| {
                        v["window"] = json!({ "media": "ill-moulin", "x": 0.1, "y": 0.1, "w": 0.3 })
                    },
                )],
                "a window or a framing",
            ),
            (
                "a framing",
                vec![edited(&|v| {
                    v["view"] = json!({ "zoom": 2, "panX": 0, "panY": 0 })
                })],
                "a window or a framing",
            ),
            (
                "no session under way",
                vec![(
                    LIVE_FILE,
                    json!({ "id": "s4", "date": "not a day", "startedAt": "20:02" }),
                )],
                "no fixture describes the session under way",
            ),
        ];
        for (what, over, why) in cases {
            let plan = replaced(&over);
            let left: Vec<&Skipped> = unmapped(&plan)
                .into_iter()
                .filter(|(name, _)| name == DIFFUSION_FILE)
                .collect();
            assert_eq!(left.len(), 1, "{what}: {:?}", plan.skipped);
            assert!(left[0].1.contains(why), "{what}: {:?}", left[0]);
            assert!(rows(&plan, "diffusion").is_empty(), "{what}");
            assert!(!plan.loaded.iter().any(|n| n == DIFFUSION_FILE), "{what}");
            // The instance and its combat stay planned.
            assert_eq!(rows(&plan, "token").len(), 8, "{what}");
        }
    }

    // The combat of an instance on another session than the one under way is stamped at the fixtures' first stamp
    #[test]
    fn the_combat_of_an_instance_on_no_session_under_way_is_stamped_at_the_first_stamp() {
        let plan = replaced(&[(
            LIVE_FILE,
            json!({ "id": "s4", "date": "not a day", "startedAt": "20:02" }),
        )]);
        assert_eq!(
            rows(&plan, "combat"),
            [&json!({
                "id": id("combat", "bmi-gue-s4"), "instance_id": id("battlemap_instance", "bmi-gue-s4"),
                "round": 1,
                "started_at": "2026-08-20T10:00:00.000000Z",
                "ended_at": "2026-08-20T10:00:00.000001Z",
            })]
        );
        assert_eq!(rows(&plan, "initiative_entry").len(), 4);
    }

    // The store's live session comes first: the one under way stays in preparation
    #[test]
    fn a_live_session_of_the_store_keeps_the_one_under_way_in_preparation() {
        let s4 = id("session", "s4");
        let before = stated(&mock());
        assert!(before.contains(&(json!(s4), json!("live"), json!("2026-10-17T20:02:00Z"))));
        // Nothing live, or its own row after a restart: planned as is.
        for held in [vec![], vec![s4.clone()]] {
            let mut plan = mock();
            plan.leave_out_second_live(&held);
            assert_eq!(stated(&plan), before, "{held:?}");
            assert_eq!(rows(&plan, "diffusion").len(), 1, "{held:?}");
            assert!(unmapped(&plan).is_empty(), "{:?}", plan.skipped);
        }
        // Another session live, of this campaign or another.
        let mut plan = mock();
        plan.leave_out_second_live(&[id("session", "elsewhere")]);
        assert!(
            stated(&plan)
                .iter()
                .all(|(_, status, started)| status != "live" && started.is_null()),
            "{:?}",
            stated(&plan)
        );
        assert_eq!(stated(&plan).len(), before.len());
        assert!(rows(&plan, "diffusion").is_empty());
        let files: Vec<&str> = unmapped(&plan).iter().map(|(f, _)| f.as_str()).collect();
        assert_eq!(files, [LIVE_FILE, DIFFUSION_FILE]);
        assert!(
            !plan
                .loaded
                .iter()
                .any(|n| n == LIVE_FILE || n == DIFFUSION_FILE)
        );
        // The instance and its tokens stay planned.
        assert_eq!(rows(&plan, "token").len(), 8);
    }

    // AC3: a scene the store gave another instance loads neither the instance nor its tokens, combat or seats
    #[test]
    fn a_scene_the_store_gave_another_instance_loads_neither_instance_nor_tokens() {
        let (instance, scene) = (id("battlemap_instance", "bmi-gue-s4"), id("scene", "sc3"));
        let table_rows = |plan: &Fixtures| {
            (
                rows(plan, "battlemap_instance").len(),
                rows(plan, "token").len(),
                rows(plan, "combat").len(),
                rows(plan, "initiative_entry").len(),
            )
        };
        // Another scene's instance, and the fixture's own after a restart: planned as is.
        for held in [
            vec![],
            vec![("dm".to_string(), id("scene", "sc1"))],
            vec![(instance.clone(), scene.clone())],
        ] {
            let mut plan = mock();
            let before = mock();
            plan.leave_out_taken_scenes(&held);
            assert_eq!(plan, before, "{held:?}");
            assert_eq!(table_rows(&plan), (1, 8, 1, 4));
        }
        // The DM's own instance on sc3, with or without the fixture's.
        for held in [
            vec![("dm".to_string(), scene.clone())],
            vec![(instance.clone(), scene.clone()), ("dm".to_string(), scene)],
        ] {
            let mut plan = mock();
            plan.leave_out_taken_scenes(&held);
            assert_eq!(table_rows(&plan), (0, 0, 0, 0), "{held:?}");
            assert!(!plan.loaded.iter().any(|f| f == INSTANCE_FILE));
            assert_eq!(plan.skipped.len(), mock().skipped.len() + 1);
            assert!(
                plan.skipped
                    .iter()
                    .any(|(f, why)| f == INSTANCE_FILE && why.contains("already has an instance")),
                "{:?}",
                plan.skipped
            );
            assert!(
                plan.names
                    .iter()
                    .all(|(slug, _)| { slug != "bmi-gue-s4" && !slug.starts_with("t-") }),
                "{:?}",
                plan.names
            );
            // The scenes and the rest stay planned.
            assert_eq!(rows(&plan, "scene").len(), rows(&mock(), "scene").len());
        }
    }

    #[test]
    fn an_instance_that_does_not_fit_is_skipped_alone_and_loads_no_row() {
        let edited = |file: &str, edit: &dyn Fn(&mut Value)| {
            let mut value = mock_file(file);
            edit(&mut value);
            (file.to_string(), value)
        };
        let instance = |edit: &dyn Fn(&mut Value)| edited(INSTANCE_FILE, edit);
        let token =
            |n: usize, edit: &dyn Fn(&mut Value)| instance(&|v| edit(&mut v["state"]["tokens"][n]));
        // Each case replaces one or two files, and says why the instance is left out.
        type Case = (&'static str, Vec<(String, Value)>, &'static str);
        let cases: Vec<Case> = vec![
            (
                "a combat running",
                vec![instance(&|v| v["state"]["combat"] = json!({ "round": 1 }))],
                "combat running",
            ),
            (
                "an initiative of nobody's",
                vec![instance(&|v| v["pjInitiative"]["nobody"] = json!(10))],
                "names \"nobody\", no PC's token",
            ),
            (
                "an initiative of an NPC's",
                vec![instance(&|v| v["pjInitiative"]["vesna"] = json!(10))],
                "names \"vesna\", no PC's token",
            ),
            (
                "an initiative of 0",
                vec![instance(&|v| v["pjInitiative"]["elowen"] = json!(0))],
                "not a whole number from 1 to 40",
            ),
            (
                "an initiative of 41",
                vec![instance(&|v| v["pjInitiative"]["elowen"] = json!(41))],
                "not a whole number from 1 to 40",
            ),
            (
                "an initiative with a fraction",
                vec![instance(&|v| v["pjInitiative"]["elowen"] = json!(12.5))],
                "not a whole number from 1 to 40",
            ),
            (
                "an initiative that is a text",
                vec![instance(&|v| v["pjInitiative"]["elowen"] = json!("18"))],
                "not a whole number from 1 to 40",
            ),
            (
                "initiatives that are a list",
                vec![instance(&|v| v["pjInitiative"] = json!([18]))],
                "pjInitiative is not an object",
            ),
            (
                "a session nobody gives scenes to",
                vec![instance(&|v| v["session"] = json!("s3"))],
                "no fixture describes the session",
            ),
            (
                "a scene the session lacks",
                vec![instance(&|v| v["scene"] = json!("sc9"))],
                "is no scene of the session",
            ),
            (
                "a scene that names no battlemap",
                vec![edited(BOARD_FILE, &|v| {
                    v["scenes"][2]["battlemap"] = json!(null)
                })],
                "does not name",
            ),
            (
                "a scene that names another",
                vec![edited(BOARD_FILE, &|v| {
                    v["scenes"][2]["battlemap"] = json!("bmi-x")
                })],
                "does not name",
            ),
            (
                "a battlemap the scene does not link",
                vec![instance(&|v| v["battlemap"] = json!("bm-moulin"))],
                "is not linked to",
            ),
            (
                "an illustration for a battlemap",
                vec![
                    instance(&|v| v["battlemap"] = json!("ill-moulin")),
                    edited(BOARD_FILE, &|v| {
                        v["scenes"][2]["media"] = json!(["ill-moulin"])
                    }),
                ],
                "is no battlemap of its campaign",
            ),
            (
                "a slug an element has",
                vec![
                    instance(&|v| v["id"] = json!("elowen")),
                    edited(BOARD_FILE, &|v| {
                        v["scenes"][2]["battlemap"] = json!("elowen")
                    }),
                ],
                "names two rows",
            ),
            (
                "a slug a session has",
                vec![token(0, &|t| t["id"] = json!("s1"))],
                "names two rows",
            ),
            (
                "a slug a scene has",
                vec![token(0, &|t| t["id"] = json!("sc1"))],
                "names two rows",
            ),
            (
                "a slug an entry has",
                vec![token(0, &|t| t["id"] = json!("tc-gue"))],
                "names two rows",
            ),
            (
                "a slug a medium has",
                vec![token(0, &|t| t["id"] = json!("bm-gue"))],
                "names two rows",
            ),
            (
                "a token named as the instance",
                vec![token(0, &|t| t["id"] = json!("bmi-gue-s4"))],
                "names two rows",
            ),
            (
                "a token twice",
                vec![token(1, &|t| t["id"] = json!("t-elowen"))],
                "names two rows",
            ),
            (
                "a character nobody describes",
                vec![token(0, &|t| t["char"] = json!("nobody"))],
                "is no \"pj\"",
            ),
            (
                "an NPC that is a PC",
                vec![token(4, &|t| t["char"] = json!("elowen"))],
                "is no \"pnj\"",
            ),
            (
                "a PC that is not present",
                vec![edited(SESSIONS, &|v| {
                    v["sessions"][3]["present"] = json!(["elowen", "brannoc", "sirielle"]);
                })],
                "is not present at",
            ),
            (
                "hit points the sheet does not give",
                vec![token(4, &|t| {
                    t["hp"] = json!(12);
                    t["hpMax"] = json!(12);
                })],
                "are not those of the sheet",
            ),
            (
                "a PC twice",
                vec![token(1, &|t| t["char"] = json!("elowen"))],
                "stands twice",
            ),
            (
                "an NPC numbered twice",
                vec![token(5, &|t| t["num"] = json!(1))],
                "stands twice",
            ),
            (
                "a kind of nobody's",
                vec![token(0, &|t| t["kind"] = json!("monster"))],
                "is of kind",
            ),
            (
                "a PC with a number",
                vec![token(0, &|t| t["num"] = json!(1))],
                "has a number or hit points",
            ),
            (
                "a PC with hit points",
                vec![token(0, &|t| t["hp"] = json!(10))],
                "has a number or hit points",
            ),
            (
                "an NPC with no number",
                vec![token(4, &|t| t["num"] = json!(null))],
                "has no number",
            ),
            (
                "an NPC with no hit points",
                vec![token(4, &|t| t["hp"] = json!(null))],
                "has no number or no hit points",
            ),
            (
                "an NPC with more hit points than its maximum",
                vec![token(4, &|t| t["hp"] = json!(12))],
                "within its maximum",
            ),
            (
                "an NPC dead with hit points left",
                vec![token(4, &|t| t["dead"] = json!(true))],
                "if and only if",
            ),
            (
                "an NPC at no hit point who is not dead",
                vec![token(4, &|t| t["hp"] = json!(0))],
                "if and only if",
            ),
            (
                "a number that is no whole number",
                vec![token(4, &|t| t["num"] = json!(1.5))],
                "num of t-bandit-1 is not a whole number from 1",
            ),
            (
                "a number of zero",
                vec![token(4, &|t| t["num"] = json!(0))],
                "num of t-bandit-1 is not a whole number from 1",
            ),
            (
                "negative hit points",
                vec![token(4, &|t| t["hp"] = json!(-1))],
                "hp of t-bandit-1 is not a whole number from 0",
            ),
            (
                "a hex out of the column",
                vec![token(0, &|t| t["q"] = json!(3_000_000_000_u64))],
                "q of t-elowen is not a whole number",
            ),
            (
                "a hex that is no number",
                vec![token(0, &|t| t["r"] = json!("six"))],
                "r of t-elowen is not a whole number",
            ),
            (
                "a visibility that is no boolean",
                vec![token(0, &|t| t["visible"] = json!("yes"))],
                "visible of t-elowen is not a boolean",
            ),
            (
                "a state that is no text",
                vec![token(0, &|t| t["states"] = json!([7]))],
                "is not a text",
            ),
            (
                "no states",
                vec![token(0, &|t| t["states"] = json!(null))],
                "states is not a list",
            ),
            (
                "no name",
                vec![token(0, &|t| t["name"] = json!(""))],
                "has no name",
            ),
            (
                "no tokens list",
                vec![instance(&|v| v["state"]["tokens"] = json!(null))],
                "tokens is not a list",
            ),
        ];
        for (what, over, why) in cases {
            let over: Vec<(&str, Value)> =
                over.iter().map(|(n, v)| (n.as_str(), v.clone())).collect();
            let plan = replaced(&over);
            let left: Vec<&Skipped> = unmapped(&plan)
                .into_iter()
                .filter(|(name, _)| name == INSTANCE_FILE)
                .collect();
            assert_eq!(left.len(), 1, "{what}: {:?}", plan.skipped);
            assert!(left[0].1.contains(why), "{what}: {:?}", left[0]);
            assert!(left[0].1.starts_with("cannot be mapped"), "{what}");
            assert!(!plan.loaded.iter().any(|n| n == INSTANCE_FILE), "{what}");
            for table in ["battlemap_instance", "token", "combat", "initiative_entry"] {
                assert!(rows(&plan, table).is_empty(), "{what}: {table}");
            }
            assert!(
                plan.names.iter().all(|(slug, _)| slug != "bmi-gue-s4"),
                "{what}"
            );
            assert!(
                plan.names.iter().all(|(slug, _)| slug != "t-elowen"),
                "{what}"
            );
            // Nothing else is left out: the scenes load, and the media stay.
            assert_eq!(unmapped(&plan).len(), 1, "{what}: {:?}", plan.skipped);
            assert_eq!(rows(&plan, "scene").len(), 3, "{what}");
            assert_eq!(rows(&plan, "media").len(), 7, "{what}");
        }
    }

    #[test]
    fn a_file_with_no_instance_leaves_the_scenes_and_the_rest_loaded() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../mock/fixtures");
        let mut files: Vec<(String, String)> = std::fs::read_dir(&dir)
            .expect("the fixtures folder")
            .filter_map(|entry| {
                let path = entry.ok()?.path();
                let name = path.file_name()?.to_str()?.to_string();
                (name != INSTANCE_FILE && path.extension().is_some_and(|e| e == "json"))
                    .then(|| (name, std::fs::read_to_string(&path).expect("a fixture")))
            })
            .collect();
        files.sort();
        let files: Vec<(&str, &str)> = files
            .iter()
            .map(|(n, t)| (n.as_str(), t.as_str()))
            .collect();
        let folder = folder("no-instance", &files);
        let plan = fixtures(&folder).expect("the folder is listed");
        std::fs::remove_dir_all(&folder).ok();
        assert!(unmapped(&plan).is_empty(), "{:?}", plan.skipped);
        assert_eq!(rows(&plan, "scene").len(), 3);
        assert!(rows(&plan, "battlemap_instance").is_empty());
        assert!(rows(&plan, "token").is_empty());
    }
}
