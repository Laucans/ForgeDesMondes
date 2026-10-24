//! What the fake promises beyond the contract: it answers from the mock-up's
//! fixtures, the level-up changes what `ddbResync` says and nothing else, and
//! no failure is a panic.

use platform_conformance::{fixtures_of, repository_root};
use ports_character_sheets::fake::{FakeCharacterSheets, FixtureError, LEVEL_UP_DESCRIPTION};
use ports_character_sheets::{
    CharacterId, CharacterSheet, CharacterSheets, CharacterSheetsError, Entry, StatBlock,
};
use serde_json::{Value, json};

const ELOWEN: u64 = 112_847_390;
const BRANNOC: u64 = 112_850_214;

fn fixture(name: &str) -> Value {
    let path = repository_root().join("mock/fixtures").join(name);
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    serde_json::from_str(&text).expect("a fixture is JSON")
}

fn fake() -> FakeCharacterSheets {
    FakeCharacterSheets::from_fixtures(&fixture("entities.json"), &fixture("EntityEditor.json"))
        .expect("the mock-up's fixtures")
}

fn id(value: u64) -> CharacterId {
    CharacterId::new(value).unwrap()
}

fn names(entries: &[Entry]) -> Vec<&str> {
    entries.iter().map(|entry| entry.name.as_str()).collect()
}

/// The ten fields of a statblock, sound by the ranges of `StatBlock@1`.
fn is_sound(block: &StatBlock) -> bool {
    let scores = [
        block.strength,
        block.dexterity,
        block.constitution,
        block.intelligence,
        block.wisdom,
        block.charisma,
    ];
    let entry = |e: &Entry| !e.name.trim().is_empty() && !e.description.trim().is_empty();
    scores.iter().all(|score| (1..=30).contains(score))
        && block.armor_class > 0
        && block.hp_max > 0
        && !block.actions.is_empty()
        && block.actions.iter().all(entry)
        && block.spells.iter().all(entry)
}

#[tokio::test]
async fn elowen_is_answered_as_the_dm_last_synchronised_her() {
    let sheet = fake().fetch(id(ELOWEN)).await.unwrap();
    assert_eq!(sheet.id, id(ELOWEN));
    assert_eq!(sheet.name, "Elowen Ventargent");
    assert_eq!(sheet.class_and_level, "Elfe des bois, rôdeuse niveau 5");
    let block = &sheet.statblock;
    assert_eq!(
        [
            block.strength,
            block.dexterity,
            block.constitution,
            block.intelligence,
            block.wisdom,
            block.charisma
        ],
        [12, 18, 14, 10, 15, 9]
    );
    assert_eq!((block.armor_class, block.hp_max), (16, 44));
    assert_eq!(
        names(&block.actions),
        ["Ennemi juré (morts-vivants)", "Attaque supplémentaire"]
    );
    assert_eq!(
        names(&block.spells),
        ["Marque du chasseur", "Passage sans trace"]
    );
}

#[tokio::test]
async fn after_the_level_up_elowen_carries_the_patched_values() {
    let fake = fake();
    let before = fake.fetch(id(ELOWEN)).await.unwrap();
    assert!(fake.level_up(id(ELOWEN)));
    let after = fake.fetch(id(ELOWEN)).await.unwrap();

    let patch = &fixture("EntityEditor.json")["ddbResync"]["elowen"]["patch"];
    assert_eq!(
        after.statblock.hp_max,
        patch["pvMax"].as_u64().unwrap() as u32
    );
    assert_eq!(after.statblock.hp_max, 52);
    assert_eq!(
        after.statblock.wisdom,
        patch["stats"]["SAG"].as_u64().unwrap() as u32
    );
    assert_eq!(after.statblock.wisdom, 16);
    assert_eq!(after.class_and_level, patch["type"].as_str().unwrap());
    assert_eq!(
        names(&after.statblock.spells),
        [
            "Marque du chasseur",
            "Passage sans trace",
            "Explorateur né amélioré",
            "Communication avec les animaux"
        ]
    );
    assert!(
        after.statblock.spells[2..]
            .iter()
            .all(|spell| spell.description == LEVEL_UP_DESCRIPTION)
    );

    // What the patch does not name stays as it was.
    let kept = |sheet: &CharacterSheet| {
        let b = &sheet.statblock;
        (
            sheet.id,
            sheet.name.clone(),
            [
                b.strength,
                b.dexterity,
                b.constitution,
                b.intelligence,
                b.charisma,
            ],
            b.armor_class,
            b.actions.clone(),
        )
    };
    assert_eq!(kept(&before), kept(&after));
}

#[tokio::test]
async fn a_character_with_no_level_up_of_its_own_stays_as_she_is() {
    let fake = fake();
    let before = fake.fetch(id(BRANNOC)).await.unwrap();
    assert!(fake.level_up(id(BRANNOC)));
    assert_eq!(fake.fetch(id(BRANNOC)).await.unwrap(), before);
}

#[tokio::test]
async fn a_clone_is_the_same_fake() {
    let fake = fake();
    let clone = fake.clone();
    clone.level_up(id(ELOWEN));
    assert_eq!(fake.fetch(id(ELOWEN)).await.unwrap().statblock.hp_max, 52);
}

#[tokio::test]
async fn the_four_player_characters_are_known_and_the_npcs_are_not() {
    let fake = fake();
    let entities = fixture("entities.json");
    let mut known = 0;
    for character in entities["characters"].as_array().unwrap() {
        let linked = character["ddb"]["url"].as_str();
        if let Some(url) = linked {
            let sheet = fake.fetch(CharacterId::parse(url).unwrap()).await.unwrap();
            assert_eq!(sheet.name, character["name"].as_str().unwrap());
            assert!(is_sound(&sheet.statblock), "{}", sheet.name);
            known += 1;
        }
    }
    assert_eq!(known, 4);
    assert_eq!(fake.fetch(id(1)).await, Err(CharacterSheetsError::NotFound));
}

#[tokio::test]
async fn a_level_up_nobody_planned_is_not_played() {
    assert!(!fake().level_up(id(1)));
}

#[tokio::test]
async fn a_sheet_is_the_identity_and_the_statblock_and_nothing_the_dm_owns() {
    let sheet = fake().fetch(id(ELOWEN)).await.unwrap();
    let json = serde_json::to_value(&sheet).unwrap();
    let mut keys: Vec<_> = json.as_object().unwrap().keys().cloned().collect();
    keys.sort();
    assert_eq!(keys, ["classAndLevel", "id", "name", "statblock"]);
    let mut keys: Vec<_> = json["statblock"]
        .as_object()
        .unwrap()
        .keys()
        .cloned()
        .collect();
    keys.sort();
    assert_eq!(
        keys,
        [
            "actions",
            "armorClass",
            "charisma",
            "constitution",
            "dexterity",
            "hpMax",
            "intelligence",
            "spells",
            "strength",
            "wisdom"
        ]
    );
    assert_eq!(json["id"], json!(ELOWEN));
}

#[tokio::test]
async fn the_builders_make_it_fail_as_the_service_does() {
    let built = FakeCharacterSheets::new()
        .with_private(id(2))
        .with_changed_shape(id(3));
    assert_eq!(
        built.fetch(id(2)).await,
        Err(CharacterSheetsError::NotPublic)
    );
    assert!(matches!(
        built.fetch(id(3)).await,
        Err(CharacterSheetsError::ServiceChanged(_))
    ));
    assert_eq!(
        built.fetch(id(4)).await,
        Err(CharacterSheetsError::NotFound)
    );
    let sheet = fake().fetch(id(ELOWEN)).await.unwrap();
    let built = built.with_sheet(sheet.clone());
    assert_eq!(built.fetch(id(ELOWEN)).await, Ok(sheet));
}

#[tokio::test]
async fn a_sheet_given_by_hand_is_not_overwritten_by_a_level_up_written_for_another() {
    let fake = fake();
    let mut custom = fake.fetch(id(ELOWEN)).await.unwrap();
    custom.name = "Autre".into();
    let fake = fake.with_sheet(custom.clone());
    assert!(!fake.level_up(id(ELOWEN)));
    assert_eq!(fake.fetch(id(ELOWEN)).await, Ok(custom));
}

#[test]
fn a_feature_is_a_spell_only_when_its_text_says_a_spell_of_a_level() {
    let mut entities = fixture("entities.json");
    entities["characters"][0]["features"] = json!([
        {"name": "Sortie rapide", "text": "Sortie du combat sans provoquer."},
        {"name": "Boule", "text": "Sort de niveau 3 : sphère de feu."}
    ]);
    let fake =
        FakeCharacterSheets::from_fixtures(&entities, &fixture("EntityEditor.json")).unwrap();
    let sheet = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap()
        .block_on(fake.fetch(id(ELOWEN)))
        .unwrap();
    assert_eq!(names(&sheet.statblock.actions), ["Sortie rapide"]);
    assert_eq!(names(&sheet.statblock.spells), ["Boule"]);
}

#[test]
fn the_sheet_is_statblock_shaped_by_the_concept_s_own_fixtures() {
    let fixtures = fixtures_of("StatBlock@1");
    let ten = [
        "strength",
        "dexterity",
        "constitution",
        "intelligence",
        "wisdom",
        "charisma",
        "armorClass",
        "hpMax",
        "actions",
        "spells",
    ];
    let mut complete = 0;
    for case in fixtures
        .cases
        .iter()
        .filter(|c| c.output["complete"] == true)
    {
        let block: StatBlock = serde_json::from_value(case.input.clone())
            .unwrap_or_else(|e| panic!("{}: {e}", case.name));
        assert!(is_sound(&block), "{}", case.name);
        let wanted: serde_json::Map<_, _> = case
            .input
            .as_object()
            .unwrap()
            .iter()
            .filter(|(key, _)| ten.contains(&key.as_str()))
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect();
        assert_eq!(
            serde_json::to_value(&block).unwrap(),
            Value::Object(wanted),
            "{}",
            case.name
        );
        complete += 1;
    }
    assert!(complete >= 4);
}

#[test]
fn a_fixture_the_fake_cannot_read_is_an_error_not_a_panic() {
    let editor = fixture("EntityEditor.json");
    let entities = fixture("entities.json");
    let broken = |edit: &dyn Fn(&mut Value)| {
        let mut entities = entities.clone();
        edit(&mut entities);
        FakeCharacterSheets::from_fixtures(&entities, &editor).map(|_| ())
    };
    assert!(matches!(
        broken(&|e| e["characters"] = json!("none")),
        Err(FixtureError(_))
    ));
    assert!(
        broken(&|e| e["characters"][0]["ddb"]["url"] = json!("https://example.org/characters/1"))
            .is_err()
    );
    assert!(broken(&|e| e["characters"][0]["stats"]["SAG"] = json!("quinze")).is_err());
    assert!(broken(&|e| e["characters"][0]["stats"]["CHA"] = json!(-1)).is_err());
    assert!(broken(&|e| e["characters"][0]["ca"] = json!(null)).is_err());
    assert!(broken(&|e| e["characters"][0]["features"] = json!({})).is_err());
    assert!(broken(&|e| e["characters"][0]["features"][0]["text"] = json!(3)).is_err());
    assert!(broken(&|e| e["characters"][0]["type"] = json!(null)).is_err());

    let mut editor = editor;
    editor["ddbResync"]["elowen"]["patch"]["pvMax"] = json!("beaucoup");
    assert!(FakeCharacterSheets::from_fixtures(&entities, &editor).is_err());
    let rendered = FakeCharacterSheets::from_fixtures(&entities, &editor)
        .map(|_| ())
        .unwrap_err()
        .to_string();
    assert!(rendered.contains("elowen"), "{rendered}");
}

#[test]
fn each_patch_key_is_read_or_refused_by_its_own_type() {
    let entities = fixture("entities.json");
    let with = |patch: Value| {
        let editor = json!({"ddbResync": {"elowen": {"patch": patch, "changes": []}, "default": {"patch": {}}}});
        FakeCharacterSheets::from_fixtures(&entities, &editor)
    };
    assert!(with(json!({"type": 5})).is_err());
    assert!(with(json!({"ca": "x"})).is_err());
    assert!(with(json!({"stats": {"FOR": "x"}})).is_err());
    assert!(with(json!({"type": "Elfe", "pvMax": 60, "ca": 17, "stats": {"FOR": 13}})).is_ok());
}

#[tokio::test]
async fn every_ability_of_the_patch_lands_on_its_own_score() {
    let entities = fixture("entities.json");
    let editor = json!({"ddbResync": {"elowen": {"patch": {
        "ca": 17, "stats": {"FOR": 1, "DEX": 2, "CON": 3, "INT": 4, "SAG": 5, "CHA": 6}
    }}, "default": {"patch": {}}}});
    let fake = FakeCharacterSheets::from_fixtures(&entities, &editor).unwrap();
    fake.level_up(id(ELOWEN));
    let b = fake.fetch(id(ELOWEN)).await.unwrap().statblock;
    assert_eq!(
        [
            b.strength,
            b.dexterity,
            b.constitution,
            b.intelligence,
            b.wisdom,
            b.charisma
        ],
        [1, 2, 3, 4, 5, 6]
    );
    assert_eq!(b.armor_class, 17);
    assert_eq!(b.hp_max, 44);
}

#[tokio::test]
async fn only_what_the_level_up_adds_to_the_spells_line_is_added() {
    let entities = fixture("entities.json");
    let editor = json!({"ddbResync": {"elowen": {"patch": {}, "changes": [
        {"field": "PV max", "old": "44", "new": "Phantôme"},
        {"field": "Sorts et capacités", "old": "A, B", "new": "A , B,, C"}
    ]}, "default": {"patch": {}}}});
    let fake = FakeCharacterSheets::from_fixtures(&entities, &editor).unwrap();
    fake.level_up(id(ELOWEN));
    let spells = fake.fetch(id(ELOWEN)).await.unwrap().statblock.spells;
    assert_eq!(
        names(&spells),
        ["Marque du chasseur", "Passage sans trace", "C"]
    );
}

#[test]
fn a_fixture_error_says_what_is_wrong() {
    assert_eq!(
        FixtureError("elowen: x".into()).to_string(),
        "fixture illisible : elowen: x"
    );
}
