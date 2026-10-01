//! One rate-limiter type for every bucket in `[rate_limit]`.

use std::collections::HashMap;
use std::hash::Hash;
use std::num::NonZeroU32;
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

use governor::clock::{Clock, DefaultClock};
use governor::{DefaultKeyedRateLimiter, Quota, RateLimiter};
use postit_config::RateBucket;

pub struct KeyedLimiter<K: Hash + Eq + Clone> {
    inner: DefaultKeyedRateLimiter<K>,
    clock: DefaultClock,
    /// Keys refused by [`Self::penalize`], until their next token. Lets a caller refuse a
    /// key *before* doing expensive work (token verification), which a governor check
    /// cannot do without consuming a token.
    blocked: Mutex<HashMap<K, Instant>>,
}

impl<K: Hash + Eq + Clone> KeyedLimiter<K> {
    /// `bucket` is validated non-zero by `postit-config`; zero falls back to 1 defensively.
    #[must_use]
    pub fn new(bucket: &RateBucket) -> Self {
        let rate = NonZeroU32::new(bucket.rate_per_minute).unwrap_or(NonZeroU32::MIN);
        let burst = NonZeroU32::new(bucket.burst).unwrap_or(NonZeroU32::MIN);
        Self {
            inner: RateLimiter::keyed(Quota::per_minute(rate).allow_burst(burst)),
            clock: DefaultClock::default(),
            blocked: Mutex::new(HashMap::new()),
        }
    }

    /// # Errors
    ///
    /// Returns how long to wait when `key` is over its limit.
    pub fn check(&self, key: &K) -> Result<(), Duration> {
        self.inner
            .check_key(key)
            .map_err(|not_until| not_until.wait_time_from(self.clock.now()))
    }

    /// Charges `key` like [`Self::check`]; when it is over the limit, also records it as
    /// blocked until its next token so [`Self::blocked`] refuses it without charging.
    ///
    /// # Errors
    ///
    /// Returns how long to wait when `key` is over its limit.
    pub fn penalize(&self, key: &K) -> Result<(), Duration> {
        self.check(key).inspect_err(|wait| {
            self.blocked
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .insert(key.clone(), Instant::now() + *wait);
        })
    }

    /// The remaining block for `key` from an earlier failed [`Self::penalize`], if any.
    #[must_use]
    pub fn blocked(&self, key: &K) -> Option<Duration> {
        self.blocked
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(key)
            .and_then(|until| until.checked_duration_since(Instant::now()))
            .filter(|left| !left.is_zero())
    }

    /// Drops state for keys whose buckets are full again, and expired blocks;
    /// `postit-server` calls this every minute so the key maps cannot grow without bound.
    pub fn retain_recent(&self) {
        self.inner.retain_recent();
        let now = Instant::now();
        self.blocked
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .retain(|_, until| *until > now);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allows_the_burst_then_refuses_with_a_wait() {
        let limiter = KeyedLimiter::new(&RateBucket {
            rate_per_minute: 1,
            burst: 2,
        });
        assert!(limiter.check(&"a").is_ok());
        assert!(limiter.check(&"a").is_ok());
        let wait = limiter.check(&"a").err();
        assert!(wait.is_some_and(|w| w > Duration::ZERO));
        assert!(limiter.check(&"b").is_ok(), "keys have separate buckets");
    }

    #[test]
    fn a_failed_penalty_blocks_the_key_without_charging_it_again() {
        let limiter = KeyedLimiter::new(&RateBucket {
            rate_per_minute: 1,
            burst: 1,
        });
        assert!(limiter.blocked(&"a").is_none());
        assert!(limiter.penalize(&"a").is_ok());
        assert!(
            limiter.blocked(&"a").is_none(),
            "within its burst a key is not blocked"
        );
        assert!(limiter.penalize(&"a").is_err());
        assert!(
            limiter
                .blocked(&"a")
                .is_some_and(|left| left > Duration::ZERO)
        );
        assert!(limiter.blocked(&"b").is_none());
    }
}
