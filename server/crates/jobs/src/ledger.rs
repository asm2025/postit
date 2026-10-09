//! Records every try of every job in the job ledger (`jobs`, `job_attempts`). The ledger is
//! best-effort around execution: a failed ledger write is logged and never changes whether
//! or how a job runs.

use chrono::Utc;
use postit_data::jobs::{
    AttemptError, AttemptOutcome, Begin, ErrorKind, Finish, JobState, JobsRepo,
};

use crate::backend::HandlerState;
use crate::console_spec::ConsoleSpec;
use crate::dispatch::{self, Dispatched, Envelope, Outcome};
use crate::job::JobId;

pub(crate) async fn run_recorded(
    state: &HandlerState,
    envelope: Envelope,
    job_id: JobId,
    attempt: u32,
) -> Outcome {
    let attempt_no = i32::try_from(attempt).unwrap_or(i32::MAX);
    let job_type = envelope.job_type.clone();
    let max_attempts = state
        .registry
        .get(&job_type)
        .and_then(|r| r.retry.max_attempts())
        .and_then(|n| i32::try_from(n).ok());

    // `None`: the ledger could not be read. The job runs unrecorded, because a ledger
    // failure must never change whether a job runs.
    let begin = match begin(state, job_id, attempt_no, max_attempts).await {
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
            dispatch::record_skipped_run(&state.pool, &state.registry, &envelope).await;
            return Outcome::Done;
        }
        // Already finished; its recurring run (if any) already has its outcome.
        Some(Begin::Finished) => return Outcome::Done,
        Some(Begin::Run) | None => {}
    }

    let dispatched =
        dispatch::dispatch_detailed(&state.pool, &state.registry, envelope, job_id, attempt).await;
    if begin == Some(Begin::Run) {
        match finish(state, job_id, attempt_no, &job_type, &dispatched).await {
            Ok(true) => {}
            Ok(false) => tracing::warn!(
                job_id = %job_id.0, attempt,
                "the try was superseded while it ran; its result was not recorded"
            ),
            Err(err) => {
                tracing::warn!(job_id = %job_id.0, error = %err, "could not record the end of a try");
            }
        }
    }
    dispatched.outcome
}

async fn begin(
    state: &HandlerState,
    id: JobId,
    attempt: i32,
    max: Option<i32>,
) -> Result<Begin, postit_data::DataError> {
    let mut conn = state.pool.acquire().await?;
    JobsRepo::begin_attempt(&mut conn, id.0, attempt, &state.worker, max).await
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
    let error = d.failure.as_ref().map(|f| AttemptError {
        kind: f.kind,
        code: f.kind.default_code(),
        message: show_message.then_some(f.message.as_str()),
    });
    let (outcome, job_state, next_run_at) = match (&d.outcome, d.failure.as_ref().map(|f| f.kind)) {
        (Outcome::Done, _) => (AttemptOutcome::Succeeded, JobState::Succeeded, None),
        (Outcome::RetryAfter(delay), _) => (
            AttemptOutcome::Retrying,
            JobState::Failed,
            Some(Utc::now() + chrono::Duration::from_std(*delay).unwrap_or_default()),
        ),
        (Outcome::Abort(_), Some(ErrorKind::RetriesExhausted)) => {
            (AttemptOutcome::Failed, JobState::Dead, None)
        }
        (Outcome::Abort(_), _) => (AttemptOutcome::Killed, JobState::Killed, None),
    };
    let mut conn = state.pool.acquire().await?;
    JobsRepo::finish_attempt(
        &mut conn,
        id.0,
        attempt,
        &Finish {
            outcome,
            state: job_state,
            next_run_at,
            error,
        },
    )
    .await
}
