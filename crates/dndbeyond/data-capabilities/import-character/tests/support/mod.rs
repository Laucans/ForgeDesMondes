//! The D&D Beyond characters the tests import, as the `character-sheets`
//! port answers them.

// Each test file compiles this module alone, and none uses all of it.
#![allow(dead_code)]

use ports_character_sheets::{CharacterId, CharacterSheet, Entry, StatBlock};

/// Elowen's id on D&D Beyond, as the mock-up's fixtures link her.
pub const ELOWEN: u64 = 112_847_390;

/// Brannoc's.
pub const BRANNOC: u64 = 112_850_214;

/// Elowen's URL.
pub fn elowen_url() -> String {
    id(ELOWEN).url()
}

/// The id `n`.
pub fn id(n: u64) -> CharacterId {
    CharacterId::new(n).expect("a character id")
}

/// The sheet of the character `n`, named `name`, of `hp` hit points.
pub fn sheet(n: u64, name: &str, hp: u32) -> CharacterSheet {
    CharacterSheet {
        id: id(n),
        name: name.to_owned(),
        class_and_level: "Elfe des bois, rôdeuse niveau 5".to_owned(),
        statblock: StatBlock {
            strength: 12,
            dexterity: 18,
            constitution: 14,
            intelligence: 10,
            wisdom: 15,
            charisma: 9,
            armor_class: 16,
            hp_max: hp,
            actions: vec![Entry::new(
                "Attaque supplémentaire",
                "Deux attaques quand elle utilise l’action Attaquer.",
            )],
            spells: vec![Entry::new(
                "Marque du chasseur",
                "Sort de niveau 1 : 1d6 dégâts supplémentaires contre la cible marquée.",
            )],
        },
    }
}

/// Elowen as D&D Beyond states her, 44 hit points.
pub fn elowen() -> CharacterSheet {
    sheet(ELOWEN, "Elowen Ventargent", 44)
}
