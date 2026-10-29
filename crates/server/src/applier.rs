//! The single applier this server runs: the engine's queue driven every
//! second, as `crates/dataguard/server`'s applier drives it, and at once
//! when a write reaches the wire — a command waits for no tick before it is
//! applied, so what one tab does shows in the other within the second (R114).
//!
//! It never runs while a write is being answered ([`Writes`]): the engine
//! queues a command, then the follow-ups its `Prepare` asked for, one by
//! one, and an applier run in between applied the command alone — an entry
//! « à créer » stamped with an element not saved yet, rejected by the
//! table's foreign key (R103).

use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::extract::{Request, State};
use axum::http::Method;
use axum::middleware::{self, Next};
use axum::response::Response;
use dataguard_server::engine::engine::Engine;
use tokio::sync::{Notify, RwLock};
use tokio::task::JoinHandle;

/// How long the applier waits when no write wakes it: the holds' and the
/// parked commands' expiries are seen within it.
const TICK: Duration = Duration::from_secs(1);

/// The routes that read and never write: no reason to wake the applier.
const READS: &str = "/capabilities/";

/// The writes the wire is answering, each holding a read guard while it is:
/// the applier runs only when it takes the write guard at once.
pub type Writes = Arc<RwLock<()>>;

/// The single applier over `engine`: every second, and as soon as `wake` is
/// notified, unless a write is being answered. Its handle is the caller's,
/// to abort on shutdown.
#[must_use]
pub fn applier(engine: Arc<Engine>, wake: Arc<Notify>, writes: Writes) -> JoinHandle<()> {
    driving(wake, writes, TICK, move || {
        let engine = Arc::clone(&engine);
        async move {
            if let Err(e) = engine.run_once().await {
                eprintln!("applier: {e}");
            }
        }
    })
}

/// `run` once per `tick`, and once more each time `wake` is notified —
/// skipped while a write holds `writes`: its answer wakes it again.
fn driving<F, R>(wake: Arc<Notify>, writes: Writes, tick: Duration, run: R) -> JoinHandle<()>
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
            if let Ok(_quiet) = writes.try_write() {
                run().await;
            }
        }
    })
}

/// `app` holding `writes` while it answers each write — a command
/// submitted, confirmed, cancelled or requeued, a hold or a review — and
/// notifying `wake` once it has been answered, its command and their
/// follow-ups queued.
pub fn waking(app: Router, wake: Arc<Notify>, writes: Writes) -> Router {
    app.layer(middleware::from_fn_with_state((wake, writes), woken))
}

/// One request through [`waking`]: answered — a write under a read guard of
/// `writes` — then the applier woken when it was a write.
async fn woken(
    State((wake, writes)): State<(Arc<Notify>, Writes)>,
    request: Request,
    next: Next,
) -> Response {
    let write = request.method() == Method::POST && !request.uri().path().starts_with(READS);
    if !write {
        return next.run(request).await;
    }
    let answering = writes.read().await;
    let response = next.run(request).await;
    drop(answering);
    wake.notify_one();
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

    /// `uri` sent with `POST` through `waking` over routes that answer 202
    /// while `writes` is held, 200 when it is not: whether it was, answering.
    async fn through(wake: &Arc<Notify>, writes: &Writes, uri: &str) -> bool {
        let held = |writes: Writes| async move {
            if writes.try_write().is_err() {
                StatusCode::ACCEPTED
            } else {
                StatusCode::OK
            }
        };
        let (commands, reads) = (Arc::clone(writes), Arc::clone(writes));
        let app = Router::new()
            .route("/commands", post(move || held(commands)))
            .route("/capabilities/{id}", post(move || held(reads)));
        let request = Request::post(uri).body(Body::empty()).expect("a request");
        let response = waking(app, Arc::clone(wake), Arc::clone(writes))
            .oneshot(request)
            .await
            .expect("an answer");
        response.status() == StatusCode::ACCEPTED
    }

    #[tokio::test]
    async fn a_command_wakes_the_applier() {
        let wake = Arc::new(Notify::new());
        through(&wake, &Writes::default(), "/commands").await;
        assert!(notified(&wake).await);
    }

    #[tokio::test]
    async fn a_read_does_not_wake_the_applier() {
        let wake = Arc::new(Notify::new());
        through(
            &wake,
            &Writes::default(),
            "/capabilities/table.getBattlemapInstance",
        )
        .await;
        assert!(!notified(&wake).await);
    }

    #[tokio::test]
    async fn a_command_holds_the_writes_while_answered_and_releases_them() {
        let writes = Writes::default();
        let wake = Arc::new(Notify::new());
        assert!(through(&wake, &writes, "/commands").await);
        assert!(writes.try_write().is_ok(), "released once answered");
    }

    #[tokio::test]
    async fn a_read_does_not_hold_the_writes() {
        let writes = Writes::default();
        let wake = Arc::new(Notify::new());
        assert!(!through(&wake, &writes, "/capabilities/table.getBattlemapInstance").await);
    }

    #[tokio::test]
    async fn the_applier_waits_while_a_write_is_answered() {
        let wake = Arc::new(Notify::new());
        let writes = Writes::default();
        let runs = Arc::new(AtomicUsize::new(0));
        let counted = Arc::clone(&runs);
        let handle = driving(
            Arc::clone(&wake),
            Arc::clone(&writes),
            Duration::from_millis(10),
            move || {
                let counted = Arc::clone(&counted);
                async move {
                    counted.fetch_add(1, Ordering::SeqCst);
                }
            },
        );
        let answering = writes.read().await;
        wake.notify_one();
        tokio::time::sleep(Duration::from_millis(100)).await;
        let while_answering = runs.load(Ordering::SeqCst);
        drop(answering);
        tokio::time::sleep(Duration::from_millis(100)).await;
        handle.abort();
        assert_eq!(while_answering, 0, "the applier ran amid a write");
        assert!(
            runs.load(Ordering::SeqCst) >= 1,
            "the applier never ran after it"
        );
    }

    #[tokio::test]
    async fn the_applier_runs_when_woken_without_waiting_for_its_tick() {
        let wake = Arc::new(Notify::new());
        let runs = Arc::new(AtomicUsize::new(0));
        let counted = Arc::clone(&runs);
        let handle = driving(
            Arc::clone(&wake),
            Writes::default(),
            Duration::from_secs(3600),
            move || {
                let counted = Arc::clone(&counted);
                async move {
                    counted.fetch_add(1, Ordering::SeqCst);
                }
            },
        );
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
            Writes::default(),
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
