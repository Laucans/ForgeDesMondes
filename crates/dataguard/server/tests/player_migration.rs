//! The Player migration (`crates/data/migrations/0003_player.sql`) against
//! the harness's PostgreSQL (`DATABASE_URL`): replayed over the campaign
//! table it needs, it is what `data/model.json` says it is, a campaign's
//! deletion takes its players with it, and the table has nothing to make a
//! player a campaign element. Each test works in a scratch database of its
//! own, dropped when it ends.

use serde_json::Value;
use tokio_postgres::error::SqlState;
use tokio_postgres::{Client, NoTls};

const CAMPAIGN: &str = include_str!("../../../data/migrations/0002_campaign.sql");
const MIGRATION: &str = include_str!("../../../data/migrations/0003_player.sql");

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
/// Valombre and Brume and a player at each table.
struct Scratch {
    client: Client,
    name: String,
}

impl Scratch {
    async fn new(name: &str) -> Scratch {
        let name = format!("player_test_{name}_{}", std::process::id());
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
                 INSERT INTO player (campaign_id, name, notes) VALUES \
                 ('{VALOMBRE}', 'Claire', 'Préfère les scènes d''enquête.'), \
                 ('{VALOMBRE}', 'Marc', NULL), \
                 ('{BRUME}', 'Inès', NULL)"
            ))
            .await
            .expect("two campaigns and their players");
        Scratch { client, name }
    }

    /// The names of the players at `campaign`'s table, sorted.
    async fn players(&self, campaign: &str) -> Vec<String> {
        self.client
            .query(
                "SELECT name FROM player WHERE campaign_id = $1::text::uuid ORDER BY name",
                &[&campaign],
            )
            .await
            .expect("the players read")
            .iter()
            .map(|row| row.get(0))
            .collect()
    }

    /// Every row, as text: what a replay must leave as it was.
    async fn snapshot(&self) -> Vec<String> {
        self.client
            .query("SELECT row_to_json(p)::text FROM player p ORDER BY 1", &[])
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
                 FROM pg_attribute a WHERE a.attrelid = 'player'::regclass \
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
                 WHERE c.conrelid = 'player'::regclass AND c.contype = 'f' ORDER BY 1",
                &[],
            )
            .await
            .expect("the constraints read")
            .iter()
            .map(|row| (row.get(0), row.get(1), row.get(2), row.get(3)))
            .collect()
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

/// `player` as `data/model.json` describes it: each field's name, type,
/// nullability and reference, in the table's order.
fn modelled() -> Vec<(String, String, bool, Option<String>)> {
    let model: Value = serde_json::from_str(include_str!("../../../../data/model.json"))
        .expect("data/model.json is JSON");
    let table = model["tables"]
        .as_array()
        .expect("tables")
        .iter()
        .find(|table| table["name"] == "player")
        .expect("data/model.json describes player");
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
        at("0002_campaign.sql") < at("0003_player.sql"),
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
async fn deleting_a_campaign_deletes_its_players_and_theirs_alone() {
    let s = Scratch::new("cascade").await;
    // Relation Player.campaignId: onDelete cascade, onUpdate propagate.
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
        .expect("a campaign with players is deleted");
    assert!(s.players(VALOMBRE).await.is_empty());
    assert_eq!(s.players(BRUME).await, ["Inès"]);
    s.drop().await;
}

#[tokio::test]
async fn a_campaign_s_new_id_is_propagated_to_its_players() {
    let s = Scratch::new("propagate").await;
    let moved = "c1000000-0000-4000-8000-0000000000ff";
    s.client
        .batch_execute(&format!(
            "UPDATE campaign SET id = '{moved}' WHERE id = '{VALOMBRE}'"
        ))
        .await
        .expect("the campaign's id changes");
    assert_eq!(s.players(moved).await, ["Claire", "Marc"]);
    assert!(s.players(VALOMBRE).await.is_empty());
    s.drop().await;
}

#[tokio::test]
async fn a_player_belongs_to_a_campaign_that_exists_and_has_a_name() {
    let s = Scratch::new("required").await;
    for (sql, code) in [
        (
            "INSERT INTO player (campaign_id, name) VALUES (gen_random_uuid(), 'Personne')",
            SqlState::FOREIGN_KEY_VIOLATION,
        ),
        (
            "INSERT INTO player (name) VALUES ('Sans table')",
            SqlState::NOT_NULL_VIOLATION,
        ),
        (
            &format!("INSERT INTO player (campaign_id) VALUES ('{VALOMBRE}')"),
            SqlState::NOT_NULL_VIOLATION,
        ),
    ] {
        let error = s.client.batch_execute(sql).await.expect_err(sql);
        let db = error.as_db_error().expect("a refusal of the database");
        assert_eq!(db.code(), &code, "{sql}");
    }
    // Added now, under an id of its own, unless the DM gives one.
    let row = s
        .client
        .query_one(
            &format!(
                "INSERT INTO player (campaign_id, name) VALUES ('{VALOMBRE}', 'Léa') \
                 RETURNING id IS NOT NULL, created_at > now() - interval '1 minute'"
            ),
            &[],
        )
        .await
        .expect("a player with a name at a campaign's table");
    assert!(row.get::<_, bool>(0) && row.get::<_, bool>(1));
    s.drop().await;
}

#[tokio::test]
async fn player_not_character_leaves_the_table_no_sheet_no_token_and_no_pc() {
    let s = Scratch::new("not_character").await;
    let columns: Vec<String> = s.columns().await.into_iter().map(|c| c.0).collect();
    assert_eq!(
        columns,
        ["id", "campaign_id", "name", "notes", "created_at"]
    );
    // The PC points to its player, never the other way round: the player's
    // one link is to their campaign.
    let targets: Vec<String> = s.foreign_keys().await.into_iter().map(|k| k.1).collect();
    assert_eq!(targets, ["campaign.id"]);
    s.drop().await;
}
