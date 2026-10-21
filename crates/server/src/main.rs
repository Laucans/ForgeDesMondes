//! The product's one server binary: env → migrations → engine → fixtures →
//! listen (see the library for what each variable means).
//!
//!   server              serves
//!   server inventory    lists the registered units, with what each needs

use std::path::Path;
use std::process::ExitCode;
use std::sync::Arc;

use dataguard_server::engine::plugins::Shared;
use dataguard_server::{MEMORY, Planted, applier, planted_with};
use ports_llm::SharedLlm;
use ports_speech_to_text::SharedSpeechToText;
use server::{Config, Error, Ports, app, fixtures, inventory, llm, reads, speech_to_text};

/// The product's migrations, the Data layer's `crates/data/migrations/*.sql`
/// — among the tracked `*migrations/*.sql` `scripts/rules-runtime.sh` and
/// `scripts/dump-schema.sh` read. The rules' runtime applies the same files
/// with `psql` before it starts this server, and records nothing in
/// `public.data_migrations`: until the two agree, a migration must hold
/// being run twice (`IF NOT EXISTS`).
const MIGRATIONS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../data/migrations");

#[tokio::main]
async fn main() -> ExitCode {
    if std::env::args().nth(1).as_deref() == Some("inventory") {
        print!("{}", inventory());
        return ExitCode::SUCCESS;
    }
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

    let (reads, names) = match &config.fixtures {
        None => {
            eprintln!("server: no FIXTURES_DIR — starting without fixtures");
            (reads(&config).await?, Vec::new())
        }
        Some(dir) => match fixtures(dir) {
            Err(e) => {
                eprintln!(
                    "server: cannot read the fixtures in {}: {e} — starting without them",
                    dir.display()
                );
                (reads(&config).await?, Vec::new())
            }
            Ok(plan) => {
                if config.database_url == MEMORY {
                    eprintln!("server: the in-memory store holds no table — no fixture loaded");
                } else {
                    // One transaction: the fixtures are all there, or none is.
                    let loaded = match data::read_write().await {
                        Ok(mut client) => data::insert_missing(&mut client, &plan.tables).await,
                        Err(e) => Err(e),
                    };
                    match loaded {
                        Ok(rows) => eprintln!(
                            "server: fixtures in {}: {} loaded ({rows} row(s) inserted), {} skipped",
                            dir.display(),
                            plan.loaded.len(),
                            plan.skipped.len()
                        ),
                        Err(e) => eprintln!(
                            "server: cannot load the fixtures in {}: {e} — starting without them",
                            dir.display()
                        ),
                    }
                }
                for (name, why) in &plan.skipped {
                    eprintln!("server: fixture {name} skipped — {why}");
                }
                (reads(&config).await?, plan.names)
            }
        },
    };
    let reads = Arc::new(reads.named(names));

    // One port each, shared: the legacy chain and the units' `Prepare`s
    // reach the same fake, and read the same journal.
    let llm = SharedLlm::new(llm(config.ports, config.fixtures.as_deref()));
    let speech = SharedSpeechToText::new(speech_to_text(config.ports, config.fixtures.as_deref()));
    let mut shared = Shared::default();
    shared.insert(Arc::new(llm.clone()));
    shared.insert(Arc::new(speech.clone()));

    // After the fixtures: the engine records the aliases the tables hold.
    let Planted { engine, rows } = planted_with(&config.database_url, shared).await?;
    let engine = Arc::new(engine);
    let (llm, speech) = (Arc::new(llm), Arc::new(speech));
    if config.read_only_url.is_none() && config.database_url != MEMORY {
        eprintln!(
            "server: no DATABASE_URL_READONLY — the Capabilities read through DATABASE_URL, every read in a READ ONLY transaction"
        );
    }

    let addr = config.addr;
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .map_err(|source| Error::Serve { addr, source })?;
    let applier = applier(engine.clone());
    eprintln!("server: listening on {addr}");
    let served = axum::serve(listener, app(engine, rows, llm, speech, reads))
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
