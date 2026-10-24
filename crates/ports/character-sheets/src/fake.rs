//! The fake of the `character-sheets` port: it answers from the mock-up's
//! fixtures, reaches nothing, and fails the way the real service does.
//!
//! [`FakeCharacterSheets::from_fixtures`] reads `mock/fixtures/entities.json`
//! (the characters that have a D&D Beyond link, under the id that link names)
//! and `mock/fixtures/EntityEditor.json` (`ddbResync`, what a level-up
//! changes). A character is first answered as the DM last synchronised it;
//! [`FakeCharacterSheets::level_up`] plays the level-up on D&D Beyond's side,
//! after which the sheet carries the patched values. The fixtures do not tell
//! an action from a spell: a feature whose text starts with « Sort de niveau »
//! is a spell, any other is an action, and what a level-up adds to the
//! « Sorts et capacités » line is a spell with [`LEVEL_UP_DESCRIPTION`].
//!
//! A handle is shared: a clone is the same fake, so the code that plays the
//! level-up holds the one the units fetch from.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use serde_json::Value;

use crate::{CharacterId, CharacterSheet, CharacterSheets, CharacterSheetsError, Entry, StatBlock};

/// What the fixtures give a spell that a level-up adds: they name it, they do
/// not describe it.
pub const LEVEL_UP_DESCRIPTION: &str = "Ajouté à la fiche D&D Beyond lors de la montée de niveau.";

/// The line of `ddbResync.<entity>.changes` that lists abilities and spells.
const SPELLS_FIELD: &str = "Sorts et capacités";

/// A fixture the fake cannot read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FixtureError(pub String);

impl fmt::Display for FixtureError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "fixture illisible : {}", self.0)
    }
}

impl std::error::Error for FixtureError {}

#[derive(Debug, Default)]
struct State {
    sheets: BTreeMap<CharacterId, CharacterSheet>,
    after_level_up: BTreeMap<CharacterId, CharacterSheet>,
    private: BTreeSet<CharacterId>,
    changed: BTreeSet<CharacterId>,
}

/// The fake. A clone shares what it knows.
#[derive(Clone, Debug, Default)]
pub struct FakeCharacterSheets {
    state: Arc<Mutex<State>>,
}

impl FakeCharacterSheets {
    /// A fake that knows no character: every fetch is [`CharacterSheetsError::NotFound`].
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// A fake that knows the characters of `entities` (`entities.json`) that
    /// have a D&D Beyond link, and the level-ups of `editor`
    /// (`EntityEditor.json`).
    ///
    /// # Errors
    ///
    /// [`FixtureError`] when a character with a link lacks a field of a sheet,
    /// or when a patch is not what `ddbResync` says it is.
    pub fn from_fixtures(entities: &Value, editor: &Value) -> Result<Self, FixtureError> {
        let fake = Self::new();
        let characters = entities["characters"]
            .as_array()
            .ok_or_else(|| FixtureError("entities.json: `characters` is not a list".into()))?;
        for character in characters {
            let Some(url) = character["ddb"]["url"].as_str() else {
                continue;
            };
            let entity = text(character, "id").map_err(FixtureError)?;
            let id = CharacterId::parse(url)
                .map_err(|error| FixtureError(format!("{entity}: {error}")))?;
            let before =
                sheet_of(id, character).map_err(|e| FixtureError(format!("{entity}: {e}")))?;
            let resync = match &editor["ddbResync"][entity] {
                Value::Null => &editor["ddbResync"]["default"],
                own => own,
            };
            let after = level_up_of(&before, resync)
                .map_err(|e| FixtureError(format!("ddbResync.{entity}: {e}")))?;
            let mut state = fake.state();
            state.sheets.insert(id, before);
            state.after_level_up.insert(id, after);
        }
        Ok(fake)
    }

    /// Makes the fake answer `sheet`, as the service would for its id. A
    /// level-up the fixtures planned for that character is dropped: it was
    /// written for another sheet.
    #[must_use]
    pub fn with_sheet(self, sheet: CharacterSheet) -> Self {
        let mut state = self.state();
        state.after_level_up.remove(&sheet.id);
        state.sheets.insert(sheet.id, sheet);
        drop(state);
        self
    }

    /// Makes the character `id` exist but not be shared as public.
    #[must_use]
    pub fn with_private(self, id: CharacterId) -> Self {
        self.state().private.insert(id);
        self
    }

    /// Makes the service answer `id` in a shape the port does not know.
    #[must_use]
    pub fn with_changed_shape(self, id: CharacterId) -> Self {
        self.state().changed.insert(id);
        self
    }

    /// Plays the level-up of the character `id` on D&D Beyond's side: its sheet
    /// carries the patched values from now on. Answers whether the fake knew a
    /// level-up for it.
    pub fn level_up(&self, id: CharacterId) -> bool {
        let mut state = self.state();
        let Some(after) = state.after_level_up.get(&id).cloned() else {
            return false;
        };
        state.sheets.insert(id, after);
        true
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl CharacterSheets for FakeCharacterSheets {
    async fn fetch(&self, id: CharacterId) -> Result<CharacterSheet, CharacterSheetsError> {
        let state = self.state();
        if state.changed.contains(&id) {
            return Err(CharacterSheetsError::ServiceChanged(
                "forme inconnue (fake)".into(),
            ));
        }
        if state.private.contains(&id) {
            return Err(CharacterSheetsError::NotPublic);
        }
        state
            .sheets
            .get(&id)
            .cloned()
            .ok_or(CharacterSheetsError::NotFound)
    }
}

/// The text of `key` in `value`.
fn text<'a>(value: &'a Value, key: &str) -> Result<&'a str, String> {
    value[key]
        .as_str()
        .ok_or_else(|| format!("`{key}` is not text"))
}

/// The number of `key` in `value`, as a figure of a sheet.
fn figure(value: &Value, key: &str) -> Result<u32, String> {
    value[key]
        .as_u64()
        .and_then(|n| u32::try_from(n).ok())
        .ok_or_else(|| format!("`{key}` is not a whole number"))
}

/// The six abilities, under the fixtures' (French) abbreviations, in the
/// order of a statblock.
const ABILITIES: [&str; 6] = ["FOR", "DEX", "CON", "INT", "SAG", "CHA"];

/// The sheet a character of `entities.json` was last synchronised as.
fn sheet_of(id: CharacterId, character: &Value) -> Result<CharacterSheet, String> {
    let mut scores = [0u32; 6];
    for (score, ability) in scores.iter_mut().zip(ABILITIES) {
        *score = figure(&character["stats"], ability)?;
    }
    let features = character["features"]
        .as_array()
        .ok_or("`features` is not a list")?;
    let (mut actions, mut spells) = (Vec::new(), Vec::new());
    for feature in features {
        let description = text(feature, "text")?;
        let entry = Entry::new(text(feature, "name")?, description);
        if description.starts_with("Sort de niveau") {
            spells.push(entry);
        } else {
            actions.push(entry);
        }
    }
    let [
        strength,
        dexterity,
        constitution,
        intelligence,
        wisdom,
        charisma,
    ] = scores;
    Ok(CharacterSheet {
        id,
        name: text(character, "name")?.to_owned(),
        class_and_level: text(character, "type")?.to_owned(),
        statblock: StatBlock {
            strength,
            dexterity,
            constitution,
            intelligence,
            wisdom,
            charisma,
            armor_class: figure(character, "ca")?,
            hp_max: figure(character, "pvMax")?,
            actions,
            spells,
        },
    })
}

/// `before`, after the level-up `resync` (an entry of `ddbResync`) describes:
/// its `patch` for the type, the hit points and the scores, its `changes` for
/// the spells it adds.
fn level_up_of(before: &CharacterSheet, resync: &Value) -> Result<CharacterSheet, String> {
    let mut after = before.clone();
    let patch = &resync["patch"];
    if let Some(line) = patch.get("type") {
        line.as_str()
            .ok_or("`patch.type` is not text")?
            .clone_into(&mut after.class_and_level);
    }
    if patch.get("pvMax").is_some() {
        after.statblock.hp_max = figure(patch, "pvMax")?;
    }
    if patch.get("ca").is_some() {
        after.statblock.armor_class = figure(patch, "ca")?;
    }
    if let Some(stats) = patch.get("stats") {
        let block = &mut after.statblock;
        for (ability, score) in [
            ("FOR", &mut block.strength),
            ("DEX", &mut block.dexterity),
            ("CON", &mut block.constitution),
            ("INT", &mut block.intelligence),
            ("SAG", &mut block.wisdom),
            ("CHA", &mut block.charisma),
        ] {
            if stats.get(ability).is_some() {
                *score = figure(stats, ability)?;
            }
        }
    }
    for change in resync["changes"].as_array().into_iter().flatten() {
        if change["field"].as_str() != Some(SPELLS_FIELD) {
            continue;
        }
        let known = names(text(change, "old")?);
        for name in names(text(change, "new")?) {
            if !known.contains(&name) {
                after
                    .statblock
                    .spells
                    .push(Entry::new(name, LEVEL_UP_DESCRIPTION));
            }
        }
    }
    Ok(after)
}

/// The names of a « Sorts et capacités » line: `A, B, C`.
fn names(line: &str) -> Vec<String> {
    line.split(',')
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_owned)
        .collect()
}
