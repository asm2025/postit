use std::collections::HashMap;
use std::future::Future;
use std::sync::Arc;

use postit_config::JobsSettings;
use postit_data::locks::{JOBS_MIGRATIONS_LOCK_KEY, with_session_lock};
use sqlx::PgPool;
use tokio::sync::watch;

use crate::backend::{self, Backend};
use crate::error::JobsError;
use crate::job::Queue;
use crate::registry::JobRegistry;
use crate::relay;

/// Runs the job storage's migrations under an advisory lock, so every role can call this
/// at startup concurrently. Call after `postit_data::Db::run_migrations`.
///
/// # Errors
///
/// Returns [`JobsError`] if the lock or a migration fails.
pub async fn migrate(pool: &PgPool) -> Result<(), JobsError> {
    let inner = pool.clone();
    // The closure must return `Result<_, DataError>`; the storage's own result rides
    // inside it and is unwrapped after the lock is released.
    with_session_lock(pool, JOBS_MIGRATIONS_LOCK_KEY, move || async move {
        Ok(backend::migrate_storage(&inner).await)
    })
    .await?
}

/// A worker process's job runtime: the outbox relay plus one worker pool per [`Queue`].
pub struct Worker {
    pool: PgPool,
    settings: JobsSettings,
    registry: JobRegistry,
}

impl Worker {
    #[must_use]
    pub fn new(pool: PgPool, settings: &JobsSettings, registry: JobRegistry) -> Self {
        Self {
            pool,
            settings: settings.clone(),
            registry,
        }
    }

    /// Runs the outbox relay and one worker pool per queue until `shutdown` resolves, then
    /// drains in-flight jobs and returns. Call [`migrate`] first.
    ///
    /// # Errors
    ///
    /// Returns [`JobsError::Backend`] if the job storage cannot be reached or is not
    /// migrated.
    pub async fn run<F>(self, shutdown: F) -> Result<(), JobsError>
    where
        F: Future<Output = ()> + Send + 'static,
    {
        let poll = self.settings.outbox_poll_interval;
        // `Backend` is `Clone`: a cheap handle, the storages are built over the pool.
        let backend = Backend::connect(&self.pool, poll).await?;
        let registry = Arc::new(self.registry);
        let (stop_tx, stop_rx) = watch::channel(false);

        let relay_task = tokio::spawn(relay::run(
            self.pool.clone(),
            Arc::clone(&registry),
            backend.clone(),
            poll,
            stop_rx.clone(),
        ));
        tokio::spawn(async move {
            shutdown.await;
            let _ = stop_tx.send(true);
        });

        let concurrency: HashMap<Queue, u32> = Queue::ALL
            .into_iter()
            .map(|q| {
                (
                    q,
                    self.settings
                        .concurrency
                        .get(q.as_str())
                        .copied()
                        .unwrap_or(1),
                )
            })
            .collect();
        let result = backend
            .run(self.pool.clone(), registry, concurrency, stop_rx)
            .await;
        let _ = relay_task.await;
        result
    }
}
