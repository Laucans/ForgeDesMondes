//! The campaign's sounds (`mock/fixtures/sounds.json`), loaded with the other
//! fixtures and read back through `soundbox.listSounds` over PostgreSQL
//! (`DATABASE_URL`): what the rules' runtime serves to the soundbox. The
//! database is a `ScratchDb`, dropped when the test ends.

mod common;

use std::collections::BTreeSet;
use std::path::Path;

use common::Reads;
use platform_scratch_db::ScratchDb;
use serde_json::{Value, json};
use server::{fixtures, id as uuid_of};

fn fixtures_dir() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../mock/fixtures")
}

/// The names of the sounds `answer` lists, in order.
fn names(answer: &Value) -> Vec<String> {
    answer["sounds"]
        .as_array()
        .expect("sounds")
        .iter()
        .map(|sound| sound["name"].as_str().expect("a name").to_string())
        .collect()
}

// AC1: the soundbox lists the sounds of sounds.json with their tags and volumes (R16)
// AC2: the name search and the tag filter keep the sounds they name (R110)
// AC3: a restart loads no sound twice, and leaves a sound the DM changed alone
#[tokio::test]
async fn the_campaign_s_sounds_are_loaded_listed_filtered_and_loaded_once() {
    let scratch = ScratchDb::create("sound_fixtures")
        .await
        .expect("a scratch database");
    let mut client = scratch.connect().await.expect("a connection");
    let migrations = Path::new(env!("CARGO_MANIFEST_DIR")).join("../data/migrations");
    data::migrate(&mut client, &migrations)
        .await
        .expect("migrated");
    let plan = fixtures(&fixtures_dir()).expect("the fixtures");
    data::insert_missing(&mut client, &plan.tables)
        .await
        .expect("the fixtures are inserted");

    let told: Value = serde_json::from_str(
        &std::fs::read_to_string(fixtures_dir().join("sounds.json")).expect("the fixture"),
    )
    .expect("JSON");
    let told = told["sounds"].as_array().expect("sounds");
    let campaign = uuid_of("campaign", "cendres-valombre");
    let reads = Reads::open(scratch.url()).await.expect("the reads open");
    let list = |variables: Value| {
        let reads = &reads;
        async move {
            reads
                .run("soundbox.listSounds", &variables)
                .await
                .expect("an answer")
        }
    };

    // AC1: every sound of the file, its tags and its volume, under the campaign's master.
    let all = list(json!({ "campaign": campaign })).await;
    assert_eq!(all["masterVolume"], 80, "{all}");
    let listed = all["sounds"].as_array().expect("sounds");
    assert_eq!(listed.len(), told.len(), "{all}");
    for sound in told {
        let slug = sound["id"].as_str().expect("a slug");
        let row = listed
            .iter()
            .find(|row| row["id"] == uuid_of("sound", slug))
            .unwrap_or_else(|| panic!("{slug} is not listed: {all}"));
        assert_eq!(row["name"], sound["name"], "{slug}");
        assert_eq!(row["tags"], sound["tags"], "{slug}");
        assert_eq!(row["volume"], sound["volume"], "{slug}");
        assert_eq!(row["file"], sound["file"], "{slug}");
        assert_eq!(row["playing"], false, "{slug}");
    }
    let door = listed
        .iter()
        .find(|row| row["name"] == "Porte qui grince")
        .expect("the door");
    assert_eq!(door["tags"], json!(["porte", "bois", "intérieur"]));
    assert_eq!(door["volume"], 80);
    let crowd = listed
        .iter()
        .find(|row| row["name"] == "Bruit de foule en taverne")
        .expect("the crowd");
    assert_eq!(crowd["tags"], json!(["foule", "taverne", "intérieur"]));
    assert_eq!(crowd["volume"], 55);
    // The effective volume is the sound's times the master's: 55 % of 80 %.
    assert!((crowd["gain"].as_f64().expect("a gain") - 0.44).abs() < 1e-9);
    assert_eq!(names(&all)[0], "Porte qui grince", "the file's order");

    // AC2: « gri » keeps the door alone; the tag « taverne », the two taverne sounds.
    let searched = list(json!({ "campaign": campaign, "search": "gri" })).await;
    assert_eq!(names(&searched), ["Porte qui grince"], "{searched}");
    let tavern = list(json!({ "campaign": campaign, "tag": "taverne" })).await;
    assert_eq!(
        names(&tavern),
        ["Bruit de foule en taverne", "Chope qui se brise"],
        "{tavern}"
    );

    // AC3: a restart plans the same rows and loads none of them twice; a sound
    // the DM changed since keeps what the DM made of it.
    let ids = |answer: &Value| -> BTreeSet<String> {
        answer["sounds"]
            .as_array()
            .expect("sounds")
            .iter()
            .map(|sound| sound["id"].to_string())
            .collect()
    };
    let before = ids(&all);
    client
        .batch_execute(&format!(
            "UPDATE sound SET volume = 0.3, name = 'Porte du DM' WHERE id = '{}'",
            uuid_of("sound", "snd-porte")
        ))
        .await
        .expect("an update");
    client
        .batch_execute(&format!(
            "UPDATE campaign SET sound_master_volume = 0.5 WHERE id = '{campaign}'"
        ))
        .await
        .expect("an update");
    let again = fixtures(&fixtures_dir()).expect("the fixtures");
    assert_eq!(
        data::insert_missing(&mut client, &again.tables)
            .await
            .expect("the fixtures are planned again"),
        0,
        "a restart loads nothing"
    );
    let restarted = list(json!({ "campaign": campaign })).await;
    assert_eq!(ids(&restarted), before, "no sound twice, none lost");
    assert_eq!(
        restarted["sounds"].as_array().map(Vec::len),
        Some(told.len())
    );
    assert_eq!(
        restarted["masterVolume"], 50,
        "the DM's master volume stays"
    );
    let changed = restarted["sounds"]
        .as_array()
        .expect("sounds")
        .iter()
        .find(|row| row["id"] == uuid_of("sound", "snd-porte"))
        .expect("the door");
    assert_eq!(changed["name"], "Porte du DM");
    assert_eq!(changed["volume"], 30);

    // A campaign no soundbox describes lists nothing, under the default master.
    let quiet = list(json!({ "campaign": uuid_of("campaign", "nuit-carnaval") })).await;
    assert_eq!(quiet, json!({ "masterVolume": 100, "sounds": [] }));

    drop(reads);
    drop(client);
}
