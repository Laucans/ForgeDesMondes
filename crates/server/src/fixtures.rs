//! The mock-up's fixtures (`mock/fixtures/*.json`) as the rows the store
//! holds.
//!
//! [`fixtures`] reads the folder and plans the load; it touches no database.
//! A fixture a landed aggregate holds (the Campaign, Player and Entity
//! aggregates') becomes the rows of its tables, in the order the foreign keys
//! and the Entity aggregate's triggers need; the others are skipped with their
//! reason. What is planned is loaded whole, in one transaction, by
//! [`data::insert_missing`]: the tables' CHECKs and invariants judge every row
//! as they judge any write.
//!
//! The fixtures name their rows by slug (`cendres-valombre`, `elowen`) and
//! the tables key them by uuid: each id, and each reference to it, becomes
//! the UUIDv5 of `<table>:<slug>` under [`NAMESPACE`], so a restart plans the
//! same ids and loads nothing twice. [`Fixtures::names`] pairs each slug with
//! its uuid. A row with no slug of its own is named by what identifies it:
//! `entity_alias:<element slug>:<alias>`, `entity_version:<element slug>:<n>`.
//!
//! What the tables have no column for is not loaded: the portrait and the
//! crop (they point at a media row, which waits for the Media aggregate), the
//! speed and the descriptive `type` line of a sheet, the art and illustration
//! of a place, the links between places and characters.

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
const VERSION: &[&str] = &[
    "id",
    "entity_id",
    "version_number",
    "source",
    "snapshot",
    "diff",
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
    read.plan(&mut plan);
    Ok(plan)
}

/// What the files that mapped say. A file maps whole or not at all: nothing
/// of one that fails is kept.
#[derive(Default)]
struct Read {
    /// The campaigns by slug: name, kind, pitch, art, glyph, and the dates
    /// when a fixture gives them.
    campaigns: BTreeMap<String, Map<String, Value>>,
    entities: Option<Entities>,
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
            ));
        }
        for (slug, row) in rows {
            self.campaign(&slug, &row);
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

    /// The rows of everything read, in the order they are inserted.
    fn plan(self, plan: &mut Fixtures) {
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
        // Fixed and in fixture order, a microsecond apart, so that a list
        // keeps the fixture's.
        let at = format!("2026-08-20T10:00:00.{index:06}Z");
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

/// A day (`2026-08-20`) as the midnight UTC the column holds; null stays null.
fn date(value: &Value, key: &str) -> Result<Value, String> {
    match value[key].as_str() {
        None => Ok(Value::Null),
        Some(day) if day.len() == 10 => Ok(json!(format!("{day}T00:00:00Z"))),
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
                "campaign.getCampaign.json",
                "campaign.listCampaigns.json",
                "entities.json"
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
                "entity_version"
            ]
        );
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
}
