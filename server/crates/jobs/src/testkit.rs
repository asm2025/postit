//! Test helpers for crates that run jobs: settings with a short poll interval, a worker
//! running in the background, and a polling wait.

use std::collections::HashMap;
use std::time::Duration;

use postit_config::{JobHistoryRetention, JobSchedules, JobsSettings};
use sqlx::PgPool;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

use crate::{JobRegistry, JobsError, Worker, migrate};

/// Poll 200 ms, concurrency 2 per queue, daily schedules.
#[must_use]
pub fn jobs_settings() -> JobsSettings {
    JobsSettings {
        concurrency: HashMap::from([
            ("mail".to_string(), 2),
            ("maintenance".to_string(), 2),
            ("default".to_string(), 2),
        ]),
        outbox_poll_interval: Duration::from_millis(200),
        history_retention: JobHistoryRetention {
            succeeded: Duration::from_hours(7 * 24),
            failed: Duration::from_hours(30 * 24),
        },
        schedules: JobSchedules {
            job_history_purge: "0 10 3 * * *".into(),
            purge_pending_users: "0 20 3 * * *".into(),
            audit_retention: "0 30 3 * * *".into(),
            data_retention: "0 40 3 * * *".into(),
        },
    }
}

/// A [`Worker`] running on a background task until [`RunningWorker::stop`].
pub struct RunningWorker {
    stop: oneshot::Sender<()>,
    handle: JoinHandle<Result<(), JobsError>>,
}

impl RunningWorker {
    /// Runs [`migrate`], then starts a worker with [`jobs_settings`].
    pub async fn start(pool: PgPool, registry: JobRegistry) -> Self {
        Self::start_with(pool, jobs_settings(), registry).await
    }

    /// Runs [`migrate`], then starts a worker with `settings`.
    pub async fn start_with(pool: PgPool, settings: JobsSettings, registry: JobRegistry) -> Self {
        migrate(&pool)
            .await
            .unwrap_or_else(|e| unreachable!("job storage migrations: {e}"));
        let (stop, stopped) = oneshot::channel::<()>();
        let worker = Worker::new(pool, &settings, registry);
        let handle = tokio::spawn(worker.run(async move {
            let _ = stopped.await;
        }));
        Self { stop, handle }
    }

    /// Signals shutdown and waits (up to 30 s) for in-flight jobs to finish.
    pub async fn stop(self) {
        let _ = self.stop.send(());
        let _ = tokio::time::timeout(Duration::from_secs(30), self.handle).await;
    }
}

/// Polls `condition` every 50 ms until it holds or `timeout` elapses. Returns whether it held.
pub async fn wait_until(timeout: Duration, condition: impl Fn() -> bool) -> bool {
    let deadline = tokio::time::Instant::now() + timeout;
    while tokio::time::Instant::now() < deadline {
        if condition() {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    condition()
}
