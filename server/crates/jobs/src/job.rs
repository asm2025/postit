use std::time::Duration;

use serde::Serialize;
use serde::de::DeserializeOwned;
use uuid::Uuid;

/// A job payload. Payloads hold IDs only — never an address, name, body, token, or any
/// other user content — because job rows are visible to operators and the admin console.
pub trait Job: Serialize + DeserializeOwned + Send + Sync + 'static {
    const JOB_TYPE: &'static str;
    const QUEUE: Queue;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Queue {
    Mail,
    Maintenance,
    Default,
}

impl Queue {
    pub const ALL: [Queue; 3] = [Queue::Mail, Queue::Maintenance, Queue::Default];

    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Mail => "mail",
            Self::Maintenance => "maintenance",
            Self::Default => "default",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct JobId(pub Uuid);

#[derive(Debug, Clone, Copy)]
pub struct JobContext {
    pub job_id: JobId,
    /// 1-based.
    pub attempt: u32,
    /// `None` = retried without limit.
    pub max_attempts: Option<u32>,
}

impl JobContext {
    #[must_use]
    pub fn is_last_attempt(&self) -> bool {
        self.max_attempts.is_some_and(|max| self.attempt >= max)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetryPolicy {
    /// One attempt. For jobs whose own domain state drives retries (plan 03's publishing).
    None,
    Backoff {
        max_attempts: Option<u32>,
        initial: Duration,
        max: Duration,
    },
}

impl RetryPolicy {
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
