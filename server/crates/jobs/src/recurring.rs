use std::sync::Arc;

use chrono::Utc;
use cron::Schedule;
use postit_data::recurring_runs::RecurringRunsRepo;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use tokio::sync::watch;
use uuid::Uuid;

use crate::queue::JobQueue;

/// The payload of every recurring job in the storage: which `job_recurring_runs` row this
/// execution belongs to. The handler itself gets `J::default()`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub(crate) struct RecurringPayload {
    pub run_id: Uuid,
}

pub(crate) struct RecurringSpec {
    pub name: &'static str,
    pub schedule: Schedule,
}

/// Sleeps until each upcoming tick of `spec.schedule`, then tries to claim it. Every worker
/// process runs this loop; the unique `(name, scheduled_for)` insert lets exactly one of
/// them enqueue the tick. Missed ticks (process down) are not back-filled.
pub(crate) async fn run(
    pool: PgPool,
    queue: JobQueue,
    spec: Arc<RecurringSpec>,
    mut stop: watch::Receiver<bool>,
) {
    loop {
        let Some(next) = spec.schedule.upcoming(Utc).next() else {
            tracing::warn!(
                job = spec.name,
                "recurring schedule has no upcoming occurrence; idle until shutdown"
            );
            let _ = stop.wait_for(|stopped| *stopped).await;
            return;
        };
        let wait = (next - Utc::now()).to_std().unwrap_or_default();
        tokio::select! {
            _ = stop.changed() => return,
            () = tokio::time::sleep(wait) => {}
        }
        if *stop.borrow() {
            return;
        }
        if let Err(err) = claim_and_enqueue(&pool, &queue, spec.name, next).await {
            tracing::warn!(job = spec.name, error = %err, "recurring tick failed");
        }
    }
}

async fn claim_and_enqueue(
    pool: &PgPool,
    queue: &JobQueue,
    name: &'static str,
    tick: chrono::DateTime<Utc>,
) -> Result<(), crate::JobsError> {
    let mut tx = pool.begin().await?;
    let run_id = Uuid::now_v7();
    if !RecurringRunsRepo::try_insert_scheduled(&mut tx, run_id, name, tick).await? {
        tx.rollback().await?;
        return Ok(());
    }
    let payload = serde_json::to_value(RecurringPayload { run_id })?;
    let job_id = queue.enqueue_raw_in(&mut tx, name, &payload, None).await?;
    RecurringRunsRepo::set_job_id(&mut tx, run_id, job_id.0).await?;
    tx.commit().await?;
    Ok(())
}
