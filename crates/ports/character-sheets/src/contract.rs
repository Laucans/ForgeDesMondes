//! The contract suite of the `character-sheets` port: what every adapter does
//! the same.
//!
//! An adapter crate runs it against itself from a test of its own, on
//! whatever stands in for the service (a recorded response served locally):
//!
//! ```ignore
//! #[tokio::test]
//! async fn the_adapter_passes_the_contract() {
//!     let scenario = Scenario {
//!         public: CharacterId::new(112_847_390).unwrap(),
//!         expected: /* the sheet the stand-in's response maps to */,
//!         private: CharacterId::new(2).unwrap(),
//!         unknown: CharacterId::new(3).unwrap(),
//!         changed: CharacterId::new(4).unwrap(),
//!     };
//!     ports_character_sheets::contract::run(&MyAdapter::new(), &scenario).await;
//! }
//! ```
//!
//! It holds the adapter to: a public character is answered by its sheet, under
//! the id that was asked, the same each time; a character not shared as public
//! fails with [`CharacterSheetsError::NotPublic`]; a character D&D Beyond does
//! not know fails with [`CharacterSheetsError::NotFound`]; a character the
//! service answers in another shape fails with
//! [`CharacterSheetsError::ServiceChanged`]. A failed check panics with what it
//! was about.

use crate::{CharacterId, CharacterSheet, CharacterSheets, CharacterSheetsError};

/// What the adapter's service stand-in knows.
#[derive(Clone, Debug)]
pub struct Scenario {
    /// A character shared as public.
    pub public: CharacterId,
    /// The sheet the stand-in's answer for `public` stands for.
    pub expected: CharacterSheet,
    /// A character that exists and is not shared as public.
    pub private: CharacterId,
    /// A character D&D Beyond has no trace of.
    pub unknown: CharacterId,
    /// A character the service answers in a shape the adapter does not know.
    pub changed: CharacterId,
}

/// Runs the whole suite on `port`.
///
/// # Panics
///
/// When the adapter breaks the contract, or the scenario contradicts itself.
pub async fn run<P: CharacterSheets>(port: &P, scenario: &Scenario) {
    assert_eq!(
        scenario.expected.id, scenario.public,
        "the scenario's sheet is not the public character's"
    );
    let others = [scenario.private, scenario.unknown, scenario.changed];
    assert!(
        !others.contains(&scenario.public)
            && scenario.private != scenario.unknown
            && scenario.private != scenario.changed
            && scenario.unknown != scenario.changed,
        "the scenario's four characters are not distinct"
    );
    a_public_character_is_answered_by_its_sheet(port, scenario).await;
    the_same_character_is_answered_the_same(port, scenario).await;
    a_private_character_is_not_public(port, scenario).await;
    an_unknown_character_is_not_found(port, scenario).await;
    a_changed_service_is_a_typed_error(port, scenario).await;
}

async fn a_public_character_is_answered_by_its_sheet<P: CharacterSheets>(
    port: &P,
    scenario: &Scenario,
) {
    let sheet = port
        .fetch(scenario.public)
        .await
        .unwrap_or_else(|error| panic!("the public character failed: {error}"));
    assert_eq!(sheet.id, scenario.public, "the id answered");
    assert_eq!(
        sheet, scenario.expected,
        "the sheet of the public character"
    );
}

async fn the_same_character_is_answered_the_same<P: CharacterSheets>(
    port: &P,
    scenario: &Scenario,
) {
    let first = port.fetch(scenario.public).await;
    let second = port.fetch(scenario.public).await;
    assert_eq!(first, second, "the same character, fetched twice");
}

async fn a_private_character_is_not_public<P: CharacterSheets>(port: &P, scenario: &Scenario) {
    assert_eq!(
        port.fetch(scenario.private).await,
        Err(CharacterSheetsError::NotPublic),
        "a private character"
    );
}

async fn an_unknown_character_is_not_found<P: CharacterSheets>(port: &P, scenario: &Scenario) {
    assert_eq!(
        port.fetch(scenario.unknown).await,
        Err(CharacterSheetsError::NotFound),
        "an unknown character"
    );
}

async fn a_changed_service_is_a_typed_error<P: CharacterSheets>(port: &P, scenario: &Scenario) {
    let result = port.fetch(scenario.changed).await;
    assert!(
        matches!(result, Err(CharacterSheetsError::ServiceChanged(_))),
        "a service that changed shape: {result:?}"
    );
}
