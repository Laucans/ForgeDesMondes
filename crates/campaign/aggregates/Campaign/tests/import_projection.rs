//! The Campaign aggregate's projection of an import over PostgreSQL
//! (`DATABASE_URL`): the copy `campaign.importCampaign` writes on
//! `Campaign/<its new id>` reaches the product's tables inside the
//! transaction that applies it, once (`import-creates-copy`) — the original's
//! rows and its last export untouched, a copy the tables lost written again
//! when the store starts, and what the tables refuse rejecting the command
//! with nothing written. The import's `Prepare` is #575's: its manifest is
//! declared here, the row it writes built by hand, as the Concept
//! CampaignArchive@1 gives the copy.

// One `json!` holds the whole copy.
#![recursion_limit = "512"]

use std::collections::BTreeMap;

use harness_dataguard::command::State;
use platform_test_kit::{Bench, Scratch, Served};
use serde_json::{Value, json};

const IMPORT: &str = r#"{"dataCapability":"campaign.importCampaign","owner":"dataguard","version":1,"effect":"insert","target":{"aggregate":"Campaign","id":"$id"},"touches":["Campaign.id","Campaign.name","Campaign.kind","Campaign.pitch","Campaign.art","Campaign.glyph","Campaign.coverMediaId","Campaign.createdAt","Campaign.importedAt","Campaign.originCampaignId","Campaign.soundMasterVolume"],"payload":{"archive":"object"},"mode":"overwrite","invariants":["import-creates-copy"],"permissions":[],"callableBy":["*"],"idempotencyKey":"optional"}"#;
/// The product's export, as its `data-capability.json` declares it.
const EXPORT: &str = r#"{"dataCapability":"campaign.exportCampaign","owner":"dataguard","version":1,"effect":"update","target":{"aggregate":"Campaign","id":"$id"},"touches":["Campaign.lastExportedAt"],"payload":{"id":"uuid"},"mode":"overwrite","invariants":["export-stamps-date"],"permissions":[],"callableBy":["*"],"idempotencyKey":"optional"}"#;

/// The original campaign, its player, its PJ and its place, as the fixtures
/// write them, and when it was last exported.
const VALOMBRE: &str = "c1000000-0000-4000-8000-000000000001";
const CLAIRE: &str = "b1000000-0000-4000-8000-000000000001";
const ELOWEN: &str = "e1000000-0000-4000-8000-000000000001";
const EXPORTED: &str = "2026-10-20T10:00:00+00:00";

/// The copy's rows' ids.
const COPY: &str = "c2000000-0000-4000-8000-000000000001";
const MARC: &str = "b2000000-0000-4000-8000-000000000001";
const ELODIE: &str = "e2000000-0000-4000-8000-000000000001";
const GARDE: &str = "e2000000-0000-4000-8000-000000000002";
const VALOMBRE_HAUTE: &str = "e2000000-0000-4000-8000-000000000003";
const CARTE: &str = "a2000000-0000-4000-8000-000000000001";
const PORTRAIT: &str = "a2000000-0000-4000-8000-000000000002";
const PLUIE: &str = "d2000000-0000-4000-8000-000000000001";
const SESSION: &str = "5e000000-0000-4000-8000-000000000001";
const SCENE: &str = "5c000000-0000-4000-8000-000000000001";
const LINE: &str = "1e000000-0000-4000-8000-000000000001";

/// The product's tables, each as its rows' JSON.
type Tables = BTreeMap<String, Vec<Value>>;

/// A database migrated as the server migrates it, holding the original.
async fn scratch(label: &str) -> Scratch {
    let scratch = Scratch::new(label).await;
    scratch
        .seed(&format!(
            "INSERT INTO campaign (id, name, kind, pitch, last_exported_at)
                 VALUES ('{VALOMBRE}', 'Les Cendres de Valombre', 'campagne', 'Une cité sous la cendre.', '{EXPORTED}');
             INSERT INTO player (id, campaign_id, name, notes) VALUES ('{CLAIRE}', '{VALOMBRE}', 'Claire', 'Joue Elowen.');
             INSERT INTO entity (id, campaign_id, type, name)
                 VALUES ('{ELOWEN}', '{VALOMBRE}', 'pj', 'Elowen');
             INSERT INTO entity_version (entity_id, version_number, source, snapshot, diff)
                 VALUES ('{ELOWEN}', 1, 'dm', '{{}}', '[]');
             INSERT INTO character (entity_id, player_id, token_color) VALUES ('{ELOWEN}', '{CLAIRE}', '#334455');
             INSERT INTO sound (id, campaign_id, name, file_path, tags, volume)
                 VALUES ('a1000000-0000-4000-8000-000000000009', '{VALOMBRE}', 'Cloche', '/s/cloche.ogg', '{{cloche}}', 0.7);
             INSERT INTO session (id, campaign_id, position, title, status, microphone_on)
                 VALUES ('5e100000-0000-4000-8000-000000000001', '{VALOMBRE}', 1, 'Le pont', 'closed', false);"
        ))
        .await;
    scratch
}

/// The engine over `scratch`, the aggregate's projection warmed: what the
/// server does at each start.
async fn started(scratch: &Scratch) -> Served {
    Bench::new(
        IMPORT,
        include_str!("../aggregate.json"),
        [aggregate_campaign::plugin()],
    )
    .planting(EXPORT)
    .postgres(scratch)
    .await
}

/// The row a full copy writes: the campaign's fields and the sections, ids
/// new, every reference the copy's own.
fn copy() -> Value {
    let stats = json!({
        "strength": 10, "dexterity": 14, "constitution": 12, "intelligence": 11,
        "wisdom": 13, "charisma": 8, "armor_class": 15, "hp_max": 31
    });
    let sheet = |entity: &str, player: Value, portrait: Value, crop: Value| {
        let mut sheet = json!({
            "entity_id": entity, "player_id": player, "initial_prompt": "Une rôdeuse.",
            "background": null, "behavior": null, "spells": ["sommeil"], "actions": [],
            "portrait_media_id": portrait, "token_media_id": null, "token_crop": crop,
            "token_color": "#aa3322", "dndbeyond_id": null, "dndbeyond_synced_at": null
        });
        sheet
            .as_object_mut()
            .expect("an object")
            .extend(stats.as_object().expect("an object").clone());
        sheet
    };
    let version = |id: &str,
                   entity: &str,
                   number: u64,
                   source: &str,
                   restored: Value,
                   diff: Value| {
        json!({
            "id": id, "entity_id": entity, "version_number": number, "source": source,
            "snapshot": { "name": "Elodie" }, "diff": diff, "restored_from_version_id": restored,
            "created_at": "2026-10-01T09:00:00Z"
        })
    };
    let element = |id: &str,
                   kind: &str,
                   name: &str,
                   character: Value,
                   aliases: Value,
                   versions: Value| {
        json!({
            "id": id, "campaign_id": COPY, "type": kind, "name": name, "description": null,
            "notes": null, "created_at": "2026-10-01T09:00:00Z", "updated_at": "2026-10-02T09:00:00Z",
            "character": character, "aliases": aliases, "versions": versions
        })
    };
    let v1 = "f2000000-0000-4000-8000-000000000001";
    json!({
        "id": COPY, "name": "Les Cendres de Valombre", "kind": "campagne",
        "pitch": "Une cité sous la cendre.", "art": null, "glyph": "V",
        "coverMediaId": CARTE, "createdAt": "2026-09-01T08:00:00Z",
        "importedAt": "2026-10-25T12:00:00Z", "originCampaignId": VALOMBRE,
        "soundMasterVolume": 0.8,
        "players": [
            { "id": MARC, "campaign_id": COPY, "name": "Marc", "notes": null, "created_at": "2026-09-01T08:00:00Z" }
        ],
        "elements": [
            element(
                ELODIE, "pj", "Elodie",
                sheet(ELODIE, json!(MARC), json!(PORTRAIT), json!({ "x": 0.1, "y": 0.2, "scale": 1.5 })),
                json!(["Elo", "Dame Grise"]),
                json!([
                    version(v1, ELODIE, 1, "dm", Value::Null, json!([])),
                    version("f2000000-0000-4000-8000-000000000002", ELODIE, 2, "dm", Value::Null,
                        json!([{ "field": "name", "old": "Elodie", "new": "Elodie II" }])),
                    version("f2000000-0000-4000-8000-000000000003", ELODIE, 3, "restauration", json!(v1),
                        json!([{ "field": "name", "old": "Elodie II", "new": "Elodie" }]))
                ])
            ),
            element(
                GARDE, "pnj", "Le garde", sheet(GARDE, Value::Null, Value::Null, Value::Null),
                json!([]),
                json!([version("f2000000-0000-4000-8000-000000000004", GARDE, 1, "dm", Value::Null, json!([]))])
            ),
            element(
                VALOMBRE_HAUTE, "lieu", "Valombre-Haute", Value::Null, json!(["La Haute"]),
                json!([version("f2000000-0000-4000-8000-000000000005", VALOMBRE_HAUTE, 1, "dm", Value::Null, json!([]))])
            )
        ],
        "media": [
            {
                "id": CARTE, "campaign_id": COPY, "kind": "battlemap", "title": "La haute ville",
                "file_path": "/m/haute.png", "width_px": 2000, "height_px": 1500,
                "place_id": VALOMBRE_HAUTE, "uploaded_at": "2026-09-02T08:00:00Z",
                "battlemap_grid": {
                    "media_id": CARTE, "hex_size_px": 60, "offset_x": 0, "offset_y": 12.5,
                    "orientation": "pointe", "color": "#ffffff", "opacity": 0.4, "feet_per_hex": 5
                }
            },
            {
                "id": PORTRAIT, "campaign_id": COPY, "kind": "illustration", "title": "Elodie",
                "file_path": "/m/elodie.png", "width_px": 400, "height_px": 400,
                "place_id": null, "uploaded_at": "2026-09-02T08:00:00Z", "battlemap_grid": null
            }
        ],
        "sounds": [
            {
                "id": PLUIE, "campaign_id": COPY, "name": "Pluie", "file_path": "/s/pluie.ogg",
                "tags": ["pluie", "ambiance"], "volume": 0.5, "created_at": "2026-09-03T08:00:00Z"
            }
        ],
        "sessions": [
            {
                "id": SESSION, "campaign_id": COPY, "position": 1, "title": "Le pont",
                "planned_on": "2026-10-03", "status": "closed", "started_at": "2026-10-03T18:00:00Z",
                "closed_at": "2026-10-03T22:00:00Z", "microphone_on": false, "summary": "Ils passent.",
                "summary_model": null, "summary_edited_at": null,
                "session_present_character": [
                    { "session_id": SESSION, "character_id": ELODIE, "added_at": "2026-10-03T18:05:00Z" }
                ],
                "scenes": [
                    {
                        "id": SCENE, "session_id": SESSION, "position": 1, "title": "La herse", "notes": null,
                        "scene_entity": [{ "scene_id": SCENE, "entity_id": GARDE, "position": 1 }],
                        "scene_media": [{ "scene_id": SCENE, "media_id": CARTE }]
                    }
                ],
                "journal_lines": [
                    { "id": LINE, "session_id": SESSION, "scene_id": SCENE, "spoken_at": "2026-10-03T18:30:00Z", "text": "Halte !" }
                ]
            }
        ]
    })
}

/// The copy of an archive without media, sounds or sessions: its sections
/// absent or null, and nothing of it pointing at a medium.
fn copy_without_media_sounds_or_sessions() -> Value {
    let mut row = copy();
    let fields = row.as_object_mut().expect("an object");
    fields.remove("coverMediaId");
    fields.remove("media");
    fields.insert("sounds".into(), Value::Null);
    fields.remove("sessions");
    let elements = fields["elements"].as_array_mut().expect("a list");
    elements[0]["character"]["portrait_media_id"] = Value::Null;
    elements[0]["character"]["token_crop"] = Value::Null;
    row
}

async fn imported(served: &Served, row: &Value) -> State {
    served
        .applied("campaign.importCampaign", row.clone(), Some(COPY))
        .await
        .status
}

/// Every row of every table of the product, as JSON, by table: the
/// DataGuard's own (`dataguard_*`) and the migrations' record aside.
async fn tables(scratch: &Scratch) -> Tables {
    let names = scratch
        .client()
        .query(
            "SELECT tablename::text FROM pg_tables
             WHERE schemaname = 'public' AND tablename NOT LIKE 'dataguard%'
                 AND tablename <> 'data_migrations'
             ORDER BY tablename",
            &[],
        )
        .await
        .expect("the tables");
    let mut tables = Tables::new();
    for name in names {
        let name: String = name.get(0);
        let rows = scratch
            .client()
            .query(
                &format!("SELECT to_jsonb(t) FROM {name} t ORDER BY to_jsonb(t)::text"),
                &[],
            )
            .await
            .expect("a table's rows")
            .iter()
            .map(|row| row.get::<_, Value>(0))
            .collect();
        tables.insert(name, rows);
    }
    tables
}

/// The tables in which `left` and `right` differ, an alias taken by its
/// text: the archive gives it no id, the table draws one each time.
fn same(left: &Tables, right: &Tables) -> Vec<String> {
    let keyed = |tables: &Tables, name: &str| -> Vec<Value> {
        let mut rows = tables[name].clone();
        if name == "entity_alias" {
            for row in &mut rows {
                row.as_object_mut().expect("a row").remove("id");
            }
            rows.sort_by_key(ToString::to_string);
        }
        rows
    };
    left.keys()
        .filter(|name| keyed(left, name) != keyed(right, name))
        .cloned()
        .collect()
}

fn assert_same(left: &Tables, right: &Tables, what: &str) {
    assert!(
        same(left, right).is_empty(),
        "{what}: differ in {:?}",
        same(left, right)
    );
}

/// How many rows each table holds more than `before`.
fn added(before: &Tables, after: &Tables) -> BTreeMap<String, usize> {
    after
        .iter()
        .map(|(name, rows)| (name.clone(), rows.len() - before[name].len()))
        .filter(|(_, count)| *count > 0)
        .collect()
}

/// Whether every row of `before` is still in `after`, as it was.
fn untouched(before: &Tables, after: &Tables) -> bool {
    before
        .iter()
        .all(|(name, rows)| rows.iter().all(|row| after[name].contains(row)))
}

/// The count a query answers.
async fn count(scratch: &Scratch, sql: &str) -> i64 {
    scratch
        .client()
        .query_one(sql, &[])
        .await
        .expect("a count")
        .get(0)
}

fn counts(pairs: &[(&str, usize)]) -> BTreeMap<String, usize> {
    pairs
        .iter()
        .map(|(name, count)| ((*name).to_string(), *count))
        .collect()
}

#[tokio::test]
async fn import_creates_copy_a_full_copy_reaches_every_table_under_the_ids_it_carries() {
    let scratch = scratch("import_full").await;
    let served = started(&scratch).await;
    let before = tables(&scratch).await;

    assert_eq!(imported(&served, &copy()).await, State::Applied);

    let after = tables(&scratch).await;
    assert_eq!(
        added(&before, &after),
        counts(&[
            ("battlemap_grid", 1),
            ("campaign", 1),
            ("character", 2),
            ("entity", 3),
            ("entity_alias", 3),
            ("entity_version", 5),
            ("media", 2),
            ("player", 1),
            ("scene", 1),
            ("scene_entity", 1),
            ("scene_media", 1),
            ("session", 1),
            ("session_journal_line", 1),
            ("session_present_character", 1),
            ("sound", 1),
        ])
    );
    // The original, its rows and its last export, as they were.
    assert!(untouched(&before, &after));
    assert_eq!(
        scratch
            .rows(
                "SELECT last_exported_at::text AS at FROM campaign WHERE id = $1::uuid",
                &[json!(VALOMBRE)]
            )
            .await,
        [json!({ "at": "2026-10-20 10:00:00+00" })]
    );
    // The copy: its own columns, never exported, its cover set.
    assert_eq!(
        scratch
            .rows(
                "SELECT name, kind, glyph, cover_media_id::text AS cover, imported_at::text AS imported,
                     origin_campaign_id::text AS origin, sound_master_volume::text AS volume,
                     last_exported_at IS NULL AS never_exported
                 FROM campaign WHERE id = $1::uuid",
                &[json!(COPY)]
            )
            .await,
        [json!({
            "name": "Les Cendres de Valombre", "kind": "campagne", "glyph": "V", "cover": CARTE,
            "imported": "2026-10-25 12:00:00+00", "origin": VALOMBRE, "volume": "0.80",
            "never_exported": true
        })]
    );
    // Every reference resolves to the copy's rows.
    assert_eq!(
        count(
            &scratch,
            &format!(
                "SELECT count(*) FROM character c
                     JOIN player p ON p.id = c.player_id AND p.id = '{MARC}'
                     JOIN media m ON m.id = c.portrait_media_id AND m.id = '{PORTRAIT}'
                 WHERE c.entity_id = '{ELODIE}' AND c.token_crop = '{{\"x\":0.1,\"y\":0.2,\"scale\":1.5}}'::jsonb
                     AND c.spells = '[\"sommeil\"]'::jsonb"
            )
        )
        .await,
        1
    );
    assert_eq!(
        count(
            &scratch,
            &format!(
                "SELECT count(*) FROM media m
                     JOIN entity e ON e.id = m.place_id AND e.id = '{VALOMBRE_HAUTE}'
                     JOIN battlemap_grid g ON g.media_id = m.id
                 WHERE m.id = '{CARTE}' AND m.campaign_id = '{COPY}'
                     AND g.hex_size_px = 60 AND g.offset_y = 12.5 AND g.opacity = 0.4 AND g.feet_per_hex = 5"
            )
        )
        .await,
        1
    );
    assert_eq!(
        count(
            &scratch,
            &format!(
                "SELECT count(*) FROM session s
                     JOIN session_present_character p ON p.session_id = s.id AND p.character_id = '{ELODIE}'
                     JOIN scene c ON c.session_id = s.id AND c.id = '{SCENE}'
                     JOIN scene_entity se ON se.scene_id = c.id AND se.entity_id = '{GARDE}' AND se.position = 1
                     JOIN scene_media sm ON sm.scene_id = c.id AND sm.media_id = '{CARTE}'
                     JOIN session_journal_line j ON j.session_id = s.id AND j.scene_id = c.id AND j.id = '{LINE}'
                 WHERE s.id = '{SESSION}' AND s.campaign_id = '{COPY}' AND s.planned_on = '2026-10-03'"
            )
        )
        .await,
        1
    );
    assert_eq!(
        count(
            &scratch,
            &format!(
                "SELECT count(*) FROM sound WHERE id = '{PLUIE}' AND campaign_id = '{COPY}'
                     AND tags = '{{pluie,ambiance}}' AND volume = 0.5"
            )
        )
        .await,
        1
    );
    // The versions, in their order, the restoration pointing at the first.
    assert_eq!(
        scratch
            .rows(
                "SELECT version_number, source, restored_from_version_id::text AS restored
                 FROM entity_version WHERE entity_id = $1::uuid ORDER BY version_number",
                &[json!(ELODIE)]
            )
            .await,
        [
            json!({ "version_number": 1, "source": "dm", "restored": null }),
            json!({ "version_number": 2, "source": "dm", "restored": null }),
            json!({ "version_number": 3, "source": "restauration", "restored": "f2000000-0000-4000-8000-000000000001" }),
        ]
    );
    assert_eq!(
        scratch
            .rows(
                "SELECT alias FROM entity_alias WHERE entity_id = $1::uuid ORDER BY alias",
                &[json!(ELODIE)]
            )
            .await,
        [json!({ "alias": "Dame Grise" }), json!({ "alias": "Elo" })]
    );
}

#[tokio::test]
async fn import_creates_copy_a_copy_without_media_sounds_or_sessions_writes_its_players_and_elements_only()
 {
    let scratch = scratch("import_light").await;
    let served = started(&scratch).await;
    let before = tables(&scratch).await;

    assert_eq!(
        imported(&served, &copy_without_media_sounds_or_sessions()).await,
        State::Applied
    );

    let after = tables(&scratch).await;
    assert_eq!(
        added(&before, &after),
        counts(&[
            ("campaign", 1),
            ("character", 2),
            ("entity", 3),
            ("entity_alias", 3),
            ("entity_version", 5),
            ("player", 1),
        ])
    );
    assert!(untouched(&before, &after));
}

#[tokio::test]
async fn import_creates_copy_exporting_the_copy_stamps_it_alone() {
    let scratch = scratch("import_then_export").await;
    let served = started(&scratch).await;
    assert_eq!(imported(&served, &copy()).await, State::Applied);
    let before = tables(&scratch).await;

    let exported = served
        .applied(
            "campaign.exportCampaign",
            json!({ "lastExportedAt": "2026-10-26T08:00:00Z" }),
            Some(COPY),
        )
        .await;
    assert_eq!(exported.status, State::Applied, "{exported:?}");

    let after = tables(&scratch).await;
    assert!(added(&before, &after).is_empty());
    let changed: Vec<_> = after["campaign"]
        .iter()
        .filter(|row| !before["campaign"].contains(row))
        .collect();
    assert_eq!(changed.len(), 1);
    assert_eq!(changed[0]["id"], COPY);
    assert_eq!(changed[0]["last_exported_at"], "2026-10-26T08:00:00+00:00");
    assert!(untouched(
        &BTreeMap::from_iter(
            before
                .iter()
                .filter(|(name, _)| *name != "campaign")
                .map(|(name, rows)| (name.clone(), rows.clone()))
        ),
        &after
    ));
}

#[tokio::test]
async fn import_creates_copy_a_copy_the_tables_lost_is_written_again_once_when_the_store_starts() {
    let scratch = scratch("import_warm").await;
    let served = started(&scratch).await;
    assert_eq!(imported(&served, &copy()).await, State::Applied);
    let before = tables(&scratch).await;

    // The tables lose the copy, and nothing else: the engine's row stays.
    scratch
        .seed(&format!("DELETE FROM campaign WHERE id = '{COPY}'"))
        .await;
    assert!(!same(&tables(&scratch).await, &before).is_empty());

    // A start writes it again, whole...
    let restarted = started(&scratch).await;
    assert_same(&tables(&scratch).await, &before, "the tables");

    // ...and another overwrites nothing and duplicates nothing: what the DM
    // has changed since is kept, what has gone from a copy the table holds
    // stays gone.
    scratch
        .seed(&format!(
            "UPDATE player SET name = 'Marc-Antoine' WHERE id = '{MARC}';
             DELETE FROM sound WHERE id = '{PLUIE}'"
        ))
        .await;
    let changed = tables(&scratch).await;
    drop(restarted);
    drop(started(&scratch).await);
    assert_same(&tables(&scratch).await, &changed, "the tables");
}

#[tokio::test]
async fn import_creates_copy_a_copy_the_tables_refuse_at_a_start_is_written_whole_or_not_at_all() {
    let scratch = scratch("import_warm_refused").await;
    let served = started(&scratch).await;
    assert_eq!(imported(&served, &copy()).await, State::Applied);

    // The copy is lost, and its PJ's id taken by another campaign's element
    // since: the copy cannot be written again.
    scratch
        .seed(&format!(
            "DELETE FROM campaign WHERE id = '{COPY}';
             INSERT INTO entity (id, campaign_id, type, name) VALUES ('{ELODIE}', '{VALOMBRE}', 'pnj', 'Autre');
             INSERT INTO entity_version (entity_id, version_number, source, snapshot, diff)
                 VALUES ('{ELODIE}', 1, 'dm', '{{}}', '[]')"
        ))
        .await;
    let before = tables(&scratch).await;

    drop(served);
    drop(started(&scratch).await);

    // The store started, and not a row of the copy was written: neither its
    // campaign nor its players, written before the element that is refused.
    assert_same(&tables(&scratch).await, &before, "the tables");
    assert_eq!(
        count(
            &scratch,
            &format!("SELECT count(*) FROM campaign WHERE id = '{COPY}'")
        )
        .await,
        0
    );
}

/// The copy with `edit` made on it.
fn edited(edit: impl FnOnce(&mut Value)) -> Value {
    let mut row = copy();
    edit(&mut row);
    row
}

#[tokio::test]
async fn import_creates_copy_a_row_the_tables_refuse_rejects_the_command_and_writes_nothing() {
    let scratch = scratch("import_refused").await;
    let served = started(&scratch).await;
    let before = tables(&scratch).await;

    let refused: [(&str, Value); 12] = [
        // A CHECK of the table: a grid's opacity, a sound's volume, the
        // master volume, a campaign's kind, a grid's hex size.
        (
            "grid opacity",
            edited(|r| r["media"][0]["battlemap_grid"]["opacity"] = json!(1.5)),
        ),
        (
            "sound volume",
            edited(|r| r["sounds"][0]["volume"] = json!(1.5)),
        ),
        (
            "master volume",
            edited(|r| r["soundMasterVolume"] = json!(1.2)),
        ),
        ("kind", edited(|r| r["kind"] = json!("saga"))),
        (
            "hex size",
            edited(|r| r["media"][0]["battlemap_grid"]["hex_size_px"] = json!(0)),
        ),
        // A value the table cannot hold: a number too large, an id that is
        // none, a section that is no list.
        (
            "volume overflow",
            edited(|r| r["sounds"][0]["volume"] = json!(12345.5)),
        ),
        (
            "no uuid",
            edited(|r| r["players"][0]["id"] = json!("pas un uuid")),
        ),
        ("no list", edited(|r| r["players"] = json!("Marc"))),
        // A NOT NULL, a foreign key.
        ("nameless", edited(|r| r["name"] = Value::Null)),
        (
            "session without campaign",
            edited(|r| {
                r["sessions"][0]["campaign_id"] = json!("5e000000-0000-4000-8000-0000000000ff")
            }),
        ),
        // What the tables judge at the end of the copy: a PJ's player at
        // another campaign's table, an element without a version, a
        // battlemap without its grid.
        (
            "player outside the campaign",
            edited(|r| r["elements"][0]["character"]["player_id"] = json!(CLAIRE)),
        ),
        (
            "battlemap without grid",
            edited(|r| r["media"][0]["battlemap_grid"] = Value::Null),
        ),
    ];
    for (what, row) in refused {
        let result = served
            .applied("campaign.importCampaign", row, Some(COPY))
            .await;
        assert_eq!(result.status, State::Rejected, "{what}: {result:?}");
        assert_same(&tables(&scratch).await, &before, what);
    }

    let versionless = edited(|r| r["elements"][1]["versions"] = json!([]));
    assert_eq!(imported(&served, &versionless).await, State::Rejected);
    assert_eq!(
        tables(&scratch).await,
        before,
        "an element without a version"
    );

    // None of it kept the copy from being written afterwards.
    assert_eq!(imported(&served, &copy()).await, State::Applied);
}
