//! `campaign.importCampaign` over PostgreSQL (`DATABASE_URL`), as the server
//! plants it beside Campaign, whose projection writes its tables: each test
//! works in a scratch database of its own, migrated as the server migrates it,
//! dropped when it ends (`platform_test_kit::Scratch`). What the tables hold
//! after an import: a separate copy of every row the archive carries, under
//! new ids, every reference the copy's own; the original's rows, byte for
//! byte, as they were; the same archive twice, two copies.

use std::collections::BTreeMap;
use std::sync::Arc;

use harness_dataguard::command::State;
use harness_dataguard::plugins::RefusalKind;
use harness_dataguard::store::RandomIds;
use platform_test_kit::{Bench, Scratch, Served, file, refused};
use serde_json::{Value, json};

/// The campaign the archive was made from, its player and its PJ: they exist
/// in the database the import runs in, under the very ids the archive holds.
const ORIGINAL: &str = "c0000001-0000-4000-8000-000000000001";
const CAMILLE: &str = "a0000001-0000-4000-8000-000000000001";
const KAEL: &str = "e0000001-0000-4000-8000-000000000001";
const EXPORTED: &str = "2026-09-01T18:00:00+00:00";

/// The product's tables, each as its rows' JSON.
type Tables = BTreeMap<String, Vec<Value>>;

/// A database migrated as the server migrates it, holding the original.
async fn scratch(label: &str) -> Scratch {
    let scratch = Scratch::new(label).await;
    scratch
        .seed(&format!(
            "INSERT INTO campaign (id, name, kind, pitch, last_exported_at)
                 VALUES ('{ORIGINAL}', 'Les cendres de Valombre', 'campagne', 'Une vallée qui brûle encore.', '{EXPORTED}');
             INSERT INTO player (id, campaign_id, name, notes)
                 VALUES ('{CAMILLE}', '{ORIGINAL}', 'Camille', 'Joue le mardi.');
             INSERT INTO entity (id, campaign_id, type, name)
                 VALUES ('{KAEL}', '{ORIGINAL}', 'pj', 'Kael');
             INSERT INTO entity_version (entity_id, version_number, source, snapshot, diff)
                 VALUES ('{KAEL}', 1, 'dm', '{{}}', '[]');
             INSERT INTO character (entity_id, player_id, token_color) VALUES ('{KAEL}', '{CAMILLE}', '#c0392b');"
        ))
        .await;
    scratch
}

/// The engine as the server plants this unit and its aggregate over
/// `scratch`'s tables, the ids drawn at random as in the product.
async fn served(scratch: &Scratch) -> Served {
    Bench::new(
        include_str!("../data-capability.json"),
        &file("crates/campaign/aggregates/Campaign/aggregate.json"),
        [aggregate_campaign::plugin(), import_campaign::plugin()],
    )
    .ids(Arc::new(RandomIds))
    .postgres(scratch)
    .await
}

/// The Concept's case whose name starts with `name`: the archive, and the
/// copy the Concept gives it.
fn case(name: &str) -> (Value, Value) {
    let case = platform_conformance::fixtures_of("CampaignArchive@1")
        .cases
        .into_iter()
        .find(|case| case.name.starts_with(name))
        .unwrap_or_else(|| panic!("no case starting with {name:?}"));
    (case.input["archive"].clone(), case.output)
}

/// An import queued then settled: the id of the campaign it made, and what it
/// became.
async fn imported(served: &Served, archive: &Value) -> (String, State) {
    let submitted = served
        .submit(
            "campaign.importCampaign",
            json!({ "archive": archive }),
            None,
        )
        .await
        .expect("queued");
    let mut settled = served.settled().await;
    assert_eq!(settled.len(), 1, "one command settled: {settled:?}");
    let id = submitted
        .partition
        .trim_start_matches("Campaign/")
        .to_owned();
    (id, settled.remove(0).status)
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

/// The rows of `list` (a member of the copy's rows), at any depth below
/// `rows`.
fn count_of(rows: &Value, member: &str) -> usize {
    rows.as_array().map_or(0, |rows| {
        rows.iter()
            .map(|row| row[member].as_array().map_or(0, Vec::len))
            .sum()
    })
}

/// How many rows of the copy `output` the tables must hold more, by table.
fn expected(output: &Value) -> BTreeMap<String, usize> {
    let rows = |section: &str| output[section].as_array().map_or(0, Vec::len);
    let elements = &output["elements"];
    let media = &output["media"];
    let sessions = &output["sessions"];
    let scenes: Vec<Value> = sessions
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|session| session["scenes"].as_array().cloned().unwrap_or_default())
        .collect();
    let scenes = Value::Array(scenes);
    [
        ("campaign", 1),
        ("player", rows("players")),
        ("entity", rows("elements")),
        (
            "character",
            elements
                .as_array()
                .into_iter()
                .flatten()
                .filter(|e| !e["character"].is_null())
                .count(),
        ),
        ("entity_alias", count_of(elements, "aliases")),
        ("entity_version", count_of(elements, "versions")),
        ("media", rows("media")),
        (
            "battlemap_grid",
            media
                .as_array()
                .into_iter()
                .flatten()
                .filter(|m| !m["battlemap_grid"].is_null())
                .count(),
        ),
        ("sound", rows("sounds")),
        ("session", rows("sessions")),
        (
            "session_present_character",
            count_of(sessions, "session_present_character"),
        ),
        ("scene", count_of(sessions, "scenes")),
        ("scene_entity", count_of(&scenes, "scene_entity")),
        ("scene_media", count_of(&scenes, "scene_media")),
        ("session_journal_line", count_of(sessions, "journal_lines")),
    ]
    .into_iter()
    .filter(|(_, count)| *count > 0)
    .map(|(name, count)| (name.to_owned(), count))
    .collect()
}

/// How many rows of the copy `campaign` made reach a row of another
/// campaign, through each reference the copy holds.
const REACHING_OUT: &str = "SELECT count(*) FROM (
    SELECT 1 FROM character ch JOIN entity e ON e.id = ch.entity_id
        JOIN player p ON p.id = ch.player_id WHERE e.campaign_id = $1 AND p.campaign_id <> $1
    UNION ALL SELECT 1 FROM character ch JOIN entity e ON e.id = ch.entity_id
        JOIN media m ON m.id IN (ch.portrait_media_id, ch.token_media_id)
        WHERE e.campaign_id = $1 AND m.campaign_id <> $1
    UNION ALL SELECT 1 FROM campaign c JOIN media m ON m.id = c.cover_media_id
        WHERE c.id = $1 AND m.campaign_id <> $1
    UNION ALL SELECT 1 FROM media m JOIN entity e ON e.id = m.place_id
        WHERE m.campaign_id = $1 AND e.campaign_id <> $1
    UNION ALL SELECT 1 FROM entity_version v JOIN entity e ON e.id = v.entity_id
        JOIN entity_version r ON r.id = v.restored_from_version_id
        WHERE e.campaign_id = $1 AND r.entity_id <> v.entity_id
    UNION ALL SELECT 1 FROM session_present_character pc JOIN session s ON s.id = pc.session_id
        JOIN entity e ON e.id = pc.character_id WHERE s.campaign_id = $1 AND e.campaign_id <> $1
    UNION ALL SELECT 1 FROM scene_entity se JOIN scene sc ON sc.id = se.scene_id
        JOIN session s ON s.id = sc.session_id JOIN entity e ON e.id = se.entity_id
        WHERE s.campaign_id = $1 AND e.campaign_id <> $1
    UNION ALL SELECT 1 FROM scene_media sm JOIN scene sc ON sc.id = sm.scene_id
        JOIN session s ON s.id = sc.session_id JOIN media m ON m.id = sm.media_id
        WHERE s.campaign_id = $1 AND m.campaign_id <> $1
    UNION ALL SELECT 1 FROM session_journal_line j JOIN session s ON s.id = j.session_id
        JOIN scene sc ON sc.id = j.scene_id WHERE s.campaign_id = $1 AND sc.session_id <> j.session_id
) reaching";

/// The count a query answers about the campaign `id`, its `$1`.
async fn count(scratch: &Scratch, sql: &str, id: &str) -> i64 {
    scratch
        .client()
        .query_one(&sql.replace("$1", "$1::text::uuid"), &[&id])
        .await
        .expect("a count")
        .get(0)
}

#[tokio::test]
async fn import_creates_copy_every_row_of_the_archive_reaches_the_tables_under_new_ids() {
    let scratch = scratch("import_campaign_full").await;
    let served = served(&scratch).await;
    let (archive, output) = case("a full archive");
    let before = tables(&scratch).await;

    let (copy, status) = imported(&served, &archive).await;
    assert_eq!(status, State::Applied);

    let after = tables(&scratch).await;
    assert_eq!(added(&before, &after), expected(&output), "the copy's rows");
    assert!(
        untouched(&before, &after),
        "the original's rows are as they were"
    );
    assert_eq!(
        count(
            &scratch,
            "SELECT count(*) FROM campaign WHERE id <> $1",
            &copy
        )
        .await,
        1,
        "one campaign more: the original"
    );

    // Every reference points at the copy's rows, never the original's.
    assert_eq!(count(&scratch, REACHING_OUT, &copy).await, 0);
    for (table, column) in [
        ("player", "campaign_id"),
        ("entity", "campaign_id"),
        ("media", "campaign_id"),
        ("sound", "campaign_id"),
        ("session", "campaign_id"),
    ] {
        let sql = format!(
            "SELECT count(*) FROM {table} WHERE {column} = $1 AND id IN (SELECT id FROM {table} WHERE {column} = '{ORIGINAL}')"
        );
        assert_eq!(
            count(&scratch, &sql, &copy).await,
            0,
            "{table}: no id of the original's"
        );
    }

    // The copy's campaign is the archive's, stamped as imported from it and
    // never exported.
    let campaign = scratch
        .rows(
            "SELECT id::text, name, kind, glyph, origin_campaign_id::text AS origin,
                 imported_at IS NOT NULL AS imported, last_exported_at IS NULL AS never_exported,
                 sound_master_volume::float8 AS volume, cover_media_id IS NULL AS no_cover
             FROM campaign WHERE id::text = $1",
            &[json!(copy)],
        )
        .await;
    assert_eq!(campaign.len(), 1);
    assert_eq!(campaign[0]["name"], json!("Les cendres de Valombre"));
    assert_eq!(campaign[0]["origin"], json!(ORIGINAL));
    assert_ne!(
        campaign[0]["id"], campaign[0]["origin"],
        "a new id, though the original exists"
    );
    assert_eq!(campaign[0]["imported"], json!(true));
    assert_eq!(campaign[0]["never_exported"], json!(true));
    assert_eq!(campaign[0]["volume"], json!(0.8));
    assert_eq!(
        campaign[0]["no_cover"],
        json!(false),
        "the cover follows to the copy's medium"
    );
}

#[tokio::test]
async fn import_creates_copy_the_original_keeps_its_rows_and_its_last_export() {
    let scratch = scratch("import_campaign_original").await;
    let served = served(&scratch).await;
    let (archive, _) = case("a full archive");
    let before = tables(&scratch).await;

    let (copy, status) = imported(&served, &archive).await;
    assert_eq!(status, State::Applied);

    let original = scratch
        .rows(
            "SELECT name, kind, pitch, (last_exported_at = $2::text::timestamptz) AS last_export_kept,
                 (SELECT count(*) FROM player WHERE campaign_id = c.id) AS players,
                 (SELECT count(*) FROM entity WHERE campaign_id = c.id) AS elements
             FROM campaign c WHERE c.id::text = $1",
            &[json!(ORIGINAL), json!(EXPORTED)],
        )
        .await;
    assert_eq!(original.len(), 1, "the original is there");
    assert_eq!(original[0]["last_export_kept"], json!(true));
    assert_eq!(original[0]["players"], json!(1));
    assert_eq!(original[0]["elements"], json!(1));
    assert!(untouched(&before, &tables(&scratch).await));

    let campaigns = scratch
        .rows("SELECT id::text FROM campaign ORDER BY 1", &[])
        .await;
    assert_eq!(campaigns.len(), 2, "two campaigns side by side");
    assert!(campaigns.iter().any(|c| c["id"] == json!(copy)));
}

#[tokio::test]
async fn import_creates_copy_the_same_archive_twice_makes_two_separate_copies() {
    let scratch = scratch("import_campaign_twice").await;
    let served = served(&scratch).await;
    let (archive, output) = case("a full archive");
    let before = tables(&scratch).await;

    let (first, status) = imported(&served, &archive).await;
    assert_eq!(status, State::Applied);
    let between = tables(&scratch).await;
    let (second, status) = imported(&served, &archive).await;
    assert_eq!(status, State::Applied);
    let after = tables(&scratch).await;

    assert_ne!(first, second);
    assert_eq!(added(&before, &between), expected(&output));
    assert_eq!(
        added(&between, &after),
        expected(&output),
        "the second copy has every row too"
    );
    assert!(untouched(&between, &after), "the first copy is as it was");
    assert_eq!(count(&scratch, REACHING_OUT, &second).await, 0);
    assert_eq!(count(&scratch, REACHING_OUT, &first).await, 0);
}

#[tokio::test]
async fn an_archive_without_media_sounds_or_sessions_copies_its_campaign_players_and_elements_only()
{
    let scratch = scratch("import_campaign_light").await;
    let served = served(&scratch).await;
    let (archive, output) = case("an archive exported before");
    let before = tables(&scratch).await;

    let (_, status) = imported(&served, &archive).await;
    assert_eq!(status, State::Applied);

    let after = tables(&scratch).await;
    let added = added(&before, &after);
    assert_eq!(added, expected(&output));
    for table in [
        "media",
        "battlemap_grid",
        "sound",
        "session",
        "scene",
        "session_journal_line",
    ] {
        assert!(!added.contains_key(table), "{table}: nothing");
    }
    assert!(added.contains_key("player") && added.contains_key("entity"));
}

#[tokio::test]
async fn an_archive_that_is_none_writes_nothing() {
    let scratch = scratch("import_campaign_refused").await;
    let served = served(&scratch).await;
    let (archive, _) = case("a full archive");
    let before = tables(&scratch).await;

    for (member, value) in [("format", json!("campaign-export")), ("version", json!(2))] {
        let mut wrong = archive.clone();
        wrong[member] = value;
        let message = refused(
            served
                .submit("campaign.importCampaign", json!({ "archive": wrong }), None)
                .await,
            RefusalKind::Unprocessable,
        );
        assert!(message.contains(member), "{member}: {message}");
    }
    assert!(served.settled().await.is_empty(), "nothing was queued");
    assert_eq!(tables(&scratch).await, before, "no row was written");
}
