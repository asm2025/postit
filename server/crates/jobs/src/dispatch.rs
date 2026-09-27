use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::PgPool;

use crate::error::JobError;
use crate::job::{JobContext, JobId};
use crate::registry::JobRegistry;

/// What the job storage holds: one envelope type per queue, dispatched by `job_type`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct Envelope {
    pub job_type: String,
    pub payload: Value,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Outcome {
    Done,
    RetryAfter(Duration),
    /// Do not retry. The message is safe to store (no user content).
    Abort(String),
}

pub(crate) async fn dispatch(
    _pool: &PgPool,
    registry: &JobRegistry,
    envelope: Envelope,
    job_id: JobId,
    attempt: u32,
) -> Outcome {
    let Some(registration) = registry.get(&envelope.job_type) else {
        return Outcome::Abort(format!(
            "job type `{}` is not registered",
            envelope.job_type
        ));
    };
    let ctx = JobContext {
        job_id,
        attempt,
        max_attempts: registration.retry.max_attempts(),
    };
    match (registration.handler)(envelope.payload, ctx).await {
        Ok(()) => Outcome::Done,
        Err(JobError::Fatal(message)) => Outcome::Abort(message),
        Err(JobError::Retry(message)) => match registration.retry.delay_before_next(attempt) {
            Some(delay) => Outcome::RetryAfter(delay),
            None => Outcome::Abort(message),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::job::{Job, Queue, RetryPolicy};

    #[derive(Serialize, Deserialize)]
    struct Flaky;
    impl Job for Flaky {
        const JOB_TYPE: &'static str = "flaky";
        const QUEUE: Queue = Queue::Default;
    }

    #[derive(Serialize, Deserialize)]
    struct Broken;
    impl Job for Broken {
        const JOB_TYPE: &'static str = "broken";
        const QUEUE: Queue = Queue::Default;
    }

    fn registry() -> JobRegistry {
        let mut registry = JobRegistry::default();
        registry
            .register(
                RetryPolicy::Backoff {
                    max_attempts: Some(2),
                    initial: Duration::from_millis(100),
                    max: Duration::from_secs(1),
                },
                |_: Flaky, _| async { Err(JobError::Retry("try again".into())) },
            )
            .unwrap_or_else(|e| unreachable!("register flaky: {e}"));
        registry
            .register(
                RetryPolicy::Backoff {
                    max_attempts: None,
                    initial: Duration::from_secs(1),
                    max: Duration::from_secs(1),
                },
                |_: Broken, _| async { Err(JobError::Fatal("bad input".into())) },
            )
            .unwrap_or_else(|e| unreachable!("register broken: {e}"));
        registry
    }

    fn envelope(job_type: &str) -> Envelope {
        Envelope {
            job_type: job_type.into(),
            payload: Value::Null,
        }
    }

    #[sqlx::test(migrations = "../data/migrations")]
    async fn retry_until_the_cap_then_abort(pool: PgPool) {
        let registry = registry();
        let id = JobId(uuid::Uuid::now_v7());
        assert_eq!(
            dispatch(&pool, &registry, envelope("flaky"), id, 1).await,
            Outcome::RetryAfter(Duration::from_millis(100))
        );
        assert_eq!(
            dispatch(&pool, &registry, envelope("flaky"), id, 2).await,
            Outcome::Abort("try again".into())
        );
    }

    #[sqlx::test(migrations = "../data/migrations")]
    async fn fatal_aborts_even_with_retries_left(pool: PgPool) {
        let id = JobId(uuid::Uuid::now_v7());
        assert_eq!(
            dispatch(&pool, &registry(), envelope("broken"), id, 1).await,
            Outcome::Abort("bad input".into())
        );
    }

    #[sqlx::test(migrations = "../data/migrations")]
    async fn unknown_type_aborts(pool: PgPool) {
        let id = JobId(uuid::Uuid::now_v7());
        assert!(matches!(
            dispatch(&pool, &registry(), envelope("nope"), id, 1).await,
            Outcome::Abort(_)
        ));
    }
}
