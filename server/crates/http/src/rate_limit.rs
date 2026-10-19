//! Per-bucket rate limiters, so an agent can stay under a platform's published quotas
//! before the server has to say no.

use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

use governor::clock::{Clock, DefaultClock, ReasonablyRealtime};
use governor::middleware::NoOpMiddleware;
use governor::state::keyed::DefaultKeyedStateStore;
use governor::{Quota, RateLimiter};

use crate::error::HttpError;

/// One named bucket and its quota, such as "post" at 300 per 3 hours.
#[derive(Clone, Debug)]
pub struct BucketSpec {
    /// The bucket name agents pass to [`RateLimiters::acquire`].
    pub name: String,
    /// How fast the bucket admits requests per key.
    pub quota: Quota,
}

type KeyedLimiter<C> =
    RateLimiter<String, DefaultKeyedStateStore<String>, C, NoOpMiddleware<<C as Clock>::Instant>>;

/// A set of named token buckets, each tracking capacity separately per key (usually an
/// account id), so one account's burst never slows another's.
///
/// These limits are per process: every process holds its own counters. Limits shared across
/// processes (several workers, or the API and a worker) use the quota tables in the
/// database, which arrive with B2, not this type.
#[derive(Clone)]
pub struct RateLimiters<C: Clock = DefaultClock> {
    buckets: Arc<HashMap<String, KeyedLimiter<C>>>,
}

impl<C: Clock> fmt::Debug for RateLimiters<C> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut names: Vec<_> = self.buckets.keys().collect();
        names.sort();
        f.debug_struct("RateLimiters")
            .field("buckets", &names)
            .finish()
    }
}

impl RateLimiters {
    /// Builds the buckets on the real clock. A repeated name keeps the last spec.
    #[must_use]
    pub fn new(specs: Vec<BucketSpec>) -> Self {
        Self::with_clock(specs, DefaultClock::default())
    }
}

impl<C: Clock + Clone> RateLimiters<C> {
    /// Builds the buckets on `clock`, for tests that drive time by hand.
    #[allow(clippy::needless_pass_by_value)] // by value: callers hand the clock over
    #[must_use]
    pub fn with_clock(specs: Vec<BucketSpec>, clock: C) -> Self {
        let buckets = specs
            .into_iter()
            .map(|spec| {
                (
                    spec.name,
                    RateLimiter::new(spec.quota, DefaultKeyedStateStore::default(), clock.clone()),
                )
            })
            .collect();
        Self {
            buckets: Arc::new(buckets),
        }
    }
}

impl<C: Clock> RateLimiters<C> {
    fn bucket(&self, name: &str) -> Result<&KeyedLimiter<C>, HttpError> {
        self.buckets
            .get(name)
            .ok_or_else(|| HttpError::Permanent(format!("unknown rate-limit bucket {name}")))
    }

    /// Takes one permit if the bucket has capacity for `key` now, without waiting.
    ///
    /// # Errors
    ///
    /// [`HttpError::Permanent`] when `bucket` was never configured.
    pub fn try_acquire(&self, bucket: &str, key: &str) -> Result<bool, HttpError> {
        Ok(self.bucket(bucket)?.check_key(&key.to_string()).is_ok())
    }
}

impl<C: Clock + ReasonablyRealtime> RateLimiters<C> {
    /// Waits until the bucket admits a request for `key`, then takes the permit.
    ///
    /// # Errors
    ///
    /// [`HttpError::Permanent`] when `bucket` was never configured.
    pub async fn acquire(&self, bucket: &str, key: &str) -> Result<(), HttpError> {
        self.bucket(bucket)?.until_key_ready(&key.to_string()).await;
        Ok(())
    }
}
