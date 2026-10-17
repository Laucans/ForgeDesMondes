//! The DataGuard server's own binary, over the library of this crate:
//!
//!   dataguard-server migrate          applies the engine's own migration
//!   dataguard-server serve [addr]     serves, 127.0.0.1:8800 by default
//!
//! `DATABASE_URL` names the database, or is `memory` (see the library).

use std::sync::Arc;

use dataguard_server::{MEMORY, applier, plant, router};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let url = std::env::var("DATABASE_URL").map_err(|_| "DATABASE_URL must name the database")?;
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("migrate") if url == MEMORY => {
            println!("nothing to migrate: the in-memory store");
            Ok(())
        }
        Some("migrate") => {
            let (client, connection) = tokio_postgres::connect(&url, tokio_postgres::NoTls).await?;
            tokio::spawn(connection);
            client
                .batch_execute(harness_dataguard::postgres::MIGRATION)
                .await?;
            println!("migrated");
            Ok(())
        }
        Some("serve") | None => {
            let engine = Arc::new(plant(&url).await?);
            let addr = args.next().unwrap_or_else(|| "127.0.0.1:8800".to_string());
            let app = router(engine.clone());
            let listener = tokio::net::TcpListener::bind(&addr).await?;
            let applier = applier(engine);
            println!("dataguard-server on {addr}");
            axum::serve(listener, app)
                .with_graceful_shutdown(async {
                    let _ = tokio::signal::ctrl_c().await;
                })
                .await?;
            applier.abort();
            Ok(())
        }
        Some(other) => Err(format!("unknown command {other}: migrate | serve [addr]").into()),
    }
}
