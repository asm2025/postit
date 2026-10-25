use std::time::Duration;

use postit_jobs::{Job, JobContext, JobError, JobId, JobRegistry, JobsError, Queue, RetryPolicy};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
struct Probe;

impl Job for Probe {
    const JOB_TYPE: &'static str = "probe";
    const QUEUE: Queue = Queue::Default;
}

async fn ok(_: Probe, _: JobContext) -> Result<(), JobError> {
    Ok(())
}

#[test]
fn registering_a_job_type_twice_is_an_error() {
    let mut registry = JobRegistry::default();
    registry
        .register(RetryPolicy::None, ok)
        .unwrap_or_else(|e| unreachable!("first: {e}"));
    let Err(err) = registry.register(RetryPolicy::None, ok) else {
        unreachable!("second registration unexpectedly succeeded");
    };
    assert!(matches!(err, JobsError::DuplicateJobType("probe")));
    assert_eq!(registry.job_types(), vec!["probe".to_string()]);
}

#[test]
fn backoff_doubles_up_to_the_cap_and_stops_at_max_attempts() {
    let policy = RetryPolicy::Backoff {
        max_attempts: Some(4),
        initial: Duration::from_secs(1),
        max: Duration::from_secs(3),
    };
    assert_eq!(policy.max_attempts(), Some(4));
    assert_eq!(policy.delay_before_next(1), Some(Duration::from_secs(1)));
    assert_eq!(policy.delay_before_next(2), Some(Duration::from_secs(2)));
    assert_eq!(policy.delay_before_next(3), Some(Duration::from_secs(3)));
    assert_eq!(policy.delay_before_next(4), None);
}

#[test]
fn unlimited_backoff_never_runs_out() {
    let policy = RetryPolicy::Backoff {
        max_attempts: None,
        initial: Duration::from_secs(30),
        max: Duration::from_secs(3600),
    };
    assert_eq!(policy.max_attempts(), None);
    assert_eq!(
        policy.delay_before_next(1_000),
        Some(Duration::from_secs(3600))
    );
}

#[test]
fn no_retry_means_one_attempt() {
    assert_eq!(RetryPolicy::None.max_attempts(), Some(1));
    assert_eq!(RetryPolicy::None.delay_before_next(1), None);
}

#[test]
fn last_attempt_is_reported_only_at_the_cap() {
    let job_id = JobId(uuid::Uuid::now_v7());
    let ctx = |attempt| JobContext {
        job_id,
        attempt,
        max_attempts: Some(3),
    };
    assert!(!ctx(1).is_last_attempt());
    assert!(!ctx(2).is_last_attempt());
    assert!(ctx(3).is_last_attempt());
    let unlimited = JobContext {
        job_id,
        attempt: 99,
        max_attempts: None,
    };
    assert!(!unlimited.is_last_attempt());
}
