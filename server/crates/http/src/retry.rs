//! Retries inside one outbound call: full-jitter exponential backoff, a sleep budget, and
//! server reset headers, with the idempotency rules that keep a non-idempotent request from
//! ever being sent twice when it may already have taken effect.

use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use rand::rngs::StdRng;
use rand::{RngExt as _, SeedableRng as _};
use reqwest::header::{HeaderMap, HeaderName};

use crate::client::{check_literal, send_traced};
use crate::error::{HttpError, retry_after_at, status_error};
use crate::policy::{AddressPolicy, rejected_host};
use crate::redact::redacted_error_text;

/// Whether sending a request twice is safe. Decides which failures may be retried.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Idempotency {
    /// Repeating the request has the same effect as sending it once (GET, PUT, DELETE, or a
    /// POST with an idempotency key).
    Idempotent,
    /// Repeating the request could act twice (publishing a post). Retried only when the
    /// server provably never acted on it.
    NonIdempotent,
}

/// How hard one call retries. Delivery-level retries belong to the publishing engine; this
/// covers retries inside one agent call.
#[derive(Clone, Debug)]
pub struct RetryPolicy {
    /// Total attempts, the first one included.
    pub max_attempts: u32,
    /// The backoff ceiling for the first retry; doubles per attempt.
    pub base_delay: Duration,
    /// The largest backoff ceiling.
    pub max_delay: Duration,
    /// The longest server-requested wait honored in place. A longer one surfaces as
    /// [`HttpError::RateLimited`] with the full value, for the caller to schedule.
    pub max_retry_after: Duration,
    /// The cap on total time spent sleeping between attempts of one call.
    pub budget: Duration,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            base_delay: Duration::from_millis(500),
            max_delay: Duration::from_secs(30),
            max_retry_after: Duration::from_mins(15),
            budget: Duration::from_secs(60),
        }
    }
}

/// One place a server can say when to try again.
#[derive(Clone, Debug)]
pub enum ResetHeader {
    /// The standard `Retry-After` header: delta-seconds or an HTTP-date.
    RetryAfter,
    /// A header holding the number of seconds to wait.
    DeltaSeconds(HeaderName),
    /// A header holding the reset instant as Unix epoch seconds (X's `x-rate-limit-reset`).
    EpochSeconds(HeaderName),
    /// A header holding the reset instant as Unix epoch milliseconds.
    EpochMillis(HeaderName),
}

/// The reset headers an agent's platform sends, in order; the first one present with a
/// parsable value wins.
#[derive(Clone, Debug)]
pub struct RateLimitHeaders(pub Vec<ResetHeader>);

impl Default for RateLimitHeaders {
    fn default() -> Self {
        Self(vec![ResetHeader::RetryAfter])
    }
}

/// The source of "now" for HTTP-date and epoch reset headers, injectable for tests.
pub type Now = Arc<dyn Fn() -> DateTime<Utc> + Send + Sync>;

/// The random source for retry delays. Production seeds it from the OS; tests use a fixed
/// seed so delays reproduce exactly.
#[derive(Debug)]
pub struct Jitter {
    rng: StdRng,
}

impl Jitter {
    /// A jitter source seeded from the operating system.
    #[must_use]
    pub fn from_entropy() -> Self {
        Self {
            rng: rand::make_rng(),
        }
    }

    /// A jitter source with a fixed seed, for reproducible tests.
    #[must_use]
    pub fn seeded(seed: u64) -> Self {
        Self {
            rng: StdRng::seed_from_u64(seed),
        }
    }

    /// Full-jitter backoff for the retry after `attempt` (1-based): a uniform pick in
    /// `0..=min(max_delay, base_delay * 2^(attempt - 1))`.
    pub fn backoff(&mut self, attempt: u32, policy: &RetryPolicy) -> Duration {
        let exponent = attempt.saturating_sub(1).min(31);
        let ceiling = policy
            .base_delay
            .saturating_mul(1_u32 << exponent)
            .min(policy.max_delay);
        let nanos = u64::try_from(ceiling.as_nanos()).unwrap_or(u64::MAX);
        Duration::from_nanos(self.rng.random_range(0..=nanos))
    }

    /// `base` plus a random extra of less than 10%, so clients told the same reset instant
    /// do not all return at once.
    pub fn spread(&mut self, base: Duration) -> Duration {
        let tenth = u64::try_from((base / 10).as_nanos()).unwrap_or(u64::MAX);
        if tenth == 0 {
            return base;
        }
        base.saturating_add(Duration::from_nanos(self.rng.random_range(0..tenth)))
    }
}

/// Everything [`send_with_retry`] needs besides the request builder and the jitter source.
pub struct RetryRequest<'a> {
    /// The client to send with: from `build_client`, or `build_public_client` when
    /// `address_policy` is set.
    pub client: &'a reqwest::Client,
    /// Attempts, delays, and budget.
    pub policy: &'a RetryPolicy,
    /// Whether the request may be sent twice.
    pub idempotency: Idempotency,
    /// Where the platform says when to try again.
    pub headers: &'a RateLimitHeaders,
    /// When set, every attempt goes through `execute_public`'s IP-literal check.
    pub address_policy: Option<AddressPolicy>,
    /// "Now" for date-based reset headers.
    pub now: Now,
}

impl fmt::Debug for RetryRequest<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RetryRequest")
            .field("policy", &self.policy)
            .field("idempotency", &self.idempotency)
            .field("headers", &self.headers)
            .field("address_policy", &self.address_policy)
            .finish_non_exhaustive()
    }
}

/// What to do after a failed attempt.
enum Decision {
    /// Give up and return this error now.
    Stop(HttpError),
    /// Retry after `delay`, or return `error` if attempts or budget run out.
    Retry {
        error: HttpError,
        delay: Duration,
        classification: &'static str,
    },
}

/// Sends a request, retrying under `req.policy`. `make` builds a fresh request per attempt
/// (a streaming body is consumed by each send); an error from it stops the retry and is
/// returned as is.
///
/// Idempotent requests retry transient failures, rate limiting, and 5xx/429 responses.
/// Non-idempotent requests retry only connect failures and 429/503 responses that carry a
/// reset header; a timeout, any other failure after connect, or any other 5xx returns
/// [`HttpError::OutcomeUnknown`] at once. A reset value over `max_retry_after` returns
/// [`HttpError::RateLimited`] with the full value without sleeping.
///
/// # Errors
///
/// Returns the last error once attempts or the sleep budget run out; a non-success response
/// becomes [`HttpError::Status`] (see [`status_error`]).
pub async fn send_with_retry<F>(
    req: RetryRequest<'_>,
    jitter: &mut Jitter,
    mut make: F,
) -> Result<reqwest::Response, HttpError>
where
    F: FnMut() -> Result<reqwest::Request, HttpError>,
{
    let max_attempts = req.policy.max_attempts.max(1);
    let mut slept = Duration::ZERO;
    let mut attempt: u32 = 0;
    loop {
        attempt = attempt.saturating_add(1);
        let request = make()?;
        if let Some(policy) = req.address_policy {
            check_literal(&request, policy)?;
        }

        let decision = match send_traced(req.client, request, Some(attempt)).await {
            Ok(response) => {
                let status = response.status();
                if status.is_success() || status.is_redirection() {
                    return Ok(response);
                }
                decide_status(&req, response, jitter, attempt).await
            }
            Err(err) => decide_transport(&req, err, jitter, attempt),
        };

        let (error, delay, classification) = match decision {
            Decision::Stop(error) => {
                tracing::debug!(attempt, "http request not retried");
                return Err(error);
            }
            Decision::Retry {
                error,
                delay,
                classification,
            } => (error, delay, classification),
        };

        let total = slept.saturating_add(delay);
        if attempt >= max_attempts || total > req.policy.budget {
            tracing::debug!(
                attempt,
                classification,
                "http retry stopped: attempts or budget exhausted"
            );
            return Err(error);
        }
        tracing::debug!(
            attempt,
            classification,
            delay_ms = u64::try_from(delay.as_millis()).unwrap_or(u64::MAX),
            "retrying http request"
        );
        tokio::time::sleep(delay).await;
        slept = total;
    }
}

async fn decide_status(
    req: &RetryRequest<'_>,
    response: reqwest::Response,
    jitter: &mut Jitter,
    attempt: u32,
) -> Decision {
    let now = (req.now)();
    let code = response.status().as_u16();
    let reset = reset_delay(req.headers, response.headers(), now);
    let retryable = match req.idempotency {
        Idempotency::Idempotent => code == 429 || code >= 500,
        Idempotency::NonIdempotent => (code == 429 || code == 503) && reset.is_some(),
    };
    if !retryable {
        if req.idempotency == Idempotency::NonIdempotent && code >= 500 {
            return Decision::Stop(HttpError::OutcomeUnknown(format!("server error: {code}")));
        }
        return Decision::Stop(status_error(response, now).await);
    }

    let delay = match reset {
        Some(wait) if wait > req.policy.max_retry_after => {
            return Decision::Stop(HttpError::RateLimited {
                retry_after: Some(wait),
            });
        }
        Some(wait) => jitter.spread(wait),
        None => jitter.backoff(attempt, req.policy),
    };
    Decision::Retry {
        error: status_error(response, now).await,
        delay,
        classification: if code == 429 {
            "rate limited"
        } else {
            "server error"
        },
    }
}

fn decide_transport(
    req: &RetryRequest<'_>,
    err: reqwest::Error,
    jitter: &mut Jitter,
    attempt: u32,
) -> Decision {
    if let Some(host) = rejected_host(&err) {
        return Decision::Stop(HttpError::AddressRejected { host });
    }
    // A connect failure (TLS handshake included) means the request never left: safe to
    // retry even when not idempotent.
    if err.is_connect() {
        return Decision::Retry {
            error: HttpError::from(err),
            delay: jitter.backoff(attempt, req.policy),
            classification: "connect error",
        };
    }
    if req.idempotency == Idempotency::NonIdempotent {
        return Decision::Stop(HttpError::OutcomeUnknown(redacted_error_text(&err)));
    }
    // Idempotent: any failure while sending or receiving (a timeout, a reset, a body error)
    // is safe to repeat. `HttpError::from` calls a reset `Permanent`, so decide from the raw
    // error here. Builder and redirect errors would fail the same way again.
    let retryable = !err.is_builder()
        && !err.is_redirect()
        && (err.is_request() || err.is_body() || err.is_timeout());
    let error = HttpError::from(err);
    if retryable {
        Decision::Retry {
            error,
            delay: jitter.backoff(attempt, req.policy),
            classification: "transport error",
        }
    } else {
        Decision::Stop(error)
    }
}

/// The wait named by the first configured reset header that is present and parsable.
fn reset_delay(
    headers: &RateLimitHeaders,
    map: &HeaderMap,
    now: DateTime<Utc>,
) -> Option<Duration> {
    headers.0.iter().find_map(|header| match header {
        ResetHeader::RetryAfter => retry_after_at(map, now),
        ResetHeader::DeltaSeconds(name) => header_u64(map, name).map(Duration::from_secs),
        ResetHeader::EpochSeconds(name) => {
            header_u64(map, name).map(|secs| until_epoch_millis(secs.saturating_mul(1000), now))
        }
        ResetHeader::EpochMillis(name) => {
            header_u64(map, name).map(|millis| until_epoch_millis(millis, now))
        }
    })
}

fn header_u64(map: &HeaderMap, name: &HeaderName) -> Option<u64> {
    map.get(name)?.to_str().ok()?.trim().parse().ok()
}

/// Time from `now` until an epoch-milliseconds instant; zero when it has passed.
fn until_epoch_millis(epoch_millis: u64, now: DateTime<Utc>) -> Duration {
    let now_millis = u64::try_from(now.timestamp_millis()).unwrap_or(0);
    Duration::from_millis(epoch_millis.saturating_sub(now_millis))
}
