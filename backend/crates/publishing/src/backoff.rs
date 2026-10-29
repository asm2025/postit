//! How long the engine waits before trying a delivery again or asking the platform again.

use std::time::Duration;

use rand::RngExt as _;

/// The first retry's ceiling; each attempt doubles it.
const RETRY_BASE: Duration = Duration::from_secs(30);
/// The longest a retry waits.
const RETRY_MAX: Duration = Duration::from_mins(30);
/// The first poll's delay; each poll doubles it.
const POLL_BASE: Duration = Duration::from_secs(15);
/// The longest a poll waits.
const POLL_MAX: Duration = Duration::from_mins(15);

/// The ceiling of the delay before the retry after `attempt` earlier attempts:
/// `min(30 min, 30 s × 2^attempt)`.
#[must_use]
pub fn retry_ceiling(attempt: i32) -> Duration {
    doubled(RETRY_BASE, attempt, RETRY_MAX)
}

/// The delay before the retry after `attempt` earlier attempts, with full jitter: a uniform pick
/// in `[0, retry_ceiling(attempt)]`. A platform's own `RetryAfter` wins over it.
#[must_use]
pub fn retry_delay(attempt: i32) -> Duration {
    let ceiling = u64::try_from(retry_ceiling(attempt).as_millis()).unwrap_or(u64::MAX);
    Duration::from_millis(rand::rng().random_range(0..=ceiling))
}

/// The delay before the status poll after `poll` earlier polls: `min(15 min, 15 s × 2^poll)`,
/// without jitter.
#[must_use]
pub fn poll_delay(poll: i32) -> Duration {
    doubled(POLL_BASE, poll, POLL_MAX)
}

/// `min(max, base × 2^n)`; a negative `n` counts as zero.
fn doubled(base: Duration, n: i32, max: Duration) -> Duration {
    let exponent = u32::try_from(n.max(0)).unwrap_or(0).min(31);
    base.saturating_mul(1_u32 << exponent).min(max)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retry_ceiling_doubles_up_to_thirty_minutes() {
        assert_eq!(retry_ceiling(0), Duration::from_secs(30));
        assert_eq!(retry_ceiling(1), Duration::from_mins(1));
        assert_eq!(retry_ceiling(5), Duration::from_mins(16));
        assert_eq!(retry_ceiling(6), Duration::from_mins(30));
        assert_eq!(retry_ceiling(40), Duration::from_mins(30));
        assert_eq!(retry_ceiling(-1), Duration::from_secs(30));
    }

    #[test]
    fn retry_delay_stays_within_its_ceiling() {
        for attempt in 0..8 {
            for _ in 0..50 {
                assert!(retry_delay(attempt) <= retry_ceiling(attempt));
            }
        }
    }

    #[test]
    fn poll_delay_doubles_up_to_fifteen_minutes() {
        assert_eq!(poll_delay(0), Duration::from_secs(15));
        assert_eq!(poll_delay(1), Duration::from_secs(30));
        assert_eq!(poll_delay(5), Duration::from_mins(8));
        assert_eq!(poll_delay(6), Duration::from_mins(15));
        assert_eq!(poll_delay(31), Duration::from_mins(15));
    }
}
