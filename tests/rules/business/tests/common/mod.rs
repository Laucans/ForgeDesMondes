//! What the business rules' tests share: the conventions they follow when
//! they talk to the product over the shell's wire protocol, by identifier.
//!
//! - a list Capability answers an array, or an object holding it under the
//!   key its name suggests (`items`);
//! - an entity Capability answers the entity, or an object holding it under
//!   `entity` (`entity`); a battlemap instance, under `instance`;
//! - a command that does not settle `applied` fails the test with its
//!   violations (`apply`) — contract H carries no payload, so what a command
//!   produced is read back through a Capability;
//! - a test that needs data of its own creates it with a unique name, so
//!   that tests running side by side never share a row they change.
//!
//! The mock-up's fixtures are the data every test starts from; the mock-up's
//! fixtures are loaded by the rules' runtime.
// Each test file uses a subset of these helpers.
#![allow(dead_code)]

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

pub use rules_business::{Server, check, fixture, rule};
pub use serde_json::{Value, json};

/// The campaign the fixtures describe.
pub const CAMPAIGN: &str = "cendres-valombre";

/// The elements of a list Capability's answer.
pub fn items(data: &Value, key: &str) -> Vec<Value> {
    data.as_array()
        .or_else(|| data.get(key).and_then(Value::as_array))
        .cloned()
        .unwrap_or_default()
}

/// A string field, empty when absent.
pub fn text<'a>(value: &'a Value, key: &str) -> &'a str {
    value.get(key).and_then(Value::as_str).unwrap_or_default()
}

/// The entity of an entity Capability's answer.
pub fn entity(data: &Value) -> &Value {
    data.get("entity").unwrap_or(data)
}

/// The battlemap instance of its Capability's answer.
pub fn instance(data: &Value) -> &Value {
    data.get("instance").unwrap_or(data)
}

/// The element of a list whose `id` is given.
pub fn by_id<'a>(list: &'a [Value], id: &str) -> Option<&'a Value> {
    list.iter().find(|v| text(v, "id") == id)
}

/// The element of a list whose `name` is given.
pub fn by_name<'a>(list: &'a [Value], name: &str) -> Option<&'a Value> {
    list.iter().find(|v| text(v, "name") == name)
}

/// A name no other test uses.
pub fn unique(prefix: &str) -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    format!(
        "{prefix} {nanos}-{}",
        COUNTER.fetch_add(1, Ordering::Relaxed)
    )
}

/// Sends a command and requires it applied.
pub fn apply(
    s: &Server,
    data_capability: &str,
    payload: Value,
    target: Option<&str>,
) -> Result<Value, String> {
    let result = s.command(data_capability, payload, target)?;
    match result.get("status").and_then(Value::as_str) {
        Some("applied") => Ok(result),
        _ => Err(format!("{data_capability} was not applied: {result}")),
    }
}

/// Reads the characters of a campaign.
pub fn characters(s: &Server, campaign: &str) -> Result<Vec<Value>, String> {
    let data = s.read("campaign.listCharacters", json!({ "campaign": campaign }))?;
    Ok(items(&data, "characters"))
}

/// Reads the places of a campaign.
pub fn places(s: &Server, campaign: &str) -> Result<Vec<Value>, String> {
    let data = s.read("campaign.listPlaces", json!({ "campaign": campaign }))?;
    Ok(items(&data, "places"))
}

/// Reads the events of a campaign.
pub fn events(s: &Server, campaign: &str) -> Result<Vec<Value>, String> {
    let data = s.read("campaign.listEvents", json!({ "campaign": campaign }))?;
    Ok(items(&data, "events"))
}

/// Reads an entity, character, place or event.
pub fn get_entity(s: &Server, id: &str) -> Result<Value, String> {
    let data = s.read("campaign.getEntity", json!({ "id": id }))?;
    Ok(entity(&data).clone())
}

/// Creates a campaign of its own and answers its id.
pub fn new_campaign(s: &Server, kind: &str) -> Result<String, String> {
    let name = unique("Campagne de test");
    apply(
        s,
        "campaign.createCampaign",
        json!({ "name": name, "type": kind }),
        None,
    )?;
    let data = s.read("campaign.listCampaigns", json!({}))?;
    by_name(&items(&data, "campaigns"), &name)
        .map(|c| text(c, "id").to_string())
        .ok_or_else(|| format!("the campaign {name} is not listed"))
}

/// Creates a character (`pj` or `pnj`) with the given fields and answers its id.
pub fn new_character(
    s: &Server,
    campaign: &str,
    kind: &str,
    fields: Value,
) -> Result<String, String> {
    let name = unique(if kind == "pj" {
        "PJ de test"
    } else {
        "PNJ de test"
    });
    let mut entity = json!({ "kind": kind, "name": name });
    if let (Some(base), Some(extra)) = (entity.as_object_mut(), fields.as_object()) {
        base.extend(extra.clone());
    }
    let name = text(&entity, "name").to_string();
    apply(
        s,
        "campaign.saveEntity",
        json!({ "campaign": campaign, "entity": entity }),
        None,
    )?;
    by_name(&characters(s, campaign)?, &name)
        .map(|c| text(c, "id").to_string())
        .ok_or_else(|| format!("the character {name} is not listed"))
}

/// Creates a session of its own, with the PJ present, and answers its id.
pub fn new_session(s: &Server, campaign: &str, present: &[&str]) -> Result<String, String> {
    let title = unique("Séance de test");
    apply(
        s,
        "session.createSession",
        json!({ "campaign": campaign, "title": title, "present": present }),
        None,
    )?;
    let data = s.read("session.listSessions", json!({ "campaign": campaign }))?;
    by_name_or_title(&items(&data, "sessions"), &title)
        .ok_or_else(|| format!("the session {title} is not listed"))
}

pub fn by_name_or_title(list: &[Value], title: &str) -> Option<String> {
    list.iter()
        .find(|v| text(v, "title") == title)
        .map(|v| text(v, "id").to_string())
}

/// Adds a scene to a session and answers its id.
pub fn new_scene(s: &Server, session: &str, extra: Value) -> Result<String, String> {
    let title = unique("Scène de test");
    let mut payload = json!({ "session": session, "title": title });
    if let (Some(base), Some(more)) = (payload.as_object_mut(), extra.as_object()) {
        base.extend(more.clone());
    }
    apply(s, "session.addScene", payload, None)?;
    let data = s.read("session.listScenes", json!({ "session": session }))?;
    by_name_or_title(&items(&data, "scenes"), &title)
        .ok_or_else(|| format!("the scene {title} is not listed"))
}

/// Gives a scene an instance of a battlemap and answers the instance's id.
pub fn new_instance(
    s: &Server,
    session: &str,
    scene: &str,
    battlemap: &str,
) -> Result<String, String> {
    apply(
        s,
        "table.createBattlemapInstance",
        json!({ "session": session, "scene": scene, "battlemap": battlemap }),
        None,
    )?;
    let data = s.read("session.listScenes", json!({ "session": session }))?;
    let scenes = items(&data, "scenes");
    by_id(&scenes, scene)
        .map(|sc| text(sc, "battlemap").to_string())
        .filter(|id| !id.is_empty())
        .ok_or_else(|| format!("the scene {scene} has no battlemap instance"))
}

/// A battlemap instance of its own, on the fixtures' battlemap, in a new session.
pub fn fresh_instance(s: &Server, present: &[&str]) -> Result<String, String> {
    let session = new_session(s, CAMPAIGN, present)?;
    let scene = new_scene(s, &session, json!({}))?;
    new_instance(s, &session, &scene, "bm-gue")
}

/// Places `count` tokens of a character on an instance.
pub fn place(
    s: &Server,
    instance_id: &str,
    character: &str,
    (q, r): (i64, i64),
    count: u32,
) -> Result<(), String> {
    apply(
        s,
        "table.placeTokens",
        json!({ "character": character, "q": q, "r": r, "count": count }),
        Some(instance_id),
    )
    .map(|_| ())
}

/// The tokens of an instance, as the DM or the players see them.
pub fn tokens(s: &Server, instance_id: &str, view: &str) -> Result<Vec<Value>, String> {
    let data = s.read(
        "table.getBattlemapInstance",
        json!({ "instance": instance_id, "view": view }),
    )?;
    Ok(instance(&data)
        .pointer("/state/tokens")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default())
}

/// The combat of an instance, as the DM or the players see it; `Null` when none.
pub fn combat(s: &Server, instance_id: &str, view: &str) -> Result<Value, String> {
    let data = s.read(
        "table.getBattlemapInstance",
        json!({ "instance": instance_id, "view": view }),
    )?;
    Ok(instance(&data)
        .pointer("/state/combat")
        .cloned()
        .unwrap_or(Value::Null))
}

/// The ids of a combat's participants, in initiative order.
pub fn order(combat: &Value) -> Vec<String> {
    combat
        .get("order")
        .and_then(Value::as_array)
        .map(|o| o.iter().map(|p| text(p, "id").to_string()).collect())
        .unwrap_or_default()
}

/// The tokens of an instance that stand for the character `id`.
pub fn of_character(tokens: &[Value], id: &str) -> Vec<Value> {
    tokens
        .iter()
        .filter(|t| text(t, "char") == id)
        .cloned()
        .collect()
}

/// The token of an instance named `name`.
pub fn token_named(tokens: &[Value], name: &str) -> Result<Value, String> {
    by_name(tokens, name)
        .cloned()
        .ok_or_else(|| format!("no token named {name}"))
}

/// Saves the AI settings of a provider the fixtures describe (`cloud` or `opencode`) in API mode.
pub fn use_api(s: &Server, provider: &str) -> Result<(), String> {
    let settings = fixture("AiSettings");
    let p = &settings["providers"][provider];
    apply(
        s,
        "ai.saveSettings",
        json!({
            "mode": "api",
            "url": p["url"],
            "generationModel": p["generationModel"],
            "summaryModel": p["summaryModel"],
            "key": ""
        }),
        None,
    )
    .map(|_| ())
}

/// Saves the AI settings in binary mode (`opencode` or `claude`).
pub fn use_binary(s: &Server, binary: &str) -> Result<(), String> {
    let settings = fixture("AiSettings");
    let provider = if binary == "claude" {
        "cloud"
    } else {
        "opencode"
    };
    let p = &settings["providers"][provider];
    apply(
        s,
        "ai.saveSettings",
        json!({
            "mode": "binary",
            "binary": binary,
            "generationModel": p["generationModel"],
            "summaryModel": p["summaryModel"]
        }),
        None,
    )
    .map(|_| ())
}
