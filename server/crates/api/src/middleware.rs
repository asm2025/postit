use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use axum::extract::{ConnectInfo, Request, State};
use axum::http::header;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

use crate::client_ip::{ClientIp, resolve};
use crate::error::{ApiError, ErrorCode};
use crate::state::AppState;

/// Put into the extensions of every request that carries an `Authorization` header; the
/// `Auth` extractor marks it when the token authenticates. A request that finishes
/// unmarked — bad or empty token, JWKS unavailable, an internal error, or a route that never
/// authenticates (probes, `/auth/config`, docs, 404s) — is charged to the per-IP bucket.
#[derive(Clone, Default)]
pub struct Authenticated(Arc<AtomicBool>);

impl Authenticated {
    pub fn mark(&self) {
        self.0.store(true, Ordering::Release);
    }

    fn is_marked(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}

/// Drops an incoming `x-request-id` that is not a UUID, so `SetRequestIdLayer` replaces it
/// with a fresh UUID v7 instead of echoing arbitrary client text into logs and problems.
pub async fn sanitize_request_id(mut req: Request, next: Next) -> Response {
    let valid = req
        .headers()
        .get("x-request-id")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| uuid::Uuid::parse_str(v).is_ok());
    if !valid {
        req.headers_mut().remove("x-request-id");
    }
    next.run(req).await
}

/// Resolves [`ClientIp`] into the request extensions.
pub async fn client_ip(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    mut req: Request,
    next: Next,
) -> Response {
    let xff = req
        .headers()
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok());
    let ip = resolve(peer.ip(), xff, &state.settings.trusted_proxies);
    req.extensions_mut().insert(ClientIp(ip));
    next.run(req).await
}

/// The per-IP bucket. A request without an `Authorization` header is charged up front. A
/// request with one is charged only if it ends without authenticating (see
/// [`Authenticated`]), so a team behind one NAT address never shares a bucket while every
/// failed or unused token is still bounded per IP. An IP that exhausted its bucket that way
/// is refused before its next token is verified.
pub async fn ip_rate_limit(
    State(state): State<AppState>,
    mut req: Request,
    next: Next,
) -> Response {
    let limited = |wait| {
        ApiError::new(ErrorCode::RateLimited)
            .with_retry_after(wait)
            .into_response()
    };
    let Some(ClientIp(ip)) = req.extensions().get::<ClientIp>().copied() else {
        return next.run(req).await;
    };
    if !req.headers().contains_key(header::AUTHORIZATION) {
        if let Err(wait) = state.limits.ip.check(&ip) {
            return limited(wait);
        }
        return next.run(req).await;
    }
    if let Some(wait) = state.limits.ip.blocked(&ip) {
        return limited(wait);
    }
    let outcome = Authenticated::default();
    req.extensions_mut().insert(outcome.clone());
    let response = next.run(req).await;
    if outcome.is_marked() {
        return response;
    }
    match state.limits.ip.penalize(&ip) {
        Ok(()) => response,
        Err(wait) => limited(wait),
    }
}
