//! The adapter against a recorded v5 response, served from 127.0.0.1: the
//! `character-sheets` port's contract suite, and what the suite does not
//! name — the URL asked, the redirect refused, the ways the service fails.
//! No network: every request goes to a server this file starts.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use axum::Router;
use axum::extract::{Path, State};
use axum::http::{StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use ports_character_sheets::contract::{Scenario, run};
use ports_character_sheets::{
    CharacterId, CharacterSheet, CharacterSheets, CharacterSheetsError, Entry,
    SharedCharacterSheets, StatBlock,
};
use ports_character_sheets_dndbeyond::DndBeyond;

const ELOWEN: &str = include_str!("fixtures/elowen-v5.json");
const PRIVATE: &str = include_str!("fixtures/private-v5.json");
const NOT_FOUND: &str = include_str!("fixtures/not-found-v5.json");
const CHANGED: &str = include_str!("fixtures/changed-v5.json");

const PUBLIC_ID: u64 = 112_847_390;
const PRIVATE_ID: u64 = 2;
const UNKNOWN_ID: u64 = 3;
const CHANGED_ID: u64 = 4;
const REDIRECTING_ID: u64 = 5;
const DOWN_ID: u64 = 6;
const NOT_JSON_ID: u64 = 7;
const PRIVATE_WITH_OK_ID: u64 = 8;
const GONE_WITH_OK_ID: u64 = 9;
const HUGE_ID: u64 = 10;
const LARGE_ID: u64 = 11;

/// What the adapter reads of an answer, at most.
const MAX_BODY: usize = 4 * 1024 * 1024;

/// What the stand-in answers to each id: a status and a body.
type Answers = HashMap<u64, (StatusCode, &'static str)>;

/// The recorded response, padded to exactly `size` bytes: still the same sheet.
fn padded(size: usize) -> &'static str {
    let marker = "\"readonlyUrl\": \"/characters/112847390\"";
    assert!(ELOWEN.contains(marker));
    let padding = size - ELOWEN.len();
    let body = ELOWEN.replace(
        marker,
        &format!(
            "\"readonlyUrl\": \"/characters/112847390{}\"",
            " ".repeat(padding)
        ),
    );
    assert_eq!(body.len(), size);
    Box::leak(body.into_boxed_str())
}

#[derive(Clone)]
struct Stand {
    answers: Arc<Answers>,
    seen: Arc<Mutex<Vec<String>>>,
}

async fn character(State(stand): State<Stand>, Path(id): Path<u64>, uri: Uri) -> Response {
    stand.seen.lock().unwrap().push(uri.to_string());
    if id == REDIRECTING_ID {
        return (
            StatusCode::FOUND,
            [(header::LOCATION, "http://127.0.0.1:1/elsewhere")],
        )
            .into_response();
    }
    match stand.answers.get(&id) {
        Some((status, body)) => {
            (*status, [(header::CONTENT_TYPE, "application/json")], *body).into_response()
        }
        None => (StatusCode::NOT_FOUND, NOT_FOUND).into_response(),
    }
}

/// Starts the stand-in; the adapter on it, and the paths it was asked.
async fn serve() -> (DndBeyond, Arc<Mutex<Vec<String>>>, SocketAddr) {
    let answers: Answers = HashMap::from([
        (PUBLIC_ID, (StatusCode::OK, ELOWEN)),
        (PRIVATE_ID, (StatusCode::FORBIDDEN, PRIVATE)),
        (UNKNOWN_ID, (StatusCode::NOT_FOUND, NOT_FOUND)),
        (CHANGED_ID, (StatusCode::OK, CHANGED)),
        (DOWN_ID, (StatusCode::SERVICE_UNAVAILABLE, "maintenance")),
        (NOT_JSON_ID, (StatusCode::OK, "<html>maintenance</html>")),
        (PRIVATE_WITH_OK_ID, (StatusCode::OK, PRIVATE)),
        (GONE_WITH_OK_ID, (StatusCode::OK, NOT_FOUND)),
        (LARGE_ID, (StatusCode::OK, padded(MAX_BODY))),
        (HUGE_ID, (StatusCode::OK, padded(MAX_BODY + 1))),
    ]);
    let seen = Arc::new(Mutex::new(Vec::new()));
    let app = Router::new()
        .route("/character/v5/character/{id}", get(character))
        .with_state(Stand {
            answers: Arc::new(answers),
            seen: seen.clone(),
        });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let adapter = DndBeyond::at(format!("http://{addr}/character/v5/character")).unwrap();
    (adapter, seen, addr)
}

fn id(value: u64) -> CharacterId {
    CharacterId::new(value).unwrap()
}

/// What the recorded response maps to, worked out by hand from it.
fn elowen() -> CharacterSheet {
    CharacterSheet {
        id: id(PUBLIC_ID),
        name: "Elowen Ventargent".into(),
        class_and_level: "Wood Elf, Ranger 6".into(),
        statblock: StatBlock {
            strength: 12,
            dexterity: 18,
            constitution: 14,
            intelligence: 10,
            wisdom: 16,
            charisma: 9,
            armor_class: 17,
            hp_max: 52,
            actions: vec![
                Entry::new(
                    "Favored Enemy",
                    "You always have the Hunter's Mark spell prepared.",
                ),
                Entry::new(
                    "Extra Attack",
                    "You can attack twice, instead of once, whenever you take the Attack action on your turn. Twice Not three times",
                ),
                Entry::new("Natural Explorer", "Natural Explorer"),
            ],
            spells: vec![
                Entry::new(
                    "Hunter's Mark",
                    "You choose a creature you can see within range and mystically mark it as your quarry. Until the spell ends, you deal an extra 1d6 damage to the target whenever you hit it with a weapon attack. \"Marked\" creatures cannot hide & vanish.",
                ),
                Entry::new(
                    "Pass without Trace",
                    "A veil of shadows and silence radiates from you, masking you and your companions from detection.",
                ),
                Entry::new(
                    "Animal Friendship",
                    "This spell lets you convince a beast that you mean it no harm.",
                ),
                Entry::new(
                    "Druidcraft",
                    "Whispering to the spirits of nature, you create one of several effects.",
                ),
            ],
        },
    }
}

fn scenario() -> Scenario {
    Scenario {
        public: id(PUBLIC_ID),
        expected: elowen(),
        private: id(PRIVATE_ID),
        unknown: id(UNKNOWN_ID),
        changed: id(CHANGED_ID),
    }
}

#[tokio::test]
async fn the_adapter_passes_the_contract() {
    let (adapter, _, _) = serve().await;
    run(&adapter, &scenario()).await;
}

#[tokio::test]
async fn the_adapter_behind_the_shared_handle_passes_the_contract() {
    let (adapter, _, _) = serve().await;
    run(&SharedCharacterSheets::new(adapter), &scenario()).await;
}

#[tokio::test]
async fn the_recorded_sheet_maps_stats_ac_hp_spells_and_actions() {
    let (adapter, _, _) = serve().await;
    assert_eq!(adapter.fetch(id(PUBLIC_ID)).await, Ok(elowen()));
}

#[tokio::test]
async fn it_asks_the_one_url_built_from_the_id_and_nothing_else() {
    let (adapter, seen, _) = serve().await;
    adapter.fetch(id(PUBLIC_ID)).await.unwrap();
    assert_eq!(*seen.lock().unwrap(), ["/character/v5/character/112847390"]);
}

#[tokio::test]
async fn a_redirect_is_not_followed() {
    let (adapter, seen, _) = serve().await;
    assert_eq!(
        adapter.fetch(id(REDIRECTING_ID)).await,
        Err(CharacterSheetsError::ServiceChanged(
            "réponse HTTP 302 inattendue".into()
        ))
    );
    assert_eq!(seen.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn a_service_that_is_down_is_unreachable_not_changed() {
    let (adapter, _, _) = serve().await;
    assert!(matches!(
        adapter.fetch(id(DOWN_ID)).await,
        Err(CharacterSheetsError::Unreachable(reason)) if reason.contains("503")
    ));
}

#[tokio::test]
async fn nothing_listening_is_unreachable_and_says_nothing_of_the_url() {
    let adapter = DndBeyond::at("http://127.0.0.1:1/character/v5/character").unwrap();
    let Err(CharacterSheetsError::Unreachable(reason)) = adapter.fetch(id(PUBLIC_ID)).await else {
        panic!("unreachable")
    };
    assert_eq!(reason, "aucune réponse du service");
}

#[tokio::test]
async fn a_body_that_is_not_json_is_a_change_of_shape() {
    let (adapter, _, _) = serve().await;
    assert_eq!(
        adapter.fetch(id(NOT_JSON_ID)).await,
        Err(CharacterSheetsError::ServiceChanged(
            "la réponse n'est pas du JSON".into()
        ))
    );
}

#[tokio::test]
async fn a_failure_in_a_200_is_read_by_its_message() {
    let (adapter, _, _) = serve().await;
    assert_eq!(
        adapter.fetch(id(PRIVATE_WITH_OK_ID)).await,
        Err(CharacterSheetsError::NotPublic)
    );
    assert_eq!(
        adapter.fetch(id(GONE_WITH_OK_ID)).await,
        Err(CharacterSheetsError::NotFound)
    );
}

#[tokio::test]
async fn a_changed_shape_says_what_is_missing() {
    let (adapter, _, _) = serve().await;
    let Err(CharacterSheetsError::ServiceChanged(what)) = adapter.fetch(id(CHANGED_ID)).await
    else {
        panic!("a change of shape")
    };
    assert_eq!(what, "data.name");
}

#[tokio::test]
async fn an_answer_is_read_up_to_four_mebibytes_and_not_a_byte_more() {
    let (adapter, _, _) = serve().await;
    // The stand-in answers the recorded sheet under another id: the mapping
    // refuses it for its `data.id`, which proves the body was read whole.
    assert_eq!(
        adapter.fetch(id(LARGE_ID)).await,
        Err(CharacterSheetsError::ServiceChanged("data.id".into()))
    );
    assert_eq!(
        adapter.fetch(id(HUGE_ID)).await,
        Err(CharacterSheetsError::ServiceChanged(
            "réponse démesurée".into()
        ))
    );
}
