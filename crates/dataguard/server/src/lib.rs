//! The product's DataGuard server: the engine of `harness-dataguard` bound
//! to this product's aggregates and DataCapabilities, over its PostgreSQL,
//! speaking the wire protocol the shell client and the mock already speak.
//!
//! What a binary serves it with: [`plant`] builds the engine, [`router`]
//! answers the wire protocol through it, [`applier`] runs the single
//! applier. This crate's own binary (`main.rs`) and the product's one server
//! (`crates/server`) both mount them.
//!
//! `DATABASE_URL` names the database — or is `memory`, and the server runs
//! on the engine's in-memory store, which forgets everything at exit: for
//! a mock-up, a demo, a smoke test. The invariants a product executes
//! beyond `aggregate.json`'s rules, and the AI that proposes
//! reconciliations, are wired in `plant()` — the two places this file
//! expects the product to grow.

use std::sync::Arc;

use axum::extract::{Path, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use harness_dataguard::Error;
use harness_dataguard::aggregate::Aggregate;
use harness_dataguard::api::{HoldRequest, ReadRequest, ReadResponse, ReviewDecision, Submission};
pub use harness_dataguard::engine::Engine;
use harness_dataguard::manifest::DataCapability;
use harness_dataguard::memory::{MemoryNotifier, MemoryStore};
use harness_dataguard::postgres::{PgStore, SystemClock};
use harness_dataguard::registry::Registry;
use harness_dataguard::store::Store;

/// The `DATABASE_URL` that means no database: the in-memory store.
pub const MEMORY: &str = "memory";

mod registry {
    include!(concat!(env!("OUT_DIR"), "/registry.rs"));
}

type Shared = Arc<Engine>;

/// Why the engine could not be planted.
#[derive(Debug, thiserror::Error)]
pub enum PlantError {
    /// The database could not be reached.
    #[error("database: {0}")]
    Database(#[from] tokio_postgres::Error),
    /// A manifest the checkout holds does not parse.
    #[error("manifest: {0}")]
    Manifest(#[from] serde_json::Error),
    /// The engine refused its store or its queue.
    #[error("engine: {0}")]
    Engine(#[from] Error),
}

/// The engine over this product's manifests and its database.
///
/// # Errors
///
/// [`PlantError`] when the database cannot be reached, a manifest does not
/// parse, or the engine cannot load its queue.
pub async fn plant(url: &str) -> Result<Engine, PlantError> {
    let store: Arc<dyn Store> = if url == MEMORY {
        Arc::new(MemoryStore::default())
    } else {
        let (client, connection) = tokio_postgres::connect(url, tokio_postgres::NoTls).await?;
        tokio::spawn(async move {
            if let Err(e) = connection.await {
                eprintln!("postgres: {e}");
            }
        });
        Arc::new(PgStore::open(client).await?)
    };
    let mut engine = Engine::new(
        store,
        Arc::new(MemoryNotifier::default()),
        Arc::new(SystemClock),
    )
    .with_registry(Registry::parse(registry::REGISTRY)?);
    for text in registry::AGGREGATES {
        engine = engine.with_aggregate(Aggregate::parse(text)?);
    }
    for text in registry::DATA_CAPABILITIES {
        engine = engine.with_data_capability(DataCapability::parse(text)?);
    }
    // engine = engine.with_invariants(Arc::new(my_product::Invariants));
    // engine = engine.with_proposer(Arc::new(my_product::Proposer));
    engine.load().await?;
    Ok(engine)
}

/// The shell's wire protocol, answered by `engine`.
pub fn router(engine: Arc<Engine>) -> Router {
    Router::new()
        .route("/capabilities/{id}", post(capability))
        .route("/commands", post(submit))
        .route("/commands/{id}", get(status))
        .route("/commands/{id}/confirm", post(confirm))
        .route("/commands/{id}/cancel", post(cancel))
        .route("/commands/{id}/requeue", post(requeue))
        .route("/holds", post(hold))
        .route("/holds/{id}/renew", post(renew))
        .route("/holds/{id}/release", post(release))
        .route("/reviews/{id}", post(decide))
        .route("/dataVersion", get(version))
        .with_state(engine)
}

/// The single applier: every second, each partition's head. It runs until
/// the handle is aborted.
pub fn applier(engine: Arc<Engine>) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            if let Err(e) = engine.run_once().await {
                eprintln!("applier: {e}");
            }
        }
    })
}

fn status_of(error: &Error) -> axum::http::StatusCode {
    use axum::http::StatusCode;
    match error {
        Error::NotFound(_) | Error::UnknownDataCapability(_) | Error::UnregisteredQuery(_) => {
            StatusCode::NOT_FOUND
        }
        Error::Forbidden(_) => StatusCode::FORBIDDEN,
        Error::NotNow(_) => StatusCode::CONFLICT,
        Error::Store(_) => StatusCode::SERVICE_UNAVAILABLE,
    }
}

type Reply<T> =
    Result<(axum::http::StatusCode, Json<T>), (axum::http::StatusCode, Json<serde_json::Value>)>;

fn reply<T>(status: axum::http::StatusCode, result: Result<T, Error>) -> Reply<T> {
    result.map(|value| (status, Json(value))).map_err(|e| {
        (
            status_of(&e),
            Json(serde_json::json!({ "error": e.to_string() })),
        )
    })
}

async fn submit(
    State(engine): State<Shared>,
    Json(body): Json<Submission>,
) -> Reply<harness_dataguard::api::Submitted> {
    reply(axum::http::StatusCode::ACCEPTED, engine.submit(body).await)
}

async fn status(State(engine): State<Shared>, Path(id): Path<String>) -> Reply<serde_json::Value> {
    reply(
        axum::http::StatusCode::OK,
        engine
            .status(&id)
            .await
            .map(|r| serde_json::json!({ "result": r })),
    )
}

#[derive(serde::Deserialize)]
struct By {
    by: String,
}

async fn confirm(
    State(engine): State<Shared>,
    Path(id): Path<String>,
    Json(who): Json<By>,
) -> Reply<serde_json::Value> {
    reply(
        axum::http::StatusCode::OK,
        engine
            .confirm(&id, &who.by)
            .await
            .map(|r| serde_json::json!({ "result": r })),
    )
}

async fn cancel(
    State(engine): State<Shared>,
    Path(id): Path<String>,
    Json(who): Json<By>,
) -> Reply<serde_json::Value> {
    reply(
        axum::http::StatusCode::OK,
        engine
            .cancel(&id, &who.by)
            .await
            .map(|r| serde_json::json!({ "result": r })),
    )
}

async fn requeue(
    State(engine): State<Shared>,
    Path(id): Path<String>,
    Json(who): Json<By>,
) -> Reply<harness_dataguard::api::Submitted> {
    reply(
        axum::http::StatusCode::ACCEPTED,
        engine.requeue(&id, &who.by).await,
    )
}

async fn hold(
    State(engine): State<Shared>,
    Json(body): Json<HoldRequest>,
) -> Reply<harness_dataguard::queue::Hold> {
    let ttl = body
        .ttl
        .as_deref()
        .and_then(harness_dataguard::aggregate::duration_seconds);
    reply(
        axum::http::StatusCode::CREATED,
        engine.hold(&body.partition, &body.by, ttl).await,
    )
}

async fn renew(
    State(engine): State<Shared>,
    Path(id): Path<String>,
) -> Reply<harness_dataguard::queue::Hold> {
    reply(axum::http::StatusCode::OK, engine.renew(&id).await)
}

async fn release(State(engine): State<Shared>, Path(id): Path<String>) -> Reply<serde_json::Value> {
    reply(
        axum::http::StatusCode::OK,
        engine.release(&id).await.map(|()| serde_json::json!({})),
    )
}

async fn decide(
    State(engine): State<Shared>,
    Path(id): Path<String>,
    Json(body): Json<ReviewDecision>,
) -> Reply<serde_json::Value> {
    reply(
        axum::http::StatusCode::OK,
        engine
            .decide(&id, &body.by, &body.roles, body.approved)
            .await
            .map(|r| serde_json::json!({ "result": r })),
    )
}

/// A read through a persisted query: served only when the registry holds
/// the Capability's query under this hash (contract C). What the query
/// answers is the product's read crate's — `data` is where it plugs in.
async fn capability(
    State(engine): State<Shared>,
    Path(id): Path<String>,
    Json(body): Json<ReadRequest>,
) -> Reply<ReadResponse> {
    let served = engine.served(&id, &body.sha256);
    let answer = match served {
        Ok(()) => engine.data_version().await.map(|as_of| ReadResponse {
            as_of,
            // The product's read crate answers here:
            // `<system>_read::run(&id, &body.variables)`.
            data: serde_json::Value::Null,
        }),
        Err(e) => Err(e),
    };
    reply(axum::http::StatusCode::OK, answer)
}

async fn version(State(engine): State<Shared>) -> Reply<serde_json::Value> {
    reply(
        axum::http::StatusCode::OK,
        engine
            .data_version()
            .await
            .map(|v| serde_json::json!({ "dataVersion": v })),
    )
}

#[cfg(test)]
mod tests {
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    use super::*;

    /// What `router` answers to one request, over the in-memory engine.
    async fn answer(request: Request<Body>) -> (StatusCode, serde_json::Value) {
        let engine = plant(MEMORY).await.expect("the in-memory engine");
        let response = router(Arc::new(engine))
            .oneshot(request)
            .await
            .expect("an answer");
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("a body");
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null),
        )
    }

    #[tokio::test]
    async fn the_memory_url_plants_the_in_memory_engine() {
        let engine = plant(MEMORY).await.expect("no database is reached");
        assert_eq!(engine.data_version().await.expect("a version"), 0);
    }

    #[tokio::test]
    async fn the_router_answers_the_data_version() {
        let request = Request::get("/dataVersion").body(Body::empty()).unwrap();
        let (status, body) = answer(request).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body, serde_json::json!({ "dataVersion": 0 }));
    }

    #[tokio::test]
    async fn an_unknown_data_capability_is_refused_typed() {
        let request = Request::post("/commands")
            .header("content-type", "application/json")
            .body(Body::from(r#"{"dataCapability":"x.y"}"#))
            .unwrap();
        let (status, body) = answer(request).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(
            body,
            serde_json::json!({ "error": "no DataCapability named x.y" })
        );
    }

    #[test]
    fn each_refusal_has_its_status() {
        let cases = [
            (Error::NotFound(String::new()), StatusCode::NOT_FOUND),
            (
                Error::UnknownDataCapability(String::new()),
                StatusCode::NOT_FOUND,
            ),
            (
                Error::UnregisteredQuery(String::new()),
                StatusCode::NOT_FOUND,
            ),
            (Error::Forbidden(String::new()), StatusCode::FORBIDDEN),
            (Error::NotNow(String::new()), StatusCode::CONFLICT),
            (Error::Store(String::new()), StatusCode::SERVICE_UNAVAILABLE),
        ];
        for (error, status) in cases {
            assert_eq!(status_of(&error), status, "{error:?}");
        }
    }
}
