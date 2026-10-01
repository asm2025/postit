use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use serde_json::{Value, json};

use crate::error::{ApiError, ErrorCode, ProblemDetails};
use crate::state::Readiness;

#[utoipa::path(get, path = "/health", tag = "probes", responses((status = 200, description = "Process is alive")))]
pub async fn health() -> Json<Value> {
    Json(json!({ "status": "ok" }))
}

/// # Errors
///
/// Returns a 503 problem naming the first failing readiness check.
#[utoipa::path(get, path = "/ready", tag = "probes", responses(
    (status = 200, description = "Ready to serve"),
    (status = 503, description = "A dependency is not ready", body = ProblemDetails, content_type = "application/problem+json"),
))]
pub async fn ready(State(readiness): State<Arc<dyn Readiness>>) -> Result<Json<Value>, ApiError> {
    readiness
        .check()
        .await
        .map(|()| Json(json!({ "status": "ready" })))
        .map_err(|failing| {
            ApiError::new(ErrorCode::Unavailable).with_detail(format!("not ready: {failing}"))
        })
}
