use std::sync::Arc;
use std::time::Duration;

use postit_data::job_outbox::{JobOutboxRepo, OUTBOX_CHANNEL};
use sqlx::PgPool;
use sqlx::postgres::PgListener;
use tokio::sync::watch;

use crate::backend::Backend;
use crate::dispatch::Envelope;
use crate::error::JobsError;
use crate::registry::JobRegistry;

const BATCH: i64 = 100;

/// Moves every claimable outbox row into the job storage. Returns how many rows it moved.
///
/// Claim, push and delete share one transaction, so a crash at any point either commits
/// all three or none. Each push runs in its own savepoint and uses the outbox row's ID as
/// the task ID: a task already stored under that ID (pushed by a relay whose delete never
/// committed) is not stored twice, and its outbox row is deleted all the same.
pub(crate) async fn drain_once(
    pool: &PgPool,
    registry: &JobRegistry,
    backend: &Backend,
) -> Result<usize, JobsError> {
    let job_types = registry.job_types();
    let mut moved = 0;
    loop {
        let mut tx = pool.begin().await?;
        let rows = JobOutboxRepo::claim_batch(&mut tx, &job_types, BATCH).await?;
        if rows.is_empty() {
            tx.commit().await?;
            return Ok(moved);
        }
        for row in rows {
            let Some(registration) = registry.get(&row.job_type) else {
                continue; // unreachable: claim_batch filters by registered types
            };
            let envelope = Envelope {
                job_type: row.job_type,
                payload: row.payload,
            };
            backend
                .push(
                    &mut tx,
                    registration.queue,
                    row.id,
                    &envelope,
                    row.run_at,
                    registration.retry.max_attempts(),
                )
                .await?;
            JobOutboxRepo::delete(&mut tx, row.id).await?;
            moved += 1;
        }
        tx.commit().await?;
    }
}

/// Drains on every committed outbox insert (`NOTIFY`) and every `poll_interval`, until
/// `stop` becomes `true`. A dropped listener connection is re-established on the next poll,
/// with an immediate drain, so nothing waits longer than one poll interval.
pub(crate) async fn run(
    pool: PgPool,
    registry: Arc<JobRegistry>,
    backend: Backend,
    poll_interval: Duration,
    mut stop: watch::Receiver<bool>,
) {
    let mut listener: Option<PgListener> = None;
    loop {
        if listener.is_none() {
            listener = connect_listener(&pool).await;
        }
        if let Err(err) = drain_once(&pool, &registry, &backend).await {
            tracing::warn!(error = %err, "outbox relay drain failed");
        }
        let notified = async {
            match listener.as_mut() {
                Some(l) => l.recv().await.is_ok(),
                None => std::future::pending().await,
            }
        };
        let mut reconnect = false;
        tokio::select! {
            _ = stop.changed() => return,
            ok = notified => reconnect = !ok,
            () = tokio::time::sleep(poll_interval) => {}
        }
        if reconnect {
            listener = None;
        }
        if *stop.borrow() {
            return;
        }
    }
}

async fn connect_listener(pool: &PgPool) -> Option<PgListener> {
    let mut listener = match PgListener::connect_with(pool).await {
        Ok(listener) => listener,
        Err(err) => {
            tracing::warn!(error = %err, "outbox relay could not connect its listener");
            return None;
        }
    };
    match listener.listen(OUTBOX_CHANNEL).await {
        Ok(()) => Some(listener),
        Err(err) => {
            tracing::warn!(error = %err, "outbox relay could not LISTEN");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    use postit_data::job_outbox::JobOutboxRepo;
    use serde::{Deserialize, Serialize};
    use sqlx::PgPool;

    use crate::backend::{Backend, Pushed};
    use crate::dispatch::Envelope;
    use crate::testkit::{RunningWorker, wait_until};
    use crate::{Job, JobRegistry, Queue, RetryPolicy, migrate};

    #[derive(Serialize, Deserialize)]
    struct Once {}
    impl Job for Once {
        const JOB_TYPE: &'static str = "once";
        const QUEUE: Queue = Queue::Default;
    }

    /// Simulates a relay that pushed a row into the storage and crashed before deleting it.
    /// The next drain must not store (and so not run) the job a second time.
    #[sqlx::test(migrations = "../data/migrations")]
    async fn a_row_pushed_before_a_crash_runs_once(pool: PgPool) {
        migrate(&pool)
            .await
            .unwrap_or_else(|e| unreachable!("migrate: {e}"));
        let id = uuid::Uuid::now_v7();
        let mut conn = pool
            .acquire()
            .await
            .unwrap_or_else(|e| unreachable!("acquire: {e}"));
        JobOutboxRepo::insert(
            &mut conn,
            id,
            "once",
            &serde_json::json!({}),
            chrono::Utc::now(),
        )
        .await
        .unwrap_or_else(|e| unreachable!("insert: {e}"));
        drop(conn);

        let backend = Backend::connect(&pool, Duration::from_millis(200))
            .await
            .unwrap_or_else(|e| unreachable!("backend: {e}"));
        let mut tx = pool
            .begin()
            .await
            .unwrap_or_else(|e| unreachable!("begin: {e}"));
        let envelope = Envelope {
            job_type: "once".into(),
            payload: serde_json::json!({}),
        };
        let pushed = backend
            .push(
                &mut tx,
                Queue::Default,
                id,
                &envelope,
                chrono::Utc::now(),
                Some(1),
            )
            .await
            .unwrap_or_else(|e| unreachable!("push: {e}"));
        assert!(matches!(pushed, Pushed::Stored));
        tx.commit()
            .await
            .unwrap_or_else(|e| unreachable!("commit: {e}"));
        // …crash: the outbox row was never deleted.

        let runs = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&runs);
        let mut registry = JobRegistry::default();
        registry
            .register(RetryPolicy::None, move |_: Once, _| {
                let counter = Arc::clone(&counter);
                async move {
                    counter.fetch_add(1, Ordering::SeqCst);
                    Ok(())
                }
            })
            .unwrap_or_else(|e| unreachable!("register: {e}"));
        let worker = RunningWorker::start(pool.clone(), registry).await;

        assert!(wait_until(Duration::from_secs(10), || runs.load(Ordering::SeqCst) >= 1).await);
        let outbox_empty = || async {
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM job_outbox")
                .fetch_one(&pool)
                .await
                .unwrap_or(1)
                == 0
        };
        tokio::time::sleep(Duration::from_secs(2)).await;
        assert!(outbox_empty().await);
        assert_eq!(runs.load(Ordering::SeqCst), 1);
        worker.stop().await;
    }

    /// A duplicate push inside a caller's transaction reports `AlreadyStored` and leaves the
    /// transaction usable (the push runs in a savepoint).
    #[sqlx::test(migrations = "../data/migrations")]
    async fn a_duplicate_push_leaves_the_transaction_usable(pool: PgPool) {
        migrate(&pool)
            .await
            .unwrap_or_else(|e| unreachable!("migrate: {e}"));
        let backend = Backend::connect(&pool, Duration::from_millis(200))
            .await
            .unwrap_or_else(|e| unreachable!("backend: {e}"));
        let id = uuid::Uuid::now_v7();
        let envelope = Envelope {
            job_type: "once".into(),
            payload: serde_json::json!({}),
        };
        let mut tx = pool
            .begin()
            .await
            .unwrap_or_else(|e| unreachable!("begin: {e}"));
        let now = chrono::Utc::now();
        let first = backend
            .push(&mut tx, Queue::Default, id, &envelope, now, Some(1))
            .await
            .unwrap_or_else(|e| unreachable!("first push: {e}"));
        let second = backend
            .push(&mut tx, Queue::Default, id, &envelope, now, Some(1))
            .await
            .unwrap_or_else(|e| unreachable!("second push: {e}"));
        assert!(matches!(first, Pushed::Stored));
        assert!(matches!(second, Pushed::AlreadyStored));
        // Still usable: an aborted transaction would reject this statement.
        let one: i32 = sqlx::query_scalar("SELECT 1")
            .fetch_one(&mut *tx)
            .await
            .unwrap_or_else(|e| unreachable!("query after duplicate: {e}"));
        assert_eq!(one, 1);
        tx.commit()
            .await
            .unwrap_or_else(|e| unreachable!("commit: {e}"));
    }
}
