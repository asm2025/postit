use std::panic::AssertUnwindSafe;
use std::time::Duration;

use chrono::{DateTime, Utc};
use futures::FutureExt;
use postit_core::Clock;
use postit_data::jobs::{ErrorKind, JobsRepo};
use postit_data::recurring_runs::{RecurringRunsRepo, RunOutcome};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::PgPool;

use crate::error::JobError;
use crate::job::{JobContext, JobId};
use crate::recurring::RecurringPayload;
use crate::registry::{JobRegistry, Registration};

/// How long a job of an unknown type waits before it is fetched again.
pub(crate) const UNKNOWN_TYPE_REQUEUE_DELAY: Duration = Duration::from_secs(60);

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
    /// The job's type is unknown to this worker but may be known to a newer one: run it
    /// again after the delay without counting a try.
    Requeue(Duration),
    /// Do not retry. The message is safe to store (no user content).
    Abort(String),
}

/// Why a try did not succeed, for the job ledger. The message is safe to store only for job
/// types that opted in (`ConsoleSpec::show_error_message`).
pub(crate) struct Failure {
    pub kind: ErrorKind,
    pub message: String,
}

pub(crate) struct Dispatched {
    pub outcome: Outcome,
    pub failure: Option<Failure>,
}

fn recurring_run_id(registration: &Registration, payload: &Value) -> Option<uuid::Uuid> {
    registration
        .recurring
        .then(|| serde_json::from_value::<RecurringPayload>(payload.clone()).ok())
        .flatten()
        .map(|payload| payload.run_id)
}

/// Closes the recurring run of a job that was cancelled before it ran, as `Failed`.
/// A no-op for non-recurring and unregistered job types.
pub(crate) async fn record_skipped_run(
    pool: &PgPool,
    registry: &JobRegistry,
    envelope: &Envelope,
    clock: &dyn Clock,
) {
    let Some(run_id) = registry
        .get(&envelope.job_type)
        .and_then(|r| recurring_run_id(r, &envelope.payload))
    else {
        return;
    };
    if let Err(err) = record_run_outcome(pool, run_id, RunOutcome::Failed, clock.now()).await {
        tracing::warn!(%run_id, error = %err, "could not record recurring run outcome");
    }
}

/// A job of a type this worker does not register. A newer worker relayed it and an older
/// one fetched it (a rolling deploy), so it is re-queued while its ledger row is younger
/// than `max_age`. Past that, or with no readable ledger row, the type is treated as gone
/// for good and the job is aborted.
async fn unknown_type(
    pool: &PgPool,
    job_type: &str,
    job_id: JobId,
    max_age: Duration,
    clock: &dyn Clock,
) -> Dispatched {
    let created_at = match pool.acquire().await {
        Ok(mut conn) => JobsRepo::get(&mut conn, job_id.0)
            .await
            .ok()
            .flatten()
            .map(|job| job.created_at),
        Err(_) => None,
    };
    let young = created_at.is_some_and(|created_at| {
        clock.now() - created_at < chrono::Duration::from_std(max_age).unwrap_or_default()
    });
    if young {
        tracing::info!(job_type, job_id = %job_id.0,
            "job type is not registered here; re-queued for a newer worker");
        return Dispatched {
            outcome: Outcome::Requeue(UNKNOWN_TYPE_REQUEUE_DELAY),
            failure: None,
        };
    }
    let message = format!("job type `{job_type}` is not registered");
    Dispatched {
        outcome: Outcome::Abort(message.clone()),
        failure: Some(Failure {
            kind: ErrorKind::UnknownType,
            message,
        }),
    }
}

pub(crate) async fn dispatch_detailed(
    pool: &PgPool,
    registry: &JobRegistry,
    envelope: Envelope,
    job_id: JobId,
    attempt: u32,
    clock: &dyn Clock,
    unknown_type_max_age: Duration,
) -> Dispatched {
    let Some(registration) = registry.get(&envelope.job_type) else {
        return unknown_type(
            pool,
            &envelope.job_type,
            job_id,
            unknown_type_max_age,
            clock,
        )
        .await;
    };
    let ctx = JobContext {
        job_id,
        attempt,
        max_attempts: registration.retry.max_attempts(),
    };
    let recurring_run_id = recurring_run_id(registration, &envelope.payload);
    // A panic is a fatal failure of this run, not of the worker: catching it here (rather
    // than around `dispatch`) keeps any bookkeeping after the handler call running.
    let result = AssertUnwindSafe((registration.handler)(envelope.payload, ctx))
        .catch_unwind()
        .await;
    let (outcome, failure) = match result {
        Ok(Ok(())) => (Outcome::Done, None),
        Ok(Err(JobError::Fatal(message))) => (
            Outcome::Abort(message.clone()),
            Some(Failure {
                kind: ErrorKind::Fatal,
                message,
            }),
        ),
        Ok(Err(JobError::Retry(message))) => match registration.retry.delay_before_next(attempt) {
            Some(delay) => (
                Outcome::RetryAfter(delay),
                Some(Failure {
                    kind: ErrorKind::Retry,
                    message,
                }),
            ),
            None => (
                Outcome::Abort(message.clone()),
                Some(Failure {
                    kind: ErrorKind::RetriesExhausted,
                    message,
                }),
            ),
        },
        Err(_) => {
            let message = "the job handler panicked".to_string();
            (
                Outcome::Abort(message.clone()),
                Some(Failure {
                    kind: ErrorKind::Panic,
                    message,
                }),
            )
        }
    };
    if let Some(run_id) = recurring_run_id {
        let run_outcome = match &outcome {
            Outcome::Done => Some(RunOutcome::Succeeded),
            Outcome::Abort(_) => Some(RunOutcome::Failed),
            Outcome::RetryAfter(_) | Outcome::Requeue(_) => None,
        };
        if let Some(run_outcome) = run_outcome
            && let Err(err) = record_run_outcome(pool, run_id, run_outcome, clock.now()).await
        {
            tracing::warn!(%run_id, error = %err, "could not record recurring run outcome");
        }
    }
    Dispatched { outcome, failure }
}

async fn record_run_outcome(
    pool: &PgPool,
    run_id: uuid::Uuid,
    outcome: RunOutcome,
    at: DateTime<Utc>,
) -> Result<(), crate::error::JobsError> {
    let mut conn = pool.acquire().await?;
    RecurringRunsRepo::finish(&mut conn, run_id, outcome, at).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const MAX_AGE: Duration = Duration::from_hours(7 * 24);

    async fn dispatch(
        pool: &PgPool,
        registry: &JobRegistry,
        envelope: Envelope,
        job_id: JobId,
        attempt: u32,
    ) -> Outcome {
        dispatch_detailed(
            pool,
            registry,
            envelope,
            job_id,
            attempt,
            &postit_core::SystemClock,
            MAX_AGE,
        )
        .await
        .outcome
    }
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

    #[derive(Serialize, Deserialize)]
    struct Panics;
    impl Job for Panics {
        const JOB_TYPE: &'static str = "panics";
        const QUEUE: Queue = Queue::Default;
    }

    #[sqlx::test(migrations = "../data/migrations")]
    async fn a_panicking_handler_aborts(pool: PgPool) {
        let mut registry = registry();
        registry
            .register(RetryPolicy::None, |_: Panics, _| async {
                unreachable!("handler panics on purpose")
            })
            .unwrap_or_else(|e| unreachable!("register panics: {e}"));
        let id = JobId(uuid::Uuid::now_v7());
        assert_eq!(
            dispatch(&pool, &registry, envelope("panics"), id, 1).await,
            Outcome::Abort("the job handler panicked".into())
        );
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
    async fn dispatch_detailed_classifies_each_failure(pool: PgPool) {
        let registry = registry();
        let id = JobId(uuid::Uuid::now_v7());
        let kind = |d: &Dispatched| d.failure.as_ref().map(|f| f.kind);
        assert_eq!(
            kind(
                &dispatch_detailed(
                    &pool,
                    &registry,
                    envelope("nope"),
                    id,
                    1,
                    &postit_core::SystemClock,
                    MAX_AGE
                )
                .await
            ),
            Some(ErrorKind::UnknownType)
        );
        assert_eq!(
            kind(
                &dispatch_detailed(
                    &pool,
                    &registry,
                    envelope("broken"),
                    id,
                    1,
                    &postit_core::SystemClock,
                    MAX_AGE
                )
                .await
            ),
            Some(ErrorKind::Fatal)
        );
        assert_eq!(
            kind(
                &dispatch_detailed(
                    &pool,
                    &registry,
                    envelope("flaky"),
                    id,
                    1,
                    &postit_core::SystemClock,
                    MAX_AGE
                )
                .await
            ),
            Some(ErrorKind::Retry)
        );
        assert_eq!(
            kind(
                &dispatch_detailed(
                    &pool,
                    &registry,
                    envelope("flaky"),
                    id,
                    2,
                    &postit_core::SystemClock,
                    MAX_AGE
                )
                .await
            ),
            Some(ErrorKind::RetriesExhausted)
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
