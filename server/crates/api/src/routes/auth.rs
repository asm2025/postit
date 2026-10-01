use axum::Json;
use axum::extract::State;

use crate::dto::AuthConfigDto;
use crate::state::AppState;

/// Public OIDC settings the client needs to start a sign-in.
#[utoipa::path(get, path = "/api/v1/auth/config", tag = "auth",
    responses((status = 200, body = AuthConfigDto)))]
pub async fn config(State(state): State<AppState>) -> Json<AuthConfigDto> {
    let s = &state.settings;
    Json(AuthConfigDto {
        issuer: s.issuer.as_str().trim_end_matches('/').to_string(),
        client_id: s.client_id.clone(),
        scopes: s.scopes.clone(),
    })
}
