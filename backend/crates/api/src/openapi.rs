//! The generated API contract. `openapi()` is the single source for the served spec and
//! `cargo xtask openapi` (which writes `backend/openapi.json`).

use axum::Json;
use axum::Router;
use axum::extract::State;
use axum::routing::get;
use postit_config::Environment;
use utoipa::openapi::security::{AuthorizationCode, Flow, OAuth2, Scopes, SecurityScheme};
use utoipa::{Modify, OpenApi};

use crate::state::AppState;

/// Stand-in authorize URL used in the generated spec, because the real one depends on the deployment.
pub const PLACEHOLDER_AUTHORIZE: &str = "https://idp.invalid/authorize";
/// Stand-in token URL used in the generated spec.
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
        crate::routes::me::get_preferences,
        crate::routes::me::put_preferences,
        crate::routes::users::list,
        crate::routes::users::get,
        crate::routes::users::patch,
        crate::routes::users::delete,
        crate::routes::accounts::credentials,
        crate::routes::accounts::confirm,
        crate::routes::accounts::list,
        crate::routes::accounts::get,
        crate::routes::accounts::update,
        crate::routes::accounts::check,
        crate::routes::accounts::disconnect,
        crate::routes::accounts::import,
        crate::routes::media::upload,
        crate::routes::media::list,
        crate::routes::media::usage,
        crate::routes::media::get,
        crate::routes::media::delete,
        crate::routes::media::content,
        crate::routes::media::thumbnail,
        crate::routes::media::view_url,
        crate::routes::media::start_session,
        crate::routes::media::append,
        crate::routes::media::session_head,
        crate::routes::media::complete,
        crate::routes::audit::list,
        crate::routes::jobs::list,
        crate::routes::jobs::stats,
        crate::routes::jobs::get,
        crate::routes::jobs::recurring,
        crate::routes::jobs::retry,
        crate::routes::jobs::cancel,
        crate::routes::jobs::delete,
        crate::routes::jobs::trigger,
    ),
    components(schemas(
        crate::dto::UserDto,
        crate::dto::MeDto,
        crate::dto::AuthConfigDto,
        crate::dto::DeleteMeRequest,
        crate::dto::LocaleDto,
        crate::dto::PreferencesDto,
        crate::dto::UpdatePreferences,
        crate::dto::PatchUserRequest,
        crate::dto::RoleDto,
        crate::dto::StatusDto,
        crate::dto::AuditEventDto,
        crate::dto::UserRef,
        crate::dto::AuditEventKindDto,
        crate::dto_jobs::JobStateDto,
        crate::dto_jobs::JobActionsDto,
        crate::dto_jobs::JobDto,
        crate::dto_jobs::AttemptDto,
        crate::dto_jobs::JobDetailDto,
        crate::dto_jobs::JobCountDto,
        crate::dto_jobs::ThroughputBucketDto,
        crate::dto_jobs::JobStatsDto,
        crate::dto_jobs::RecurringJobDto,
        crate::dto_jobs::JobActionResponse,
        crate::dto_accounts::AccountStatusDto,
        crate::dto_accounts::ConnectKindDto,
        crate::dto_accounts::AccountDto,
        crate::dto_accounts::DiscoveredAccountDto,
        crate::dto_accounts::CredentialCheckResponse,
        crate::dto_accounts::ConfirmRequest,
        crate::dto_accounts::AccountChangeDto,
        crate::dto_accounts::ConnectedAccountDto,
        crate::dto_accounts::ConfirmResponse,
        crate::dto_accounts::UpdateAccountRequest,
        crate::dto_accounts::DisconnectResponse,
        crate::dto_accounts::ImportStatusDto,
        crate::dto_accounts::ImportRowDto,
        crate::dto_accounts::ImportReportDto,
        crate::dto_media::MediaKindDto,
        crate::dto_media::MediaStatusDto,
        crate::dto_media::MediaDto,
        crate::dto_media::UploadSessionRequest,
        crate::dto_media::UploadSessionDto,
        crate::dto_media::UploadOffsetDto,
        crate::dto_media::MediaUsageDto,
        crate::dto_media::ViewUrlsDto,
        crate::error::ProblemDetails,
        crate::error::ErrorCode,
    )),
    modifiers(&SecurityAddon),
)]
struct ApiDoc;

/// The `OpenAPI` document for the API.
#[must_use]
pub fn openapi() -> utoipa::openapi::OpenApi {
    let mut spec = ApiDoc::openapi();
    spec.info.version = env!("CARGO_PKG_VERSION").to_string();
    spec
}

/// The `OpenAPI` document as pretty-printed JSON with a trailing newline.
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
/// web and mobile public client through PKCE.
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
