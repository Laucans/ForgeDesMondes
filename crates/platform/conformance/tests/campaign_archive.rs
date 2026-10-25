//! The runner over `CampaignArchive@1`: it finds and loads the fixtures, they
//! agree with `concept.json`, and an implementation of the Concept — the
//! small one below, local to this test — passes them.

use std::collections::BTreeSet;

use platform_conformance::{failures, fixtures_of, repository_root};
use serde_json::{Map, Value, json};

const ID: &str = "CampaignArchive@1";
/// The members of a version's snapshot, and the `field` of a diff entry,
/// that hold the id of a row.
const VERSION_REFERENCES: [&str; 3] = ["playerId", "portraitMediaId", "tokenMediaId"];
const SECTIONS: [&str; 8] = [
    "origin_campaign_id",
    "imported_at",
    "campaign",
    "players",
    "elements",
    "media",
    "sounds",
    "sessions",
];

/// The archive's rows, in a section or a member that may be missing.
fn list<'a>(row: &'a Value, member: &str) -> &'a [Value] {
    row.get(member)
        .and_then(Value::as_array)
        .map_or(&[], Vec::as_slice)
}

/// Every id the archive holds.
fn held(archive: &Value) -> BTreeSet<String> {
    let mut ids = BTreeSet::new();
    let mut take = |row: &Value| {
        ids.insert(row["id"].as_str().expect("an id").to_owned());
    };
    take(&archive["campaign"]);
    for section in ["players", "media", "sounds"] {
        list(archive, section).iter().for_each(&mut take);
    }
    for element in list(archive, "elements") {
        take(element);
        list(element, "versions").iter().for_each(&mut take);
    }
    for session in list(archive, "sessions") {
        take(session);
        list(session, "scenes").iter().for_each(&mut take);
        list(session, "journal_lines").iter().for_each(&mut take);
    }
    ids
}

/// `CampaignArchive@1`, as the Concept's definition words it.
struct Copy<'a> {
    new_ids: &'a Map<String, Value>,
    held: BTreeSet<String>,
}

impl Copy<'_> {
    /// The new id of a row of the archive.
    fn id(&self, old: &Value) -> Value {
        self.new_ids[old.as_str().expect("an id")].clone()
    }

    /// A reference: the new id of the row it names, null when it names none
    /// of the archive's.
    fn reference(&self, old: &Value) -> Value {
        match old.as_str() {
            Some(id) if self.held.contains(id) => self.new_ids[id].clone(),
            _ => Value::Null,
        }
    }

    /// `row` with `members` replaced.
    fn with(row: &Value, members: &[(&str, Value)]) -> Value {
        let mut row = row.clone();
        for (member, value) in members {
            row[member] = value.clone();
        }
        row
    }

    /// A link row, when the row it links to is the archive's.
    fn link(&self, row: &Value, owner: (&str, &Value), other: &str) -> Option<Value> {
        let target = self.reference(&row[other]);
        (!target.is_null()).then(|| Self::with(row, &[(owner.0, owner.1.clone()), (other, target)]))
    }

    fn rows(rows: &[Value], f: impl FnMut(&Value) -> Value) -> Value {
        Value::Array(rows.iter().map(f).collect())
    }

    fn version(&self, version: &Value, element: &Value) -> Value {
        let snapshot = version["snapshot"].as_object().expect("snapshot");
        let snapshot: Map<String, Value> = snapshot
            .iter()
            .map(|(member, value)| {
                let value = if VERSION_REFERENCES.contains(&member.as_str()) {
                    self.reference(value)
                } else {
                    value.clone()
                };
                (member.clone(), value)
            })
            .collect();
        let diff = Self::rows(list(version, "diff"), |entry| {
            if VERSION_REFERENCES.contains(&entry["field"].as_str().expect("field")) {
                Self::with(
                    entry,
                    &[
                        ("old", self.reference(&entry["old"])),
                        ("new", self.reference(&entry["new"])),
                    ],
                )
            } else {
                entry.clone()
            }
        });
        Self::with(
            version,
            &[
                ("id", self.id(&version["id"])),
                ("entity_id", element.clone()),
                (
                    "restored_from_version_id",
                    self.reference(&version["restored_from_version_id"]),
                ),
                ("snapshot", Value::Object(snapshot)),
                ("diff", diff),
            ],
        )
    }

    fn element(&self, element: &Value, campaign: &Value) -> Value {
        let id = self.id(&element["id"]);
        let character =
            element
                .get("character")
                .filter(|c| !c.is_null())
                .map_or(Value::Null, |sheet| {
                    Self::with(
                        sheet,
                        &[
                            ("entity_id", id.clone()),
                            ("player_id", self.reference(&sheet["player_id"])),
                            (
                                "portrait_media_id",
                                self.reference(&sheet["portrait_media_id"]),
                            ),
                            ("token_media_id", self.reference(&sheet["token_media_id"])),
                        ],
                    )
                });
        let versions = Self::rows(list(element, "versions"), |v| self.version(v, &id));
        Self::with(
            element,
            &[
                ("id", id.clone()),
                ("campaign_id", campaign.clone()),
                ("character", character),
                ("aliases", Value::Array(list(element, "aliases").to_vec())),
                ("versions", versions),
            ],
        )
    }

    fn medium(&self, medium: &Value, campaign: &Value) -> Value {
        let id = self.id(&medium["id"]);
        let grid = medium
            .get("battlemap_grid")
            .filter(|g| !g.is_null())
            .map_or(Value::Null, |grid| {
                Self::with(grid, &[("media_id", id.clone())])
            });
        Self::with(
            medium,
            &[
                ("id", id.clone()),
                ("campaign_id", campaign.clone()),
                ("place_id", self.reference(&medium["place_id"])),
                ("battlemap_grid", grid),
            ],
        )
    }

    fn scene(&self, scene: &Value, session: &Value) -> Value {
        let id = self.id(&scene["id"]);
        let links = |member: &str, other: &str| {
            Value::Array(
                list(scene, member)
                    .iter()
                    .filter_map(|row| self.link(row, ("scene_id", &id), other))
                    .collect(),
            )
        };
        Self::with(
            scene,
            &[
                ("id", id.clone()),
                ("session_id", session.clone()),
                ("scene_entity", links("scene_entity", "entity_id")),
                ("scene_media", links("scene_media", "media_id")),
            ],
        )
    }

    fn session(&self, session: &Value, campaign: &Value) -> Value {
        let id = self.id(&session["id"]);
        let present = list(session, "session_present_character")
            .iter()
            .filter_map(|row| self.link(row, ("session_id", &id), "character_id"))
            .collect();
        let lines = Self::rows(list(session, "journal_lines"), |line| {
            Self::with(
                line,
                &[
                    ("id", self.id(&line["id"])),
                    ("session_id", id.clone()),
                    ("scene_id", self.reference(&line["scene_id"])),
                ],
            )
        });
        Self::with(
            session,
            &[
                ("id", id.clone()),
                ("campaign_id", campaign.clone()),
                ("session_present_character", Value::Array(present)),
                (
                    "scenes",
                    Self::rows(list(session, "scenes"), |s| self.scene(s, &id)),
                ),
                ("journal_lines", lines),
            ],
        )
    }
}

fn copy(input: Value) -> Value {
    let archive = &input["archive"];
    let new_ids = input["new_ids"].as_object().expect("new_ids");
    let copy = Copy {
        new_ids,
        held: held(archive),
    };
    let campaign = copy.id(&archive["campaign"]["id"]);
    let with_campaign = |row: &Value| {
        Copy::with(
            row,
            &[
                ("id", copy.id(&row["id"])),
                ("campaign_id", campaign.clone()),
            ],
        )
    };
    json!({
        "origin_campaign_id": archive["origin_campaign_id"],
        "imported_at": input["imported_at"],
        "campaign": Copy::with(&archive["campaign"], &[
            ("id", campaign.clone()),
            ("cover_media_id", copy.reference(&archive["campaign"]["cover_media_id"])),
            ("last_exported_at", Value::Null),
            ("imported_at", input["imported_at"].clone()),
            ("origin_campaign_id", archive["origin_campaign_id"].clone()),
        ]),
        "players": Copy::rows(list(archive, "players"), with_campaign),
        "elements": Copy::rows(list(archive, "elements"), |e| copy.element(e, &campaign)),
        "media": Copy::rows(list(archive, "media"), |m| copy.medium(m, &campaign)),
        "sounds": Copy::rows(list(archive, "sounds"), with_campaign),
        "sessions": Copy::rows(list(archive, "sessions"), |s| copy.session(s, &campaign)),
    })
}

fn concept() -> Value {
    let path = repository_root().join("concepts/CampaignArchive/v1/concept.json");
    let text = std::fs::read_to_string(&path).expect("concept.json");
    serde_json::from_str(&text).expect("concept.json is JSON")
}

fn case(name_starts_with: &str) -> platform_conformance::Case {
    fixtures_of(ID)
        .cases
        .into_iter()
        .find(|case| case.name.starts_with(name_starts_with))
        .unwrap_or_else(|| panic!("no case starting with {name_starts_with:?}"))
}

/// Every string of a JSON value, keys excluded.
fn strings<'a>(value: &'a Value, found: &mut Vec<&'a str>) {
    match value {
        Value::String(text) => found.push(text),
        Value::Array(items) => items.iter().for_each(|item| strings(item, found)),
        Value::Object(members) => members.values().for_each(|item| strings(item, found)),
        _ => {}
    }
}

#[test]
fn conformance_the_fixtures_load_and_an_implementation_passes_them() {
    platform_conformance::check(ID, copy);
}

#[test]
fn a_wrong_implementation_is_caught_by_the_fixtures() {
    // Each one breaks one rule of the definition.
    let origin_id_rewritten = |input: Value| {
        let mut answer = copy(input.clone());
        let campaign = answer["campaign"]["id"].clone();
        answer["origin_campaign_id"] = campaign;
        answer
    };
    let origin_of_the_campaign_row = |input: Value| {
        let mut answer = copy(input.clone());
        answer["campaign"]["origin_campaign_id"] =
            input["archive"]["campaign"]["origin_campaign_id"].clone();
        answer
    };
    let keeps_the_last_export = |input: Value| {
        let mut answer = copy(input.clone());
        answer["campaign"]["last_exported_at"] =
            input["archive"]["campaign"]["last_exported_at"].clone();
        answer
    };
    let version_ids_left_alone = |input: Value| {
        let mut answer = copy(input.clone());
        for (element, original) in answer["elements"]
            .as_array_mut()
            .expect("elements")
            .iter_mut()
            .zip(input["archive"]["elements"].as_array().expect("elements"))
        {
            element["versions"] = Value::Array(
                original["versions"]
                    .as_array()
                    .expect("versions")
                    .iter()
                    .zip(element["versions"].as_array().expect("versions"))
                    .map(|(was, now)| {
                        Copy::with(
                            now,
                            &[
                                ("snapshot", was["snapshot"].clone()),
                                ("diff", was["diff"].clone()),
                            ],
                        )
                    })
                    .collect(),
            );
        }
        answer
    };
    let outside_kept = |input: Value| {
        let mut answer = copy(input.clone());
        if let Some(lines) = answer
            .pointer_mut("/sessions/0/journal_lines")
            .and_then(Value::as_array_mut)
        {
            for (line, was) in lines
                .iter_mut()
                .zip(list(&input["archive"]["sessions"][0], "journal_lines"))
            {
                line["scene_id"] = was["scene_id"].clone();
            }
        }
        answer
    };
    let sections_missing_stay_missing = |input: Value| {
        let mut answer = copy(input);
        if answer["media"] == json!([]) {
            answer.as_object_mut().expect("copy").remove("media");
        }
        answer
    };
    let fixtures = fixtures_of(ID);
    for wrong in [
        &origin_id_rewritten as &dyn Fn(Value) -> Value,
        &origin_of_the_campaign_row,
        &keeps_the_last_export,
        &version_ids_left_alone,
        &outside_kept,
        &sections_missing_stay_missing,
    ] {
        assert!(!failures(&fixtures, wrong).is_empty());
    }
}

#[test]
fn in_every_case_the_new_ids_are_new_distinct_and_cover_the_archive() {
    for case in fixtures_of(ID).cases {
        let new_ids = case.input["new_ids"].as_object().expect("new_ids");
        let held = held(&case.input["archive"]);
        let given: BTreeSet<&String> = new_ids.keys().collect();
        assert_eq!(
            given,
            held.iter().collect(),
            "{}: one new id for each id of the archive",
            case.name
        );
        let fresh: BTreeSet<&str> = new_ids
            .values()
            .map(|id| id.as_str().expect("id"))
            .collect();
        assert_eq!(fresh.len(), new_ids.len(), "{}: no two alike", case.name);
        assert!(
            fresh.iter().all(|id| !held.contains(*id)),
            "{}: none is an id of the archive",
            case.name
        );
    }
}

#[test]
fn a_full_copy_keeps_no_id_of_the_original_but_its_origin() {
    let case = case("a full archive becomes a new campaign");
    let archive = &case.input["archive"];
    let held = held(archive);
    let mut found = Vec::new();
    strings(&case.output, &mut found);
    for text in found {
        if text == archive["origin_campaign_id"] {
            continue;
        }
        assert!(!held.contains(text), "{text} is an id of the original");
    }
    assert_eq!(
        case.output["origin_campaign_id"],
        archive["origin_campaign_id"]
    );
    assert_eq!(
        case.output["campaign"]["origin_campaign_id"],
        archive["origin_campaign_id"]
    );
    assert_ne!(case.output["campaign"]["id"], archive["campaign"]["id"]);

    // The full archive carries what the acceptance names: two players, a PJ
    // linked to one, a place with a medium, a battlemap with its grid, a
    // sound, a session with a scene and a journal line.
    assert_eq!(archive["players"].as_array().map(Vec::len), Some(2));
    assert!(
        archive["elements"]
            .as_array()
            .expect("elements")
            .iter()
            .any(|e| { e["type"] == "pj" && e["character"]["player_id"].is_string() })
    );
    assert!(archive["media"].as_array().expect("media").iter().any(|m| {
        m["kind"] == "battlemap" && m["battlemap_grid"].is_object() && m["place_id"].is_string()
    }));
    assert_eq!(archive["sounds"].as_array().map(Vec::len), Some(1));
    let session = &archive["sessions"][0];
    assert_eq!(session["scenes"].as_array().map(Vec::len), Some(1));
    assert!(
        !session["journal_lines"]
            .as_array()
            .expect("lines")
            .is_empty()
    );
}

#[test]
fn every_reference_of_the_copy_points_at_a_row_of_the_copy() {
    let case = case("a full archive becomes a new campaign");
    let output = &case.output;
    let ids = held(&json!({
        "campaign": output["campaign"], "players": output["players"], "elements": output["elements"],
        "media": output["media"], "sounds": output["sounds"], "sessions": output["sessions"],
    }));
    let new_ids: BTreeSet<&str> = case.input["new_ids"]
        .as_object()
        .expect("new_ids")
        .values()
        .map(|id| id.as_str().expect("id"))
        .collect();
    assert_eq!(
        ids.iter().map(String::as_str).collect::<BTreeSet<_>>(),
        new_ids
    );

    let campaign = &output["campaign"]["id"];
    let element = &output["elements"][0];
    assert_eq!(element["character"]["entity_id"], element["id"]);
    assert!(
        ids.contains(
            element["character"]["player_id"]
                .as_str()
                .expect("player_id")
        )
    );
    assert!(
        ids.contains(
            element["character"]["portrait_media_id"]
                .as_str()
                .expect("portrait")
        )
    );
    assert!(
        output["players"]
            .as_array()
            .expect("players")
            .iter()
            .all(|p| &p["campaign_id"] == campaign)
    );
    for version in element["versions"].as_array().expect("versions") {
        assert_eq!(version["entity_id"], element["id"]);
    }
    let restoration = &element["versions"][3];
    assert_eq!(restoration["source"], "restauration");
    assert_eq!(
        restoration["restored_from_version_id"],
        element["versions"][0]["id"]
    );
    // A snapshot and a diff name the copy's rows, and nothing else changed.
    assert_eq!(
        element["versions"][1]["snapshot"]["portraitMediaId"],
        output["media"][1]["id"]
    );
    assert_eq!(
        element["versions"][2]["diff"][0]["old"],
        output["players"][0]["id"]
    );
    assert_eq!(
        element["versions"][2]["diff"][0]["new"],
        output["players"][1]["id"]
    );
    let medium = &output["media"][0];
    assert_eq!(medium["place_id"], output["elements"][2]["id"]);
    assert_eq!(medium["battlemap_grid"]["media_id"], medium["id"]);
    let session = &output["sessions"][0];
    let scene = &session["scenes"][0];
    assert_eq!(
        session["session_present_character"][0]["character_id"],
        element["id"]
    );
    assert_eq!(scene["session_id"], session["id"]);
    assert_eq!(scene["scene_entity"][0]["scene_id"], scene["id"]);
    assert_eq!(
        scene["scene_entity"][0]["entity_id"],
        output["elements"][2]["id"]
    );
    assert_eq!(scene["scene_media"][0]["media_id"], medium["id"]);
    assert_eq!(session["journal_lines"][0]["scene_id"], scene["id"]);
    assert_eq!(session["journal_lines"][1]["scene_id"], Value::Null);
}

#[test]
fn an_archive_missing_media_sounds_and_sessions_copies_with_them_empty() {
    let case = case("an archive exported before media, sounds and sessions");
    for section in ["media", "sounds", "sessions"] {
        assert!(
            case.input["archive"].get(section).is_none(),
            "{section} is in the input"
        );
        assert_eq!(case.output[section], json!([]));
    }
    assert!(
        !case.output["players"]
            .as_array()
            .expect("players")
            .is_empty()
    );
    assert!(
        !case.output["elements"]
            .as_array()
            .expect("elements")
            .is_empty()
    );
}

#[test]
fn an_origin_that_still_exists_is_kept_and_the_campaign_still_gets_a_new_id() {
    let case = case("an archive whose origin_campaign_id names a campaign that still exists");
    let archive = &case.input["archive"];
    // The original is there: the archive's campaign is the origin.
    assert_eq!(archive["campaign"]["id"], archive["origin_campaign_id"]);
    assert_ne!(case.output["campaign"]["id"], archive["campaign"]["id"]);
    assert_eq!(
        case.output["origin_campaign_id"],
        archive["origin_campaign_id"]
    );
    // The campaign row's own origin was another copy's: it is not the copy's.
    assert_ne!(
        archive["campaign"]["origin_campaign_id"],
        archive["origin_campaign_id"]
    );
    assert_eq!(
        case.output["campaign"]["origin_campaign_id"],
        archive["origin_campaign_id"]
    );
    assert_eq!(
        case.output["campaign"]["imported_at"],
        case.input["imported_at"]
    );
    assert_eq!(case.output["campaign"]["last_exported_at"], Value::Null);
}

#[test]
fn what_the_archive_does_not_hold_becomes_null_or_leaves_with_its_link() {
    let case = case("a reference to something outside the archive");
    let archive = &case.input["archive"];
    let output = &case.output;
    assert!(archive["campaign"]["cover_media_id"].is_string());
    assert_eq!(output["campaign"]["cover_media_id"], Value::Null);
    let sheet = &output["elements"][0]["character"];
    assert_eq!(sheet["player_id"], Value::Null);
    assert_eq!(sheet["portrait_media_id"], Value::Null);
    assert_eq!(output["media"][0]["place_id"], Value::Null);
    let versions = &output["elements"][0]["versions"];
    assert_eq!(versions[0]["snapshot"]["playerId"], Value::Null);
    assert_eq!(versions[0]["diff"][2]["new"], Value::Null);
    assert_eq!(versions[1]["snapshot"]["portraitMediaId"], Value::Null);
    assert_eq!(versions[3]["restored_from_version_id"], Value::Null);
    let session = &output["sessions"][0];
    assert_eq!(session["journal_lines"][0]["scene_id"], Value::Null);
    // The three link rows naming a row outside are gone; the others stay.
    let before = &archive["sessions"][0];
    assert_eq!(
        before["session_present_character"].as_array().map(Vec::len),
        Some(2)
    );
    assert_eq!(
        session["session_present_character"]
            .as_array()
            .map(Vec::len),
        Some(1)
    );
    assert_eq!(
        before["scenes"][0]["scene_entity"].as_array().map(Vec::len),
        Some(3)
    );
    assert_eq!(
        session["scenes"][0]["scene_entity"]
            .as_array()
            .map(Vec::len),
        Some(2)
    );
    assert_eq!(
        before["scenes"][0]["scene_media"].as_array().map(Vec::len),
        Some(2)
    );
    assert_eq!(
        session["scenes"][0]["scene_media"].as_array().map(Vec::len),
        Some(1)
    );
}

#[test]
fn versions_are_copied_as_they_are_but_for_the_ids_they_name() {
    let case = case("a full archive becomes a new campaign");
    let strip = |version: &Value| {
        let mut version = version.clone();
        let object = version.as_object_mut().expect("version");
        for member in [
            "id",
            "entity_id",
            "restored_from_version_id",
            "snapshot",
            "diff",
        ] {
            object.remove(member);
        }
        version
    };
    let before = case.input["archive"]["elements"]
        .as_array()
        .expect("elements");
    let after = case.output["elements"].as_array().expect("elements");
    for (was, now) in before.iter().zip(after) {
        let (was, now) = (
            was["versions"].as_array().expect("versions"),
            now["versions"].as_array().expect("versions"),
        );
        assert_eq!(was.len(), now.len());
        for (was, now) in was.iter().zip(now) {
            assert_eq!(
                strip(was),
                strip(now),
                "number, source and date are not touched"
            );
            assert_eq!(
                was["diff"]
                    .as_array()
                    .map(|d| d.iter().map(|e| e["field"].clone()).collect::<Vec<_>>()),
                now["diff"]
                    .as_array()
                    .map(|d| d.iter().map(|e| e["field"].clone()).collect::<Vec<_>>()),
                "the entries keep their order"
            );
        }
    }
}

#[test]
fn concept_json_points_at_the_fixtures_and_every_output_obeys_it() {
    let concept = concept();
    assert_eq!(concept["concept"], "CampaignArchive");
    assert_eq!(concept["version"], 1);
    assert_eq!(concept["kind"], "derived");
    assert_eq!(concept["conformance"], "./fixtures.json");

    let fields: Vec<&str> = concept["output"]
        .as_object()
        .expect("output")
        .keys()
        .map(String::as_str)
        .collect();
    let mut sections = SECTIONS.to_vec();
    let mut named = fields.clone();
    named.sort_unstable();
    sections.sort_unstable();
    assert_eq!(named, sections, "the output names each section of the copy");

    // The definition names every section and every column of the archive.
    let definition = concept["definition"].as_str().expect("definition");
    let archive = &case("a full archive becomes a new campaign").input["archive"];
    let mut rows: Vec<&Value> = vec![
        &archive["campaign"],
        &archive["players"][0],
        &archive["media"][0],
        &archive["sounds"][0],
    ];
    rows.extend([
        &archive["media"][0]["battlemap_grid"],
        &archive["elements"][0],
        &archive["elements"][0]["character"],
        &archive["elements"][0]["versions"][0],
    ]);
    let session = &archive["sessions"][0];
    rows.extend([
        session,
        &session["session_present_character"][0],
        &session["scenes"][0],
        &session["scenes"][0]["scene_entity"][0],
        &session["scenes"][0]["scene_media"][0],
        &session["journal_lines"][0],
    ]);
    for row in rows {
        for column in row.as_object().expect("row").keys() {
            assert!(
                definition.contains(column.as_str()),
                "the definition does not name {column}"
            );
        }
    }

    for case in fixtures_of(ID).cases {
        let output = case.output.as_object().expect("an output is an object");
        assert_eq!(output.len(), fields.len(), "{}: fields", case.name);
        assert!(
            fields.iter().all(|field| output.contains_key(*field)),
            "{}",
            case.name
        );
        for section in ["players", "elements", "media", "sounds", "sessions"] {
            assert!(
                output[section].is_array(),
                "{}: {section} is a list",
                case.name
            );
        }
    }
}
