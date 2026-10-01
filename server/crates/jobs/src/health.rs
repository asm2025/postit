//! Worker liveness for `/ready`.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use tokio::sync::watch;
use tokio::task::JoinSet;

/// Whether this process's job worker can make progress: storage migrated and every
/// supervised task (queue workers, the insert listener, the outbox relay, recurring loops)
/// still running. Cheap to clone; read by the worker port's `/ready`.
#[derive(Clone, Default)]
pub struct WorkerHealth {
    started: Arc<AtomicBool>,
    dead: Arc<AtomicBool>,
}

impl WorkerHealth {
    #[must_use]
    pub fn is_ready(&self) -> bool {
        self.started.load(Ordering::Acquire) && !self.dead.load(Ordering::Acquire)
    }

    pub(crate) fn mark_started(&self) {
        self.started.store(true, Ordering::Release);
    }

    pub(crate) fn mark_dead(&self, what: &str) {
        tracing::error!(
            task = what,
            "job worker task ended before shutdown; worker not ready"
        );
        self.dead.store(true, Ordering::Release);
    }
}

/// Joins `tasks`; any task that ends (returns or panics) while `shutdown` is still false
/// marks `health` dead.
pub(crate) async fn supervise(
    mut tasks: JoinSet<()>,
    shutdown: watch::Receiver<bool>,
    health: WorkerHealth,
    what: &'static str,
) {
    while let Some(joined) = tasks.join_next().await {
        if let Err(err) = &joined {
            tracing::error!(error = %err, task = what, "job worker task ended abnormally");
        }
        if !*shutdown.borrow() {
            health.mark_dead(what);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn a_task_panicking_before_shutdown_marks_the_worker_dead() {
        let health = WorkerHealth::default();
        health.mark_started();
        let (_stop_tx, stop_rx) = watch::channel(false);
        let mut tasks = JoinSet::new();
        tasks.spawn(async { unreachable!("simulated queue supervisor panic") });
        supervise(tasks, stop_rx, health.clone(), "queue").await;
        assert!(!health.is_ready());
    }

    #[tokio::test]
    async fn tasks_ending_after_shutdown_keep_the_worker_healthy() {
        let health = WorkerHealth::default();
        health.mark_started();
        let (stop_tx, stop_rx) = watch::channel(false);
        let _ = stop_tx.send(true);
        let mut tasks = JoinSet::new();
        tasks.spawn(async {});
        supervise(tasks, stop_rx, health.clone(), "queue").await;
        assert!(health.is_ready());
    }

    #[test]
    fn not_ready_before_start() {
        assert!(!WorkerHealth::default().is_ready());
    }
}
