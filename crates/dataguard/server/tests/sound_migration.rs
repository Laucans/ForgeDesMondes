//! The Sound migration (`crates/data/migrations/0007_sound.sql`) against the
//! harness's PostgreSQL (`DATABASE_URL`): replayed over the campaign table it
//! needs, it is what `data/model.json` says it is, a campaign's deletion
//! takes its sounds with it, and the table refuses a volume out of range or
//! tags not normalized under the invariant's own id. Each test works in a
//! scratch database of its own, dropped when it ends.

use serde_json::Value;
use tokio_postgres::error::SqlState;
use tokio_postgres::{Client, NoTls};

const CAMPAIGN: &str = include_str!("../../../data/migrations/0002_campaign.sql");
const MIGRATION: &str = include_str!("../../../data/migrations/0007_sound.sql");

const VALOMBRE: &str = "c1000000-0000-4000-8000-000000000001";
const BRUME: &str = "c1000000-0000-4000-8000-000000000002";

fn url() -> String {
    std::env::var("DATABASE_URL").expect("DATABASE_URL names the harness's PostgreSQL")
}

async fn connect(config: &tokio_postgres::Config) -> Client {
    let (client, connection) = config.connect(NoTls).await.expect("PostgreSQL");
    tokio::spawn(connection);
    client
}

/// A database of its own, holding the campaign table, migrated once, with
/// Valombre and Brume and sounds in each.
struct Scratch {
    client: Client,
    name: String,
}

impl Scratch {
    async fn new(name: &str) -> Scratch {
        let name = format!("sound_test_{name}_{}", std::process::id());
        let config: tokio_postgres::Config = url().parse().expect("a PostgreSQL URL");
        let admin = connect(&config).await;
        for sql in [
            format!("DROP DATABASE IF EXISTS {name} WITH (FORCE)"),
            format!("CREATE DATABASE {name}"),
        ] {
            admin.batch_execute(&sql).await.expect("a scratch database");
        }
        let client = connect(config.clone().dbname(&name)).await;
        client
            .batch_execute(CAMPAIGN)
            .await
            .expect("the campaign table");
        client.batch_execute(MIGRATION).await.expect("migrated");
        client
            .batch_execute(&format!(
                "INSERT INTO campaign (id, name, kind) VALUES \
                 ('{VALOMBRE}', 'Les Cendres de Valombre', 'campagne'), \
                 ('{BRUME}', 'La Brume', 'oneshot'); \
                 INSERT INTO sound (campaign_id, name, file_path, tags, volume) VALUES \
                 ('{VALOMBRE}', 'Porte qui grince', 'sons/porte.ogg', '{{porte,taverne}}', 0.8), \
                 ('{VALOMBRE}', 'Taverne animée', 'sons/taverne.ogg', '{{taverne}}', 0.5), \
                 ('{BRUME}', 'Corne de brume', 'sons/corne.ogg', '{{}}', 1)"
            ))
            .await
            .expect("two campaigns and their sounds");
        Scratch { client, name }
    }

    /// The names of `campaign`'s sounds, sorted.
    async fn sounds(&self, campaign: &str) -> Vec<String> {
        self.client
            .query(
                "SELECT name FROM sound WHERE campaign_id = $1::text::uuid ORDER BY name",
                &[&campaign],
            )
            .await
            .expect("the sounds read")
            .iter()
            .map(|row| row.get(0))
            .collect()
    }

    /// Every row, as text: what a replay must leave as it was.
    async fn snapshot(&self) -> Vec<String> {
        self.client
            .query("SELECT row_to_json(s)::text FROM sound s ORDER BY 1", &[])
            .await
            .expect("the table reads")
            .iter()
            .map(|row| row.get(0))
            .collect()
    }

    /// The table's columns: name, type and nullability, in the table's order.
    async fn columns(&self) -> Vec<(String, String, bool)> {
        self.client
            .query(
                "SELECT a.attname, format_type(a.atttypid, a.atttypmod), NOT a.attnotnull \
                 FROM pg_attribute a WHERE a.attrelid = 'sound'::regclass \
                 AND a.attnum > 0 AND NOT a.attisdropped ORDER BY a.attnum",
                &[],
            )
            .await
            .expect("the columns read")
            .iter()
            .map(|row| {
                let ty: String = row.get(1);
                // As the model writes it, pg_dump's own alias.
                let ty = ty.replace("timestamp with time zone", "timestamptz");
                (row.get(0), ty, row.get(2))
            })
            .collect()
    }

    /// The table's foreign keys: column, referenced `table.column`, and the
    /// actions on delete and on update, as `pg_constraint` codes them.
    async fn foreign_keys(&self) -> Vec<(String, String, String, String)> {
        self.client
            .query(
                "SELECT a.attname, c.confrelid::regclass::text || '.' || r.attname, \
                 c.confdeltype::text, c.confupdtype::text \
                 FROM pg_constraint c \
                 JOIN pg_attribute a ON a.attrelid = c.conrelid AND a.attnum = c.conkey[1] \
                 JOIN pg_attribute r ON r.attrelid = c.confrelid AND r.attnum = c.confkey[1] \
                 WHERE c.conrelid = 'sound'::regclass AND c.contype = 'f' ORDER BY 1",
                &[],
            )
            .await
            .expect("the constraints read")
            .iter()
            .map(|row| (row.get(0), row.get(1), row.get(2), row.get(3)))
            .collect()
    }

    /// The constraint `sql` is refused under: a CHECK names its invariant.
    async fn refused_by(&self, sql: &str) -> (SqlState, Option<String>) {
        let error = self.client.batch_execute(sql).await.expect_err(sql);
        let db = error.as_db_error().expect("a refusal of the database");
        (db.code().clone(), db.constraint().map(str::to_string))
    }

    async fn drop(self) {
        drop(self.client);
        let config: tokio_postgres::Config = url().parse().expect("a PostgreSQL URL");
        connect(&config)
            .await
            .batch_execute(&format!("DROP DATABASE {} WITH (FORCE)", self.name))
            .await
            .expect("the scratch database dropped");
    }
}

/// `sound` as `data/model.json` describes it: each field's name, type,
/// nullability and reference, in the table's order.
fn modelled() -> Vec<(String, String, bool, Option<String>)> {
    let model: Value = serde_json::from_str(include_str!("../../../../data/model.json"))
        .expect("data/model.json is JSON");
    let table = model["tables"]
        .as_array()
        .expect("tables")
        .iter()
        .find(|table| table["name"] == "sound")
        .expect("data/model.json describes sound");
    table["fields"]
        .as_array()
        .expect("fields")
        .iter()
        .map(|field| {
            (
                field["name"].as_str().expect("a name").to_string(),
                field["type"].as_str().expect("a type").to_string(),
                field["nullable"].as_bool().expect("a nullability"),
                field["references"].as_str().map(str::to_string),
            )
        })
        .collect()
}

/// An insert of one of Valombre's sounds with `tags` and `volume` as SQL.
fn sound_with(tags: &str, volume: &str) -> String {
    format!(
        "INSERT INTO sound (campaign_id, name, file_path, tags, volume) VALUES \
         ('{VALOMBRE}', 'Écho', 'sons/echo.ogg', {tags}, {volume})"
    )
}

#[tokio::test]
async fn the_migration_replays_as_a_no_op_over_the_campaign_table() {
    let mut s = Scratch::new("replay").await;
    let first = s.snapshot().await;
    assert_eq!(first.len(), 3);
    for _ in 0..2 {
        s.client
            .batch_execute(MIGRATION)
            .await
            .expect("replayed as psql replays it");
        assert_eq!(s.snapshot().await, first);
    }
    // The server's applier after the rules' runtime's psql, which records
    // nothing: it runs every file again, in order, and nothing moves.
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../data/migrations");
    let applied = data::migrate(&mut s.client, std::path::Path::new(dir))
        .await
        .expect("replayed by the Data layer");
    let at = |file: &str| {
        applied
            .iter()
            .position(|f| f == file)
            .unwrap_or_else(|| panic!("{file} is applied: {applied:?}"))
    };
    assert!(
        at("0002_campaign.sql") < at("0007_sound.sql"),
        "{applied:?}"
    );
    assert_eq!(s.snapshot().await, first);
    s.drop().await;
}

#[tokio::test]
async fn the_table_is_what_data_model_json_describes() {
    let s = Scratch::new("model").await;
    let keys = s.foreign_keys().await;
    let real: Vec<(String, String, bool, Option<String>)> = s
        .columns()
        .await
        .into_iter()
        .map(|(name, ty, nullable)| {
            let references = keys
                .iter()
                .find(|(column, ..)| *column == name)
                .map(|(_, to, ..)| to.clone());
            (name, ty, nullable, references)
        })
        .collect();
    assert_eq!(real, modelled());
    s.drop().await;
}

#[tokio::test]
async fn deleting_a_campaign_deletes_its_sounds_and_theirs_alone() {
    let s = Scratch::new("cascade").await;
    // Relation Sound.campaignId: onDelete cascade, onUpdate propagate.
    assert_eq!(
        s.foreign_keys().await,
        [(
            "campaign_id".to_string(),
            "campaign.id".to_string(),
            "c".to_string(),
            "c".to_string()
        )]
    );
    s.client
        .batch_execute(&format!("DELETE FROM campaign WHERE id = '{VALOMBRE}'"))
        .await
        .expect("a campaign with sounds is deleted");
    assert!(s.sounds(VALOMBRE).await.is_empty());
    assert_eq!(s.sounds(BRUME).await, ["Corne de brume"]);
    s.drop().await;
}

#[tokio::test]
async fn a_campaign_s_new_id_is_propagated_to_its_sounds() {
    let s = Scratch::new("propagate").await;
    let moved = "c1000000-0000-4000-8000-0000000000ff";
    s.client
        .batch_execute(&format!(
            "UPDATE campaign SET id = '{moved}' WHERE id = '{VALOMBRE}'"
        ))
        .await
        .expect("the campaign's id changes");
    assert_eq!(
        s.sounds(moved).await,
        ["Porte qui grince", "Taverne animée"]
    );
    assert!(s.sounds(VALOMBRE).await.is_empty());
    s.drop().await;
}

#[tokio::test]
async fn a_sound_belongs_to_a_campaign_that_exists_and_has_a_name_and_a_file() {
    let s = Scratch::new("required").await;
    for (sql, code) in [
        (
            "INSERT INTO sound (campaign_id, name, file_path) \
             VALUES (gen_random_uuid(), 'Personne', 'sons/rien.ogg')"
                .to_string(),
            SqlState::FOREIGN_KEY_VIOLATION,
        ),
        (
            "INSERT INTO sound (name, file_path) VALUES ('Sans campagne', 'sons/rien.ogg')"
                .to_string(),
            SqlState::NOT_NULL_VIOLATION,
        ),
        (
            format!(
                "INSERT INTO sound (campaign_id, file_path) VALUES ('{VALOMBRE}', 'sons/x.ogg')"
            ),
            SqlState::NOT_NULL_VIOLATION,
        ),
        (
            format!("INSERT INTO sound (campaign_id, name) VALUES ('{VALOMBRE}', 'Sans fichier')"),
            SqlState::NOT_NULL_VIOLATION,
        ),
        (sound_with("NULL", "0.5"), SqlState::NOT_NULL_VIOLATION),
        (sound_with("'{}'", "NULL"), SqlState::NOT_NULL_VIOLATION),
    ] {
        assert_eq!(s.refused_by(&sql).await.0, code, "{sql}");
    }
    // Added now, under an id of its own, untagged at full volume, unless the
    // DM says otherwise.
    let row = s
        .client
        .query_one(
            &format!(
                "INSERT INTO sound (campaign_id, name, file_path) \
                 VALUES ('{VALOMBRE}', 'Vent', 'sons/vent.ogg') \
                 RETURNING id IS NOT NULL, created_at > now() - interval '1 minute', \
                 tags = '{{}}', volume = 1"
            ),
            &[],
        )
        .await
        .expect("a sound with a name and a file in a campaign");
    assert!((0..4).all(|i| row.get::<_, bool>(i)));
    s.drop().await;
}

#[tokio::test]
async fn sound_volume_range_refuses_a_volume_out_of_0_to_1() {
    let s = Scratch::new("volume_range").await;
    for volume in ["1.2", "-0.01", "1.01"] {
        assert_eq!(
            s.refused_by(&sound_with("'{}'", volume)).await,
            (
                SqlState::CHECK_VIOLATION,
                Some("sound_volume_range".to_string())
            ),
            "{volume}"
        );
    }
    for volume in ["0", "1", "0.35"] {
        s.client
            .batch_execute(&sound_with("'{}'", volume))
            .await
            .unwrap_or_else(|e| panic!("{volume} is in range: {e}"));
    }
    s.drop().await;
}

#[tokio::test]
async fn tags_normalized_refuses_capitals_a_tag_twice_or_none() {
    let s = Scratch::new("tags_normalized").await;
    for tags in [
        "'{Porte}'",
        "'{porte,ÉCHO}'",
        "'{porte,taverne,porte}'",
        "'{porte,NULL}'",
        "'{{porte},{taverne}}'",
    ] {
        assert_eq!(
            s.refused_by(&sound_with(tags, "0.5")).await,
            (
                SqlState::CHECK_VIOLATION,
                Some("tags_normalized".to_string())
            ),
            "{tags}"
        );
    }
    for tags in ["'{}'", "'{écho,grotte}'", "'{porte-2}'"] {
        s.client
            .batch_execute(&sound_with(tags, "0.5"))
            .await
            .unwrap_or_else(|e| panic!("{tags} are normalized: {e}"));
    }
    // An update is judged as an insert is.
    assert_eq!(
        s.refused_by("UPDATE sound SET tags = tags || 'Taverne'::text")
            .await
            .1
            .as_deref(),
        Some("tags_normalized")
    );
    s.drop().await;
}
