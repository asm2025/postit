//! Status polling for platforms that process uploads asynchronously.

use std::time::Duration;

use postit_core::{AgentError, PublishProgress};
use tokio::time::{Instant, sleep};

/// How a polling loop ended without an error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Poll<T> {
    /// The step produced a value.
    Ready(T),
    /// The timeout passed before the step produced a value.
    TimedOut,
}

/// Runs `step` at once, then again after each `interval`, until it returns `Some`, fails, or the
/// next wait would pass `timeout`. The step always runs at least once, even when `timeout` is
/// shorter than `interval`. A step error returns at once. With `progress`, a requested
/// cancellation after an empty step returns [`AgentError::Cancelled`], checked before the
/// timeout. The loop never sleeps past `timeout`: when `timeout` is not a multiple of `interval`
/// it returns [`Poll::TimedOut`] at the last step that fits (a 3.5 s timeout with a 1 s interval
/// ends at 3 s, after four steps).
///
/// # Errors
/// The step's error, or [`AgentError::Cancelled`].
pub async fn poll_until<T, F, Fut>(
    interval: Duration,
    timeout: Duration,
    progress: Option<&dyn PublishProgress>,
    mut step: F,
) -> Result<Poll<T>, AgentError>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<Option<T>, AgentError>>,
{
    let started = Instant::now();
    loop {
        if let Some(value) = step().await? {
            return Ok(Poll::Ready(value));
        }
        if progress.is_some_and(PublishProgress::cancel_requested) {
            return Err(AgentError::Cancelled);
        }
        if started.elapsed().saturating_add(interval) > timeout {
            return Ok(Poll::TimedOut);
        }
        sleep(interval).await;
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

    use async_trait::async_trait;

    use super::*;

    struct Flag(AtomicBool);

    #[async_trait]
    impl PublishProgress for Flag {
        async fn save_container_ref(&self, _: &str) -> Result<(), AgentError> {
            Ok(())
        }
        fn cancel_requested(&self) -> bool {
            self.0.load(Ordering::SeqCst)
        }
    }

    const SEC: Duration = Duration::from_secs(1);

    #[tokio::test(start_paused = true)]
    async fn poll_ready_on_third_step() {
        let calls = AtomicU32::new(0);
        let start = Instant::now();
        let out = poll_until(SEC, SEC * 10, None, || async {
            let n = calls.fetch_add(1, Ordering::SeqCst) + 1;
            Ok((n == 3).then_some(n))
        })
        .await
        .unwrap();
        assert_eq!(out, Poll::Ready(3));
        assert_eq!(start.elapsed(), SEC * 2);
    }

    #[tokio::test(start_paused = true)]
    async fn poll_times_out() {
        let start = Instant::now();
        let out = poll_until(SEC, SEC * 3, None, || async { Ok(None::<u8>) })
            .await
            .unwrap();
        assert_eq!(out, Poll::TimedOut);
        assert!(start.elapsed() >= SEC * 3);
    }

    #[tokio::test(start_paused = true)]
    async fn poll_times_out_without_sleeping_past_a_non_multiple_timeout() {
        let calls = AtomicU32::new(0);
        let start = Instant::now();
        let out = poll_until(SEC, SEC * 3 + SEC / 2, None, || async {
            calls.fetch_add(1, Ordering::SeqCst);
            Ok(None::<u8>)
        })
        .await
        .unwrap();
        assert_eq!(out, Poll::TimedOut);
        assert_eq!(start.elapsed(), SEC * 3);
        assert_eq!(calls.load(Ordering::SeqCst), 4);
    }

    #[tokio::test(start_paused = true)]
    async fn poll_cancel_wins_over_timeout() {
        let flag = Flag(AtomicBool::new(true));
        let err = poll_until(SEC * 5, SEC, Some(&flag), || async { Ok(None::<u8>) })
            .await
            .unwrap_err();
        assert!(matches!(err, AgentError::Cancelled));
    }

    #[tokio::test(start_paused = true)]
    async fn poll_runs_once_when_timeout_below_interval() {
        let calls = AtomicU32::new(0);
        let out = poll_until(SEC * 5, SEC, None, || async {
            calls.fetch_add(1, Ordering::SeqCst);
            Ok(None::<u8>)
        })
        .await
        .unwrap();
        assert_eq!(out, Poll::TimedOut);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn poll_cancel_stops() {
        let flag = Flag(AtomicBool::new(false));
        let calls = AtomicU32::new(0);
        let err = poll_until(SEC, SEC * 10, Some(&flag), || async {
            if calls.fetch_add(1, Ordering::SeqCst) == 1 {
                flag.0.store(true, Ordering::SeqCst);
            }
            Ok(None::<u8>)
        })
        .await
        .unwrap_err();
        assert!(matches!(err, AgentError::Cancelled));
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test(start_paused = true)]
    async fn poll_step_error_returns() {
        let calls = AtomicU32::new(0);
        let err = poll_until(SEC, SEC * 10, None, || async {
            calls.fetch_add(1, Ordering::SeqCst);
            Err::<Option<u8>, _>(AgentError::Transport {
                message: "down".to_owned(),
            })
        })
        .await
        .unwrap_err();
        assert!(matches!(err, AgentError::Transport { .. }));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }
}
