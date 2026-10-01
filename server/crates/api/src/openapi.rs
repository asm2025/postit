//! The generated API contract. `openapi()` is the single source for the served spec and
//! `cargo xtask openapi` (which writes `api/openapi.json`).

use axum::Json;
use axum::Router;
use axum::extract::State;
use axum::routing::get;
use postit_config::Environment;
use utoipa::openapi::security::{AuthorizationCode, Flow, OAuth2, Scopes, SecurityScheme};
use utoipa::{Modify, OpenApi};

use crate::state::AppState;

pub const PLACEHOLDER_AUTHORIZE: &str = "https://idp.invalid/authorize";
pub const PLACEHOLDER_TOKEN: &str = "https://idp.invalid/token";

struct SecurityAddon;

impl Modify for SecurityAddon {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        let components = openapi.components.get_or_insert_with(Default::default);
        components.add_security_scheme(
            "oidc",
            SecurityScheme::OAuth2(OAuth2::new([Flow::AuthorizationCode(
                AuthorizationCode::new(
                    PLACEHOLDER_AUTHORIZE,
                    PLACEHOLDER_TOKEN,
                    Scopes::from_iter([
                        ("openid", "OpenID Connect sign-in"),
                        ("profile", "Name and username"),
                        ("email", "Email address"),
                        ("offline_access", "Refresh tokens"),
                    ]),
                ),
            )])),
        );
    }
}

#[derive(OpenApi)]
#[openapi(
    info(
        title = "postit API",
        description = "Self-hosted social publishing for a small team."
    ),
    paths(
        crate::routes::probes::health,
        crate::routes::probes::ready,
        crate::routes::auth::config,
        crate::routes::me::get_me,
        crate::routes::me::delete_me,
        crate::routes::users::list,
        crate::routes::users::get,
        crate::routes::users::patch,
        crate::routes::users::delete,
        crate::routes::audit::list,
    ),
    components(schemas(
        crate::dto::UserDto,
        crate::dto::MeDto,
        crate::dto::AuthConfigDto,
        crate::dto::DeleteMeRequest,
        crate::dto::PatchUserRequest,
        crate::dto::RoleDto,
        crate::dto::StatusDto,
        crate::dto::AuditEventDto,
        crate::dto::UserRef,
        crate::error::ProblemDetails,
        crate::error::ErrorCode,
    )),
    modifiers(&SecurityAddon),
)]
struct ApiDoc;

#[must_use]
pub fn openapi() -> utoipa::openapi::OpenApi {
    let mut spec = ApiDoc::openapi();
    spec.info.version = env!("CARGO_PKG_VERSION").to_string();
    spec
}

#[must_use]
pub fn openapi_json_pretty() -> String {
    let mut text = serde_json::to_string_pretty(&openapi()).unwrap_or_default();
    text.push('\n');
    text
}

async fn served_spec(State(state): State<AppState>) -> Json<serde_json::Value> {
    let mut spec = serde_json::to_value(openapi()).unwrap_or_default();
    if let Some(doc) = state.auth.discovery_document() {
        let flow = &mut spec["components"]["securitySchemes"]["oidc"]["flows"]["authorizationCode"];
        if let Some(url) = doc.authorization_endpoint {
            flow["authorizationUrl"] = url.into();
        }
        if let Some(url) = doc.token_endpoint {
            flow["tokenUrl"] = url.into();
        }
    }
    Json(spec)
}

/// `/api/openapi.json` always; Swagger UI at `/docs` outside production, signing in with the
/// Flutter public client through PKCE.
pub fn docs_router(state: &AppState) -> Router<AppState> {
    let router = Router::new().route("/api/openapi.json", get(served_spec));
    if state.settings.environment == Environment::Production {
        return router;
    }
    let oauth = utoipa_swagger_ui::oauth::Config::new()
        .client_id(&state.settings.client_id)
        .scopes(state.settings.scopes.clone())
        .use_pkce_with_authorization_code_grant(true);
    let swagger = utoipa_swagger_ui::SwaggerUi::new("/docs")
        .config(utoipa_swagger_ui::Config::new(["/api/openapi.json"]))
        .oauth(oauth);
    router.merge(swagger)
}
