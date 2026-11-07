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
        crate::routes::me::workspaces,
        crate::routes::notifications::list,
        crate::routes::notifications::mark_read,
        crate::routes::notifications::unread_count,
        crate::routes::push::vapid_public_key,
        crate::routes::push::register,
        crate::routes::push::delete,
        crate::routes::delegations::list,
        crate::routes::delegations::grant,
        crate::routes::delegations::change,
        crate::routes::delegations::revoke,
        crate::routes::delegations::list_received,
        crate::routes::delegations::leave,
        crate::routes::users::lookup,
        crate::routes::users::list,
        crate::routes::users::get,
        crate::routes::users::patch,
        crate::routes::users::delete,
        crate::routes::accounts::credentials,
        crate::routes::accounts::start_oauth,
        crate::routes::accounts::pending,
        crate::routes::accounts::confirm,
        crate::routes::platform_catalog::list,
        crate::routes::platform_catalog::icon,
        crate::routes::accounts::publish_options,
        crate::routes::workspace::get_brand_profile,
        crate::routes::workspace::put_brand_profile,
        crate::routes::workspace::get_writing_style,
        crate::routes::workspace::put_writing_style,
        crate::routes::posts::list,
        crate::routes::posts::create,
        crate::routes::posts::validate_draft,
        crate::routes::posts::validate_post,
        crate::routes::publishing::publish,
        crate::routes::publishing::calendar,
        crate::routes::publishing::list_jobs,
        crate::routes::publishing::get_job,
        crate::routes::publishing::reschedule,
        crate::routes::publishing::cancel_job,
        crate::routes::publishing::get_delivery,
        crate::routes::publishing::cancel_delivery,
        crate::routes::publishing::retry_delivery,
        crate::routes::publishing::resolve_delivery,
        crate::routes::posts::get,
        crate::routes::posts::patch,
        crate::routes::posts::delete,
        crate::routes::posts::put_override,
        crate::routes::posts::delete_override,
        crate::routes::organize::list_groups,
        crate::routes::organize::create_group,
        crate::routes::organize::replace_group,
        crate::routes::organize::delete_group,
        crate::routes::organize::list_campaigns,
        crate::routes::organize::create_campaign,
        crate::routes::organize::patch_campaign,
        crate::routes::organize::delete_campaign,
        crate::routes::accounts::list,
        crate::routes::accounts::get,
        crate::routes::accounts::update,
        crate::routes::accounts::check,
        crate::routes::accounts::disconnect,
        crate::routes::accounts::test_email,
        crate::routes::accounts::dns_check,
        crate::routes::accounts::import,
        crate::routes::platforms::oauth_callback,
        crate::routes::platforms::platform_event,
        crate::routes::contacts::list,
        crate::routes::contacts::create,
        crate::routes::contacts::get,
        crate::routes::contacts::update,
        crate::routes::contacts::delete,
        crate::routes::contacts::lists,
        crate::routes::contacts::create_list,
        crate::routes::contacts::rename_list,
        crate::routes::contacts::delete_list,
        crate::routes::contacts::add_member,
        crate::routes::contacts::remove_member,
        crate::routes::contacts::import,
        crate::routes::contacts::get_import,
        crate::routes::contacts::export,
        crate::routes::contacts::suppressions,
        crate::routes::contacts::remove_suppression,
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
        crate::routes::media::public_content,
        crate::routes::media::public_thumbnail,
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
        crate::dto::KindSwitchDto,
        crate::dto::KindSwitchUpdate,
        postit_core::NotificationKind,
        crate::dto::PatchUserRequest,
        crate::dto::RoleDto,
        crate::dto::StatusDto,
        crate::dto::AuditEventDto,
        crate::dto::UserRef,
        crate::dto::AuditEventKindDto,
        crate::dto_jobs::JobStateDto,
        crate::dto_jobs::DisabledReasonDto,
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
        crate::dto_accounts::StartOAuthRequest,
        crate::dto_accounts::OAuthStartResponse,
        crate::dto_accounts::PendingConnectionDto,
        crate::dto_accounts::ConfirmRequest,
        crate::dto_accounts::AccountChangeDto,
        crate::dto_accounts::ConnectedAccountDto,
        crate::dto_accounts::ConfirmResponse,
        crate::dto_accounts::UpdateAccountRequest,
        crate::dto_workspace::BrandProfileDto,
        crate::dto_workspace::PutBrandProfileRequest,
        crate::dto_workspace::EmojiUseDto,
        crate::dto_workspace::HashtagUseDto,
        crate::dto_workspace::WritingStyleDto,
        crate::dto_workspace::PutWritingStyleRequest,
        crate::dto_posts::PostStatusDto,
        crate::dto_posts::PostSummaryDto,
        crate::dto_posts::PostMediaDto,
        crate::dto_posts::PostOverrideDto,
        crate::dto_posts::LastPublishedDto,
        crate::dto_posts::PostDto,
        crate::dto_posts::CreatePostRequest,
        crate::dto_posts::PatchPostRequest,
        crate::dto_posts::PutOverrideRequest,
        crate::dto_posts::ValidateDraftRequest,
        crate::dto_posts::ValidatePostRequest,
        crate::dto_posts::TargetReportDto,
        crate::dto_posts::ValidationDto,
        crate::dto_publishing::CalendarKindDto,
        crate::dto_publishing::CalendarItemDto,
        crate::dto_publishing::CalendarDto,
        crate::dto_publishing::OperationDto,
        crate::dto_publishing::DeliveryStateDto,
        crate::dto_publishing::LastErrorDto,
        crate::dto_publishing::DirectCountsDto,
        crate::dto_publishing::DeliveryDto,
        crate::dto_publishing::PublishJobSummaryDto,
        crate::dto_publishing::PublishJobDto,
        crate::dto_publishing::PublishRequest,
        crate::dto_publishing::RescheduleRequest,
        crate::dto_publishing::RescheduleOutcomeDto,
        crate::dto_publishing::CancelReportDto,
        crate::dto_publishing::RetryRequest,
        crate::dto_publishing::ResolveRequest,
        postit_core::JobStatus,
        postit_core::ParkReason,
        postit_core::ResolvedOutcome,
        postit_core::ScheduleInput,
        crate::dto_organize::TargetGroupDto,
        crate::dto_organize::TargetGroupRequest,
        crate::dto_organize::CampaignDto,
        crate::dto_organize::CreateCampaignRequest,
        crate::dto_organize::PatchCampaignRequest,
        crate::dto_accounts::DisconnectResponse,
        crate::dto_accounts::TestEmailRequest,
        postit_core::EmailDocument,
        postit_core::EmailBlock,
        postit_core::HeadingLevel,
        postit_core::EmailLanguage,
        crate::dto_accounts::DnsCheckResponse,
        postit_core::DomainFinding,
        postit_core::DomainRecord,
        postit_core::DomainStatus,
        crate::dto_accounts::ImportStatusDto,
        crate::dto_accounts::ImportRowDto,
        crate::dto_accounts::ImportReportDto,
        crate::dto_contacts::ContactStatusDto,
        crate::dto_contacts::ContactDto,
        crate::dto_contacts::CreateContactRequest,
        crate::dto_contacts::UpdateContactRequest,
        crate::dto_contacts::ContactListDto,
        crate::dto_contacts::ContactListRequest,
        crate::dto_contacts::SuppressionReasonDto,
        crate::dto_contacts::SuppressionDto,
        crate::dto_contacts::ContactImportStatusDto,
        crate::dto_contacts::ContactImportRejectionDto,
        crate::dto_contacts::ContactImportReportDto,
        crate::dto_contacts::ContactImportDto,
        crate::dto_contacts::ContactImportForm,
        postit_contacts::ImportOptions,
        postit_contacts::ColumnMap,
        crate::dto_media::MediaKindDto,
        crate::dto_media::MediaStatusDto,
        crate::dto_media::MediaDto,
        crate::dto_media::UploadSessionRequest,
        crate::dto_media::UploadSessionDto,
        crate::dto_media::UploadOffsetDto,
        crate::dto_media::MediaUsageDto,
        crate::dto_media::ViewUrlsDto,
        crate::dto_delegations::LevelDto,
        crate::dto_delegations::EndedReasonDto,
        crate::dto_delegations::DelegationUserDto,
        crate::dto_delegations::DelegationDto,
        crate::dto_delegations::GrantDelegationRequest,
        crate::dto_delegations::ChangeDelegationRequest,
        crate::dto_delegations::WorkspaceDto,
        crate::dto_delegations::LookupUserDto,
        crate::dto_delegations::LookupResponse,
        crate::dto_notifications::NotificationDto,
        crate::dto_notifications::MarkReadRequest,
        crate::dto_notifications::MarkReadResponse,
        crate::dto_notifications::UnreadCountDto,
        crate::dto_push::VapidKeyDto,
        crate::dto_push::PushPlatformDto,
        crate::dto_push::RegisterPushEndpointRequest,
        crate::dto_push::PushEndpointCreated,
        crate::error::ProblemDetails,
        crate::error::ErrorCode,
    )),
    modifiers(&SecurityAddon),
)]
struct ApiDoc;

/// The `/ai` routes, merged into [`ApiDoc`] when the `ai` feature compiles them in.
#[cfg(feature = "ai")]
#[derive(OpenApi)]
#[openapi(
    paths(
        crate::routes::ai::presets,
        crate::routes::ai::list_providers,
        crate::routes::ai::create_provider,
        crate::routes::ai::get_provider,
        crate::routes::ai::update_provider,
        crate::routes::ai::delete_provider,
        crate::routes::ai::test_provider,
        crate::routes::ai::models,
        crate::routes::ai::generate,
        crate::routes::ai::generations,
    ),
    components(schemas(
        crate::dto_ai::AiProtocolDto,
        crate::dto_ai::AiFailureReasonDto,
        crate::dto_ai::AiPresetDto,
        crate::dto_ai::AiModelSourceDto,
        crate::dto_ai::AiModelDto,
        crate::dto_ai::AiOwnerProviderDto,
        crate::dto_ai::AiDelegateProviderDto,
        crate::dto_ai::AiProviderDto,
        crate::dto_ai::AiModelLimitDto,
        crate::dto_ai::AiModalitiesDto,
        crate::dto_ai::AiModelConfigDto,
        crate::dto_ai::CreateAiProviderDto,
        crate::dto_ai::UpdateAiProviderDto,
        crate::dto_ai::AiModelListDto,
        crate::dto_ai::AiTestResultDto,
        crate::dto_ai::AiModeDto,
        crate::dto_ai::GenerateRequestDto,
        crate::dto_ai::AiUsageDto,
        crate::dto_ai::AiDraftDto,
        crate::dto_ai::GenerationDto,
        crate::dto_ai::AiGenerationOutcomeDto,
        crate::dto_ai::AiGenerationDto,
        postit_core::Post,
        postit_core::PostKind,
        postit_core::PostOverride,
        postit_core::MediaItem,
        postit_core::ReplyTo,
        postit_core::Violation,
        postit_core::Message,
    ))
)]
struct AiApiDoc;

/// The `OpenAPI` document for the API.
#[must_use]
pub fn openapi() -> utoipa::openapi::OpenApi {
    let mut spec = ApiDoc::openapi();
    #[cfg(feature = "ai")]
    spec.merge(AiApiDoc::openapi());
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
