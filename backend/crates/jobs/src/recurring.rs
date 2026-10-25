use std::sync::Arc;

use chrono::Utc;
use cron::Schedule;
use postit_core::{Clock, IdGenerator};
use postit_data::recurring_runs::RecurringRunsRepo;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use tokio::sync::watch;
use uuid::Uuid;

use crate::job::Queue;
use crate::queue::{EnqueueMeta, JobQueue};

/// The payload of every recurring job in the storage: which `job_recurring_runs` row this
/// execution belongs to. The handler itself gets `J::default()`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub(crate) struct RecurringPayload {
    pub run_id: Uuid,
}

pub(crate) struct RecurringSpec {
    pub name: &'static str,
    pub queue: Queue,
    pub schedule: Schedule,
}

/// Sleeps until each upcoming tick of `spec.schedule`, then tries to claim it. Every worker
/// process runs this loop; the unique `(name, scheduled_for)` insert lets exactly one of
/// them enqueue the tick. Missed ticks (process down) are not back-filled.
pub(crate) async fn run(
    pool: PgPool,
    queue: JobQueue,
    ids: Arc<dyn IdGenerator>,
    clock: Arc<dyn Clock>,
    spec: Arc<RecurringSpec>,
    mut stop: watch::Receiver<bool>,
) {
    loop {
        let Some(next) = spec.schedule.after(&clock.now()).next() else {
            tracing::warn!(
                job = spec.name,
                "recurring schedule has no upcoming occurrence; idle until shutdown"
            );
            let _ = stop.wait_for(|stopped| *stopped).await;
            return;
        };
        let wait = (next - clock.now()).to_std().unwrap_or_default();
        tokio::select! {
            _ = stop.changed() => return,
            () = tokio::time::sleep(wait) => {}
        }
        if *stop.borrow() {
            return;
        }
        if let Err(err) =
            claim_and_enqueue(&pool, &queue, ids.as_ref(), spec.name, spec.queue, next).await
        {
            tracing::warn!(job = spec.name, error = %err, "recurring tick failed");
        }
    }
}

async fn claim_and_enqueue(
    pool: &PgPool,
    queue: &JobQueue,
    ids: &dyn IdGenerator,
    name: &'static str,
    queue_kind: Queue,
    tick: chrono::DateTime<Utc>,
) -> Result<(), crate::JobsError> {
    let mut tx = pool.begin().await?;
    let run_id = ids.generate();
    if !RecurringRunsRepo::try_insert_scheduled(&mut tx, run_id, name, tick).await? {
        tx.rollback().await?;
        return Ok(());
    }
    let payload = serde_json::to_value(RecurringPayload { run_id })?;
    let meta = EnqueueMeta {
        queue: queue_kind.as_str(),
        recurring_name: Some(name),
        retried_from: None,
    };
    let job_id = queue
        .enqueue_raw_in(&mut tx, name, &payload, None, &meta)
        .await?;
    RecurringRunsRepo::set_job_id(&mut tx, run_id, job_id.0).await?;
    tx.commit().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use postit_core::SystemClock;

    use super::*;

    /// Hands out the given IDs in order.
    struct Sequence(Mutex<Vec<Uuid>>);

    impl IdGenerator for Sequence {
        fn generate(&self) -> Uuid {
            self.0
                .lock()
                .unwrap_or_else(|e| unreachable!("sequence lock: {e}"))
                .remove(0)
        }
    }

    /// The run and the job a tick enqueues are minted by the injected generator, so a test
    /// can pin both IDs (P5-M1).
    #[sqlx::test(migrations = "../data/migrations")]
    async fn a_tick_mints_its_ids_through_the_injected_generator(pool: PgPool) {
        let (run_id, job_id) = (Uuid::from_u128(1), Uuid::from_u128(2));
        let ids = Arc::new(Sequence(Mutex::new(vec![run_id, job_id])));
        let queue = JobQueue::new(pool.clone(), ids.clone(), Arc::new(SystemClock));

        claim_and_enqueue(
            &pool,
            &queue,
            ids.as_ref(),
            "tick_probe",
            Queue::Maintenance,
            Utc::now(),
        )
        .await
        .unwrap_or_else(|e| unreachable!("claim_and_enqueue: {e}"));

        let stored_run: Uuid =
            sqlx::query_scalar("SELECT id FROM job_recurring_runs WHERE name = 'tick_probe'")
                .fetch_one(&pool)
                .await
                .unwrap_or_else(|e| unreachable!("run row: {e}"));
        let stored_job: Uuid =
            sqlx::query_scalar("SELECT id FROM jobs WHERE recurring_name = 'tick_probe'")
                .fetch_one(&pool)
                .await
                .unwrap_or_else(|e| unreachable!("job row: {e}"));
        assert_eq!((stored_run, stored_job), (run_id, job_id));
    }
}
