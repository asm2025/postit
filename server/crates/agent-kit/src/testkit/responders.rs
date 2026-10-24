//! Wiremock response templates for the failures agents must classify.

use std::time::Duration;

use wiremock::ResponseTemplate;

/// 429, with a `Retry-After` header of `retry_after` seconds when given.
#[must_use]
pub fn rate_limited(retry_after: Option<u64>) -> ResponseTemplate {
    let response = ResponseTemplate::new(429);
    match retry_after {
        Some(secs) => response.insert_header("retry-after", secs.to_string().as_str()),
        None => response,
    }
}

/// 500 with a JSON error body.
#[must_use]
pub fn server_error() -> ResponseTemplate {
    ResponseTemplate::new(500).set_body_raw(r#"{"error":"server_error"}"#, "application/json")
}

/// 200 whose body is not valid JSON.
#[must_use]
pub fn malformed_json() -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_raw("{not json", "application/json")
}

/// A response that arrives after the client gave up: it delays 10 seconds, well past the 2
/// second request timeout of [`super::TestContext`]'s clients.
#[must_use]
pub fn dropped() -> ResponseTemplate {
    ResponseTemplate::new(200).set_delay(Duration::from_secs(10))
}
