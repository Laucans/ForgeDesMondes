//! The product's one server binary: env → migrations → engine → fixtures →
//! listen (see the library for what each variable means).

use std::path::Path;
use std::process::ExitCode;
use std::sync::Arc;

use dataguard_server::{MEMORY, Planted, applier, planted};
use server::{Config, Error, Ports, app, fixtures, llm, reads};

/// The product's migrations, the Data layer's `crates/data/migrations/*.sql`
/// — among the tracked `*migrations/*.sql` `scripts/rules-runtime.sh` and
/// `scripts/dump-schema.sh` read. The rules' runtime applies the same files
/// with `psql` before it starts this server, and records nothing in
/// `public.data_migrations`: until the two agree, a migration must hold
/// being run twice (`IF NOT EXISTS`).
const MIGRATIONS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../data/migrations");

#[tokio::main]
async fn main() -> ExitCode {
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("server: {e}");
            ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<(), Error> {
    let config = Config::read(|name| std::env::var(name).ok())?;
    match config.ports {
        Ports::Fake => {
            eprintln!("server: PORTS=fake — every port on its fake, nothing reaches outside")
        }
    }

    let migrations = Path::new(MIGRATIONS);
    if config.database_url == MEMORY {
        eprintln!("server: the in-memory store — no migration to apply");
    } else if migrations.is_dir() {
        let mut client = data::read_write().await?;
        let applied = data::migrate(&mut client, migrations).await?;
        eprintln!(
            "server: {} migration(s) applied: {applied:?}",
            applied.len()
        );
    } else {
        eprintln!("server: no migrations/ folder — no migration to apply");
    }

    let Planted { engine, rows } = planted(&config.database_url).await?;
    let engine = Arc::new(engine);
    let llm = Arc::new(llm(config.ports, config.fixtures.as_deref()));
    if config.read_only_url.is_none() && config.database_url != MEMORY {
        eprintln!(
            "server: no DATABASE_URL_READONLY — the Capabilities read through DATABASE_URL, every read in a READ ONLY transaction"
        );
    }
    let reads = Arc::new(reads(&config).await?);

    match &config.fixtures {
        None => eprintln!("server: no FIXTURES_DIR — starting without fixtures"),
        Some(dir) => match fixtures(dir) {
            Err(e) => eprintln!(
                "server: cannot read the fixtures in {}: {e} — starting without them",
                dir.display()
            ),
            Ok(skipped) => {
                eprintln!(
                    "server: fixtures in {}: 0 loaded, {} skipped",
                    dir.display(),
                    skipped.len()
                );
                for (name, why) in &skipped {
                    eprintln!("server: fixture {name} skipped — {why}");
                }
            }
        },
    }

    let addr = config.addr;
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .map_err(|source| Error::Serve { addr, source })?;
    let applier = applier(engine.clone());
    eprintln!("server: listening on {addr}");
    let served = axum::serve(listener, app(engine, rows, llm, reads))
        .with_graceful_shutdown(stopped())
        .await
        .map_err(|source| Error::Serve { addr, source });
    applier.abort();
    served
}

/// Resolves on Ctrl-C, or on the SIGTERM `scripts/rules-runtime.sh stop`
/// sends.
async fn stopped() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        if let Ok(mut term) = signal(SignalKind::terminate()) {
            tokio::select! {
                _ = tokio::signal::ctrl_c() => {}
                _ = term.recv() => {}
            }
            return;
        }
    }
    let _ = tokio::signal::ctrl_c().await;
}
