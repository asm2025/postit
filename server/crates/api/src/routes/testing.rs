//! Routes compiled only for tests (`testkit` feature): they exercise middleware and helpers
//! that no plan 02 production route uses yet. Production builds never contain them.

use std::time::Duration;

use axum::Extension;
use axum::Router;
use axum::body::Bytes;
use axum::extract::Query;
use axum::http::{HeaderMap, Method, StatusCode, header};
use axum::response::IntoResponse;
use axum::routing::{get, post, put};

use crate::error::{ApiError, ErrorCode};
use crate::extract::Scope;
use crate::idempotency::{IdempotencyKey, StoredResponse, request_hash, run};
use crate::middleware::Authenticated;
use crate::preconditions::{ETag, check_if_match};
use crate::state::AppState;

static MARKED_HITS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

#[derive(serde::Deserialize)]
struct Sleep {
    ms: u64,
}

async fn sleep(Query(q): Query<Sleep>) -> &'static str {
    tokio::time::sleep(Duration::from_millis(q.ms)).await;
    "done"
}

async fn echo(body: axum::body::Bytes) -> String {
    body.len().to_string()
}

/// Stands in for an authenticated route: marks the request, counts handler executions.
async fn marked(auth: Option<Extension<Authenticated>>) -> String {
    if let Some(Extension(a)) = auth {
        a.mark();
    }
    let hits = MARKED_HITS.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
    hits.to_string()
}

/// How many times the `/marked` handler has run in this process.
#[must_use]
pub fn marked_hits() -> usize {
    MARKED_HITS.load(std::sync::atomic::Ordering::SeqCst)
}

async fn idempotent(
    axum::extract::State(state): axum::extract::State<AppState>,
    Scope(scope): Scope,
    IdempotencyKey(key): IdempotencyKey,
    body: Bytes,
) -> Result<StoredResponse, ApiError> {
    let route = "/api/v1/_test/idempotent";
    let hash = request_hash(&Method::POST, route, &body);
    let parsed: serde_json::Value = serde_json::from_slice(&body).unwrap_or_default();
    run(
        &state.pool,
        &scope,
        key.as_deref(),
        route,
        &hash,
        || async move {
            if parsed["slow"].as_bool() == Some(true) {
                tokio::time::sleep(Duration::from_millis(300)).await;
            }
            if let Some(ms) = parsed["sleep_ms"].as_u64() {
                tokio::time::sleep(Duration::from_millis(ms)).await;
            }
            if parsed["fail"].as_bool() == Some(true) {
                return Err(ApiError::new(ErrorCode::ValidationFailed));
            }
            Ok(StoredResponse {
                status: StatusCode::CREATED,
                body: serde_json::json!({ "run": uuid::Uuid::now_v7() }),
            })
        },
    )
    .await
}

async fn versioned(Scope(_): Scope, headers: HeaderMap) -> Result<impl IntoResponse, ApiError> {
    check_if_match(&headers, ETag(7))?;
    Ok(([(header::ETAG, ETag(8).header_value())], "updated"))
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/sleep", get(sleep))
        .route("/echo", post(echo))
        .route("/marked", get(marked))
        .route("/idempotent", post(idempotent))
        .route("/versioned", put(versioned))
}
