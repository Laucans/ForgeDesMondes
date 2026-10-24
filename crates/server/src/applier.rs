//! The single applier this server runs: the engine's queue driven every
//! second, as `crates/dataguard/server`'s applier drives it, and at once
//! when a write reaches the wire — a command waits for no tick before it is
//! applied, so what one tab does shows in the other within the second (R114).

use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::extract::{Request, State};
use axum::http::Method;
use axum::middleware::{self, Next};
use axum::response::Response;
use dataguard_server::engine::engine::Engine;
use tokio::sync::Notify;
use tokio::task::JoinHandle;

/// How long the applier waits when no write wakes it: the holds' and the
/// parked commands' expiries are seen within it.
const TICK: Duration = Duration::from_secs(1);

/// The routes that read and never write: no reason to wake the applier.
const READS: &str = "/capabilities/";

/// The single applier over `engine`: every second, and as soon as `wake` is
/// notified. Its handle is the caller's, to abort on shutdown.
#[must_use]
pub fn applier(engine: Arc<Engine>, wake: Arc<Notify>) -> JoinHandle<()> {
    driving(wake, TICK, move || {
        let engine = Arc::clone(&engine);
        async move {
            if let Err(e) = engine.run_once().await {
                eprintln!("applier: {e}");
            }
        }
    })
}

/// `run` once per `tick`, and once more each time `wake` is notified.
fn driving<F, R>(wake: Arc<Notify>, tick: Duration, run: R) -> JoinHandle<()>
where
    R: Fn() -> F + Send + 'static,
    F: Future<Output = ()> + Send,
{
    tokio::spawn(async move {
        loop {
            tokio::select! {
                () = tokio::time::sleep(tick) => {}
                () = wake.notified() => {}
            }
            run().await;
        }
    })
}

/// `app` notifying `wake` once each write it answers — a command
/// submitted, confirmed, cancelled or requeued, a hold or a review — has
/// been answered, its command queued.
pub fn waking(app: Router, wake: Arc<Notify>) -> Router {
    app.layer(middleware::from_fn_with_state(wake, woken))
}

/// One request through [`waking`]: answered, then the applier woken when it
/// was a write.
async fn woken(State(wake): State<Arc<Notify>>, request: Request, next: Next) -> Response {
    let write = request.method() == Method::POST && !request.uri().path().starts_with(READS);
    let response = next.run(request).await;
    if write {
        wake.notify_one();
    }
    response
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use axum::body::Body;
    use axum::http::StatusCode;
    use axum::routing::post;
    use tower::ServiceExt;

    use super::*;

    /// Whether `wake` is notified within a short while.
    async fn notified(wake: &Notify) -> bool {
        tokio::time::timeout(Duration::from_millis(100), wake.notified())
            .await
            .is_ok()
    }

    /// `uri` sent with `POST` through `waking` over routes that answer 202.
    async fn through(wake: &Arc<Notify>, uri: &str) {
        let app = Router::new()
            .route("/commands", post(|| async { StatusCode::ACCEPTED }))
            .route(
                "/capabilities/{id}",
                post(|| async { StatusCode::ACCEPTED }),
            );
        let request = Request::post(uri).body(Body::empty()).expect("a request");
        let response = waking(app, Arc::clone(wake))
            .oneshot(request)
            .await
            .expect("an answer");
        assert_eq!(response.status(), StatusCode::ACCEPTED);
    }

    #[tokio::test]
    async fn a_command_wakes_the_applier() {
        let wake = Arc::new(Notify::new());
        through(&wake, "/commands").await;
        assert!(notified(&wake).await);
    }

    #[tokio::test]
    async fn a_read_does_not_wake_the_applier() {
        let wake = Arc::new(Notify::new());
        through(&wake, "/capabilities/table.getBattlemapInstance").await;
        assert!(!notified(&wake).await);
    }

    #[tokio::test]
    async fn the_applier_runs_when_woken_without_waiting_for_its_tick() {
        let wake = Arc::new(Notify::new());
        let runs = Arc::new(AtomicUsize::new(0));
        let counted = Arc::clone(&runs);
        let handle = driving(Arc::clone(&wake), Duration::from_secs(3600), move || {
            let counted = Arc::clone(&counted);
            async move {
                counted.fetch_add(1, Ordering::SeqCst);
            }
        });
        wake.notify_one();
        let ran = tokio::time::timeout(Duration::from_secs(1), async {
            while runs.load(Ordering::SeqCst) == 0 {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await;
        handle.abort();
        assert!(ran.is_ok(), "the applier did not run once woken");
    }

    #[tokio::test]
    async fn the_applier_runs_every_tick_unwoken() {
        let runs = Arc::new(AtomicUsize::new(0));
        let counted = Arc::clone(&runs);
        let handle = driving(
            Arc::new(Notify::new()),
            Duration::from_millis(10),
            move || {
                let counted = Arc::clone(&counted);
                async move {
                    counted.fetch_add(1, Ordering::SeqCst);
                }
            },
        );
        tokio::time::sleep(Duration::from_millis(200)).await;
        handle.abort();
        assert!(runs.load(Ordering::SeqCst) >= 2);
    }
}
