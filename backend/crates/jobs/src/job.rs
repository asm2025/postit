use std::time::Duration;

use serde::Serialize;
use serde::de::DeserializeOwned;
use uuid::Uuid;

/// A job payload. Payloads hold IDs only — never an address, name, body, token, or any
/// other user content — because job rows are visible to operators and the admin console.
pub trait Job: Serialize + DeserializeOwned + Send + Sync + 'static {
    /// Stable type name stored with each job.
    const JOB_TYPE: &'static str;
    /// The queue this type runs on.
    const QUEUE: Queue;
}

/// The queues workers consume; each has its own concurrency.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Queue {
    /// Email delivery.
    Mail,
    /// Housekeeping and retention.
    Maintenance,
    /// Everything else.
    Default,
    /// Publishing deliveries: the delivery state machine, not the job runtime, retries them.
    Publishing,
}

impl Queue {
    /// Every queue.
    pub const ALL: [Queue; 4] = [
        Queue::Mail,
        Queue::Maintenance,
        Queue::Default,
        Queue::Publishing,
    ];

    /// The stable queue name used in storage and settings.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Mail => "mail",
            Self::Maintenance => "maintenance",
            Self::Default => "default",
            Self::Publishing => "publishing",
        }
    }
}

/// ID of a job; also the task ID in job storage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct JobId(pub Uuid);

/// What a handler is told about the run it is in.
#[derive(Debug, Clone, Copy)]
pub struct JobContext {
    /// ID of the job being run.
    pub job_id: JobId,
    /// 1-based.
    pub attempt: u32,
    /// `None` = retried without limit.
    pub max_attempts: Option<u32>,
}

impl JobContext {
    /// Whether a failure now would be final.
    #[must_use]
    pub fn is_last_attempt(&self) -> bool {
        self.max_attempts.is_some_and(|max| self.attempt >= max)
    }
}

/// How failed tries are retried.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetryPolicy {
    /// One attempt. For jobs whose own domain state drives retries (plan 03's publishing).
    None,
    /// Retry with exponential backoff.
    Backoff {
        /// Total tries allowed; none means unbounded.
        max_attempts: Option<u32>,
        /// Delay before the first retry.
        initial: Duration,
        /// Largest delay between tries.
        max: Duration,
    },
}

impl RetryPolicy {
    /// Total tries the policy allows; none means unbounded.
    #[must_use]
    pub fn max_attempts(&self) -> Option<u32> {
        match self {
            Self::None => Some(1),
            Self::Backoff { max_attempts, .. } => *max_attempts,
        }
    }

    /// Delay before attempt `attempt + 1`, or `None` when `attempt` was the last one.
    /// Doubles from `initial` per attempt, capped at `max`.
    #[must_use]
    pub fn delay_before_next(&self, attempt: u32) -> Option<Duration> {
        match self {
            Self::None => None,
            Self::Backoff {
                max_attempts,
                initial,
                max,
            } => {
                if max_attempts.is_some_and(|cap| attempt >= cap) {
                    return None;
                }
                let exponent = attempt.saturating_sub(1).min(20);
                let delay = initial.saturating_mul(1u32 << exponent);
                Some(delay.min(*max))
            }
        }
    }
}
