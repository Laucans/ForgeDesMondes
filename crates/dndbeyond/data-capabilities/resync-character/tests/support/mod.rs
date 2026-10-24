//! The D&D Beyond characters the tests resynchronise, as the
//! `character-sheets` port answers them.

// Each test file compiles this module alone, and none uses all of it.
#![allow(dead_code)]

use ports_character_sheets::{CharacterId, CharacterSheet, Entry, StatBlock};

/// Elowen's id on D&D Beyond, as the mock-up's fixtures link her.
pub const ELOWEN: u64 = 112_847_390;

/// The id `n`.
pub fn id(n: u64) -> CharacterId {
    CharacterId::new(n).expect("a character id")
}

/// Elowen's actions, as the DM last synchronised them.
pub fn actions() -> Vec<Entry> {
    vec![Entry::new(
        "Attaque supplémentaire",
        "Deux attaques quand elle utilise l’action Attaquer.",
    )]
}

/// Elowen's spells, as the DM last synchronised them.
pub fn spells() -> Vec<Entry> {
    vec![Entry::new(
        "Marque du chasseur",
        "Sort de niveau 1 : 1d6 dégâts supplémentaires contre la cible marquée.",
    )]
}

/// Elowen as D&D Beyond states her before her level-up: 44 HP, WIS 15.
pub fn elowen() -> CharacterSheet {
    CharacterSheet {
        id: id(ELOWEN),
        name: "Elowen Ventargent".to_owned(),
        class_and_level: "Elfe des bois, rôdeuse niveau 5".to_owned(),
        statblock: StatBlock {
            strength: 12,
            dexterity: 18,
            constitution: 14,
            intelligence: 10,
            wisdom: 15,
            charisma: 9,
            armor_class: 16,
            hp_max: 44,
            actions: actions(),
            spells: spells(),
        },
    }
}

/// Elowen after her level-up (`ddbResync.elowen`): 52 HP, WIS 16, and two
/// spells more.
pub fn levelled_up() -> CharacterSheet {
    let mut sheet = elowen();
    sheet.class_and_level = "Elfe des bois, rôdeuse niveau 6".to_owned();
    sheet.statblock.hp_max = 52;
    sheet.statblock.wisdom = 16;
    sheet.statblock.spells.extend([
        Entry::new(
            "Explorateur né amélioré",
            "Ajouté lors de la montée de niveau.",
        ),
        Entry::new(
            "Communication avec les animaux",
            "Ajouté lors de la montée de niveau.",
        ),
    ]);
    sheet
}
