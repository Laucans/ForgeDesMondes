//! The contract suite, run against the fake — as an adapter crate runs it
//! against itself — and against ports that each break it in one way.

use std::sync::atomic::{AtomicU32, Ordering};

use platform_conformance::repository_root;
use ports_character_sheets::contract::{Scenario, run};
use ports_character_sheets::fake::FakeCharacterSheets;
use ports_character_sheets::{
    CharacterId, CharacterSheet, CharacterSheets, CharacterSheetsError, SharedCharacterSheets,
};

const ELOWEN: u64 = 112_847_390;

fn id(value: u64) -> CharacterId {
    CharacterId::new(value).unwrap()
}

fn fixture(name: &str) -> serde_json::Value {
    let path = repository_root().join("mock/fixtures").join(name);
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn fake() -> FakeCharacterSheets {
    FakeCharacterSheets::from_fixtures(&fixture("entities.json"), &fixture("EntityEditor.json"))
        .unwrap()
        .with_private(id(2))
        .with_changed_shape(id(4))
}

async fn scenario() -> Scenario {
    let expected = fake().fetch(id(ELOWEN)).await.unwrap();
    Scenario {
        public: id(ELOWEN),
        expected,
        private: id(2),
        unknown: id(3),
        changed: id(4),
    }
}

#[tokio::test]
async fn the_fake_passes_the_contract() {
    run(&fake(), &scenario().await).await;
}

#[tokio::test]
async fn the_contract_still_holds_after_the_level_up() {
    let fake = fake();
    fake.level_up(id(ELOWEN));
    let mut scenario = scenario().await;
    scenario.expected = fake.fetch(id(ELOWEN)).await.unwrap();
    run(&fake, &scenario).await;
}

/// The handle a server shares with its units is the port behind it.
#[tokio::test]
async fn the_shared_handle_passes_the_contract() {
    let shared = SharedCharacterSheets::new(fake());
    run(&shared, &scenario().await).await;
    run(&shared.clone(), &scenario().await).await;
}

/// Behind the handle, a port that breaks the contract still breaks it.
#[tokio::test]
#[should_panic(expected = "the sheet of the public character")]
async fn the_shared_handle_hands_the_port_s_own_sheet_over() {
    let mut scenario = scenario().await;
    scenario.expected.name = "Autre".into();
    run(&SharedCharacterSheets::new(fake()), &scenario).await;
}

#[tokio::test]
#[should_panic(expected = "the sheet of the public character")]
async fn the_contract_fails_a_port_that_answers_another_sheet() {
    let mut scenario = scenario().await;
    scenario.expected.statblock.hp_max += 1;
    run(&fake(), &scenario).await;
}

#[tokio::test]
#[should_panic(expected = "the scenario's sheet is not the public character's")]
async fn the_contract_refuses_a_sheet_that_is_not_the_public_character_s() {
    let mut scenario = scenario().await;
    scenario.public = id(5);
    run(&fake(), &scenario).await;
}

#[tokio::test]
#[should_panic(expected = "not distinct")]
async fn the_contract_refuses_a_private_that_is_the_public() {
    let mut scenario = scenario().await;
    scenario.private = scenario.public;
    run(&fake(), &scenario).await;
}

#[tokio::test]
#[should_panic(expected = "not distinct")]
async fn the_contract_refuses_an_unknown_that_is_the_public() {
    let mut scenario = scenario().await;
    scenario.unknown = scenario.public;
    run(&fake(), &scenario).await;
}

#[tokio::test]
#[should_panic(expected = "not distinct")]
async fn the_contract_refuses_a_changed_that_is_the_public() {
    let mut scenario = scenario().await;
    scenario.changed = scenario.public;
    run(&fake(), &scenario).await;
}

#[tokio::test]
#[should_panic(expected = "not distinct")]
async fn the_contract_refuses_a_private_that_is_the_unknown() {
    let mut scenario = scenario().await;
    scenario.unknown = scenario.private;
    run(&fake(), &scenario).await;
}

#[tokio::test]
#[should_panic(expected = "not distinct")]
async fn the_contract_refuses_a_private_that_is_the_changed() {
    let mut scenario = scenario().await;
    scenario.changed = scenario.private;
    run(&fake(), &scenario).await;
}

#[tokio::test]
#[should_panic(expected = "not distinct")]
async fn the_contract_refuses_an_unknown_that_is_the_changed() {
    let mut scenario = scenario().await;
    scenario.changed = scenario.unknown;
    run(&fake(), &scenario).await;
}

/// A port that behaves as the fake does, except for one fault.
enum Fault {
    /// Answers another id than the one asked.
    AnswersAnotherId,
    /// Answers a different hit-point maximum each time.
    Drifts,
    /// Says a private character is not found.
    HidesPrivate,
    /// Says an unknown character is private.
    FlagsUnknownPrivate,
    /// Says a changed service is not found.
    SwallowsChange,
    /// Gives a sheet for a character it cannot read.
    InventsSheet,
}

struct Faulty {
    inner: FakeCharacterSheets,
    fault: Fault,
    calls: AtomicU32,
}

impl Faulty {
    fn new(fault: Fault) -> Self {
        Self {
            inner: fake(),
            fault,
            calls: AtomicU32::new(0),
        }
    }
}

impl CharacterSheets for Faulty {
    async fn fetch(&self, asked: CharacterId) -> Result<CharacterSheet, CharacterSheetsError> {
        let calls = self.calls.fetch_add(1, Ordering::SeqCst);
        let answer = self.inner.fetch(asked).await;
        match (&self.fault, answer) {
            (Fault::AnswersAnotherId, Ok(mut sheet)) => {
                sheet.id = id(asked.get() + 1);
                Ok(sheet)
            }
            (Fault::Drifts, Ok(mut sheet)) => {
                sheet.statblock.hp_max += calls;
                Ok(sheet)
            }
            (Fault::HidesPrivate, Err(CharacterSheetsError::NotPublic)) => {
                Err(CharacterSheetsError::NotFound)
            }
            (Fault::FlagsUnknownPrivate, Err(CharacterSheetsError::NotFound)) => {
                Err(CharacterSheetsError::NotPublic)
            }
            (Fault::SwallowsChange, Err(CharacterSheetsError::ServiceChanged(_))) => {
                Err(CharacterSheetsError::NotFound)
            }
            (Fault::InventsSheet, Err(CharacterSheetsError::ServiceChanged(_))) => {
                self.inner.fetch(id(ELOWEN)).await
            }
            (_, answer) => answer,
        }
    }
}

#[tokio::test]
#[should_panic(expected = "the id answered")]
async fn the_contract_fails_a_port_that_answers_another_id() {
    run(&Faulty::new(Fault::AnswersAnotherId), &scenario().await).await;
}

#[tokio::test]
#[should_panic(expected = "fetched twice")]
async fn the_contract_fails_a_port_whose_answer_drifts() {
    // The first answer is the one `expected` names: the drift starts after it.
    run(&Faulty::new(Fault::Drifts), &scenario().await).await;
}

#[tokio::test]
#[should_panic(expected = "a private character")]
async fn the_contract_fails_a_port_that_hides_a_private_character() {
    run(&Faulty::new(Fault::HidesPrivate), &scenario().await).await;
}

#[tokio::test]
#[should_panic(expected = "an unknown character")]
async fn the_contract_fails_a_port_that_flags_an_unknown_character_private() {
    run(&Faulty::new(Fault::FlagsUnknownPrivate), &scenario().await).await;
}

#[tokio::test]
#[should_panic(expected = "a service that changed shape")]
async fn the_contract_fails_a_port_that_swallows_a_change_of_shape() {
    run(&Faulty::new(Fault::SwallowsChange), &scenario().await).await;
}

#[tokio::test]
#[should_panic(expected = "a service that changed shape")]
async fn the_contract_fails_a_port_that_invents_a_sheet_for_an_unreadable_answer() {
    run(&Faulty::new(Fault::InventsSheet), &scenario().await).await;
}
