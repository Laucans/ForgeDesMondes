//! A v5 response, mapped to the port's sheet.
//!
//! The service is unofficial and gives no armour class, no maximum of hit
//! points and no final ability score: they are computed here, from the base
//! figures and the modifiers the response lists, as the character sheet of
//! D&D Beyond does. What is modelled, and what is not:
//!
//! - **Scores**: an override wins; else the base score, the bonus and every
//!   `bonus` modifier of `<ability>-score` (race, class, background, feat, and
//!   an item the character wears and, if it asks, is attuned to), raised to
//!   any `set` modifier of the same ability.
//! - **Armour class**: the worn body armour (light: its AC and the DEX modifier;
//!   medium: DEX capped at 2; heavy: its AC) or 10 and the DEX modifier, the
//!   worn shield, and every `bonus` modifier of `armor-class`. The unarmoured
//!   defences of the barbarian and the monk are not modelled.
//! - **Hit points**: an override wins; else base, bonus, and per level the CON
//!   modifier and every `hit-points-per-level` bonus.
//! - **Spells**: every spell the sheet lists, prepared or not, once each.
//! - **Actions**: the abilities of `data.actions`, once each; a weapon in the
//!   inventory is not an action.
//!
//! Whatever is missing or of another type where this module needs it is
//! [`CharacterSheetsError::ServiceChanged`], naming the place.

use std::collections::HashSet;
use std::ops::RangeInclusive;

use ports_character_sheets::{CharacterId, CharacterSheet, CharacterSheetsError, Entry, StatBlock};
use serde_json::Value;

type Mapped<T> = Result<T, CharacterSheetsError>;

fn changed<T>(what: impl Into<String>) -> Mapped<T> {
    Err(CharacterSheetsError::ServiceChanged(what.into()))
}

/// The `subType` of the `<ability>-score` modifiers, in the order of `stats`
/// ids 1 to 6: STR, DEX, CON, INT, WIS, CHA.
const SCORE_MODIFIERS: [&str; 6] = [
    "strength-score",
    "dexterity-score",
    "constitution-score",
    "intelligence-score",
    "wisdom-score",
    "charisma-score",
];

/// The sheet of the character `id` that `body`, a v5 response, describes.
pub(crate) fn sheet(id: CharacterId, body: &Value) -> Mapped<CharacterSheet> {
    let Some(data) = body.get("data").filter(|data| data.is_object()) else {
        return changed("data");
    };
    if data
        .get("id")
        .is_some_and(|answered| answered.as_u64() != Some(id.get()))
    {
        return changed("data.id");
    }
    let name = text(data, "name", "data.name")?;
    let inventory = inventory(data)?;
    let worn = worn(&inventory);
    let modifiers = modifiers(data, &worn)?;
    let scores = scores(data, &modifiers)?;
    let (class_line, level) = classes(data)?;
    let race = data
        .get("race")
        .and_then(|race| race.get("fullName").or_else(|| race.get("baseRaceName")))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|race| !race.is_empty());
    let class_and_level = race.map_or_else(
        || class_line.clone(),
        |race| format!("{race}, {class_line}"),
    );
    let [
        strength,
        dexterity,
        constitution,
        intelligence,
        wisdom,
        charisma,
    ] = scores;
    let armor_class = armor_class(&inventory, &modifiers, ability_modifier(dexterity));
    let hp_max = hit_points(data, &modifiers, ability_modifier(constitution), level)?;
    Ok(CharacterSheet {
        id,
        name,
        class_and_level,
        statblock: StatBlock {
            strength: figure(strength, "strength")?,
            dexterity: figure(dexterity, "dexterity")?,
            constitution: figure(constitution, "constitution")?,
            intelligence: figure(intelligence, "intelligence")?,
            wisdom: figure(wisdom, "wisdom")?,
            charisma: figure(charisma, "charisma")?,
            armor_class: figure(armor_class, "armorClass")?,
            hp_max: figure(hp_max, "hpMax")?,
            actions: actions(data, &worn)?,
            spells: spells(data, &worn)?,
        },
    })
}

/// A figure of the sheet: a whole number above zero.
fn figure(value: i64, what: &str) -> Mapped<u32> {
    match u32::try_from(value) {
        Ok(figure) if figure > 0 => Ok(figure),
        _ => changed(format!("{what} = {value}")),
    }
}

/// The ability modifier of a score: its half above 10, rounded down.
const fn ability_modifier(score: i64) -> i64 {
    (score - 10).div_euclid(2)
}

fn text(parent: &Value, key: &str, path: &str) -> Mapped<String> {
    parent
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map_or_else(|| changed(path), |text| Ok(text.to_owned()))
}

/// The array under `key`, empty when there is none.
fn list<'a>(parent: &'a Value, key: &str, path: &str) -> Mapped<&'a [Value]> {
    match parent.get(key) {
        None | Some(Value::Null) => Ok(&[]),
        Some(Value::Array(items)) => Ok(items),
        Some(_) => changed(path),
    }
}

/// The items of an object of arrays (`modifiers`, `actions`, `spells`: one
/// array per origin), each with its origin.
fn grouped<'a>(parent: &'a Value, key: &str, path: &str) -> Mapped<Vec<(&'a str, &'a Value)>> {
    let origins = match parent.get(key) {
        None | Some(Value::Null) => return Ok(Vec::new()),
        Some(Value::Object(origins)) => origins,
        Some(_) => return changed(path),
    };
    let mut items = Vec::new();
    for (origin, group) in origins {
        match group {
            Value::Null => {}
            Value::Array(group) => items.extend(group.iter().map(|item| (origin.as_str(), item))),
            _ => return changed(format!("{path}.{origin}")),
        }
    }
    Ok(items)
}

/// What the character carries.
struct Item {
    definition: Option<u64>,
    equipped: bool,
    attuned: bool,
    needs_attunement: bool,
    armor_type: Option<u64>,
    armor_class: i64,
}

impl Item {
    /// Whether what it grants is in effect: worn, and attuned if it asks.
    const fn in_effect(&self) -> bool {
        self.equipped && (self.attuned || !self.needs_attunement)
    }
}

fn flag(parent: &Value, key: &str) -> bool {
    parent.get(key).and_then(Value::as_bool) == Some(true)
}

fn inventory(data: &Value) -> Mapped<Vec<Item>> {
    list(data, "inventory", "data.inventory")?
        .iter()
        .map(|item| {
            let Some(definition) = item.get("definition").filter(|d| d.is_object()) else {
                return changed("data.inventory[].definition");
            };
            let armor_type = definition.get("armorTypeId").and_then(Value::as_u64);
            let armor_class = match definition.get("armorClass").and_then(Value::as_i64) {
                Some(armor_class) => armor_class,
                None if armor_type.is_some() => {
                    return changed("data.inventory[].definition.armorClass");
                }
                None => 0,
            };
            Ok(Item {
                definition: definition.get("id").and_then(Value::as_u64),
                equipped: flag(item, "equipped"),
                attuned: flag(item, "isAttuned"),
                needs_attunement: flag(definition, "canAttune"),
                armor_type,
                armor_class,
            })
        })
        .collect()
}

/// A modifier the character benefits from.
struct Modifier<'a> {
    kind: &'a str,
    sub: &'a str,
    value: i64,
}

/// The definitions of the items whose grants are in effect.
fn worn(inventory: &[Item]) -> HashSet<u64> {
    inventory
        .iter()
        .filter(|item| item.in_effect())
        .filter_map(|item| item.definition)
        .collect()
}

/// Whether what an entry of `origin` grants counts: always, but for an item,
/// which must be one the character wears (and is attuned to, if it asks).
fn granted(origin: &str, entry: &Value, worn: &HashSet<u64>) -> bool {
    origin != "item"
        || entry
            .get("componentId")
            .and_then(Value::as_u64)
            .is_some_and(|component| worn.contains(&component))
}

/// Every modifier in effect: those of race, class, background and feats, and
/// those of the items that are worn (and attuned, if they ask).
fn modifiers<'a>(data: &'a Value, worn: &HashSet<u64>) -> Mapped<Vec<Modifier<'a>>> {
    let mut found = Vec::new();
    for (origin, modifier) in grouped(data, "modifiers", "data.modifiers")? {
        let (Some(kind), Some(sub)) = (
            modifier.get("type").and_then(Value::as_str),
            modifier.get("subType").and_then(Value::as_str),
        ) else {
            continue;
        };
        let Some(value) = ["value", "fixedValue"]
            .iter()
            .find_map(|key| modifier.get(key).and_then(Value::as_i64))
        else {
            continue;
        };
        if !granted(origin, modifier, worn) {
            continue;
        }
        found.push(Modifier { kind, sub, value });
    }
    Ok(found)
}

fn sum(modifiers: &[Modifier<'_>], kind: &str, sub: &str) -> i64 {
    modifiers
        .iter()
        .filter(|modifier| modifier.kind == kind && modifier.sub == sub)
        .map(|modifier| modifier.value)
        .sum()
}

/// The value of the ability `slot` (1 to 6) in a `stats`-like list.
fn stat(stats: &[Value], slot: u64) -> Option<i64> {
    stats
        .iter()
        .find(|stat| stat.get("id").and_then(Value::as_u64) == Some(slot))
        .and_then(|stat| stat.get("value"))
        .and_then(Value::as_i64)
}

fn scores(data: &Value, modifiers: &[Modifier<'_>]) -> Mapped<[i64; 6]> {
    let base = list(data, "stats", "data.stats")?;
    let bonus = list(data, "bonusStats", "data.bonusStats")?;
    let overridden = list(data, "overrideStats", "data.overrideStats")?;
    let mut scores = [0; 6];
    for (index, sub) in SCORE_MODIFIERS.iter().enumerate() {
        let slot = index as u64 + 1;
        scores[index] = if let Some(score) = stat(overridden, slot) {
            score
        } else if let Some(base) = stat(base, slot) {
            let score = base + stat(bonus, slot).unwrap_or_default() + sum(modifiers, "bonus", sub);
            modifiers
                .iter()
                .filter(|modifier| modifier.kind == "set" && modifier.sub == *sub)
                .fold(score, |score, modifier| score.max(modifier.value))
        } else {
            return changed(format!("data.stats[{slot}]"));
        };
    }
    Ok(scores)
}

/// The class line (`Ranger 6`, `Fighter 3 / Wizard 2`) and the total level.
fn classes(data: &Value) -> Mapped<(String, i64)> {
    let classes = list(data, "classes", "data.classes")?;
    if classes.is_empty() {
        return changed("data.classes");
    }
    let mut parts = Vec::new();
    let mut total = 0;
    for class in classes {
        let Some(definition) = class.get("definition") else {
            return changed("data.classes[].definition");
        };
        let name = text(definition, "name", "data.classes[].definition.name")?;
        let Some(level) = class
            .get("level")
            .and_then(Value::as_i64)
            .filter(|level| *level > 0)
        else {
            return changed("data.classes[].level");
        };
        parts.push(format!("{name} {level}"));
        total += level;
    }
    Ok((parts.join(" / "), total))
}

fn armor_class(inventory: &[Item], modifiers: &[Modifier<'_>], dexterity: i64) -> i64 {
    let worn = |kinds: RangeInclusive<u64>| {
        inventory
            .iter()
            .filter(|item| item.equipped)
            .find(|item| item.armor_type.is_some_and(|kind| kinds.contains(&kind)))
    };
    let body = match worn(1..=3) {
        Some(Item {
            armor_type: Some(1),
            armor_class,
            ..
        }) => armor_class + dexterity,
        Some(Item {
            armor_type: Some(2),
            armor_class,
            ..
        }) => armor_class + dexterity.min(2),
        Some(Item { armor_class, .. }) => *armor_class,
        None => 10 + dexterity,
    };
    let shield = worn(4..=4).map_or(0, |shield| shield.armor_class);
    body + shield + sum(modifiers, "bonus", "armor-class")
}

fn hit_points(
    data: &Value,
    modifiers: &[Modifier<'_>],
    constitution: i64,
    level: i64,
) -> Mapped<i64> {
    if let Some(hit_points) = data.get("overrideHitPoints").and_then(Value::as_i64) {
        return Ok(hit_points);
    }
    let Some(base) = data.get("baseHitPoints").and_then(Value::as_i64) else {
        return changed("data.baseHitPoints");
    };
    let bonus = data
        .get("bonusHitPoints")
        .and_then(Value::as_i64)
        .unwrap_or_default();
    let per_level = constitution + sum(modifiers, "bonus", "hit-points-per-level");
    Ok(base + bonus + per_level * level)
}

/// The name of an entry: `None` for a placeholder (null or blank), which the
/// sheet leaves out; an error where there is no name at all or it is not text.
fn name_of(entry: &Value, path: &str) -> Mapped<Option<String>> {
    match entry.get("name") {
        Some(Value::Null) => Ok(None),
        Some(Value::String(name)) => {
            Ok(Some(name.trim().to_owned()).filter(|name| !name.is_empty()))
        }
        _ => changed(path),
    }
}

/// The abilities of the sheet, those of the items it wears included.
fn actions(data: &Value, worn: &HashSet<u64>) -> Mapped<Vec<Entry>> {
    let mut actions = Vec::new();
    for (origin, action) in grouped(data, "actions", "data.actions")? {
        if !granted(origin, action, worn) {
            continue;
        }
        if let Some(name) = name_of(action, "data.actions[].name")? {
            actions.push(entry(name, action, &["snippet", "description"]));
        }
    }
    Ok(once_each(actions))
}

/// The spells of the sheet: those of its classes, then those of its race,
/// background, feats and the items it wears.
fn spells(data: &Value, worn: &HashSet<u64>) -> Mapped<Vec<Entry>> {
    let mut listed = Vec::new();
    for class in list(data, "classSpells", "data.classSpells")? {
        listed.extend(list(class, "spells", "data.classSpells[].spells")?);
    }
    listed.extend(
        grouped(data, "spells", "data.spells")?
            .into_iter()
            .filter(|(origin, spell)| granted(origin, spell, worn))
            .map(|(_, spell)| spell),
    );
    let mut spells = Vec::new();
    for spell in listed {
        let Some(definition) = spell.get("definition").filter(|d| d.is_object()) else {
            return changed("data.spells[].definition");
        };
        if let Some(name) = name_of(definition, "data.spells[].definition.name")? {
            spells.push(entry(name, definition, &["description"]));
        }
    }
    Ok(once_each(spells))
}

/// An entry named `name`, described by the first of `keys` that says
/// something; the name itself when none does (a sheet never holds a blank
/// description).
fn entry(name: String, from: &Value, keys: &[&str]) -> Entry {
    let description = keys
        .iter()
        .filter_map(|key| from.get(key).and_then(Value::as_str))
        .map(plain)
        .find(|description| !description.is_empty())
        .unwrap_or_else(|| name.clone());
    Entry { name, description }
}

/// `entries`, each name once: the first wins.
fn once_each(entries: Vec<Entry>) -> Vec<Entry> {
    let mut seen = HashSet::new();
    entries
        .into_iter()
        .filter(|entry| seen.insert(entry.name.clone()))
        .collect()
}

/// The text of a fragment of HTML: no tags, the usual entities decoded, white
/// space collapsed. A block tag (paragraph, break, list, table, heading) is a
/// space; any other is nothing, so `<em>mark</em>,` stays `mark,`. A `<` that
/// opens no tag (`AC < 10`, or a tag left open) is text.
pub(crate) fn plain(html: &str) -> String {
    let mut text = String::with_capacity(html.len());
    let mut tag: Option<String> = None;
    let mut characters = html.chars().peekable();
    while let Some(character) = characters.next() {
        match (&mut tag, character) {
            (None, '<')
                if characters.peek().is_some_and(|next| {
                    next.is_ascii_alphabetic() || matches!(next, '/' | '!')
                }) =>
            {
                tag = Some(String::new());
            }
            (None, _) => text.push(character),
            (Some(open), '>') => {
                if is_block(open) {
                    text.push(' ');
                }
                tag = None;
            }
            (Some(open), _) => open.push(character),
        }
    }
    if let Some(open) = tag {
        text.push('<');
        text.push_str(&open);
    }
    decode(&text)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// `text` with its entities (`&amp;`, `&#39;`, `&#x27;`, `&mdash;`…) decoded,
/// each once; an `&` that opens none stays.
fn decode(text: &str) -> String {
    let mut decoded = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find('&') {
        decoded.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        let entity = after
            .find(';')
            .filter(|end| *end <= 8)
            .and_then(|end| entity(&after[..end]).map(|character| (character, end)));
        if let Some((character, end)) = entity {
            decoded.push(character);
            rest = &after[end + 1..];
        } else {
            decoded.push('&');
            rest = after;
        }
    }
    decoded.push_str(rest);
    decoded
}

/// The character an entity names (without its `&` and `;`).
fn entity(name: &str) -> Option<char> {
    match name {
        "nbsp" => Some(' '),
        "lt" => Some('<'),
        "gt" => Some('>'),
        "quot" => Some('"'),
        "amp" => Some('&'),
        "apos" => Some('\''),
        "lsquo" => Some('‘'),
        "rsquo" => Some('’'),
        "ldquo" => Some('“'),
        "rdquo" => Some('”'),
        "ndash" => Some('–'),
        "mdash" => Some('—'),
        "hellip" => Some('…'),
        _ => {
            let digits = name.strip_prefix('#')?;
            let code = match digits.strip_prefix(['x', 'X']) {
                Some(hex) => u32::from_str_radix(hex, 16).ok()?,
                None => digits.parse().ok()?,
            };
            char::from_u32(code)
        }
    }
}

fn is_block(tag: &str) -> bool {
    let name = tag
        .trim_start_matches('/')
        .split(|c: char| !c.is_ascii_alphanumeric())
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    matches!(
        name.as_str(),
        "p" | "br"
            | "li"
            | "ul"
            | "ol"
            | "div"
            | "tr"
            | "td"
            | "th"
            | "table"
            | "h1"
            | "h2"
            | "h3"
            | "h4"
            | "h5"
            | "h6"
    )
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    const RECORDED: &str = include_str!("../tests/fixtures/elowen-v5.json");

    fn id() -> CharacterId {
        CharacterId::new(112_847_390).unwrap()
    }

    fn recorded() -> Value {
        serde_json::from_str(RECORDED).unwrap()
    }

    /// The recorded response, edited, mapped.
    fn mapped(edit: impl FnOnce(&mut Value)) -> Mapped<CharacterSheet> {
        let mut body = recorded();
        edit(&mut body);
        sheet(id(), &body)
    }

    fn block(edit: impl FnOnce(&mut Value)) -> StatBlock {
        mapped(edit).expect("a sheet").statblock
    }

    fn scores(block: &StatBlock) -> [u32; 6] {
        [
            block.strength,
            block.dexterity,
            block.constitution,
            block.intelligence,
            block.wisdom,
            block.charisma,
        ]
    }

    fn broken_at(edit: impl FnOnce(&mut Value)) -> String {
        match mapped(edit) {
            Err(CharacterSheetsError::ServiceChanged(what)) => what,
            other => panic!("a change of shape, got {other:?}"),
        }
    }

    #[test]
    fn the_recorded_response_gives_the_scores_the_modifiers_make() {
        let block = block(|_| {});
        assert_eq!(scores(&block), [12, 18, 14, 10, 16, 9]);
        assert_eq!((block.armor_class, block.hp_max), (17, 52));
    }

    #[test]
    fn an_override_wins_over_everything_a_bonus_adds_to_the_base() {
        let block = block(|body| {
            body["data"]["overrideStats"][0]["value"] = json!(20);
            body["data"]["bonusStats"][1]["value"] = json!(1);
            body["data"]["bonusStats"][3]["value"] = json!(2);
            body["data"]["overrideHitPoints"] = json!(99);
        });
        assert_eq!(scores(&block), [20, 19, 14, 12, 16, 9]);
        assert_eq!(block.hp_max, 99);
    }

    #[test]
    fn a_set_modifier_raises_a_lower_score_and_never_lowers_a_higher_one() {
        let worn = |body: &mut Value, strength: i64| {
            body["data"]["inventory"][3]["equipped"] = json!(true);
            body["data"]["inventory"][3]["isAttuned"] = json!(true);
            body["data"]["stats"][0]["value"] = json!(strength);
        };
        assert_eq!(block(|body| worn(body, 12)).strength, 19);
        assert_eq!(block(|body| worn(body, 21)).strength, 21);
        // Worn, but not attuned: nothing.
        assert_eq!(
            block(|body| {
                worn(body, 12);
                body["data"]["inventory"][3]["isAttuned"] = json!(false);
            })
            .strength,
            12
        );
        // Attuned, but not worn: nothing.
        assert_eq!(
            block(|body| {
                worn(body, 12);
                body["data"]["inventory"][3]["equipped"] = json!(false);
            })
            .strength,
            12
        );
    }

    #[test]
    fn an_item_that_asks_no_attunement_is_in_effect_once_worn() {
        let ring = |body: &mut Value| {
            body["data"]["inventory"][2]["definition"]["canAttune"] = json!(false);
        };
        assert_eq!(block(ring).armor_class, 18);
        assert_eq!(
            block(|body| {
                ring(body);
                body["data"]["inventory"][2]["equipped"] = json!(false);
            })
            .armor_class,
            17
        );
    }

    #[test]
    fn the_armour_class_follows_the_armour_worn() {
        let armour = |kind: u64, class: i64| {
            move |body: &mut Value| {
                body["data"]["inventory"][0]["definition"]["armorTypeId"] = json!(kind);
                body["data"]["inventory"][0]["definition"]["armorClass"] = json!(class);
            }
        };
        // DEX 18 (+4); the cloak adds 1.
        assert_eq!(block(armour(1, 12)).armor_class, 12 + 4 + 1);
        assert_eq!(block(armour(2, 13)).armor_class, 13 + 2 + 1);
        assert_eq!(block(armour(3, 18)).armor_class, 18 + 1);
        assert_eq!(
            block(|body| body["data"]["inventory"][0]["equipped"] = json!(false)).armor_class,
            10 + 4 + 1
        );
        // A shield is added to what is worn, and counts for nothing alone.
        let shield = |body: &mut Value| {
            body["data"]["inventory"][4]["equipped"] = json!(false);
            body["data"]["inventory"][5]["definition"]["armorTypeId"] = json!(4);
            body["data"]["inventory"][5]["definition"]["armorClass"] = json!(2);
        };
        assert_eq!(block(shield).armor_class, 12 + 4 + 2 + 1);
        assert_eq!(
            block(|body| {
                shield(body);
                body["data"]["inventory"][0]["equipped"] = json!(false);
            })
            .armor_class,
            10 + 4 + 2 + 1
        );
        // The chain mail in the pack is not worn.
        assert_eq!(
            block(|body| body["data"]["inventory"][5]["equipped"] = json!(false)).armor_class,
            17
        );
        // A weak DEX takes from the armour.
        assert_eq!(
            block(|body| body["data"]["overrideStats"][1]["value"] = json!(6)).armor_class,
            12 - 2 + 1
        );
    }

    #[test]
    fn the_first_body_armour_worn_is_the_one_that_counts() {
        let both = |body: &mut Value| {
            body["data"]["inventory"][4]["equipped"] = json!(true);
        };
        assert_eq!(block(both).armor_class, 12 + 4 + 1);
    }

    #[test]
    fn hit_points_follow_the_constitution_the_bonus_and_every_level() {
        assert_eq!(
            block(|body| body["data"]["bonusHitPoints"] = json!(5)).hp_max,
            57
        );
        assert_eq!(
            block(|body| body["data"]["overrideStats"][2]["value"] = json!(8)).hp_max,
            40 - 6
        );
        // Tough: +2 per level.
        assert_eq!(
            block(|body| {
                body["data"]["modifiers"]["feat"] = json!([
                    {"type": "bonus", "subType": "hit-points-per-level", "value": 2}
                ]);
            })
            .hp_max,
            52 + 2 * 6
        );
        let two_classes = |body: &mut Value| {
            body["data"]["classes"].as_array_mut().unwrap().push(json!({
                "level": 2, "definition": {"name": "Fighter"}
            }));
        };
        assert_eq!(block(two_classes).hp_max, 40 + 2 * 8);
        assert_eq!(
            mapped(two_classes).unwrap().class_and_level,
            "Wood Elf, Ranger 6 / Fighter 2"
        );
    }

    #[test]
    fn the_class_line_names_the_race_when_there_is_one() {
        let line = |edit: fn(&mut Value)| mapped(edit).unwrap().class_and_level;
        assert_eq!(line(|body| body["data"]["race"] = Value::Null), "Ranger 6");
        assert_eq!(
            line(|body| {
                body["data"]["race"] = json!({"baseRaceName": "Elf"});
            }),
            "Elf, Ranger 6"
        );
        assert_eq!(
            line(|body| body["data"]["race"]["fullName"] = json!(" ")),
            "Ranger 6"
        );
    }

    #[test]
    fn a_modifier_is_read_by_its_value_then_its_fixed_value_and_skipped_when_it_has_none() {
        let wisdom = |modifier: Value| {
            block(|body| body["data"]["modifiers"]["feat"] = json!([modifier])).wisdom
        };
        let plus = |extra: Value| {
            let mut modifier = json!({"type": "bonus", "subType": "wisdom-score"});
            modifier
                .as_object_mut()
                .unwrap()
                .extend(extra.as_object().unwrap().clone());
            wisdom(modifier)
        };
        assert_eq!(plus(json!({"value": 3, "fixedValue": 1})), 19);
        assert_eq!(plus(json!({"value": null, "fixedValue": 1})), 17);
        assert_eq!(plus(json!({"fixedValue": 2})), 18);
        assert_eq!(plus(json!({"value": null, "fixedValue": null})), 16);
        assert_eq!(wisdom(json!({"subType": "wisdom-score", "value": 3})), 16);
        assert_eq!(wisdom(json!({"type": "bonus", "value": 3})), 16);
        assert_eq!(
            wisdom(json!({"type": "bonus", "subType": null, "value": 3})),
            16
        );
        assert_eq!(
            wisdom(json!({"type": "penalty", "subType": "wisdom-score", "value": 3})),
            16
        );
        assert_eq!(
            wisdom(json!({"type": "bonus", "subType": "charisma-score", "value": 3})),
            16
        );
    }

    #[test]
    fn an_item_modifier_needs_the_item_it_names() {
        let wisdom = |component: Value| {
            block(|body| {
                body["data"]["modifiers"]["item"] = json!([
                    {"type": "bonus", "subType": "wisdom-score", "value": 2, "componentId": component}
                ]);
            })
            .wisdom
        };
        assert_eq!(wisdom(json!(2102)), 18);
        assert_eq!(wisdom(json!(2103)), 16);
        assert_eq!(wisdom(json!(777)), 16);
        assert_eq!(wisdom(Value::Null), 16);
        let no_component = block(|body| {
            body["data"]["modifiers"]["item"] =
                json!([{"type": "bonus", "subType": "wisdom-score", "value": 2}]);
        });
        assert_eq!(no_component.wisdom, 16);
    }

    #[test]
    fn a_spell_or_an_action_is_described_by_the_first_thing_it_says() {
        let sheet = mapped(|body| {
            body["data"]["actions"]["class"] = json!([
                {"name": "A", "snippet": "  ", "description": "<p>Long</p>"},
                {"name": "B", "snippet": "Short", "description": "Long"},
                {"name": "C", "description": null}
            ]);
            body["data"]["spells"] = Value::Null;
            body["data"]["classSpells"] = json!([
                {"spells": [{"definition": {"name": "S", "description": "<p> </p>"}}]},
                {"spells": null}
            ]);
        })
        .unwrap();
        let described: Vec<_> = sheet
            .statblock
            .actions
            .iter()
            .chain(&sheet.statblock.spells)
            .map(|entry| (entry.name.as_str(), entry.description.as_str()))
            .collect();
        assert_eq!(
            described,
            [("A", "Long"), ("B", "Short"), ("C", "C"), ("S", "S")]
        );
    }

    #[test]
    fn a_sheet_with_nothing_to_list_is_still_a_sheet() {
        let block = block(|body| {
            body["data"]["actions"] = Value::Null;
            body["data"]["spells"] = Value::Null;
            body["data"]["classSpells"] = Value::Null;
            body["data"]["inventory"] = Value::Null;
            body["data"]["modifiers"] = Value::Null;
            body["data"].as_object_mut().unwrap().remove("id");
            body["data"].as_object_mut().unwrap().remove("bonusStats");
            body["data"]
                .as_object_mut()
                .unwrap()
                .remove("overrideStats");
        });
        assert!(block.actions.is_empty() && block.spells.is_empty());
        assert_eq!(scores(&block), [12, 16, 14, 10, 13, 9]);
        assert_eq!((block.armor_class, block.hp_max), (10 + 3, 52));
    }

    #[test]
    fn what_the_service_dropped_or_retyped_is_named() {
        assert_eq!(broken_at(|body| *body = json!({})), "data");
        assert_eq!(broken_at(|body| *body = json!({"data": []})), "data");
        assert_eq!(broken_at(|body| body["data"]["id"] = json!(7)), "data.id");
        assert_eq!(
            broken_at(|body| body["data"]["id"] = json!("112847390")),
            "data.id"
        );
        assert_eq!(
            broken_at(|body| body["data"]["name"] = json!("  ")),
            "data.name"
        );
        assert_eq!(
            broken_at(|body| body["data"]["name"] = json!(3)),
            "data.name"
        );
        assert_eq!(
            broken_at(|body| {
                body["data"]["stats"].as_array_mut().unwrap().remove(2);
            }),
            "data.stats[3]"
        );
        assert_eq!(
            broken_at(|body| body["data"]["stats"][4]["value"] = Value::Null),
            "data.stats[5]"
        );
        assert_eq!(
            broken_at(|body| body["data"]["stats"] = json!({})),
            "data.stats"
        );
        assert_eq!(
            broken_at(|body| body["data"]["bonusStats"] = json!("x")),
            "data.bonusStats"
        );
        assert_eq!(
            broken_at(|body| body["data"]["overrideStats"] = json!(1)),
            "data.overrideStats"
        );
        assert_eq!(
            broken_at(|body| body["data"]["classes"] = json!([])),
            "data.classes"
        );
        assert_eq!(
            broken_at(|body| body["data"]["classes"] = Value::Null),
            "data.classes"
        );
        assert_eq!(
            broken_at(|body| body["data"]["classes"] = json!(5)),
            "data.classes"
        );
        assert_eq!(
            broken_at(|body| body["data"]["classes"][0]["level"] = json!(0)),
            "data.classes[].level"
        );
        assert_eq!(
            broken_at(|body| body["data"]["classes"][0]["level"] = json!("6")),
            "data.classes[].level"
        );
        assert_eq!(
            broken_at(|body| body["data"]["classes"][0]["definition"] = Value::Null),
            "data.classes[].definition.name"
        );
        assert_eq!(
            broken_at(|body| {
                body["data"]["classes"][0]
                    .as_object_mut()
                    .unwrap()
                    .remove("definition");
            }),
            "data.classes[].definition"
        );
        assert_eq!(
            broken_at(|body| body["data"]["classes"][0]["definition"]["name"] = json!(" ")),
            "data.classes[].definition.name"
        );
        assert_eq!(
            broken_at(|body| body["data"]["baseHitPoints"] = Value::Null),
            "data.baseHitPoints"
        );
        assert_eq!(
            broken_at(|body| body["data"]["inventory"] = json!({})),
            "data.inventory"
        );
        assert_eq!(
            broken_at(|body| body["data"]["inventory"][1]["definition"] = Value::Null),
            "data.inventory[].definition"
        );
        assert_eq!(
            broken_at(|body| body["data"]["modifiers"] = json!([])),
            "data.modifiers"
        );
        assert_eq!(
            broken_at(|body| body["data"]["modifiers"]["race"] = json!({})),
            "data.modifiers.race"
        );
        assert_eq!(
            broken_at(|body| body["data"]["actions"] = json!([])),
            "data.actions"
        );
        assert_eq!(
            broken_at(|body| body["data"]["actions"]["class"] = json!("x")),
            "data.actions.class"
        );
        assert_eq!(
            broken_at(|body| body["data"]["actions"]["class"][0]["name"] = json!(3)),
            "data.actions[].name"
        );
        assert_eq!(
            broken_at(|body| body["data"]["classSpells"] = json!({})),
            "data.classSpells"
        );
        assert_eq!(
            broken_at(|body| body["data"]["classSpells"][0]["spells"] = json!({})),
            "data.classSpells[].spells"
        );
        assert_eq!(
            broken_at(
                |body| body["data"]["classSpells"][0]["spells"][0]["definition"] = Value::Null
            ),
            "data.spells[].definition"
        );
        assert_eq!(
            broken_at(
                |body| body["data"]["classSpells"][0]["spells"][0]["definition"]["name"] = json!(3)
            ),
            "data.spells[].definition.name"
        );
        assert_eq!(
            broken_at(|body| body["data"]["spells"] = json!([])),
            "data.spells"
        );
        assert_eq!(
            broken_at(|body| body["data"]["spells"]["race"] = json!(3)),
            "data.spells.race"
        );
        assert_eq!(
            broken_at(|body| body["data"]["spells"]["race"][0]["definition"] = json!(3)),
            "data.spells[].definition"
        );
    }

    #[test]
    fn a_figure_that_is_not_above_zero_is_a_change_of_shape_naming_it() {
        assert_eq!(
            broken_at(|body| body["data"]["overrideHitPoints"] = json!(0)),
            "hpMax = 0"
        );
        assert_eq!(
            broken_at(|body| body["data"]["overrideStats"][0]["value"] = json!(-1)),
            "strength = -1"
        );
        assert_eq!(
            broken_at(|body| body["data"]["overrideStats"][1]["value"] = json!(0)),
            "dexterity = 0"
        );
        assert_eq!(
            broken_at(|body| body["data"]["overrideStats"][2]["value"] = json!(0)),
            "constitution = 0"
        );
        assert_eq!(
            broken_at(|body| body["data"]["overrideStats"][3]["value"] = json!(0)),
            "intelligence = 0"
        );
        assert_eq!(
            broken_at(|body| body["data"]["overrideStats"][4]["value"] = json!(0)),
            "wisdom = 0"
        );
        assert_eq!(
            broken_at(|body| body["data"]["overrideStats"][5]["value"] = json!(0)),
            "charisma = 0"
        );
        assert_eq!(
            broken_at(|body| {
                body["data"]["modifiers"]["class"] = json!([
                    {"type": "bonus", "subType": "armor-class", "value": -30}
                ]);
            }),
            "armorClass = -13"
        );
        assert_eq!(
            broken_at(|body| body["data"]["overrideHitPoints"] = json!(5_000_000_000_i64)),
            "hpMax = 5000000000"
        );
    }

    #[test]
    fn the_modifier_of_a_score_rounds_down() {
        let modifiers = [
            (1, -5),
            (7, -2),
            (8, -1),
            (9, -1),
            (10, 0),
            (11, 0),
            (12, 1),
            (18, 4),
            (30, 10),
        ];
        for (score, modifier) in modifiers {
            assert_eq!(ability_modifier(score), modifier, "{score}");
        }
    }

    #[test]
    fn a_placeholder_without_a_name_is_left_out_not_a_change_of_shape() {
        let sheet = mapped(|body| {
            body["data"]["actions"]["class"] = json!([
                {"name": null, "snippet": "x"},
                {"name": "  ", "snippet": "y"},
                {"name": "Kept", "snippet": "z"}
            ]);
            body["data"]["classSpells"][0]["spells"] = json!([
                {"definition": {"name": null, "description": "x"}},
                {"definition": {"name": "", "description": "y"}},
                {"definition": {"name": "Spell", "description": "z"}}
            ]);
            body["data"]["spells"] = Value::Null;
        })
        .unwrap();
        assert_eq!(sheet.statblock.actions, [Entry::new("Kept", "z")]);
        assert_eq!(sheet.statblock.spells, [Entry::new("Spell", "z")]);
        // No key at all is another matter: the service renamed it.
        assert_eq!(
            broken_at(|body| {
                body["data"]["actions"]["class"][0]
                    .as_object_mut()
                    .unwrap()
                    .remove("name");
            }),
            "data.actions[].name"
        );
    }

    #[test]
    fn the_spells_and_actions_of_an_item_count_only_while_it_is_worn() {
        let granting = |component: u64| {
            move |body: &mut Value| {
                body["data"]["spells"]["item"] = json!([
                    {"componentId": component, "definition": {"name": "Item spell", "description": "s"}}
                ]);
                body["data"]["actions"]["item"] = json!([
                    {"componentId": component, "name": "Item action", "snippet": "a"}
                ]);
            }
        };
        let granted = |component: u64| {
            let block = block(granting(component));
            (
                block.spells.iter().any(|s| s.name == "Item spell"),
                block.actions.iter().any(|a| a.name == "Item action"),
            )
        };
        assert_eq!(granted(2102), (true, true));
        assert_eq!(granted(2103), (false, false));
        assert_eq!(granted(2104), (false, false));
        assert_eq!(granted(777), (false, false));
        // Without the id of the item they name, they count for nothing.
        let nameless = block(|body| {
            granting(2102)(body);
            body["data"]["spells"]["item"][0]
                .as_object_mut()
                .unwrap()
                .remove("componentId");
            body["data"]["actions"]["item"][0]
                .as_object_mut()
                .unwrap()
                .remove("componentId");
        });
        assert!(nameless.spells.iter().all(|s| s.name != "Item spell"));
        assert!(nameless.actions.iter().all(|a| a.name != "Item action"));
    }

    #[test]
    fn an_item_without_an_id_grants_nothing_not_even_to_component_zero() {
        let wisdom = block(|body| {
            body["data"]["inventory"][1]["definition"]
                .as_object_mut()
                .unwrap()
                .remove("id");
            body["data"]["modifiers"]["item"] = json!([
                {"type": "bonus", "subType": "wisdom-score", "value": 2, "componentId": 0}
            ]);
        })
        .wisdom;
        assert_eq!(wisdom, 16);
    }

    #[test]
    fn armour_without_its_armour_class_is_a_change_of_shape_other_items_need_none() {
        assert_eq!(
            broken_at(|body| {
                body["data"]["inventory"][0]["definition"]
                    .as_object_mut()
                    .unwrap()
                    .remove("armorClass");
            }),
            "data.inventory[].definition.armorClass"
        );
        assert_eq!(
            broken_at(
                |body| body["data"]["inventory"][4]["definition"]["armorClass"] = json!("16")
            ),
            "data.inventory[].definition.armorClass"
        );
        let block = block(|body| {
            body["data"]["inventory"][5]["definition"]
                .as_object_mut()
                .unwrap()
                .remove("armorClass");
        });
        assert_eq!(block.armor_class, 17);
    }

    #[test]
    fn markup_becomes_plain_text() {
        for (html, text) in [
            ("", ""),
            ("plain", "plain"),
            ("<p>One</p><p>Two</p>", "One Two"),
            ("<P>One</P><BR/>Two", "One Two"),
            ("a<br>b<br />c", "a b c"),
            ("<em>mark</em>, <strong>bold</strong>.", "mark, bold."),
            ("<ul><li>x</li><li>y</li></ul>", "x y"),
            ("<table><tr><td>1</td><th>2</th></tr></table>", "1 2"),
            ("<div>a</div><ol><li>b</li></ol><h3>c</h3>d", "a b c d"),
            ("<span class=\"x\">a</span>b", "ab"),
            (
                "a &lt;b&gt; &quot;c&quot; &#39;d&#39; &rsquo; &nbsp;e &amp; f",
                "a <b> \"c\" 'd' ’ e & f",
            ),
            ("&amp;lt;", "&lt;"),
            ("  spaced \n\t out  ", "spaced out"),
            ("a > b", "a > b"),
            ("a <b", "a <b"),
            (
                "AC < 10 while unarmored. Then 3 <5.",
                "AC < 10 while unarmored. Then 3 <5.",
            ),
            ("a <", "a <"),
            ("if x < 5 and y > 2", "if x < 5 and y > 2"),
            ("a <!-- c --> b", "a b"),
            ("</p>x", "x"),
            (
                "&mdash; &ndash; &hellip; &ldquo;q&rdquo; &lsquo;s&rsquo; &apos;",
                "— – … “q” ‘s’ '",
            ),
            ("&#x27;a&#X27; &#8217; &#65;", "'a' ’ A"),
            ("AT&T, a & b, &", "AT&T, a & b, &"),
            (
                "&unknown; &#xZZ; &#1114112; &#; &amp",
                "&unknown; &#xZZ; &#1114112; &#; &amp",
            ),
            ("&verylongentityname;", "&verylongentityname;"),
            ("Tom &amp; Jerry &amp;amp; co", "Tom & Jerry &amp; co"),
        ] {
            assert_eq!(plain(html), text, "{html:?}");
        }
    }
}
