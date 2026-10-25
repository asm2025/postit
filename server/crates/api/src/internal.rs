//! The internal webhook Zitadel calls to send its identity emails (verification codes,
//! password resets) through postit's mailer, so they arrive in the postit layout.
//!
//! It is served on its own router and never merged into [`crate::api_router`]: it needs no
//! CORS, no bearer auth, and no request logging, because every request carries a one-time
//! code, an address, and a signed link. The `Zitadel-Signature` header is its only credential.

use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::extract::State;
use axum::http::{HeaderMap, HeaderName, StatusCode};
use axum::routing::post;
use chrono::Duration;
use postit_config::RedactedSecret;
use postit_core::Clock;
use postit_mail::{
    IdentityMessage, MailError, Mailer, RenderedMessage, bare_address, render_identity,
};
use tower_http::sensitive_headers::SetSensitiveRequestHeadersLayer;
use tower_http::timeout::TimeoutLayer;

use crate::zitadel_signature;

/// The most time a request may take, so a stalled sender cannot hold a connection open.
const REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

/// What the webhook needs: where to send, how to verify, and its limits.
#[derive(Clone)]
pub struct IdentityMailState {
    /// Delivers the rendered message.
    pub mailer: Arc<dyn Mailer>,
    /// The key Zitadel signs requests with.
    pub signing_key: RedactedSecret,
    /// Source of the current time for the timestamp check.
    pub clock: Arc<dyn Clock>,
    /// How far a signature's timestamp may differ from now.
    pub tolerance: Duration,
    /// The largest accepted request body, in bytes.
    pub max_body_bytes: usize,
}

/// Only `POST /internal/identity-mail`. No CORS, no auth extractor, no `TraceLayer` (the
/// request is never logged); the signature header is marked sensitive.
pub fn internal_router(state: IdentityMailState) -> Router {
    Router::new()
        .route("/internal/identity-mail", post(identity_mail))
        .layer(SetSensitiveRequestHeadersLayer::new([
            HeaderName::from_static(zitadel_signature::HEADER),
        ]))
        .layer(TimeoutLayer::with_status_code(
            StatusCode::REQUEST_TIMEOUT,
            REQUEST_TIMEOUT,
        ))
        .with_state(state)
}

/// True when the body read failed because it passed the length limit, not because the
/// connection or stream broke. `http_body_util::LengthLimitError` is not a dependency here, so
/// the error chain is matched on its message (covered by the 413 test).
fn is_length_limit(error: &axum::Error) -> bool {
    let mut source: Option<&(dyn std::error::Error + 'static)> = Some(error);
    while let Some(current) = source {
        if current.to_string().contains("length limit exceeded") {
            return true;
        }
        source = current.source();
    }
    false
}

/// Answers with an empty body. Logs the event type and status only: the address, code, and
/// link are secret or personal.
async fn identity_mail(
    State(state): State<IdentityMailState>,
    headers: HeaderMap,
    body: Body,
) -> StatusCode {
    let bytes = match axum::body::to_bytes(body, state.max_body_bytes).await {
        Ok(bytes) => bytes,
        Err(error) if is_length_limit(&error) => {
            tracing::warn!(status = 413, "identity mail rejected: body too large");
            return StatusCode::PAYLOAD_TOO_LARGE;
        }
        Err(_) => {
            tracing::warn!(
                status = 400,
                "identity mail rejected: body could not be read"
            );
            return StatusCode::BAD_REQUEST;
        }
    };
    let signature = headers
        .get(zitadel_signature::HEADER)
        .and_then(|v| v.to_str().ok());
    if let Err(error) = zitadel_signature::verify(
        signature,
        &bytes,
        state.signing_key.expose().as_bytes(),
        state.clock.now(),
        state.tolerance,
    ) {
        tracing::warn!(status = 401, reason = %error, "identity mail rejected: bad signature");
        return StatusCode::UNAUTHORIZED;
    }
    let Ok(message) = serde_json::from_slice::<IdentityMessage>(&bytes) else {
        tracing::warn!(status = 400, "identity mail rejected: unreadable payload");
        return StatusCode::BAD_REQUEST;
    };
    let event_type = message.context_info.event_type.as_str();
    let Some(to) = bare_address(&message.context_info.recipient_email_address) else {
        tracing::warn!(
            status = 400,
            event_type,
            "identity mail rejected: bad recipient"
        );
        return StatusCode::BAD_REQUEST;
    };
    let Ok(rendered) = render_identity(&message) else {
        // `error!`, not `warn!`: the payload was signed and parsed, so a render failure is a
        // postit template bug rather than bad input.
        tracing::error!(
            status = 500,
            event_type,
            "identity mail could not be rendered"
        );
        return StatusCode::INTERNAL_SERVER_ERROR;
    };
    let result = state
        .mailer
        .send(RenderedMessage {
            to,
            subject: rendered.subject,
            text: rendered.text,
            html: rendered.html,
        })
        .await;
    match result {
        Ok(()) => {
            tracing::info!(status = 204, event_type, "identity mail sent");
            StatusCode::NO_CONTENT
        }
        Err(MailError::Transient(_)) => {
            tracing::warn!(status = 503, event_type, "identity mail deferred");
            StatusCode::SERVICE_UNAVAILABLE
        }
        Err(MailError::Permanent(_)) => {
            tracing::warn!(
                status = 502,
                event_type,
                "identity mail rejected by the mail server"
            );
            StatusCode::BAD_GATEWAY
        }
        Err(_) => {
            tracing::error!(status = 500, event_type, "identity mail could not be sent");
            StatusCode::INTERNAL_SERVER_ERROR
        }
    }
}
