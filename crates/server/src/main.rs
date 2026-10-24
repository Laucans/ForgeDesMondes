//! The product's one server binary: env → migrations → engine → fixtures →
//! listen (see the library for what each variable means).
//!
//!   server              serves
//!   server inventory    lists the registered units, with what each needs

use std::path::Path;
use std::process::ExitCode;
use std::sync::Arc;

use dataguard_server::MEMORY;
use ports_character_sheets::SharedCharacterSheets;
use ports_llm::SharedLlm;
use ports_speech_to_text::SharedSpeechToText;
use server::{
    Config, Error, Names, Ports, app, applier, character_sheets, fixtures, inventory, llm, plant,
    renaming, shared, speech_to_text, waking,
};
use tokio::sync::Notify;

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

    let names = match &config.fixtures {
        None => {
            eprintln!("server: no FIXTURES_DIR — starting without fixtures");
            Vec::new()
        }
        Some(dir) => match fixtures(dir) {
            Err(e) => {
                eprintln!(
                    "server: cannot read the fixtures in {}: {e} — starting without them",
                    dir.display()
                );
                Vec::new()
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
                plan.names
            }
        },
    };
    let names = Names::new(names);

    // One port each, shared: the units' `Prepare`s and the journal's route
    // reach the same fake, and read the same journal.
    let llm = SharedLlm::new(llm(config.ports, config.fixtures.as_deref()));
    let speech = SharedSpeechToText::new(speech_to_text(config.ports, config.fixtures.as_deref()));
    let sheets =
        SharedCharacterSheets::new(character_sheets(config.ports, config.fixtures.as_deref()));

    // After the fixtures: the engine's aggregates warm from the tables they
    // hold (the Entity aggregate's aliases, its versions).
    let planted = plant(&config, shared(&llm, &speech, &sheets)).await?;
    if config.read_only_url.is_none() && config.database_url != MEMORY {
        eprintln!(
            "server: no DATABASE_URL_READONLY — the Capabilities read through DATABASE_URL, every read in a READ ONLY transaction"
        );
    }

    let addr = config.addr;
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .map_err(|source| Error::Serve { addr, source })?;
    // Woken by every write the wire answers: a command is applied at once,
    // not at the next second's tick.
    let wake = Arc::new(Notify::new());
    let applier = applier(Arc::clone(&planted.engine), Arc::clone(&wake));
    eprintln!("server: listening on {addr}");
    // The fixtures' slugs on the wire, their uuids in the store (D8).
    let app = waking(renaming(app(&planted, Arc::new(llm)), names), wake);
    let served = axum::serve(listener, app)
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
