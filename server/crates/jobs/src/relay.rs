use std::sync::Arc;
use std::time::Duration;

use postit_data::job_outbox::{JobOutboxRepo, OUTBOX_CHANNEL};
use sqlx::PgPool;
use sqlx::postgres::PgListener;
use tokio::sync::watch;
use uuid::Uuid;

use crate::backend::{Backend, Pushed};
use crate::dispatch::Envelope;
use crate::error::JobsError;
use crate::registry::JobRegistry;

const BATCH: i64 = 100;

/// Moves every claimable outbox row into the job storage. Returns how many rows it moved.
///
/// Claim, push and delete share one transaction, so a crash at any point either commits
/// all three or none. Each push runs in its own savepoint and uses the outbox row's ID as
/// the task ID: a task already stored under that ID (pushed by a relay whose delete never
/// committed) is not stored twice, and its outbox row is deleted all the same. A row the
/// storage rejects is logged and left in the outbox for the next pass. It is excluded from
/// later claims of this drain, so each rejected row is tried once per drain and never
/// hides the rows behind it. Only a lost connection (`Err`) abandons the batch.
pub(crate) async fn drain_once(
    pool: &PgPool,
    registry: &JobRegistry,
    backend: &Backend,
) -> Result<usize, JobsError> {
    drain_in_batches(pool, registry, backend, BATCH).await
}

async fn drain_in_batches(
    pool: &PgPool,
    registry: &JobRegistry,
    backend: &Backend,
    batch: i64,
) -> Result<usize, JobsError> {
    let job_types = registry.job_types();
    let mut rejected: Vec<Uuid> = Vec::new();
    let mut moved = 0;
    loop {
        let mut tx = pool.begin().await?;
        let rows = JobOutboxRepo::claim_batch(&mut tx, &job_types, &rejected, batch).await?;
        if rows.is_empty() {
            tx.commit().await?;
            return Ok(moved);
        }
        let mut moved_in_batch = 0;
        for row in rows {
            let Some(registration) = registry.get(&row.job_type) else {
                continue; // unreachable: claim_batch filters by registered types
            };
            let envelope = Envelope {
                job_type: row.job_type,
                payload: row.payload,
            };
            let pushed = backend
                .push(
                    &mut tx,
                    registration.queue,
                    row.id,
                    &envelope,
                    row.run_at,
                    registration.retry.max_attempts(),
                )
                .await?;
            if let Pushed::Failed(error) = pushed {
                tracing::warn!(outbox_id = %row.id, job_type = %envelope.job_type, %error,
                    "outbox relay could not push a job; it stays in the outbox for the next pass");
                rejected.push(row.id);
                continue;
            }
            JobOutboxRepo::delete(&mut tx, row.id).await?;
            moved_in_batch += 1;
        }
        tx.commit().await?;
        moved += moved_in_batch;
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

    use super::{drain_in_batches, drain_once};
    use crate::apalis_sql;
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

    /// A row the storage rejects (here: a unique violation that is not on the task-ID
    /// indexes, so it must not count as "already stored") stays in the outbox, and the other
    /// rows of its batch are still moved. A drain with only rejected rows left terminates.
    #[sqlx::test(migrations = "../data/migrations")]
    async fn a_rejected_row_stays_in_the_outbox_without_blocking_its_batch(pool: PgPool) {
        migrate(&pool)
            .await
            .unwrap_or_else(|e| unreachable!("migrate: {e}"));
        let ids: Vec<uuid::Uuid> = (0..3).map(|_| uuid::Uuid::now_v7()).collect();
        let mut conn = pool
            .acquire()
            .await
            .unwrap_or_else(|e| unreachable!("acquire: {e}"));
        for id in &ids {
            JobOutboxRepo::insert(
                &mut conn,
                *id,
                "once",
                &serde_json::json!({}),
                chrono::Utc::now(),
            )
            .await
            .unwrap_or_else(|e| unreachable!("insert: {e}"));
        }
        drop(conn);
        let task_id = |id: uuid::Uuid| ulid::Ulid::from(id).to_string();
        let rejected = ids[1];
        apalis_sql::reject_task_for_test(&pool, &task_id(rejected))
            .await
            .unwrap_or_else(|e| unreachable!("reject: {e}"));

        let mut registry = JobRegistry::default();
        registry
            .register(RetryPolicy::None, |_: Once, _| async { Ok(()) })
            .unwrap_or_else(|e| unreachable!("register: {e}"));
        let backend = Backend::connect(&pool, Duration::from_millis(200))
            .await
            .unwrap_or_else(|e| unreachable!("backend: {e}"));

        let moved = drain_once(&pool, &registry, &backend)
            .await
            .unwrap_or_else(|e| unreachable!("drain: {e}"));
        assert_eq!(moved, 2);
        let left: Vec<uuid::Uuid> = sqlx::query_scalar("SELECT id FROM job_outbox")
            .fetch_all(&pool)
            .await
            .unwrap_or_else(|e| unreachable!("outbox: {e}"));
        assert_eq!(left, vec![rejected]);
        for id in [ids[0], ids[2]] {
            let status = apalis_sql::task_status(&pool, &task_id(id))
                .await
                .unwrap_or_else(|e| unreachable!("status: {e}"));
            assert_eq!(status.as_deref(), Some("Pending"));
        }
        let status = apalis_sql::task_status(&pool, &task_id(rejected))
            .await
            .unwrap_or_else(|e| unreachable!("status: {e}"));
        assert_eq!(status, None);

        // Next pass: the rejected row is retried, still fails, and the drain returns.
        let moved = drain_once(&pool, &registry, &backend)
            .await
            .unwrap_or_else(|e| unreachable!("second drain: {e}"));
        assert_eq!(moved, 0);
    }

    /// More rejected rows than one batch, all older than the movable rows: a single drain
    /// still reaches and moves the rows behind them, and terminates.
    #[sqlx::test(migrations = "../data/migrations")]
    async fn rejected_rows_filling_whole_batches_do_not_starve_newer_rows(pool: PgPool) {
        migrate(&pool)
            .await
            .unwrap_or_else(|e| unreachable!("migrate: {e}"));
        let task_id = |id: uuid::Uuid| ulid::Ulid::from(id).to_string();
        let mut conn = pool
            .acquire()
            .await
            .unwrap_or_else(|e| unreachable!("acquire: {e}"));
        // Oldest first: 3 rejected rows (batch size 2), then 2 movable rows.
        let ids: Vec<uuid::Uuid> = (0..5).map(|_| uuid::Uuid::now_v7()).collect();
        for id in &ids {
            JobOutboxRepo::insert(
                &mut conn,
                *id,
                "once",
                &serde_json::json!({}),
                chrono::Utc::now(),
            )
            .await
            .unwrap_or_else(|e| unreachable!("insert: {e}"));
        }
        drop(conn);
        for id in &ids[..3] {
            apalis_sql::reject_task_for_test(&pool, &task_id(*id))
                .await
                .unwrap_or_else(|e| unreachable!("reject: {e}"));
        }

        let mut registry = JobRegistry::default();
        registry
            .register(RetryPolicy::None, |_: Once, _| async { Ok(()) })
            .unwrap_or_else(|e| unreachable!("register: {e}"));
        let backend = Backend::connect(&pool, Duration::from_millis(200))
            .await
            .unwrap_or_else(|e| unreachable!("backend: {e}"));

        let moved = tokio::time::timeout(
            Duration::from_secs(10),
            drain_in_batches(&pool, &registry, &backend, 2),
        )
        .await
        .unwrap_or_else(|e| unreachable!("drain did not terminate: {e}"))
        .unwrap_or_else(|e| unreachable!("drain: {e}"));
        assert_eq!(moved, 2);
        let mut left: Vec<uuid::Uuid> = sqlx::query_scalar("SELECT id FROM job_outbox")
            .fetch_all(&pool)
            .await
            .unwrap_or_else(|e| unreachable!("outbox: {e}"));
        left.sort();
        assert_eq!(left, ids[..3].to_vec());
        for id in &ids[3..] {
            let status = apalis_sql::task_status(&pool, &task_id(*id))
                .await
                .unwrap_or_else(|e| unreachable!("status: {e}"));
            assert_eq!(status.as_deref(), Some("Pending"));
        }
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
