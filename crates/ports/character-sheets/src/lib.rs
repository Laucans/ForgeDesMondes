//! The `character-sheets` port (ports.json) — the one seam to D&D Beyond.
//!
//! A DataCapability that imports or resynchronises a player character calls
//! [`CharacterSheets`] and never knows which service answers. The port is
//! read-only: it fetches a [`CharacterSheet`] and writes nothing anywhere.
//!
//! - **By URL or by id.** [`CharacterId::parse`] takes a character's D&D
//!   Beyond URL (`https://www.dndbeyond.com/characters/<id>`) or its bare id
//!   and answers the same id; a URL of another host is refused with
//!   [`CharacterSheetsError::ForeignHost`]. The port never fetches a URL it was
//!   handed: it builds its own from the id, so no pasted link can send it
//!   elsewhere.
//! - **The sheet in the product's terms.** A [`CharacterSheet`] holds the
//!   identity of the import (the id and the name), the class and level line,
//!   and a [`StatBlock`] shaped as `StatBlock@1`: the six ability scores,
//!   `armorClass`, `hpMax`, `actions` and `spells`. That is what a resync may
//!   change and nothing else: no alias, note, token or portrait, which stay
//!   the DM's (Entity invariant `dndbeyond-resync-scope`). A sheet is a
//!   statement of what the service said; whether it is a complete statblock is
//!   the Concept's judgement, not the port's.
//! - **A failure is typed.** A character not shared as public, one that does
//!   not exist, a service whose answer changed shape, a service that did not
//!   answer: each is a [`CharacterSheetsError`], never a panic.
//! - **[`fake`]** answers from the mock-up's fixtures and reaches nothing: the
//!   tests, the CI, the rules' runtime and the mock-up run on it.
//! - **[`contract`]** is the suite every adapter passes, run against itself
//!   from its own tests.
//!
//! The adapter (the public character JSON service) lives under
//! `crates/ports/character-sheets/<provider>/`; this crate holds no HTTP
//! client.

use std::fmt;
use std::future::Future;

use serde::{Deserialize, Serialize};

pub mod contract;
pub mod fake;
pub mod shared;

pub use shared::SharedCharacterSheets;

/// The hosts a character URL may name.
const HOSTS: [&str; 2] = ["www.dndbeyond.com", "dndbeyond.com"];

/// The id of a character on D&D Beyond.
///
/// ```
/// use ports_character_sheets::CharacterId;
/// let id = CharacterId::parse("https://www.dndbeyond.com/characters/112847390").unwrap();
/// assert_eq!(id, CharacterId::parse("112847390").unwrap());
/// assert_eq!(id.url(), "https://www.dndbeyond.com/characters/112847390");
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "u64")]
pub struct CharacterId(u64);

impl TryFrom<u64> for CharacterId {
    type Error = CharacterSheetsError;

    fn try_from(value: u64) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl CharacterId {
    /// The id `value`.
    ///
    /// # Errors
    ///
    /// [`CharacterSheetsError::BadReference`] for 0: D&D Beyond has no such
    /// character.
    pub fn new(value: u64) -> Result<Self, CharacterSheetsError> {
        if value == 0 {
            return Err(CharacterSheetsError::BadReference(value.to_string()));
        }
        Ok(Self(value))
    }

    /// The id a DM gave: a bare id, or a character's URL on `dndbeyond.com`
    /// (`http` or `https`, with or without `www.`, `/characters/<id>` or
    /// `/profile/<name>/characters/<id>`, any path after the id, a query or a
    /// fragment). A short link (`ddb.ac`) is another host: the port never
    /// follows a link it was handed.
    ///
    /// # Errors
    ///
    /// [`CharacterSheetsError::ForeignHost`] for a URL of another host,
    /// [`CharacterSheetsError::BadReference`] for anything else that is not a
    /// character's id or URL.
    pub fn parse(input: &str) -> Result<Self, CharacterSheetsError> {
        let input = input.trim();
        let bad = || CharacterSheetsError::BadReference(input.to_owned());
        if input.bytes().all(|byte| byte.is_ascii_digit()) {
            return input.parse().map_err(|_| bad()).and_then(Self::new);
        }
        let rest = input
            .strip_prefix("https://")
            .or_else(|| input.strip_prefix("http://"))
            .ok_or_else(bad)?;
        let (authority, path) = rest.split_once('/').ok_or_else(bad)?;
        if !HOSTS.contains(&authority.to_ascii_lowercase().as_str()) {
            return Err(CharacterSheetsError::ForeignHost(authority.to_owned()));
        }
        let path = path.split(['?', '#']).next().unwrap_or_default();
        let segments: Vec<&str> = path.split('/').collect();
        let id = match segments.as_slice() {
            ["characters", id, ..] | ["profile", _, "characters", id, ..] => id,
            _ => return Err(bad()),
        };
        if id.is_empty() || !id.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(bad());
        }
        id.parse().map_err(|_| bad()).and_then(Self::new)
    }

    /// The id as a number.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }

    /// The character's URL on D&D Beyond.
    #[must_use]
    pub fn url(self) -> String {
        format!("https://www.dndbeyond.com/characters/{}", self.0)
    }
}

impl fmt::Display for CharacterId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

/// An ability or an attack of the sheet, or one of its spells.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    /// Its name.
    pub name: String,
    /// What it does, as plain text.
    pub description: String,
}

impl Entry {
    /// An entry of this `name` and `description`.
    #[must_use]
    pub fn new(name: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
        }
    }
}

/// The ten fields of `StatBlock@1`, under the Concept's names. A modifier is
/// never part of it: it comes from `AbilityModifier@1`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StatBlock {
    /// STR.
    pub strength: u32,
    /// DEX.
    pub dexterity: u32,
    /// CON.
    pub constitution: u32,
    /// INT.
    pub intelligence: u32,
    /// WIS.
    pub wisdom: u32,
    /// CHA.
    pub charisma: u32,
    /// The armour class.
    pub armor_class: u32,
    /// The hit points maximum.
    pub hp_max: u32,
    /// The abilities and attacks of the sheet.
    pub actions: Vec<Entry>,
    /// Its spells.
    pub spells: Vec<Entry>,
}

/// A character as D&D Beyond states it: what a resync may change, and the
/// identity of the import.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CharacterSheet {
    /// The character on D&D Beyond.
    pub id: CharacterId,
    /// Its name.
    pub name: String,
    /// The race, class and level line.
    pub class_and_level: String,
    /// The statblock.
    pub statblock: StatBlock,
}

/// Why a sheet could not be fetched.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CharacterSheetsError {
    /// The text is neither a character's id nor a character's URL.
    BadReference(String),
    /// The URL is not one of D&D Beyond; the host it names.
    ForeignHost(String),
    /// The character exists but is not shared as public.
    NotPublic,
    /// D&D Beyond has no such character.
    NotFound,
    /// The service answered, but not in the shape this port was written for:
    /// the unofficial endpoint changed. What was missing or wrong.
    ServiceChanged(String),
    /// The service could not be reached or did not answer; the reason is for
    /// the DM.
    Unreachable(String),
}

impl fmt::Display for CharacterSheetsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BadReference(text) => write!(
                f,
                "« {text} » n'est ni l'identifiant ni l'adresse d'un personnage D&D Beyond"
            ),
            Self::ForeignHost(host) => write!(
                f,
                "l'adresse n'est pas celle de D&D Beyond (« {host} ») : collez l'adresse du personnage"
            ),
            Self::NotPublic => f.write_str(
                "ce personnage n'est pas public : partagez-le en public sur D&D Beyond, puis réessayez",
            ),
            Self::NotFound => f.write_str("D&D Beyond ne connaît pas ce personnage"),
            Self::ServiceChanged(what) => write!(
                f,
                "le service D&D Beyond a changé de forme et n'est plus lisible ({what})"
            ),
            Self::Unreachable(reason) => write!(f, "D&D Beyond est injoignable : {reason}"),
        }
    }
}

impl std::error::Error for CharacterSheetsError {}

/// The port: a service that gives a character's sheet from its id.
///
/// An implementation fetches only what [`CharacterSheet`] holds, answers the
/// same id back, fails with the matching [`CharacterSheetsError`] (never a
/// panic) for a private character, an unknown one and an answer of another
/// shape, and passes [`contract::run`].
pub trait CharacterSheets: Send + Sync {
    /// The sheet of the character `id`.
    fn fetch(
        &self,
        id: CharacterId,
    ) -> impl Future<Output = Result<CharacterSheet, CharacterSheetsError>> + Send;
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: u64 = 112_847_390;

    fn id(input: &str) -> Result<CharacterId, CharacterSheetsError> {
        CharacterId::parse(input)
    }

    #[test]
    fn a_bare_id_and_the_url_of_the_character_give_the_same_id() {
        let expected = CharacterId::new(ID).unwrap();
        for input in [
            "112847390",
            "  112847390\n",
            "https://www.dndbeyond.com/characters/112847390",
            "https://dndbeyond.com/characters/112847390",
            "http://www.dndbeyond.com/characters/112847390",
            "https://WWW.DndBeyond.com/characters/112847390/",
            "https://www.dndbeyond.com/characters/112847390/builder",
            "https://www.dndbeyond.com/characters/112847390?tab=spells",
            "https://www.dndbeyond.com/characters/112847390#top",
            "https://www.dndbeyond.com/profile/Julie/characters/112847390",
            "https://www.dndbeyond.com/profile/Julie/characters/112847390/builder?x=1",
        ] {
            assert_eq!(id(input), Ok(expected), "{input}");
        }
    }

    #[test]
    fn a_url_of_another_host_is_refused_with_its_host() {
        for (input, host) in [
            ("https://example.org/characters/112847390", "example.org"),
            (
                "https://www.dndbeyond.com.evil.example/characters/1",
                "www.dndbeyond.com.evil.example",
            ),
            (
                "https://www.dndbeyond.com@evil.example/characters/1",
                "www.dndbeyond.com@evil.example",
            ),
            (
                "https://www.dndbeyond.com:8443/characters/1",
                "www.dndbeyond.com:8443",
            ),
            ("https://evil.example/", "evil.example"),
            ("https://ddb.ac/characters/1", "ddb.ac"),
        ] {
            assert_eq!(
                id(input),
                Err(CharacterSheetsError::ForeignHost(host.to_owned())),
                "{input}"
            );
        }
    }

    #[test]
    fn what_is_not_a_character_is_a_bad_reference() {
        for input in [
            "",
            "   ",
            "0",
            "-5",
            "elowen",
            "12a",
            "99999999999999999999999999",
            "ftp://www.dndbeyond.com/characters/1",
            "www.dndbeyond.com/characters/1",
            "https://www.dndbeyond.com",
            "https://www.dndbeyond.com/",
            "https://www.dndbeyond.com/characters",
            "https://www.dndbeyond.com/characters/",
            "https://www.dndbeyond.com/characters/abc",
            "https://www.dndbeyond.com/characters/12a",
            "https://www.dndbeyond.com/characters/+12",
            "+12",
            "https://www.dndbeyond.com/characters/0",
            "https://www.dndbeyond.com/monsters/112847390",
            "https://www.dndbeyond.com/profile/characters/112847390",
            "https://www.dndbeyond.com/profile/Julie/characters",
            "https://www.dndbeyond.com/profile/Julie/characters/abc",
            "https://www.dndbeyond.com/profile/Julie/monsters/112847390",
            "https://www.dndbeyond.com/en/characters/112847390",
        ] {
            assert!(
                matches!(id(input), Err(CharacterSheetsError::BadReference(_))),
                "{input}: {:?}",
                id(input)
            );
        }
    }

    #[test]
    fn a_bad_reference_keeps_what_the_dm_typed_trimmed() {
        assert_eq!(
            id(" elowen "),
            Err(CharacterSheetsError::BadReference("elowen".to_owned()))
        );
    }

    #[test]
    fn the_id_reads_back_as_a_number_a_url_and_text() {
        let character = CharacterId::new(ID).unwrap();
        assert_eq!(character.get(), ID);
        assert_eq!(character.to_string(), "112847390");
        assert_eq!(
            character.url(),
            "https://www.dndbeyond.com/characters/112847390"
        );
        assert_eq!(id(&character.url()), Ok(character));
        assert!(CharacterId::new(0).is_err());
    }

    #[test]
    fn a_stored_id_is_read_back_only_if_it_is_one() {
        let sheet = serde_json::to_value(CharacterId::new(ID).unwrap()).unwrap();
        assert_eq!(sheet, serde_json::json!(ID));
        let read: CharacterId = serde_json::from_value(sheet).unwrap();
        assert_eq!(read.get(), ID);
        assert!(serde_json::from_value::<CharacterId>(serde_json::json!(0)).is_err());
        assert!(serde_json::from_value::<CharacterId>(serde_json::json!("12")).is_err());
        assert_eq!(CharacterId::try_from(7).unwrap().get(), 7);
        assert!(CharacterId::try_from(0).is_err());
    }

    #[test]
    fn the_errors_speak_to_the_dm() {
        let said = [
            CharacterSheetsError::BadReference("x".into()),
            CharacterSheetsError::ForeignHost("example.org".into()),
            CharacterSheetsError::NotPublic,
            CharacterSheetsError::NotFound,
            CharacterSheetsError::ServiceChanged("data.stats".into()),
            CharacterSheetsError::Unreachable("délai dépassé".into()),
        ]
        .map(|error| error.to_string());
        assert!(said[0].contains("« x »"));
        assert!(said[1].contains("example.org"));
        assert!(said[2].contains("public"));
        assert!(said[3].contains("ne connaît pas"));
        assert!(said[4].contains("data.stats"));
        assert!(said[5].contains("délai dépassé"));
        let distinct: std::collections::HashSet<_> = said.iter().collect();
        assert_eq!(distinct.len(), said.len());
    }
}
