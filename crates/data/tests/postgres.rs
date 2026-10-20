//! The Data layer against the harness's PostgreSQL (`DATABASE_URL`,
//! `DATABASE_URL_READONLY`). A migration test works in a scratch database of
//! its own, as `scripts/dump-schema.sh` does, a read in a scratch schema;
//! both are dropped when the test ends, so the tests run in parallel and
//! leave nothing.

use std::path::PathBuf;

use data::{Client, Error};
use serde_json::json;
use tokio_postgres::NoTls;
use tokio_postgres::error::SqlState;

/// The read-write connection, `DATABASE_URL`.
async fn admin() -> Client {
    data::read_write()
        .await
        .expect("DATABASE_URL names the harness's PostgreSQL")
}

/// A database of its own, a connection to it, and a folder of its own for
/// migration files.
struct Scratch {
    client: Client,
    name: String,
    dir: PathBuf,
}

impl Scratch {
    async fn new(name: &str) -> Scratch {
        Scratch::opened_with(name, "").await
    }

    /// The connection opened with `options`, as a URL's `?options=` gives.
    async fn opened_with(name: &str, options: &str) -> Scratch {
        let name = format!("data_test_{name}_{}", std::process::id());
        let admin = admin().await;
        // One statement each: neither runs in a transaction block.
        for sql in [
            format!("DROP DATABASE IF EXISTS {name} WITH (FORCE)"),
            format!("CREATE DATABASE {name}"),
        ] {
            admin.batch_execute(&sql).await.expect("a scratch database");
        }
        let mut config: tokio_postgres::Config = std::env::var(data::READ_WRITE_URL)
            .expect("DATABASE_URL")
            .parse()
            .expect("DATABASE_URL is a PostgreSQL URL");
        let (client, connection) = config
            .dbname(&name)
            .options(options)
            .connect(NoTls)
            .await
            .expect("the scratch database");
        tokio::spawn(connection);
        let dir = std::env::temp_dir().join(&name);
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("a scratch folder");
        Scratch { client, name, dir }
    }

    fn file(&self, name: &str, sql: &str) {
        std::fs::write(self.dir.join(name), sql).expect("a migration file");
    }

    async fn ints(&self, sql: &str) -> Vec<i32> {
        self.client
            .query(sql, &[])
            .await
            .expect("a read of the scratch database")
            .iter()
            .map(|row| row.get(0))
            .collect()
    }

    async fn tables(&self) -> Vec<String> {
        self.client
            .query(
                "SELECT table_schema || '.' || table_name FROM information_schema.tables \
                 WHERE table_schema NOT IN ('pg_catalog', 'information_schema') ORDER BY 1",
                &[],
            )
            .await
            .expect("the scratch database's tables")
            .iter()
            .map(|row| row.get(0))
            .collect()
    }

    async fn drop(self) {
        let _ = std::fs::remove_dir_all(&self.dir);
        drop(self.client);
        admin()
            .await
            .batch_execute(&format!("DROP DATABASE {} WITH (FORCE)", self.name))
            .await
            .expect("the scratch database dropped");
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
        s.ints("SELECT count(*)::int FROM public.data_migrations")
            .await,
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
    assert_eq!(s.tables().await, ["public.data_migrations", "public.first"]);
    s.drop().await;
}

#[tokio::test]
async fn an_applied_migration_edited_since_is_named_and_no_file_applied() {
    let mut s = Scratch::new("edited").await;
    s.file("01_a.sql", "CREATE TABLE a (n int);\n");
    s.file("02_b.sql", "CREATE TABLE b (n int);\n");
    data::migrate(&mut s.client, &s.dir).await.expect("applied");
    // Line endings rewritten by a checkout are no edit.
    s.file("01_a.sql", "CREATE TABLE a (n int);\r\n");
    let same = data::migrate(&mut s.client, &s.dir)
        .await
        .expect("nothing to apply");
    assert_eq!(same, [] as [String; 0]);
    // A new file sorting before the edited one is not applied either.
    s.file("00_new.sql", "CREATE TABLE new (n int);");
    s.file("02_b.sql", "CREATE TABLE b (n int, m int);");

    let error = data::migrate(&mut s.client, &s.dir)
        .await
        .expect_err("02_b.sql was edited");
    match &error {
        Error::Changed { file } => assert_eq!(file, &s.dir.join("02_b.sql")),
        other => panic!("expected Error::Changed, got {other:?}"),
    }
    assert_eq!(
        s.tables().await,
        ["public.a", "public.b", "public.data_migrations"]
    );
    s.drop().await;
}

#[tokio::test]
async fn a_migration_s_settings_do_not_outlive_it() {
    let mut s = Scratch::new("session").await;
    // Set on the client before the run: no file sees it either.
    s.client
        .batch_execute("CREATE SCHEMA before; SET search_path TO before; SET lock_timeout = '13s'")
        .await
        .expect("session settings");
    s.file(
        "01_away.sql",
        "CREATE SCHEMA elsewhere; SET search_path TO elsewhere; SET lock_timeout = '17s'; \
         CREATE TABLE moved (n int);",
    );
    s.file(
        "02_home.sql",
        "CREATE TABLE stays AS SELECT current_setting('lock_timeout') = reset_val AS fresh \
         FROM pg_settings WHERE name = 'lock_timeout';",
    );

    let first = data::migrate(&mut s.client, &s.dir).await.expect("applied");
    assert_eq!(first, ["01_away.sql", "02_home.sql"]);
    let second = data::migrate(&mut s.client, &s.dir)
        .await
        .expect("nothing to apply");
    assert_eq!(second, [] as [String; 0]);
    assert_eq!(
        s.tables().await,
        ["elsewhere.moved", "public.data_migrations", "public.stays"]
    );
    let fresh: bool = s
        .client
        .query_one("SELECT fresh FROM public.stays", &[])
        .await
        .expect("the setting 02 saw")
        .get(0);
    assert!(fresh, "02 ran with lock_timeout as the client was opened");
    s.drop().await;
}

#[tokio::test]
async fn the_record_stays_put_when_a_migration_creates_the_first_schema_searched() {
    // `app` does not exist on the first run, and does on the second.
    let mut s = Scratch::opened_with("record", "-c search_path=app,public").await;
    s.file(
        "01_app.sql",
        "CREATE SCHEMA app; CREATE TABLE app.t (n int);",
    );

    let first = data::migrate(&mut s.client, &s.dir).await.expect("applied");
    assert_eq!(first, ["01_app.sql"]);
    let second = data::migrate(&mut s.client, &s.dir)
        .await
        .expect("nothing to apply");
    assert_eq!(second, [] as [String; 0]);
    assert_eq!(s.tables().await, ["app.t", "public.data_migrations"]);
    s.drop().await;
}

#[tokio::test]
async fn the_read_only_connection_refuses_an_insert() {
    let schema = format!("data_test_refused_{}", std::process::id());
    let admin = admin().await;
    let mut reader = data::read_only()
        .await
        .expect("DATABASE_URL_READONLY names the harness's PostgreSQL");
    let role = reader
        .query_json("SELECT quote_ident(current_user) AS role", &[])
        .await
        .expect("the read-only role")[0]["role"]
        .as_str()
        .expect("a role name")
        .to_string();
    // The read-only role may write this table: only the READ ONLY
    // transaction stands in the way.
    admin
        .batch_execute(&format!(
            "DROP SCHEMA IF EXISTS {schema} CASCADE; CREATE SCHEMA {schema}; \
             CREATE TABLE {schema}.target (n int); \
             GRANT USAGE ON SCHEMA {schema} TO {role}; \
             GRANT INSERT ON {schema}.target TO {role}"
        ))
        .await
        .expect("a table the read-only role may write");

    let error = reader
        .query_json(&format!("INSERT INTO {schema}.target VALUES (1)"), &[])
        .await
        .expect_err("PostgreSQL refuses a write");
    match &error {
        Error::Refused(e) => assert_eq!(e.code(), Some(&SqlState::READ_ONLY_SQL_TRANSACTION)),
        other => panic!("expected Error::Refused, got {other:?}"),
    }
    let written = admin
        .query_one(&format!("SELECT count(*) FROM {schema}.target"), &[])
        .await
        .expect("the table read")
        .get::<_, i64>(0);
    assert_eq!(written, 0);
    admin
        .batch_execute(&format!("DROP SCHEMA {schema} CASCADE"))
        .await
        .expect("the scratch schema dropped");
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

/// A scratch table whose BEFORE INSERT trigger refuses a key it already holds,
/// as the Entity aggregate's version trigger does.
const SEEN: &str = "CREATE TABLE seen (
    n int PRIMARY KEY,
    label text NOT NULL DEFAULT 'default',
    CHECK (n > 0)
);
CREATE FUNCTION seen_once() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF EXISTS (SELECT 1 FROM seen WHERE n = NEW.n) THEN
        RAISE EXCEPTION 'seen again';
    END IF;
    RETURN NEW;
END $$;
CREATE TRIGGER seen_once BEFORE INSERT ON seen FOR EACH ROW EXECUTE FUNCTION seen_once();";

fn seen(rows: serde_json::Value) -> data::Table {
    data::Table {
        name: "seen",
        key: "n",
        columns: &["n", "label"],
        rows: rows.as_array().expect("rows").clone(),
    }
}

#[tokio::test]
async fn rows_are_inserted_once_and_a_second_load_inserts_none() {
    let mut s = Scratch::new("insert").await;
    s.file("01_seen.sql", SEEN);
    data::migrate(&mut s.client, &s.dir).await.expect("applied");
    let rows = json!([{ "n": 1, "label": "one" }, { "n": 2, "label": "two" }]);

    let first = data::insert_missing(&mut s.client, &[seen(rows.clone())])
        .await
        .expect("inserted");
    assert_eq!(first, 2);
    // The second load tries no row again: the trigger would refuse it.
    let second = data::insert_missing(&mut s.client, &[seen(rows)])
        .await
        .expect("nothing to insert");
    assert_eq!(second, 0);

    let labels: Vec<String> = s
        .client
        .query("SELECT label FROM seen ORDER BY n", &[])
        .await
        .expect("a read")
        .iter()
        .map(|row| row.get(0))
        .collect();
    assert_eq!(labels, ["one", "two"]);
    s.drop().await;
}

#[tokio::test]
async fn a_column_the_table_is_not_given_takes_its_default() {
    let mut s = Scratch::new("default").await;
    s.file("01_seen.sql", SEEN);
    data::migrate(&mut s.client, &s.dir).await.expect("applied");
    let table = data::Table {
        columns: &["n"],
        ..seen(json!([{ "n": 7 }]))
    };
    data::insert_missing(&mut s.client, &[table])
        .await
        .expect("inserted");
    let labels: Vec<String> = s
        .client
        .query("SELECT label FROM seen", &[])
        .await
        .expect("a read")
        .iter()
        .map(|row| row.get(0))
        .collect();
    assert_eq!(labels, ["default"]);
    s.drop().await;
}

#[tokio::test]
async fn a_row_the_table_holds_is_left_as_it_is() {
    let mut s = Scratch::new("kept").await;
    s.file("01_seen.sql", SEEN);
    data::migrate(&mut s.client, &s.dir).await.expect("applied");
    data::insert_missing(&mut s.client, &[seen(json!([{ "n": 1, "label": "mine" }]))])
        .await
        .expect("inserted");
    let added = data::insert_missing(
        &mut s.client,
        &[seen(
            json!([{ "n": 1, "label": "theirs" }, { "n": 2, "label": "new" }]),
        )],
    )
    .await
    .expect("inserted");
    assert_eq!(added, 1);
    let labels: Vec<String> = s
        .client
        .query("SELECT label FROM seen ORDER BY n", &[])
        .await
        .expect("a read")
        .iter()
        .map(|row| row.get(0))
        .collect();
    assert_eq!(labels, ["mine", "new"]);
    s.drop().await;
}

#[tokio::test]
async fn a_refused_row_rolls_the_whole_load_back() {
    let mut s = Scratch::new("rollback").await;
    s.file("01_seen.sql", SEEN);
    data::migrate(&mut s.client, &s.dir).await.expect("applied");
    // The second row breaks the CHECK: the first goes with it.
    let refused = data::insert_missing(&mut s.client, &[seen(json!([{ "n": 1 }, { "n": -1 }]))])
        .await
        .expect_err("n > 0 is checked");
    assert!(matches!(refused, Error::Database(_)), "{refused:?}");
    assert_eq!(s.ints("SELECT count(*)::int FROM seen").await, [0]);
    s.drop().await;
}

#[tokio::test]
async fn a_name_that_is_not_a_plain_identifier_is_refused() {
    let mut client = admin().await;
    let table = data::Table {
        name: "seen\" CASCADE; --",
        key: "n",
        columns: &["n"],
        rows: Vec::new(),
    };
    let refused = data::insert_missing(&mut client, &[table])
        .await
        .expect_err("a quote is not an identifier");
    assert!(matches!(refused, Error::Identifier(_)), "{refused:?}");
}

#[tokio::test]
async fn of_rows_sharing_a_key_the_first_is_inserted() {
    let mut s = Scratch::new("twice").await;
    s.file("01_seen.sql", SEEN);
    data::migrate(&mut s.client, &s.dir).await.expect("applied");
    let added = data::insert_missing(
        &mut s.client,
        &[seen(json!([
            { "n": 1, "label": "first" },
            { "n": 1, "label": "second" },
        ]))],
    )
    .await
    .expect("the load is not refused for a repeated key");
    assert_eq!(added, 1);
    let labels: Vec<String> = s
        .client
        .query("SELECT label FROM seen", &[])
        .await
        .expect("a read")
        .iter()
        .map(|row| row.get(0))
        .collect();
    assert_eq!(labels, ["first"]);
    s.drop().await;
}

#[tokio::test]
async fn an_identifier_may_hold_digits_and_underscores_and_nothing_else() {
    let mut s = Scratch::new("names").await;
    s.file(
        "01_names.sql",
        "CREATE TABLE seen_2 (id_1 int PRIMARY KEY, label_a text NOT NULL);",
    );
    data::migrate(&mut s.client, &s.dir).await.expect("applied");
    let table = data::Table {
        name: "seen_2",
        key: "id_1",
        columns: &["id_1", "label_a"],
        rows: vec![json!({ "id_1": 1, "label_a": "one" })],
    };
    let added = data::insert_missing(&mut s.client, &[table])
        .await
        .expect("digits and underscores are plain");
    assert_eq!(added, 1);

    for refused in ["", "Seen", "seen-2", "seen 2", "sé"] {
        let table = data::Table {
            name: refused,
            key: "id_1",
            columns: &["id_1"],
            rows: Vec::new(),
        };
        let error = data::insert_missing(&mut s.client, &[table])
            .await
            .expect_err("not a plain identifier");
        assert!(
            matches!(error, Error::Identifier(_)),
            "{refused:?}: {error:?}"
        );
    }
    s.drop().await;
}
