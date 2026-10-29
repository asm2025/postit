//! Records every try of every job in the job ledger (`jobs`, `job_attempts`). The ledger is
//! best-effort around execution: a failed ledger write is logged and never changes whether
//! or how a job runs.

use std::time::Duration;

use postit_data::jobs::{
    AttemptError, AttemptOutcome, Begin, ErrorKind, Finish, JobState, JobsRepo,
};
use postit_http::redact_secrets;

use crate::backend::HandlerState;
use crate::console_spec::ConsoleSpec;
use crate::dispatch::{self, Dispatched, Envelope, Outcome};
use crate::job::JobId;

/// Waits between the tries of the write that closes an attempt: three tries in all.
const FINISH_RETRY_DELAYS: [Duration; 2] = [Duration::from_millis(100), Duration::from_millis(500)];

pub(crate) async fn run_recorded(
    state: &HandlerState,
    envelope: Envelope,
    job_id: JobId,
    attempt: u32,
) -> Outcome {
    let job_type = envelope.job_type.clone();
    let max_attempts = state
        .registry
        .get(&job_type)
        .and_then(|r| r.retry.max_attempts())
        .and_then(|n| i32::try_from(n).ok());

    // `None`: the ledger could not be read. The job runs unrecorded, because a ledger
    // failure must never change whether a job runs.
    let begin = match begin(state, job_id, max_attempts).await {
        Ok(begin) => Some(begin),
        Err(err) => {
            tracing::warn!(job_id = %job_id.0, error = %err, "could not record the start of a try");
            None
        }
    };
    match begin {
        Some(Begin::Cancelled | Begin::Missing) => {
            if begin == Some(Begin::Missing) {
                tracing::info!(job_id = %job_id.0, "job has no ledger row (cancelled and removed); skipped");
            }
            // The handler never runs. A recurring run still gets an outcome, so its
            // `job_recurring_runs` row does not stay open forever.
            dispatch::record_skipped_run(
                &state.pool,
                &state.registry,
                &envelope,
                state.clock.as_ref(),
            )
            .await;
            return Outcome::Done;
        }
        // Already finished; its recurring run (if any) already has its outcome.
        Some(Begin::Finished) => return Outcome::Done,
        Some(Begin::Run(_)) | None => {}
    }

    // The handler's attempt number is the ledger's, not the storage's: the storage also counts
    // the re-queues of an unknown type, which record no attempt.
    let attempt = match begin {
        Some(Begin::Run(recorded)) => u32::try_from(recorded).unwrap_or(u32::MAX),
        _ => attempt,
    };
    let dispatched = dispatch::dispatch_detailed(
        &state.pool,
        &state.registry,
        envelope,
        job_id,
        attempt,
        state.clock.as_ref(),
        state.unknown_type_max_age,
    )
    .await;
    if let (Some(Begin::Run(recorded)), Outcome::Requeue(delay)) = (begin, &dispatched.outcome) {
        // Not a try: the attempt row opened above is removed and the job waits again.
        let run_at = state.clock.now() + chrono::Duration::from_std(*delay).unwrap_or_default();
        if let Err(err) = requeue(state, job_id, recorded, run_at).await {
            tracing::warn!(job_id = %job_id.0, error = %err, "could not record a re-queue");
        }
        return dispatched.outcome;
    }
    if let Some(Begin::Run(recorded)) = begin {
        match finish(state, job_id, recorded, &job_type, &dispatched).await {
            Ok(true) => {}
            Ok(false) => tracing::warn!(
                job_id = %job_id.0, attempt, recorded,
                "the try was superseded while it ran; its result was not recorded"
            ),
            Err(err) => {
                tracing::warn!(job_id = %job_id.0, error = %err, "could not record the end of a try");
            }
        }
    }
    dispatched.outcome
}

/// Opens the next attempt, numbered one past the last recorded one.
async fn begin(
    state: &HandlerState,
    id: JobId,
    max: Option<i32>,
) -> Result<Begin, postit_data::DataError> {
    let mut conn = state.pool.acquire().await?;
    JobsRepo::begin_attempt(&mut conn, id.0, 0, &state.worker, max).await
}

async fn requeue(
    state: &HandlerState,
    id: JobId,
    attempt: i32,
    run_at: chrono::DateTime<chrono::Utc>,
) -> Result<bool, postit_data::DataError> {
    let mut conn = state.pool.acquire().await?;
    JobsRepo::requeue_attempt(&mut conn, id.0, attempt, run_at).await
}

async fn finish(
    state: &HandlerState,
    id: JobId,
    attempt: i32,
    job_type: &str,
    d: &Dispatched,
) -> Result<bool, postit_data::DataError> {
    let show_message = state
        .registry
        .console()
        .get(job_type)
        .is_some_and(ConsoleSpec::shows_error_message);
    // Redacted before it is stored, so the console can only ever return the redacted text.
    let message = d
        .failure
        .as_ref()
        .filter(|_| show_message)
        .map(|f| redact_secrets(&f.message));
    let error = d.failure.as_ref().map(|f| AttemptError {
        kind: f.kind,
        code: f.kind.default_code(),
        message: message.as_deref(),
    });
    let (outcome, job_state, next_run_at) = match (&d.outcome, d.failure.as_ref().map(|f| f.kind)) {
        (Outcome::Done, _) => (AttemptOutcome::Succeeded, JobState::Succeeded, None),
        // Handled by `run_recorded` before a try is finished; never reaches here.
        (Outcome::Requeue(_), _) => (AttemptOutcome::Interrupted, JobState::Queued, None),
        (Outcome::RetryAfter(delay), _) => (
            AttemptOutcome::Retrying,
            JobState::Failed,
            Some(state.clock.now() + chrono::Duration::from_std(*delay).unwrap_or_default()),
        ),
        (Outcome::Abort(_), Some(ErrorKind::RetriesExhausted)) => {
            (AttemptOutcome::Failed, JobState::Dead, None)
        }
        (Outcome::Abort(_), _) => (AttemptOutcome::Killed, JobState::Killed, None),
    };
    let finish = Finish {
        outcome,
        state: job_state,
        next_run_at,
        error,
    };
    // The storage acks the task whatever happens here, so no later try would close this
    // attempt: a transient failure (say a pool timeout under load) is retried before the
    // row is left `running` for the history purge's sweep.
    let mut delays = FINISH_RETRY_DELAYS.iter();
    loop {
        let result = match state.pool.acquire().await {
            Ok(mut conn) => JobsRepo::finish_attempt(&mut conn, id.0, attempt, &finish).await,
            Err(err) => Err(err.into()),
        };
        match (result, delays.next()) {
            (Err(err), Some(delay)) => {
                tracing::debug!(job_id = %id.0, error = %err, "recording the end of a try failed; retrying");
                tokio::time::sleep(*delay).await;
            }
            (result, _) => return result,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use postit_data::jobs::NewJob;
    use serde_json::{Value, json};
    use sqlx::PgPool;
    use uuid::Uuid;

    use chrono::Utc;

    use super::*;
    use crate::job::RetryPolicy;
    use crate::registry::JobRegistry;

    fn state(pool: &PgPool, registry: JobRegistry) -> HandlerState {
        HandlerState {
            pool: pool.clone(),
            registry: Arc::new(registry),
            worker: "test:1".into(),
            clock: Arc::new(postit_core::SystemClock),
            unknown_type_max_age: std::time::Duration::from_hours(7 * 24),
        }
    }

    async fn insert(pool: &PgPool, job_type: &str) -> JobId {
        let id = Uuid::now_v7();
        let payload = json!({});
        let mut conn = pool.acquire().await.unwrap_or_else(|e| unreachable!("{e}"));
        JobsRepo::insert(
            &mut conn,
            &NewJob {
                id,
                job_type,
                queue: "default",
                payload: &payload,
                run_at: Utc::now(),
                recurring_name: None,
                retried_from: None,
            },
        )
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
        JobId(id)
    }

    fn envelope(job_type: &str) -> Envelope {
        Envelope {
            job_type: job_type.into(),
            payload: Value::Null,
        }
    }

    /// `(state, [(outcome, error_kind, error_code)])` of a job, attempts oldest first.
    async fn ledger(
        pool: &PgPool,
        id: JobId,
    ) -> (
        String,
        Vec<(Option<String>, Option<String>, Option<String>)>,
    ) {
        let state: String = sqlx::query_scalar("SELECT state FROM jobs WHERE id = $1")
            .bind(id.0)
            .fetch_one(pool)
            .await
            .unwrap_or_else(|e| unreachable!("{e}"));
        let attempts = sqlx::query_as(
            "SELECT outcome, error_kind, error_code FROM job_attempts
             WHERE job_id = $1 ORDER BY attempt",
        )
        .bind(id.0)
        .fetch_all(pool)
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
        (state, attempts)
    }

    #[expect(
        clippy::unnecessary_wraps,
        reason = "shorthand for expected nullable columns"
    )]
    fn s(v: &str) -> Option<String> {
        Some(v.to_string())
    }

    #[sqlx::test(migrations = "../data/migrations")]
    async fn an_unknown_job_type_is_killed_as_not_registered(pool: PgPool) {
        let st = state(&pool, JobRegistry::default());
        let id = insert(&pool, "gone_in_this_release").await;
        // Past `unknown_type_max_age` (a younger job is re-queued instead).
        sqlx::query("UPDATE jobs SET created_at = now() - interval '8 days' WHERE id = $1")
            .bind(id.0)
            .execute(&pool)
            .await
            .unwrap_or_else(|e| unreachable!("{e}"));
        let outcome = run_recorded(&st, envelope("gone_in_this_release"), id, 1).await;
        assert!(matches!(outcome, Outcome::Abort(_)));
        assert_eq!(
            ledger(&pool, id).await,
            (
                "killed".to_string(),
                vec![(s("killed"), s("unknown_type"), s("not_registered"))]
            )
        );
    }

    #[sqlx::test(migrations = "../data/migrations")]
    async fn a_try_left_open_by_a_lost_worker_is_closed_as_interrupted_by_the_next(pool: PgPool) {
        let mut registry = JobRegistry::default();
        registry
            .register(RetryPolicy::None, |_: Probe, _| async { Ok(()) })
            .unwrap_or_else(|e| unreachable!("{e}"));
        let st = state(&pool, registry);
        let id = insert(&pool, "probe").await;
        // Try 1 starts on a worker that then dies: its attempt is never finished.
        let mut conn = pool.acquire().await.unwrap_or_else(|e| unreachable!("{e}"));
        JobsRepo::begin_attempt(&mut conn, id.0, 1, "dead:1", None)
            .await
            .unwrap_or_else(|e| unreachable!("{e}"));
        drop(conn);
        assert_eq!(
            run_recorded(&st, envelope("probe"), id, 2).await,
            Outcome::Done
        );
        assert_eq!(
            ledger(&pool, id).await,
            (
                "succeeded".to_string(),
                vec![
                    (s("interrupted"), s("interrupted"), s("worker_lost")),
                    (s("succeeded"), None, None),
                ]
            )
        );
    }

    #[sqlx::test(migrations = "../data/migrations")]
    async fn a_redelivered_try_is_finished_under_the_number_the_ledger_gave_it(pool: PgPool) {
        let mut registry = JobRegistry::default();
        registry
            .register(RetryPolicy::None, |_: Probe, _| async { Ok(()) })
            .unwrap_or_else(|e| unreachable!("{e}"));
        let st = state(&pool, registry);
        let id = insert(&pool, "probe").await;
        // Try 1 failed and was recorded; the storage lost the ack and redelivers it as try 1.
        let mut conn = pool.acquire().await.unwrap_or_else(|e| unreachable!("{e}"));
        JobsRepo::begin_attempt(&mut conn, id.0, 1, "w1", None)
            .await
            .unwrap_or_else(|e| unreachable!("{e}"));
        JobsRepo::finish_attempt(
            &mut conn,
            id.0,
            1,
            &Finish {
                outcome: AttemptOutcome::Retrying,
                state: JobState::Failed,
                next_run_at: Some(Utc::now()),
                error: Some(AttemptError {
                    kind: ErrorKind::Retry,
                    code: Some("transient"),
                    message: None,
                }),
            },
        )
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
        drop(conn);
        assert_eq!(
            run_recorded(&st, envelope("probe"), id, 1).await,
            Outcome::Done
        );
        assert_eq!(
            ledger(&pool, id).await,
            (
                "succeeded".to_string(),
                vec![
                    (s("retrying"), s("retry"), s("transient")),
                    (s("succeeded"), None, None),
                ]
            )
        );
    }

    /// Makes the first `failures` writes that close an attempt fail. The count lives in a
    /// sequence because `nextval` is not rolled back with the failed transaction.
    async fn fail_closing_writes(pool: &PgPool, failures: i64) {
        for sql in [
            "CREATE SEQUENCE finish_faults",
            "CREATE TABLE finish_fault_limit (n BIGINT NOT NULL)",
            "CREATE FUNCTION fail_finish() RETURNS trigger LANGUAGE plpgsql AS $$
             BEGIN
                 IF NEW.finished_at IS NOT NULL
                    AND nextval('finish_faults') <= (SELECT n FROM finish_fault_limit) THEN
                     RAISE EXCEPTION 'injected finish failure';
                 END IF;
                 RETURN NEW;
             END $$",
            "CREATE TRIGGER fail_finish BEFORE UPDATE ON job_attempts
             FOR EACH ROW EXECUTE FUNCTION fail_finish()",
        ] {
            sqlx::query(sql)
                .execute(pool)
                .await
                .unwrap_or_else(|e| unreachable!("{e}"));
        }
        sqlx::query("INSERT INTO finish_fault_limit (n) VALUES ($1)")
            .bind(failures)
            .execute(pool)
            .await
            .unwrap_or_else(|e| unreachable!("{e}"));
    }

    /// How many closing writes were tried.
    async fn closing_writes(pool: &PgPool) -> i64 {
        sqlx::query_scalar("SELECT last_value FROM finish_faults")
            .fetch_one(pool)
            .await
            .unwrap_or_else(|e| unreachable!("{e}"))
    }

    fn probe_state(pool: &PgPool) -> HandlerState {
        let mut registry = JobRegistry::default();
        registry
            .register(RetryPolicy::None, |_: Probe, _| async { Ok(()) })
            .unwrap_or_else(|e| unreachable!("{e}"));
        state(pool, registry)
    }

    #[sqlx::test(migrations = "../data/migrations")]
    async fn a_finish_write_that_fails_once_is_retried_and_recorded(pool: PgPool) {
        let st = probe_state(&pool);
        let id = insert(&pool, "probe").await;
        fail_closing_writes(&pool, 1).await;
        assert_eq!(
            run_recorded(&st, envelope("probe"), id, 1).await,
            Outcome::Done
        );
        assert_eq!(closing_writes(&pool).await, 2);
        assert_eq!(
            ledger(&pool, id).await,
            ("succeeded".to_string(), vec![(s("succeeded"), None, None)])
        );
    }

    #[sqlx::test(migrations = "../data/migrations")]
    async fn a_finish_write_is_tried_three_times_then_given_up(pool: PgPool) {
        let st = probe_state(&pool);
        let id = insert(&pool, "probe").await;
        fail_closing_writes(&pool, 3).await;
        assert_eq!(
            run_recorded(&st, envelope("probe"), id, 1).await,
            Outcome::Done
        );
        assert_eq!(closing_writes(&pool).await, 3);
        assert_eq!(
            ledger(&pool, id).await,
            ("running".to_string(), vec![(None, None, None)])
        );
    }

    #[derive(serde::Serialize, serde::Deserialize)]
    struct Probe;
    impl crate::job::Job for Probe {
        const JOB_TYPE: &'static str = "probe";
        const QUEUE: crate::job::Queue = crate::job::Queue::Default;
    }
}
