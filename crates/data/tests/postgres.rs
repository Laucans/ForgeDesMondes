//! The Data layer against the harness's PostgreSQL (`DATABASE_URL`,
//! `DATABASE_URL_READONLY`). Each test works in a scratch schema of its own,
//! dropped when it ends, so the tests run in parallel and leave nothing.

use std::path::PathBuf;

use data::{Client, Error};
use serde_json::json;

/// A schema of its own, the read-write connection's `search_path` set to it,
/// and a folder of its own for migration files.
struct Scratch {
    client: Client,
    schema: String,
    dir: PathBuf,
}

impl Scratch {
    async fn new(name: &str) -> Scratch {
        let schema = format!("data_test_{name}_{}", std::process::id());
        let client = data::read_write()
            .await
            .expect("DATABASE_URL names the harness's PostgreSQL");
        client
            .batch_execute(&format!(
                "DROP SCHEMA IF EXISTS {schema} CASCADE; CREATE SCHEMA {schema}; SET search_path TO {schema}"
            ))
            .await
            .expect("a scratch schema");
        let dir = std::env::temp_dir().join(&schema);
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("a scratch folder");
        Scratch {
            client,
            schema,
            dir,
        }
    }

    fn file(&self, name: &str, sql: &str) {
        std::fs::write(self.dir.join(name), sql).expect("a migration file");
    }

    async fn ints(&self, sql: &str) -> Vec<i32> {
        self.client
            .query(sql, &[])
            .await
            .expect("a read of the scratch schema")
            .iter()
            .map(|row| row.get(0))
            .collect()
    }

    async fn tables(&self) -> Vec<String> {
        self.client
            .query(
                "SELECT table_name::text FROM information_schema.tables WHERE table_schema = $1 ORDER BY 1",
                &[&self.schema],
            )
            .await
            .expect("the scratch schema's tables")
            .iter()
            .map(|row| row.get(0))
            .collect()
    }

    async fn drop(self) {
        let _ = std::fs::remove_dir_all(&self.dir);
        self.client
            .batch_execute(&format!("DROP SCHEMA {} CASCADE", self.schema))
            .await
            .expect("the scratch schema dropped");
    }
}

#[tokio::test]
async fn migrations_apply_once_each_in_path_order() {
    let mut s = Scratch::new("order").await;
    // Written out of order; each file needs the one before it.
    s.file("05_e.sql", "INSERT INTO seen (n) VALUES (5);");
    s.file("03_c.sql", "INSERT INTO seen (n) VALUES (3);");
    s.file("01_a.sql", "CREATE TABLE seen (at serial, n int);");
    s.file("04_d.sql", "INSERT INTO seen (n) VALUES (4);");
    s.file("02_b.sql", "INSERT INTO seen (n) VALUES (2);");
    // Not a migration: never applied.
    s.file("README.md", "this is not SQL");

    let first = data::migrate(&mut s.client, &s.dir).await.expect("applied");
    assert_eq!(
        first,
        ["01_a.sql", "02_b.sql", "03_c.sql", "04_d.sql", "05_e.sql"]
    );
    let second = data::migrate(&mut s.client, &s.dir)
        .await
        .expect("nothing to apply");
    assert_eq!(second, [] as [String; 0]);
    assert_eq!(
        s.ints("SELECT n FROM seen ORDER BY at").await,
        [2, 3, 4, 5],
        "each file applied once, in path order"
    );
    assert_eq!(
        s.ints("SELECT count(*)::int FROM data_migrations").await,
        [5]
    );
    s.drop().await;
}

#[tokio::test]
async fn a_failing_migration_is_named_and_no_later_file_is_applied() {
    let mut s = Scratch::new("failing").await;
    s.file("001_first.sql", "CREATE TABLE first (n int);");
    s.file(
        "002_broken.sql",
        "CREATE TABLE half (n int); INSERT INTO nowhere VALUES (1);",
    );
    s.file("003_later.sql", "CREATE TABLE later (n int);");

    let error = data::migrate(&mut s.client, &s.dir)
        .await
        .expect_err("002 fails");
    match &error {
        Error::Migration { file, .. } => assert_eq!(file, &s.dir.join("002_broken.sql")),
        other => panic!("expected Error::Migration, got {other:?}"),
    }
    assert!(error.to_string().contains("002_broken.sql"), "{error}");
    // The file before it stays applied; the failing one is rolled back whole.
    assert_eq!(s.tables().await, ["data_migrations", "first"]);
    s.drop().await;
}

#[tokio::test]
async fn a_migration_s_search_path_does_not_outlive_it() {
    let mut s = Scratch::new("path").await;
    let elsewhere = format!("{}_elsewhere", s.schema);
    s.file(
        "01_away.sql",
        &format!(
            "DROP SCHEMA IF EXISTS {elsewhere} CASCADE; CREATE SCHEMA {elsewhere}; \
             SET search_path TO {elsewhere}; CREATE TABLE moved (n int);"
        ),
    );
    s.file("02_home.sql", "CREATE TABLE stays (n int);");

    let first = data::migrate(&mut s.client, &s.dir).await.expect("applied");
    assert_eq!(first, ["01_away.sql", "02_home.sql"]);
    let second = data::migrate(&mut s.client, &s.dir)
        .await
        .expect("nothing to apply");
    assert_eq!(second, [] as [String; 0]);
    assert_eq!(s.tables().await, ["data_migrations", "stays"]);
    s.client
        .batch_execute(&format!("DROP SCHEMA {elsewhere} CASCADE"))
        .await
        .expect("the other schema dropped");
    s.drop().await;
}

#[tokio::test]
async fn the_read_only_connection_refuses_an_insert() {
    let s = Scratch::new("refused").await;
    s.client
        .batch_execute("CREATE TABLE target (n int)")
        .await
        .expect("a table to write into");
    let mut reader = data::read_only()
        .await
        .expect("DATABASE_URL_READONLY names the harness's PostgreSQL");

    let error = reader
        .query_json(&format!("INSERT INTO {}.target VALUES (1)", s.schema), &[])
        .await
        .expect_err("PostgreSQL refuses a write");
    assert!(matches!(error, Error::Refused(_)), "{error:?}");
    assert_eq!(s.ints("SELECT n FROM target").await, [] as [i32; 0]);
    s.drop().await;
}

#[tokio::test]
async fn a_query_answers_its_rows_as_json() {
    let mut reader = data::read_only().await.expect("the read-only connection");
    let rows = reader
        .query_json(
            "SELECT true AS b, 2::int2 AS h, 4::int4 AS i, 8::int8 AS l, \
             0.5::float4 AS f, 1.25::float8 AS d, $1::text AS t, 'v'::varchar AS v, \
             'c'::char(1) AS c, 'nm'::name AS nm, '{\"k\":1}'::json AS j, \
             '[1,2]'::jsonb AS jb, NULL::int4 AS z \
             FROM generate_series(1, 2)",
            &[&"text"],
        )
        .await
        .expect("a read");
    let row = json!({
        "b": true, "h": 2, "i": 4, "l": 8, "f": 0.5, "d": 1.25, "t": "text",
        "v": "v", "c": "c", "nm": "nm", "j": {"k": 1}, "jb": [1, 2], "z": null
    });
    assert_eq!(rows, json!([row, row]));

    let none = reader
        .query_json("SELECT 1 AS n WHERE false", &[])
        .await
        .expect("an empty read");
    assert_eq!(none, json!([]));
}

#[tokio::test]
async fn a_column_with_no_json_reading_is_named() {
    let mut reader = data::read_only().await.expect("the read-only connection");
    let error = reader
        .query_json("SELECT 1.5::numeric AS amount", &[])
        .await
        .expect_err("numeric is cast in the query");
    match error {
        Error::Unsupported { column, ty } => {
            assert_eq!((column.as_str(), ty.as_str()), ("amount", "numeric"));
        }
        other => panic!("expected Error::Unsupported, got {other:?}"),
    }
}

#[tokio::test]
async fn a_failing_read_is_a_query_error() {
    let mut reader = data::read_only().await.expect("the read-only connection");
    let error = reader
        .query_json("SELECT * FROM no_such_table", &[])
        .await
        .expect_err("no such table");
    assert!(matches!(error, Error::Query(_)), "{error:?}");
}
