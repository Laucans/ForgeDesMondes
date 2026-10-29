//! The tokens of the mock-up's instance (`table.getBattlemapInstance.json`),
//! loaded into a migrated database (`DATABASE_URL`) with the marker of each:
//! `token.character_type` is the type of its character's entity (`pj` or
//! `pnj`), and a PC's token that carries hit points leaves the file skipped
//! while the rest loads (`pj-hp-not-tracked`). The database is a `ScratchDb`,
//! dropped when the test ends.

use std::path::{Path, PathBuf};

use platform_scratch_db::ScratchDb;
use serde_json::Value;
use server::fixtures;

const INSTANCE_FILE: &str = "table.getBattlemapInstance.json";

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../mock/fixtures")
}

/// A folder removed when it goes out of scope, a panic included.
struct Folder(PathBuf);

impl Drop for Folder {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A migrated scratch database.
async fn migrated(label: &str) -> (ScratchDb, data::Client) {
    let scratch = ScratchDb::create(label).await.expect("a scratch database");
    let mut client = scratch.connect().await.expect("a connection");
    let migrations = Path::new(env!("CARGO_MANIFEST_DIR")).join("../data/migrations");
    data::migrate(&mut client, &migrations)
        .await
        .expect("migrated");
    (scratch, client)
}

// AC1: every token row has character_type equal to its character's type and the load succeeds
#[tokio::test]
async fn every_token_is_loaded_marked_with_the_type_of_its_character() {
    let (_scratch, mut client) = migrated("fixture_token_character_type").await;
    let plan = fixtures(&fixtures_dir()).expect("the fixtures");
    assert!(
        plan.loaded.iter().any(|file| file == INSTANCE_FILE),
        "the instance is loaded: {:?}",
        plan.skipped
    );
    data::insert_missing(&mut client, &plan.tables)
        .await
        .expect("the fixtures are inserted");
    let rows = client
        .query(
            "SELECT t.id::text, t.character_type, e.type FROM token t \
             JOIN entity e ON e.id = t.character_id",
            &[],
        )
        .await
        .expect("the tokens");
    let planned = plan
        .tables
        .iter()
        .find(|table| table.name == "token")
        .map_or(0, |table| table.rows.len());
    assert!(planned > 0, "the instance plans tokens");
    assert_eq!(rows.len(), planned, "every token is loaded");
    for row in &rows {
        let (token, marker, kind): (String, String, String) = (row.get(0), row.get(1), row.get(2));
        assert_eq!(marker, kind, "the marker of token {token}");
    }
    let pcs = rows
        .iter()
        .filter(|row| row.get::<_, String>(1) == "pj")
        .count();
    assert!(
        pcs > 0 && pcs < rows.len(),
        "the instance holds PCs and NPCs"
    );
}

// AC2: a PC's token that carries HP skips the file and the rest loads (pj-hp-not-tracked)
#[tokio::test]
async fn a_pc_token_with_hit_points_skips_the_instance_and_the_rest_loads() {
    let (_scratch, mut client) = migrated("fixture_token_pc_with_hp").await;
    let folder = Folder(
        std::env::temp_dir().join(format!("fixture_token_pc_with_hp_{}", std::process::id())),
    );
    let _ = std::fs::remove_dir_all(&folder.0);
    std::fs::create_dir_all(&folder.0).expect("a folder");
    for entry in std::fs::read_dir(fixtures_dir()).expect("the fixtures") {
        let path = entry.expect("an entry").path();
        if path.extension().is_some_and(|e| e == "json") {
            std::fs::copy(&path, folder.0.join(path.file_name().expect("a name"))).expect("a copy");
        }
    }
    let path = folder.0.join(INSTANCE_FILE);
    let mut file: Value =
        serde_json::from_str(&std::fs::read_to_string(&path).expect("the instance")).expect("JSON");
    let pc = file["state"]["tokens"]
        .as_array_mut()
        .expect("tokens")
        .iter_mut()
        .find(|token| token["kind"] == "pj")
        .expect("a PC's token");
    pc["hp"] = 10.into();
    pc["hpMax"] = 10.into();
    std::fs::write(&path, file.to_string()).expect("the edited instance");

    let plan = fixtures(&folder.0).expect("the fixtures");
    assert!(
        plan.skipped
            .iter()
            .any(|(name, why)| name == INSTANCE_FILE && why.contains("a number or hit points")),
        "{:?}",
        plan.skipped
    );
    data::insert_missing(&mut client, &plan.tables)
        .await
        .expect("the rest is inserted");
    let count = |table: &'static str| {
        let client = &client;
        async move {
            client
                .query_one(&format!("SELECT count(*) FROM {table}"), &[])
                .await
                .expect("a count")
                .get::<_, i64>(0)
        }
    };
    assert_eq!(count("token").await, 0, "no token of the skipped file");
    assert_eq!(count("battlemap_instance").await, 0);
    assert!(count("campaign").await > 0, "the rest loads");
    assert!(count("scene").await > 0, "the rest loads");
}
