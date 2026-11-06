//! Request sending shared by both protocols: one deadline bounds every attempt, every wait
//! between attempts, and the body read; failures map to a [`FailureReason`] and never carry
//! text from the provider's body.

use std::sync::Arc;

use bytes::Bytes;
use postit_core::{Clock as _, SystemClock};
use postit_http::{
    HttpError, Idempotency, Jitter, PublicClient, RateLimitHeaders, RetryPolicy, RetryRequest,
    Sensitivity, send_with_retry,
};
use tokio::time::Instant;
use url::Url;

use crate::error::{FailureReason, LlmError};

/// The largest success body read.
pub(crate) const BODY_CAP: usize = 4 * 1024 * 1024;

/// Attempts per call, the first included.
const MAX_ATTEMPTS: u32 = 3;

/// Sends the requests `make` builds, at most three attempts, all before `deadline`, and
/// returns the success body. Requests are not idempotent: a lost response is never retried,
/// since a second call costs the user again.
///
/// # Errors
/// The failure's reason; `retry_after` is set when the provider sent one.
pub(crate) async fn send(
    client: &PublicClient,
    deadline: Instant,
    mut make: impl FnMut() -> Result<reqwest::Request, HttpError>,
) -> Result<Bytes, LlmError> {
    let left = deadline.saturating_duration_since(Instant::now());
    if left.is_zero() {
        return Err(LlmError::new(FailureReason::Timeout));
    }
    let policy = RetryPolicy {
        max_attempts: MAX_ATTEMPTS,
        budget: left,
        max_retry_after: left,
        ..RetryPolicy::default()
    };
    let headers = RateLimitHeaders::default();
    let call = async {
        let result = send_with_retry(
            RetryRequest {
                client: client.into(),
                policy: &policy,
                idempotency: Idempotency::NonIdempotent,
                headers: &headers,
                now: Arc::new(|| SystemClock.now()),
                limiter: None,
                sensitivity: Sensitivity::Normal,
            },
            &mut Jitter::from_entropy(),
            || {
                let mut request = make()?;
                *request.timeout_mut() = Some(deadline.saturating_duration_since(Instant::now()));
                Ok(request)
            },
        )
        .await;
        match result {
            Ok(response) => read_body(response, deadline).await,
            Err(err) => Err(map_error(&err, deadline)),
        }
    };
    tokio::time::timeout_at(deadline, call)
        .await
        .unwrap_or(Err(LlmError::new(FailureReason::Timeout)))
}

/// Reads a success body up to [`BODY_CAP`]. A redirect (the public client follows none) is
/// not the protocol's answer.
async fn read_body(mut response: reqwest::Response, deadline: Instant) -> Result<Bytes, LlmError> {
    let invalid = || LlmError::new(FailureReason::InvalidResponse);
    if !response.status().is_success() {
        return Err(invalid());
    }
    if response
        .content_length()
        .is_some_and(|n| n > BODY_CAP as u64)
    {
        return Err(invalid());
    }
    let mut body = Vec::new();
    loop {
        match response.chunk().await {
            Ok(Some(chunk)) => {
                if body.len() + chunk.len() > BODY_CAP {
                    return Err(invalid());
                }
                body.extend_from_slice(&chunk);
            }
            Ok(None) => return Ok(Bytes::from(body)),
            Err(_) => return Err(LlmError::new(lost(deadline))),
        }
    }
}

/// A call whose answer never fully arrived: the deadline cut it, or the provider failed.
fn lost(deadline: Instant) -> FailureReason {
    if Instant::now() >= deadline {
        FailureReason::Timeout
    } else {
        FailureReason::Unavailable
    }
}

/// Maps what `send_with_retry` returned. Only the status and `Retry-After` are kept; the body
/// excerpt is dropped.
fn map_error(err: &HttpError, deadline: Instant) -> LlmError {
    match err {
        HttpError::Status {
            status,
            retry_after,
            ..
        } => LlmError {
            reason: status_reason(*status),
            retry_after: *retry_after,
        },
        HttpError::RateLimited { retry_after } => LlmError {
            reason: FailureReason::RateLimited,
            retry_after: *retry_after,
        },
        HttpError::OutcomeUnknown(_) => LlmError::new(lost(deadline)),
        HttpError::Transient(_) => LlmError::new(FailureReason::Unavailable),
        HttpError::AddressRejected { .. } => LlmError::new(FailureReason::AddressRefused),
        HttpError::Permanent(_) => LlmError::new(FailureReason::InvalidResponse),
    }
}

/// The reason a non-success status gives.
fn status_reason(status: u16) -> FailureReason {
    match status {
        401 | 403 => FailureReason::KeyRejected,
        404 => FailureReason::ModelNotFound,
        429 => FailureReason::RateLimited,
        400..=499 => FailureReason::BadRequest,
        500..=599 => FailureReason::Unavailable,
        _ => FailureReason::InvalidResponse,
    }
}

/// `path` under `base`, with exactly one slash between them and the base path kept:
/// `https://h/v1` and `https://h/v1/` both give `https://h/v1/chat/completions`.
pub(crate) fn join(base: &Url, path: &str) -> Url {
    let mut url = base.clone();
    let joined = format!(
        "{}/{}",
        base.path().trim_end_matches('/'),
        path.trim_start_matches('/')
    );
    url.set_path(&joined);
    url
}

/// The text after a leading `<think>...</think>` block, which some local and reasoning models
/// put in `content`. An unclosed block is all thinking: nothing is left.
pub(crate) fn strip_think(text: &str) -> &str {
    const OPEN: &str = "<think>";
    const CLOSE: &str = "</think>";
    let trimmed = text.trim_start();
    let Some(rest) = trimmed.strip_prefix(OPEN) else {
        return text;
    };
    match rest.find(CLOSE) {
        Some(end) => rest[end + CLOSE.len()..].trim_start(),
        None => "",
    }
}

/// The JSON in a model's answer: the whole text when it parses, else the inside of the first
/// fenced code block (with or without a language tag).
pub(crate) fn extract_json(text: &str) -> Option<&str> {
    let trimmed = text.trim();
    if serde_json::from_str::<serde::de::IgnoredAny>(trimmed).is_ok() {
        return Some(trimmed);
    }
    let start = trimmed.find("```")?;
    let after = &trimmed[start + 3..];
    // The opening fence's line may name a language (`json`); the block starts after it.
    let body_start = after.find('\n')?;
    let body = &after[body_start + 1..];
    let end = body.find("```")?;
    Some(body[..end].trim())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn join_keeps_one_slash_and_the_base_path() {
        let cases = [
            (
                "https://h.example/v1",
                "https://h.example/v1/chat/completions",
            ),
            (
                "https://h.example/v1/",
                "https://h.example/v1/chat/completions",
            ),
            ("https://h.example", "https://h.example/chat/completions"),
            ("https://h.example/", "https://h.example/chat/completions"),
            (
                "https://h.example/v1beta/openai/",
                "https://h.example/v1beta/openai/chat/completions",
            ),
        ];
        for (base, want) in cases {
            let base = Url::parse(base).unwrap_or_else(|_| unreachable!());
            assert_eq!(join(&base, "/chat/completions").as_str(), want);
            assert_eq!(join(&base, "chat/completions").as_str(), want);
        }
    }

    #[test]
    fn strip_think_removes_a_leading_block_only() {
        assert_eq!(strip_think("<think>x</think>\n{}"), "{}");
        assert_eq!(strip_think("  <think>\na\nb\n</think>answer"), "answer");
        assert_eq!(strip_think("<think>never closed"), "");
        assert_eq!(strip_think("plain"), "plain");
        assert_eq!(strip_think("a <think>b</think>"), "a <think>b</think>");
    }

    #[test]
    fn extract_json_takes_whole_text_or_first_fence() {
        assert_eq!(extract_json(" {\"a\":1} "), Some("{\"a\":1}"));
        assert_eq!(
            extract_json("Sure:\n```json\n{\"a\":1}\n```\nmore ```\n{\"b\":2}\n```"),
            Some("{\"a\":1}")
        );
        assert_eq!(extract_json("```\n{\"a\":1}\n```"), Some("{\"a\":1}"));
        assert_eq!(extract_json("no json here"), None);
        assert_eq!(extract_json("```json\n{\"a\":1}"), None);
    }

    #[test]
    fn status_reasons() {
        assert_eq!(status_reason(401), FailureReason::KeyRejected);
        assert_eq!(status_reason(403), FailureReason::KeyRejected);
        assert_eq!(status_reason(404), FailureReason::ModelNotFound);
        assert_eq!(status_reason(429), FailureReason::RateLimited);
        assert_eq!(status_reason(400), FailureReason::BadRequest);
        assert_eq!(status_reason(413), FailureReason::BadRequest);
        assert_eq!(status_reason(503), FailureReason::Unavailable);
        assert_eq!(status_reason(302), FailureReason::InvalidResponse);
    }
}
