//! Public authentication config.

use axum::Json;
use axum::extract::State;

use crate::dto::AuthConfigDto;
use crate::error::ProblemDetails;
use crate::state::AppState;

/// Public OIDC settings the client needs to start a sign-in.
#[utoipa::path(get, path = "/api/v1/auth/config", operation_id = "auth_config", tag = "auth",
    responses((status = 200, body = AuthConfigDto), (status = 403, description = "`owner_only` (`X-Postit-Act-As` sent; this route does not accept it)", body = ProblemDetails, content_type = "application/problem+json"), (status = 429, description = "`rate_limited` (per-IP limit)", body = ProblemDetails, content_type = "application/problem+json")))]
pub async fn config(State(state): State<AppState>) -> Json<AuthConfigDto> {
    let s = &state.settings;
    Json(AuthConfigDto {
        issuer: s.issuer.as_str().trim_end_matches('/').to_string(),
        client_id: s.client_id.clone(),
        android_client_id: s.android_client_id.clone(),
        ios_client_id: s.ios_client_id.clone(),
        scopes: s.scopes.clone(),
    })
}
