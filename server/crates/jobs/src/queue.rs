use std::sync::Arc;

use chrono::{DateTime, Utc};
use postit_core::IdGenerator;
use postit_data::job_outbox::JobOutboxRepo;
use postit_data::jobs::{JobsRepo, NewJob};
use sqlx::{PgConnection, PgPool};
use uuid::Uuid;

use crate::error::JobsError;
use crate::job::{Job, JobId};

/// Ledger-only facts about an enqueue that the outbox does not carry.
pub(crate) struct EnqueueMeta<'a> {
    pub queue: &'a str,
    pub recurring_name: Option<&'a str>,
    pub retried_from: Option<Uuid>,
}

/// Every enqueue writes a `job_outbox` row; the relay in each worker process moves rows
/// into the job storage. Enqueueing never touches the storage backend, so an `api`-only
/// process can enqueue before any worker has started.
#[derive(Clone)]
pub struct JobQueue {
    pool: PgPool,
    ids: Arc<dyn IdGenerator>,
}

impl JobQueue {
    #[must_use]
    pub fn new(pool: PgPool, ids: Arc<dyn IdGenerator>) -> Self {
        Self { pool, ids }
    }

    /// # Errors
    ///
    /// Returns [`JobsError`] on a serialization or database failure.
    pub async fn enqueue<J: Job>(&self, job: &J) -> Result<JobId, JobsError> {
        self.enqueue_at(job, Utc::now()).await
    }

    /// # Errors
    ///
    /// Returns [`JobsError`] on a serialization or database failure.
    pub async fn enqueue_at<J: Job>(&self, job: &J, at: DateTime<Utc>) -> Result<JobId, JobsError> {
        let mut conn = self.pool().acquire().await?;
        self.enqueue_in(&mut conn, job, Some(at)).await
    }

    /// Writes the job in the caller's connection — inside the caller's transaction when
    /// `conn` is one — so the job exists if and only if that transaction commits.
    ///
    /// # Errors
    ///
    /// Returns [`JobsError`] on a serialization or database failure.
    pub async fn enqueue_in<J: Job>(
        &self,
        conn: &mut PgConnection,
        job: &J,
        run_at: Option<DateTime<Utc>>,
    ) -> Result<JobId, JobsError> {
        let payload = serde_json::to_value(job)?;
        let meta = EnqueueMeta {
            queue: J::QUEUE.as_str(),
            recurring_name: None,
            retried_from: None,
        };
        self.enqueue_raw_in(conn, J::JOB_TYPE, &payload, run_at, &meta)
            .await
    }

    pub(crate) async fn enqueue_raw_in(
        &self,
        conn: &mut PgConnection,
        job_type: &str,
        payload: &serde_json::Value,
        run_at: Option<DateTime<Utc>>,
        meta: &EnqueueMeta<'_>,
    ) -> Result<JobId, JobsError> {
        let id = self.ids.generate();
        let run_at = run_at.unwrap_or_else(Utc::now);
        JobOutboxRepo::insert(conn, id, job_type, payload, run_at).await?;
        JobsRepo::insert(
            conn,
            &NewJob {
                id,
                job_type,
                queue: meta.queue,
                payload,
                run_at,
                recurring_name: meta.recurring_name,
                retried_from: meta.retried_from,
            },
        )
        .await?;
        Ok(JobId(id))
    }

    /// The injected ID generator, so manual runs mint IDs the same way as enqueues.
    #[expect(dead_code, reason = "consumed by JobConsole (plan task 8)")]
    pub(crate) fn ids(&self) -> &dyn IdGenerator {
        self.ids.as_ref()
    }

    /// The pool the queue writes through. Task 6's relay reads outbox rows off the same
    /// pool; `enqueue_at` already goes through this accessor rather than the field directly.
    pub(crate) fn pool(&self) -> &PgPool {
        &self.pool
    }
}
