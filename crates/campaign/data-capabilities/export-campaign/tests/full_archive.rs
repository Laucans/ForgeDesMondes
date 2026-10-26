//! campaign.exportCampaign over PostgreSQL, whole: the archive holds every
//! section `CampaignArchive@1` names — the media with their battlemap grids,
//! the sounds, the sessions with their present characters, their scenes
//! (and the scenes' elements and media) and their journal — each in a stable
//! order, an empty list for a campaign with none, and the export, applied,
//! changes the date of the last export and nothing else (`export-stamps-date`).

use platform_test_kit::{Bench, Scratch, Served, uuid};
use serde_json::{Value, json};

fn campaign(n: u64) -> String {
    uuid(0xc1, n)
}

fn element(n: u64) -> String {
    uuid(0xe1, n)
}

fn medium(n: u64) -> String {
    uuid(0xd1, n)
}

fn session(n: u64) -> String {
    uuid(0x51, n)
}

fn scene(n: u64) -> String {
    uuid(0x5c, n)
}

/// Valombre, with a place and a PC, a battlemap with its grid and an
/// illustration, two sounds, two sessions (the first with a present PC, two
/// scenes — one with its place and its battlemap — and a journal line), and
/// Brume, a campaign with none of these. Rows are written newest first: the
/// archive's order is its own.
async fn seeded() -> Scratch {
    let scratch = Scratch::new("export_full_archive").await;
    let (valombre, brume) = (campaign(1), campaign(2));
    let (pc, tower) = (element(1), element(2));
    let (map, picture) = (medium(1), medium(2));
    let (first, second) = (session(1), session(2));
    let (cellar, hall) = (scene(1), scene(2));
    scratch
        .seed(&format!(
            "INSERT INTO campaign (id, name, kind, sound_master_volume)
                 VALUES ('{valombre}', 'Valombre', 'campagne', 0.8),
                        ('{brume}', 'Brume', 'oneshot', 1);
             INSERT INTO entity (id, campaign_id, type, name, created_at)
                 VALUES ('{pc}', '{valombre}', 'pj', 'Elowen', '2026-10-01T00:00:00Z'),
                        ('{tower}', '{valombre}', 'lieu', 'La Tour', '2026-10-02T00:00:00Z');
             INSERT INTO character (entity_id, hp_max, token_color) VALUES ('{pc}', 44, 'success');
             INSERT INTO entity_version (entity_id, version_number, source, snapshot, diff)
                 VALUES ('{pc}', 1, 'dm', '{{}}', '[]'), ('{tower}', 1, 'dm', '{{}}', '[]');
             INSERT INTO media (id, campaign_id, kind, title, file_path, width_px, height_px, place_id, uploaded_at)
                 VALUES ('{picture}', '{valombre}', 'illustration', 'Portrait', 'media/portrait.png', 400, 600, NULL, '2026-10-04T00:00:00Z'),
                        ('{map}', '{valombre}', 'battlemap', 'La Tour, niveau 1', 'media/tour.png', 1920, 1080, '{tower}', '2026-10-03T00:00:00Z');
             INSERT INTO battlemap_grid (media_id, hex_size_px, offset_x, offset_y, orientation, color, opacity)
                 VALUES ('{map}', 48, 3.5, 0, 'pointe', '#ffffff', 0.4);
             INSERT INTO sound (id, campaign_id, name, file_path, tags, volume, created_at)
                 VALUES ('{s2}', '{valombre}', 'Vent', 'sounds/vent.mp3', '{{}}', 1, '2026-10-06T00:00:00Z'),
                        ('{s1}', '{valombre}', 'Pluie', 'sounds/pluie.mp3', '{{meteo,calme}}', 0.5, '2026-10-05T00:00:00Z');
             INSERT INTO session (id, campaign_id, position, title, status, microphone_on)
                 VALUES ('{second}', '{valombre}', 2, 'Le bal', 'prep', false),
                        ('{first}', '{valombre}', 1, 'La Tour', 'closed', false);
             INSERT INTO session_present_character (session_id, character_id) VALUES ('{first}', '{pc}');
             INSERT INTO scene (id, session_id, position, title) VALUES
                 ('{hall}', '{first}', 2, 'Le hall'), ('{cellar}', '{first}', 1, 'La cave');
             INSERT INTO scene_entity (scene_id, entity_id, position) VALUES ('{cellar}', '{tower}', 0);
             INSERT INTO scene_media (scene_id, media_id) VALUES ('{cellar}', '{map}');
             INSERT INTO session_journal_line (id, session_id, scene_id, spoken_at, text) VALUES
                 ('{l1}', '{first}', '{cellar}', '2026-10-07T20:00:00Z', 'La porte cède.');",
            s1 = uuid(0xa1, 1),
            s2 = uuid(0xa1, 2),
            l1 = uuid(0x1a, 1),
        ))
        .await;
    scratch
}

/// The engine as the server plants the export, over `scratch`.
async fn bench(scratch: &Scratch) -> Served {
    Bench::new(
        include_str!("../data-capability.json"),
        aggregate_campaign::AGGREGATE_JSON,
        [aggregate_campaign::plugin(), export_campaign::plugin()],
    )
    .postgres(scratch)
    .await
}

/// The archive `campaign` exports.
async fn exported(bench: &Served, campaign: &str) -> Value {
    bench
        .applied("campaign.exportCampaign@1", json!({ "id": campaign }), None)
        .await
        .attachment
        .expect("the archive")["content"]
        .clone()
}

/// Every row of the product's tables, `campaign.last_exported_at` aside.
async fn tables_but_the_stamp(scratch: &Scratch) -> Vec<(String, Vec<Value>)> {
    let names = scratch
        .rows(
            "SELECT tablename::text AS name FROM pg_tables
             WHERE schemaname = 'public' AND tablename NOT LIKE 'dataguard%'
             ORDER BY tablename",
            &[],
        )
        .await;
    let mut tables = Vec::new();
    for name in names {
        let name = name["name"].as_str().expect("a table's name").to_owned();
        let mut rows = scratch
            .rows(
                &format!("SELECT to_jsonb(t) AS row FROM {name} t ORDER BY to_jsonb(t)::text"),
                &[],
            )
            .await;
        for row in &mut rows {
            if let Some(campaign) = row["row"].as_object_mut().filter(|_| name == "campaign") {
                campaign.remove("last_exported_at");
            }
        }
        tables.push((name, rows));
    }
    tables
}

#[tokio::test]
async fn the_archive_holds_the_media_sounds_and_sessions_under_the_names_of_the_concept() {
    let scratch = seeded().await;
    let bench = bench(&scratch).await;
    let content = exported(&bench, &campaign(1)).await;

    let media = content["media"].as_array().expect("media");
    let titles: Vec<&Value> = media.iter().map(|m| &m["title"]).collect();
    assert_eq!(titles, ["La Tour, niveau 1", "Portrait"], "oldest first");
    assert_eq!(media[0]["id"], medium(1));
    assert_eq!(media[0]["kind"], "battlemap");
    assert_eq!(media[0]["file_path"], "media/tour.png");
    assert_eq!(media[0]["place_id"], element(2));
    let grid = &media[0]["battlemap_grid"];
    assert_eq!(grid["media_id"], medium(1));
    assert_eq!(grid["hex_size_px"], 48.0);
    assert_eq!(grid["offset_x"], 3.5);
    assert_eq!(grid["orientation"], "pointe");
    assert_eq!(grid["feet_per_hex"], 5);
    assert_eq!(media[1]["battlemap_grid"], Value::Null);

    let sounds = content["sounds"].as_array().expect("sounds");
    let names: Vec<&Value> = sounds.iter().map(|s| &s["name"]).collect();
    assert_eq!(names, ["Pluie", "Vent"], "oldest first");
    assert_eq!(sounds[0]["file_path"], "sounds/pluie.mp3");
    assert_eq!(sounds[0]["tags"], json!(["meteo", "calme"]));
    assert_eq!(sounds[0]["volume"], 0.5);

    let sessions = content["sessions"].as_array().expect("sessions");
    let titles: Vec<&Value> = sessions.iter().map(|s| &s["title"]).collect();
    assert_eq!(titles, ["La Tour", "Le bal"], "by position");
    let first = &sessions[0];
    assert_eq!(first["id"], session(1));
    assert_eq!(first["status"], "closed");
    let present = first["session_present_character"]
        .as_array()
        .expect("present");
    assert_eq!(present.len(), 1);
    assert_eq!(present[0]["session_id"], session(1));
    assert_eq!(present[0]["character_id"], element(1));
    let scenes = first["scenes"].as_array().expect("scenes");
    let titles: Vec<&Value> = scenes.iter().map(|s| &s["title"]).collect();
    assert_eq!(titles, ["La cave", "Le hall"], "by position");
    assert_eq!(
        scenes[0]["scene_entity"],
        json!([{ "scene_id": scene(1), "entity_id": element(2), "position": 0 }])
    );
    assert_eq!(
        scenes[0]["scene_media"],
        json!([{ "scene_id": scene(1), "media_id": medium(1) }])
    );
    assert_eq!(scenes[1]["scene_entity"], json!([]));
    assert_eq!(scenes[1]["scene_media"], json!([]));
    let journal = first["journal_lines"].as_array().expect("journal");
    assert_eq!(journal.len(), 1);
    assert_eq!(journal[0]["scene_id"], scene(1));
    assert_eq!(journal[0]["text"], "La porte cède.");

    let second = &sessions[1];
    assert_eq!(second["session_present_character"], json!([]));
    assert_eq!(second["scenes"], json!([]));
    assert_eq!(second["journal_lines"], json!([]));
}

#[tokio::test]
async fn a_campaign_with_no_media_sound_or_session_exports_them_as_empty_lists() {
    let scratch = seeded().await;
    let bench = bench(&scratch).await;
    let content = exported(&bench, &campaign(2)).await;
    for section in ["players", "elements", "media", "sounds", "sessions"] {
        assert_eq!(content[section], json!([]), "{section}");
    }
}

#[tokio::test]
async fn an_applied_export_changes_the_date_of_the_last_export_and_no_other_row() {
    let scratch = seeded().await;
    let bench = bench(&scratch).await;
    let before = tables_but_the_stamp(&scratch).await;
    let stamped = scratch
        .rows(
            "SELECT last_exported_at IS NULL AS never FROM campaign WHERE id::text = $1",
            &[json!(campaign(1))],
        )
        .await;
    assert_eq!(stamped[0]["never"], true);

    exported(&bench, &campaign(1)).await;

    assert_eq!(tables_but_the_stamp(&scratch).await, before);
    let stamped = scratch
        .rows(
            "SELECT last_exported_at IS NOT NULL AS stamped FROM campaign WHERE id::text = $1",
            &[json!(campaign(1))],
        )
        .await;
    assert_eq!(stamped[0]["stamped"], true);
}
