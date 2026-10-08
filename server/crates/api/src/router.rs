use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::extract::{DefaultBodyLimit, FromRef, MatchedPath};
use axum::http::{HeaderName, HeaderValue, Method, Request, Response, StatusCode, header};
use axum::routing::get;
use tower::ServiceBuilder;
use tower_http::cors::{AllowOrigin, CorsLayer};
use tower_http::limit::RequestBodyLimitLayer;
use tower_http::request_id::{
    MakeRequestId, PropagateRequestIdLayer, RequestId, SetRequestIdLayer,
};
use tower_http::sensitive_headers::SetSensitiveRequestHeadersLayer;
use tower_http::timeout::TimeoutLayer;
use tower_http::trace::TraceLayer;
use tracing::Span;

use crate::error::render_problems;
use crate::middleware::{client_ip, ip_rate_limit, sanitize_request_id};
use crate::routes::probes;
use crate::state::{AppState, Readiness};

impl FromRef<AppState> for Arc<dyn Readiness> {
    fn from_ref(state: &AppState) -> Self {
        Arc::clone(&state.readiness)
    }
}

const REQUEST_ID: HeaderName = HeaderName::from_static("x-request-id");

/// Request IDs are UUID v7 (time-ordered), not tower-http's v4.
#[derive(Clone, Copy)]
struct MakeRequestUuidV7;

impl MakeRequestId for MakeRequestUuidV7 {
    fn make_request_id<B>(&mut self, _: &Request<B>) -> Option<RequestId> {
        HeaderValue::from_str(&uuid::Uuid::now_v7().to_string())
            .ok()
            .map(RequestId::new)
    }
}

/// The request span: method, the *matched route* (never the raw URI, whose query string can
/// carry search terms, e-mail addresses, or an OAuth `code`), and the request ID. `status`
/// is recorded when the response is ready. No header value or body is ever recorded.
/// `MatchedPath` is present because `Router::layer` wraps each route after matching.
fn make_span<B>(req: &Request<B>) -> Span {
    let route = req
        .extensions()
        .get::<MatchedPath>()
        .map_or("<unmatched>", MatchedPath::as_str);
    let request_id = req
        .headers()
        .get(&REQUEST_ID)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default();
    tracing::info_span!(
        "request",
        method = %req.method(),
        route,
        request_id,
        status = tracing::field::Empty,
    )
}

fn on_response<B>(res: &Response<B>, latency: Duration, span: &Span) {
    span.record("status", res.status().as_u16());
    tracing::debug!(latency_ms = latency.as_millis(), "response");
}

fn cors(state: &AppState) -> CorsLayer {
    CorsLayer::new()
        .allow_origin(AllowOrigin::list(state.settings.cors_origins.clone()))
        .allow_methods([
            Method::GET,
            Method::POST,
            Method::PATCH,
            Method::PUT,
            Method::DELETE,
            Method::OPTIONS,
        ])
        .allow_headers([
            header::AUTHORIZATION,
            header::CONTENT_TYPE,
            HeaderName::from_static("x-postit-act-as"),
            header::IF_MATCH,
            header::IF_NONE_MATCH,
            HeaderName::from_static("idempotency-key"),
        ])
        .expose_headers([
            header::ETAG,
            header::LOCATION,
            header::RETRY_AFTER,
            REQUEST_ID,
        ])
        .allow_credentials(false)
}

/// The API port's router (roles `all` and `api`).
pub fn api_router(state: AppState) -> Router {
    let v1 = crate::routes::v1_router();
    let app = Router::new()
        .route("/health", get(probes::health))
        .route("/ready", get(probes::ready))
        .nest("/api/v1", v1)
        .merge(crate::openapi::docs_router(&state));

    // Order (outermost first). RequestBodyLimit must be *outside* Timeout: Timeout requires
    // a `Default` response body and tower-http's limit `ResponseBody` has none. The timeout
    // still covers the handler reading the body. DefaultBodyLimit lifts axum's own 2 MB cap
    // in `Json`/`Bytes` to `body_limit`, so the configured value is the only limit.
    app.layer(
        ServiceBuilder::new()
            .layer(axum::middleware::from_fn(sanitize_request_id))
            .layer(SetRequestIdLayer::new(REQUEST_ID, MakeRequestUuidV7))
            .layer(PropagateRequestIdLayer::new(REQUEST_ID))
            .layer(axum::middleware::from_fn(render_problems))
            .layer(SetSensitiveRequestHeadersLayer::new([
                header::AUTHORIZATION,
            ]))
            .layer(
                TraceLayer::new_for_http()
                    .make_span_with(make_span)
                    .on_response(on_response),
            )
            .layer(cors(&state))
            .layer(axum::middleware::from_fn_with_state(
                state.clone(),
                client_ip,
            ))
            .layer(axum::middleware::from_fn_with_state(
                state.clone(),
                ip_rate_limit,
            ))
            .layer(RequestBodyLimitLayer::new(state.settings.body_limit))
            .layer(DefaultBodyLimit::max(state.settings.body_limit))
            .layer(TimeoutLayer::with_status_code(
                StatusCode::REQUEST_TIMEOUT,
                state.settings.request_timeout,
            )),
    )
    .with_state(state)
}

/// Request IDs and tracing only: the layers every listener carries, including the web
/// app's, which must stay outside the API's CORS, auth, rate-limit, body-limit and timeout
/// stack.
pub fn observed<S: Clone + Send + Sync + 'static>(router: Router<S>) -> Router<S> {
    router.layer(
        ServiceBuilder::new()
            .layer(axum::middleware::from_fn(sanitize_request_id))
            .layer(SetRequestIdLayer::new(REQUEST_ID, MakeRequestUuidV7))
            .layer(PropagateRequestIdLayer::new(REQUEST_ID))
            .layer(
                TraceLayer::new_for_http()
                    .make_span_with(make_span)
                    .on_response(on_response),
            ),
    )
}

/// The worker port's router: `/health` and `/ready` only.
pub fn probe_router(readiness: Arc<dyn Readiness>) -> Router {
    Router::new()
        .route("/health", get(probes::health))
        .route("/ready", get(probes::ready))
        .layer(
            ServiceBuilder::new()
                .layer(axum::middleware::from_fn(sanitize_request_id))
                .layer(SetRequestIdLayer::new(REQUEST_ID, MakeRequestUuidV7))
                .layer(PropagateRequestIdLayer::new(REQUEST_ID))
                .layer(axum::middleware::from_fn(render_problems))
                .layer(
                    TraceLayer::new_for_http()
                        .make_span_with(make_span)
                        .on_response(on_response),
                ),
        )
        .with_state(readiness)
}
