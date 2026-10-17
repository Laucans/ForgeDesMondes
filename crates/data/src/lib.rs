//! The Data layer — the adapter of the `database` port (ports.json), and the
//! one crate of the product that opens a PostgreSQL connection for the read
//! side. What the server and every read crate sit on:
//!
//! - [`read_write`]: the connection `DATABASE_URL` names, the DataGuard's and
//!   the migrations' — never a read's;
//! - [`read_only`]: the connection `DATABASE_URL_READONLY` names, whose only
//!   API is [`ReadOnly::query_json`];
//! - [`migrate`]: the applier of a folder's `*.sql` files, in path order, each
//!   once.
//!
//! A read is read-only twice over: by its role, and by the `READ ONLY`
//! transaction every query runs in, so a write is refused by PostgreSQL even
//! where the read-only URL names a role that could write.

use std::path::{Path, PathBuf};

use serde_json::{Map, Value};
use tokio_postgres::error::SqlState;
use tokio_postgres::types::Type;
use tokio_postgres::{Column, NoTls, Row};

pub use tokio_postgres::Client;
pub use tokio_postgres::types::ToSql;

/// The variable naming the read-write connection.
pub const READ_WRITE_URL: &str = "DATABASE_URL";
/// The variable naming the read-only connection.
pub const READ_ONLY_URL: &str = "DATABASE_URL_READONLY";

/// Where the applied migrations are recorded: one fixed table, whatever the
/// `search_path` of the run.
const TRACKING: &str = "public.data_migrations";

/// One applier at a time, whatever the number of servers starting.
const LOCK: &str = "SELECT pg_advisory_xact_lock(hashtext('data_migrations'))";

/// The session as the client was opened: what an earlier `SET`, `SET ROLE`
/// or `SET SESSION AUTHORIZATION` changed is undone.
const FRESH: &str = "RESET ALL; RESET SESSION AUTHORIZATION; RESET ROLE";

/// What the Data layer answers when it cannot.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The environment does not name the connection.
    #[error("{0} is not set")]
    Unset(&'static str),
    /// The database could not be reached, or refused what the layer itself
    /// asked of it.
    #[error("database: {0}")]
    Database(#[source] tokio_postgres::Error),
    /// A migrations folder or file could not be read.
    #[error("cannot read {}: {source}", path.display())]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    /// A migration file failed: it is rolled back, and no later file applied.
    #[error("migration {} failed: {source}", file.display())]
    Migration {
        file: PathBuf,
        #[source]
        source: tokio_postgres::Error,
    },
    /// A migration file was edited after it was applied: no file of the run
    /// is applied.
    #[error("migration {} was edited after it was applied", file.display())]
    Changed { file: PathBuf },
    /// PostgreSQL refused the statement on the read-only connection: a write,
    /// or a privilege the read-only role has not.
    #[error("refused on the read-only connection: {0}")]
    Refused(#[source] tokio_postgres::Error),
    /// The query failed otherwise.
    #[error("query: {0}")]
    Query(#[source] tokio_postgres::Error),
    /// A column's type has no JSON reading here: cast it in the query
    /// (`::text`, `::float8`, `to_json(…)`).
    #[error("column {column} is of type {ty}, which has no JSON reading — cast it in the query")]
    Unsupported { column: String, ty: String },
}

/// The read-write connection `DATABASE_URL` names.
///
/// # Errors
///
/// [`Error::Unset`] without the variable, [`Error::Database`] when the
/// database cannot be reached.
pub async fn read_write() -> Result<Client, Error> {
    connect(&url_of(READ_WRITE_URL)?).await
}

/// The read-only connection `DATABASE_URL_READONLY` names.
///
/// # Errors
///
/// [`Error::Unset`] without the variable, [`Error::Database`] when the
/// database cannot be reached.
pub async fn read_only() -> Result<ReadOnly, Error> {
    Ok(ReadOnly {
        client: connect(&url_of(READ_ONLY_URL)?).await?,
    })
}

fn url_of(name: &'static str) -> Result<String, Error> {
    std::env::var(name).map_err(|_| Error::Unset(name))
}

/// A connection, its driver spawned on the current Tokio runtime. The driver
/// ends when the client is dropped; a lost connection is the error of the
/// client's next call.
async fn connect(url: &str) -> Result<Client, Error> {
    let (client, connection) = tokio_postgres::connect(url, NoTls)
        .await
        .map_err(Error::Database)?;
    tokio::spawn(async move {
        let _ = connection.await;
    });
    Ok(client)
}

/// Applies every `*.sql` file directly under `dir` not applied yet, by name
/// in byte order — the order `scripts/dump-schema.sh` and
/// `scripts/rules-runtime.sh` give the files of one `migrations/` folder —
/// each in a transaction of its own that also records its name and the
/// `md5` of its text, line endings as `\n`, in `public.data_migrations`.
/// An applied file whose text has changed since stops the run before any
/// file is applied. Returns the names of the
/// files this run applied: a second run applies none.
///
/// As `psql -f` gives each file a session of its own, each file runs in the
/// session as the client was opened: a `SET` made on the client before, or
/// by an earlier file, is reset. A file holds no transaction control of its
/// own and nothing that cannot run in a transaction
/// (`CREATE INDEX CONCURRENTLY`).
///
/// # Errors
///
/// [`Error::Migration`] naming the file that failed, which is rolled back:
/// the files before it stay applied, none after it is tried.
/// [`Error::Changed`] naming the first applied file whose text has changed
/// since, before any file is applied.
/// [`Error::Io`] when `dir` or a file cannot be read, [`Error::Database`]
/// when the tracking table cannot be kept.
pub async fn migrate(client: &mut Client, dir: &Path) -> Result<Vec<String>, Error> {
    let mut files = Vec::new();
    for (name, path) in migrations(dir)? {
        let sql = std::fs::read_to_string(&path).map_err(|source| Error::Io {
            path: path.clone(),
            source,
        })?;
        files.push((name, path, sql.replace("\r\n", "\n")));
    }
    let tx = client.transaction().await.map_err(Error::Database)?;
    tx.batch_execute(FRESH).await.map_err(Error::Database)?;
    tx.batch_execute(LOCK).await.map_err(Error::Database)?;
    tx.batch_execute(&format!(
        "CREATE TABLE IF NOT EXISTS {TRACKING} (
            file text PRIMARY KEY,
            checksum text NOT NULL,
            applied_at timestamptz NOT NULL DEFAULT now()
        )"
    ))
    .await
    .map_err(Error::Database)?;
    let (names, texts): (Vec<&str>, Vec<&str>) = files
        .iter()
        .map(|(name, _, sql)| (name.as_str(), sql.as_str()))
        .unzip();
    let changed: Vec<String> = tx
        .query(
            &format!(
                "SELECT file FROM unnest($1::text[], $2::text[]) AS here (file, text) \
                 JOIN {TRACKING} USING (file) WHERE checksum <> md5(text)"
            ),
            &[&names, &texts],
        )
        .await
        .map_err(Error::Database)?
        .iter()
        .map(|row| row.get(0))
        .collect();
    if let Some((_, path, _)) = files.iter().find(|(name, ..)| changed.contains(name)) {
        return Err(Error::Changed { file: path.clone() });
    }
    tx.commit().await.map_err(Error::Database)?;
    let mut applied = Vec::new();
    for (name, path, sql) in files {
        let tx = client.transaction().await.map_err(Error::Database)?;
        tx.batch_execute(FRESH).await.map_err(Error::Database)?;
        tx.batch_execute(LOCK).await.map_err(Error::Database)?;
        let done = tx
            .query_opt(
                &format!("SELECT 1 FROM {TRACKING} WHERE file = $1"),
                &[&name],
            )
            .await
            .map_err(Error::Database)?
            .is_some();
        if done {
            continue;
        }
        tx.batch_execute(&sql)
            .await
            .map_err(|source| Error::Migration { file: path, source })?;
        tx.batch_execute(FRESH).await.map_err(Error::Database)?;
        tx.execute(
            &format!("INSERT INTO {TRACKING} (file, checksum) VALUES ($1, md5($2))"),
            &[&name, &sql],
        )
        .await
        .map_err(Error::Database)?;
        tx.commit().await.map_err(Error::Database)?;
        applied.push(name);
    }
    Ok(applied)
}

/// The `*.sql` files directly under `dir`, by name in byte order — the
/// `LC_ALL=C sort` of the scripts.
fn migrations(dir: &Path) -> Result<Vec<(String, PathBuf)>, Error> {
    let io = |source| Error::Io {
        path: dir.to_path_buf(),
        source,
    };
    let mut files = Vec::new();
    for entry in std::fs::read_dir(dir).map_err(io)? {
        let path = entry.map_err(io)?.path();
        if path.extension().is_some_and(|e| e == "sql") && path.is_file() {
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            files.push((name, path));
        }
    }
    files.sort_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));
    Ok(files)
}

/// The read-only connection: it runs queries and nothing else.
pub struct ReadOnly {
    client: Client,
}

impl ReadOnly {
    /// Runs `sql` with `params` in a `READ ONLY` transaction, rolled back
    /// after, and answers its rows as a JSON array of objects keyed by column
    /// name. A SQL `NULL` is JSON `null`.
    ///
    /// The columns read are `bool`, `int2`, `int4`, `int8`, `float4`,
    /// `float8`, `text`, `varchar`, `bpchar`, `name`, `json` and `jsonb`;
    /// any other type is cast in the query. A `NaN` or an infinity, which JSON
    /// has no number for, is `null`.
    ///
    /// # Errors
    ///
    /// [`Error::Refused`] when PostgreSQL refuses a write or a privilege,
    /// [`Error::Query`] when the query fails otherwise,
    /// [`Error::Unsupported`] for a column of another type.
    pub async fn query_json(
        &mut self,
        sql: &str,
        params: &[&(dyn ToSql + Sync)],
    ) -> Result<Value, Error> {
        let tx = self
            .client
            .build_transaction()
            .read_only(true)
            .start()
            .await
            .map_err(Error::Database)?;
        let rows = tx.query(sql, params).await.map_err(refused_or_failed)?;
        tx.rollback().await.map_err(Error::Database)?;
        rows.iter()
            .map(object)
            .collect::<Result<Vec<_>, _>>()
            .map(Value::Array)
    }
}

fn refused_or_failed(e: tokio_postgres::Error) -> Error {
    match e.code() {
        Some(code)
            if *code == SqlState::READ_ONLY_SQL_TRANSACTION
                || *code == SqlState::INSUFFICIENT_PRIVILEGE =>
        {
            Error::Refused(e)
        }
        _ => Error::Query(e),
    }
}

fn object(row: &Row) -> Result<Value, Error> {
    let mut out = Map::new();
    for (i, column) in row.columns().iter().enumerate() {
        out.insert(column.name().to_string(), cell(row, i, column)?);
    }
    Ok(Value::Object(out))
}

fn cell(row: &Row, i: usize, column: &Column) -> Result<Value, Error> {
    let value = match *column.type_() {
        Type::BOOL => row
            .try_get::<_, Option<bool>>(i)
            .map(|v| v.map(Value::from)),
        Type::INT2 => row.try_get::<_, Option<i16>>(i).map(|v| v.map(Value::from)),
        Type::INT4 => row.try_get::<_, Option<i32>>(i).map(|v| v.map(Value::from)),
        Type::INT8 => row.try_get::<_, Option<i64>>(i).map(|v| v.map(Value::from)),
        Type::FLOAT4 => row
            .try_get::<_, Option<f32>>(i)
            .map(|v| v.map(|f| Value::from(f64::from(f)))),
        Type::FLOAT8 => row.try_get::<_, Option<f64>>(i).map(|v| v.map(Value::from)),
        Type::TEXT | Type::VARCHAR | Type::BPCHAR | Type::NAME => row
            .try_get::<_, Option<String>>(i)
            .map(|v| v.map(Value::from)),
        Type::JSON | Type::JSONB => row.try_get::<_, Option<Value>>(i),
        _ => {
            return Err(Error::Unsupported {
                column: column.name().to_string(),
                ty: column.type_().name().to_string(),
            });
        }
    };
    Ok(value.map_err(Error::Query)?.unwrap_or(Value::Null))
}
