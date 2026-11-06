//! The AI drafter (plan 03 D2): the preset catalog, the owner's providers, the connection test,
//! model lists, generation, and the owner's generation history. Feature `ai`.
//!
//! Provider changes, the test, and the history are owner-only (`owner_only` under
//! `X-Postit-Act-As`); listing providers and models and generating need `edit`; the catalog
//! needs a signed-in user. The service checks each one. Generate, test, and models call the
//! provider, so they live in the custom-limits group ([`super::v1_custom_limits_router`]) with
//! a timeout of `ai.request_timeout` plus 5 seconds, and each charges the per-user
//! `rate_limit.ai_requests` bucket once, before the service is called.

use axum::Json;
use axum::extract::State;
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use postit_core::AiProviderId;
use postit_data::{Capability, OwnerScope};
use postit_drafter::CreateProvider;
use uuid::Uuid;

use crate::dto_ai::{
    AiGenerationDto, AiModelListDto, AiPresetDto, AiProviderDto, AiTestResultDto,
    CreateAiProviderDto, GenerateRequestDto, GenerationDto, UpdateAiProviderDto,
};
use crate::error::{ApiError, ErrorCode, ProblemDetails};
use crate::extract::Scope;
use crate::json::{ApiJson, ApiPath, ApiQuery};
use crate::pagination::{Page, PageQuery};
use crate::state::AppState;

/// One charge per AI call against `rate_limit.ai_requests`.
fn charge(state: &AppState, scope: &OwnerScope) -> Result<(), ApiError> {
    state
        .limits
        .ai
        .check(&scope.actor)
        .map_err(|wait| ApiError::new(ErrorCode::RateLimited).with_retry_after(wait))
}

fn invalid(field: &'static str, detail: &'static str) -> ApiError {
    ApiError::new(ErrorCode::ValidationFailed)
        .with_extension("field", field)
        .with_detail(detail)
}

/// The preset catalog: seven cloud services, four local servers. Local presets are not
/// `available` while this server refuses private endpoints (`ai.allow_private_endpoints`).
/// Any signed-in user, in any workspace they act in.
///
/// # Errors
///
/// Only the shared authentication and rate-limit refusals.
#[utoipa::path(get, path = "/api/v1/ai/presets", operation_id = "list_ai_presets", tag = "ai", security(("oidc" = [])),
    responses((status = 200, body = Vec<AiPresetDto>), (status = 401, description = "No valid bearer token (`unauthenticated`)", body = ProblemDetails, content_type = "application/problem+json"), (status = 403, description = "`account_disabled`, `account_pending`, or `delegation_invalid` (`X-Postit-Act-As` names no workspace the caller holds a live grant to)", body = ProblemDetails, content_type = "application/problem+json"), (status = 429, description = "`rate_limited`; see `Retry-After`", body = ProblemDetails, content_type = "application/problem+json"), (status = 500, description = "`internal`", body = ProblemDetails, content_type = "application/problem+json")))]
pub async fn presets(State(state): State<AppState>, Scope(_): Scope) -> Json<Vec<AiPresetDto>> {
    Json(
        state
            .drafter
            .presets()
            .into_iter()
            .map(AiPresetDto::from)
            .collect(),
    )
}

/// The owner's providers, oldest first. The owner sees each configuration without its
/// secrets (`view: owner`); a delegate sees names and models only (`view: delegate`). Needs
/// `edit`.
///
/// # Errors
///
/// 403 `capability_required` below `edit`.
#[utoipa::path(get, path = "/api/v1/ai/providers", operation_id = "list_ai_providers", tag = "ai", security(("oidc" = [])),
    responses((status = 200, body = Vec<AiProviderDto>), (status = 401, description = "No valid bearer token (`unauthenticated`)", body = ProblemDetails, content_type = "application/problem+json"), (status = 403, description = "`account_disabled`, `account_pending`, `delegation_invalid` (`X-Postit-Act-As` names no workspace the caller holds a live grant to) or `capability_required` (the grant's level does not allow it)", body = ProblemDetails, content_type = "application/problem+json"), (status = 429, description = "`rate_limited`; see `Retry-After`", body = ProblemDetails, content_type = "application/problem+json"), (status = 500, description = "`internal`", body = ProblemDetails, content_type = "application/problem+json")))]
pub async fn list_providers(
    State(state): State<AppState>,
    Scope(scope): Scope,
) -> Result<Json<Vec<AiProviderDto>>, ApiError> {
    let providers = state.drafter.list_providers(&scope).await?;
    Ok(Json(
        providers.into_iter().map(AiProviderDto::from).collect(),
    ))
}

/// Adds a provider, from a preset (`preset` with `api_key` unless the preset is local) or from
/// a pasted opencode entry (`opencode`). The key and header values are sealed and never
/// returned. The owner's first provider becomes the default. Recorded as
/// `ai_provider_created`. Owner-only.
///
/// # Errors
///
/// 422 `validation_failed`: `field` is `preset` when neither or both of `preset` and
/// `opencode` are sent; otherwise `violations` lists every rule broken (an unknown preset, a
/// missing key, a local preset while private endpoints are off, an `{env:...}` value, a bad base
/// URL).
#[utoipa::path(post, path = "/api/v1/ai/providers", operation_id = "create_ai_provider", tag = "ai", security(("oidc" = [])),
    request_body = CreateAiProviderDto,
    responses((status = 201, body = AiProviderDto, headers(("Location" = String, description = "`/api/v1/ai/providers/{id}`"))), (status = 401, description = "No valid bearer token (`unauthenticated`)", body = ProblemDetails, content_type = "application/problem+json"), (status = 403, description = "`account_disabled`, `account_pending`, `delegation_invalid` (`X-Postit-Act-As` names no workspace the caller holds a live grant to) or `owner_only` (sent through `X-Postit-Act-As`: only the owner manages AI providers)", body = ProblemDetails, content_type = "application/problem+json"), (status = 422, description = "`validation_failed`; see `field` or `violations`", body = ProblemDetails, content_type = "application/problem+json"), (status = 429, description = "`rate_limited`; see `Retry-After`", body = ProblemDetails, content_type = "application/problem+json"), (status = 500, description = "`internal`", body = ProblemDetails, content_type = "application/problem+json")))]
pub async fn create_provider(
    State(state): State<AppState>,
    Scope(scope): Scope,
    ApiJson(body): ApiJson<CreateAiProviderDto>,
) -> Result<Response, ApiError> {
    // Owner-only comes first, so a delegate never learns which body checks would fail.
    scope.require(Capability::OwnerOnly)?;
    let input = match (body.preset, body.opencode) {
        (Some(key), None) => CreateProvider::Preset {
            key,
            api_key: body.api_key,
            default_model: body.default_model,
        },
        (None, Some(entry)) if body.api_key.is_none() && body.default_model.is_none() => {
            CreateProvider::Pasted { entry }
        }
        (None, Some(_)) => {
            return Err(invalid(
                "opencode",
                "a pasted entry carries its own key and default model",
            ));
        }
        _ => return Err(invalid("preset", "send exactly one of preset and opencode")),
    };
    let provider = AiProviderDto::from(state.drafter.create_provider(&scope, input).await?);
    let location = HeaderValue::from_str(&format!("/api/v1/ai/providers/{}", provider.id()))
        .map_err(|e| ApiError::internal(&e))?;
    let mut response = (StatusCode::CREATED, Json(provider)).into_response();
    response.headers_mut().insert(header::LOCATION, location);
    Ok(response)
}

/// Changes a provider. A new `api_key` or `headers` is recorded as `ai_provider_key_changed`;
/// any other change, the default included, as `ai_provider_updated`. Owner-only.
///
/// # Errors
///
/// 404 for an unknown provider; 422 `validation_failed` with `violations`.
#[utoipa::path(patch, path = "/api/v1/ai/providers/{id}", operation_id = "update_ai_provider", tag = "ai", security(("oidc" = [])),
    params(("id" = Uuid, Path, description = "Provider ID")),
    request_body = UpdateAiProviderDto,
    responses((status = 200, body = AiProviderDto), (status = 401, description = "No valid bearer token (`unauthenticated`)", body = ProblemDetails, content_type = "application/problem+json"), (status = 403, description = "`account_disabled`, `account_pending`, `delegation_invalid` (`X-Postit-Act-As` names no workspace the caller holds a live grant to) or `owner_only` (sent through `X-Postit-Act-As`: only the owner manages AI providers)", body = ProblemDetails, content_type = "application/problem+json"), (status = 404, description = "`not_found`", body = ProblemDetails, content_type = "application/problem+json"), (status = 422, description = "`validation_failed`; see `violations`", body = ProblemDetails, content_type = "application/problem+json"), (status = 429, description = "`rate_limited`; see `Retry-After`", body = ProblemDetails, content_type = "application/problem+json"), (status = 500, description = "`internal`", body = ProblemDetails, content_type = "application/problem+json")))]
pub async fn update_provider(
    State(state): State<AppState>,
    Scope(scope): Scope,
    ApiPath(id): ApiPath<Uuid>,
    ApiJson(body): ApiJson<UpdateAiProviderDto>,
) -> Result<Json<AiProviderDto>, ApiError> {
    let view = state
        .drafter
        .update_provider(&scope, AiProviderId::from(id), body.into())
        .await?;
    Ok(Json(view.into()))
}

/// Deletes a provider. Its generations stay in the history, with no provider. Deleting the
/// default leaves the owner with none. Recorded as `ai_provider_deleted`. Owner-only.
///
/// # Errors
///
/// 404 for an unknown provider.
#[utoipa::path(delete, path = "/api/v1/ai/providers/{id}", operation_id = "delete_ai_provider", tag = "ai", security(("oidc" = [])),
    params(("id" = Uuid, Path, description = "Provider ID")),
    responses((status = 204, description = "Deleted"), (status = 401, description = "No valid bearer token (`unauthenticated`)", body = ProblemDetails, content_type = "application/problem+json"), (status = 403, description = "`account_disabled`, `account_pending`, `delegation_invalid` (`X-Postit-Act-As` names no workspace the caller holds a live grant to) or `owner_only` (sent through `X-Postit-Act-As`: only the owner manages AI providers)", body = ProblemDetails, content_type = "application/problem+json"), (status = 404, description = "`not_found`", body = ProblemDetails, content_type = "application/problem+json"), (status = 422, description = "`validation_failed`", body = ProblemDetails, content_type = "application/problem+json"), (status = 429, description = "`rate_limited`; see `Retry-After`", body = ProblemDetails, content_type = "application/problem+json"), (status = 500, description = "`internal`", body = ProblemDetails, content_type = "application/problem+json")))]
pub async fn delete_provider(
    State(state): State<AppState>,
    Scope(scope): Scope,
    ApiPath(id): ApiPath<Uuid>,
) -> Result<StatusCode, ApiError> {
    state
        .drafter
        .delete_provider(&scope, AiProviderId::from(id))
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Tests the connection: one tiny chat (`Reply with OK`, at most 5 output tokens) to the
/// default model, plus the live model list when the provider has one. A provider failure is
/// `ok: false` with its `reason`, not an error. Writes no generation. Owner-only; charges the
/// per-user AI bucket.
///
/// # Errors
///
/// 404 for an unknown provider; 422 `validation_failed` (`violations` at `default_model`) when
/// there is no model to chat with and no model list.
#[utoipa::path(post, path = "/api/v1/ai/providers/{id}/test", operation_id = "test_ai_provider", tag = "ai", security(("oidc" = [])),
    params(("id" = Uuid, Path, description = "Provider ID")),
    responses((status = 200, body = AiTestResultDto), (status = 401, description = "No valid bearer token (`unauthenticated`)", body = ProblemDetails, content_type = "application/problem+json"), (status = 403, description = "`account_disabled`, `account_pending`, `delegation_invalid` (`X-Postit-Act-As` names no workspace the caller holds a live grant to) or `owner_only` (sent through `X-Postit-Act-As`: only the owner tests AI providers)", body = ProblemDetails, content_type = "application/problem+json"), (status = 404, description = "`not_found`", body = ProblemDetails, content_type = "application/problem+json"), (status = 408, description = "`request_timeout`", body = ProblemDetails, content_type = "application/problem+json"), (status = 422, description = "`validation_failed`; see `violations`", body = ProblemDetails, content_type = "application/problem+json"), (status = 429, description = "`rate_limited`; see `Retry-After`", body = ProblemDetails, content_type = "application/problem+json"), (status = 500, description = "`internal`", body = ProblemDetails, content_type = "application/problem+json")))]
pub async fn test_provider(
    State(state): State<AppState>,
    Scope(scope): Scope,
    ApiPath(id): ApiPath<Uuid>,
) -> Result<Json<AiTestResultDto>, ApiError> {
    charge(&state, &scope)?;
    let result = state
        .drafter
        .test_provider(&scope, AiProviderId::from(id))
        .await?;
    Ok(Json(result.into()))
}

/// A provider's models: the configured ones merged with its live list, when it has one. A
/// failed live list still returns the configured models, with `remote_error`. Needs `edit`;
/// charges the per-user AI bucket.
///
/// # Errors
///
/// 404 for an unknown provider.
#[utoipa::path(get, path = "/api/v1/ai/providers/{id}/models", operation_id = "list_ai_models", tag = "ai", security(("oidc" = [])),
    params(("id" = Uuid, Path, description = "Provider ID")),
    responses((status = 200, body = AiModelListDto), (status = 401, description = "No valid bearer token (`unauthenticated`)", body = ProblemDetails, content_type = "application/problem+json"), (status = 403, description = "`account_disabled`, `account_pending`, `delegation_invalid` (`X-Postit-Act-As` names no workspace the caller holds a live grant to) or `capability_required` (the grant's level does not allow it)", body = ProblemDetails, content_type = "application/problem+json"), (status = 404, description = "`not_found`", body = ProblemDetails, content_type = "application/problem+json"), (status = 408, description = "`request_timeout`", body = ProblemDetails, content_type = "application/problem+json"), (status = 422, description = "`validation_failed`", body = ProblemDetails, content_type = "application/problem+json"), (status = 429, description = "`rate_limited`; see `Retry-After`", body = ProblemDetails, content_type = "application/problem+json"), (status = 500, description = "`internal`", body = ProblemDetails, content_type = "application/problem+json")))]
pub async fn models(
    State(state): State<AppState>,
    Scope(scope): Scope,
    ApiPath(id): ApiPath<Uuid>,
) -> Result<Json<AiModelListDto>, ApiError> {
    charge(&state, &scope)?;
    let list = state.drafter.models(&scope, AiProviderId::from(id)).await?;
    Ok(Json(list.into()))
}

/// Drafts text for each target in its limits, validated against its platform, with one
/// repair when the model's answer is not usable. Nothing is published and no post is created:
/// the draft has the shape of a post plus per-account overrides, for the user to review and
/// save. Targets that still break a rule come back with their `violations`. Needs `edit`, with
/// every target inside the caller's account limit; charges the per-user AI bucket.
///
/// # Errors
///
/// 404 for a target, media, or comment outside the caller's scope; 409 `ai_provider_required`
/// when no provider is given and the owner has no default, or the given one is gone; 422
/// `validation_failed` with `violations` (prompt over 8,000 characters, not 1 to 20 targets,
/// over 10 media, media not ready, a comment that cannot be answered); 502 `ai_provider_failed`
/// with the reason in `detail` (`Retry-After` when the provider is rate limiting the key).
#[utoipa::path(post, path = "/api/v1/ai/generate", operation_id = "generate_ai_draft", tag = "ai", security(("oidc" = [])),
    request_body = GenerateRequestDto,
    responses((status = 200, body = GenerationDto), (status = 401, description = "No valid bearer token (`unauthenticated`)", body = ProblemDetails, content_type = "application/problem+json"), (status = 403, description = "`account_disabled`, `account_pending`, `delegation_invalid` (`X-Postit-Act-As` names no workspace the caller holds a live grant to) or `capability_required` (the grant's level does not allow it)", body = ProblemDetails, content_type = "application/problem+json"), (status = 404, description = "`not_found`", body = ProblemDetails, content_type = "application/problem+json"), (status = 408, description = "`request_timeout`", body = ProblemDetails, content_type = "application/problem+json"), (status = 409, description = "`ai_provider_required`", body = ProblemDetails, content_type = "application/problem+json"), (status = 422, description = "`validation_failed`; see `violations`", body = ProblemDetails, content_type = "application/problem+json"), (status = 429, description = "`rate_limited`; see `Retry-After`", body = ProblemDetails, content_type = "application/problem+json"), (status = 500, description = "`internal`", body = ProblemDetails, content_type = "application/problem+json"), (status = 502, description = "`ai_provider_failed`; `detail` is `key_rejected`, `model_not_found`, `rate_limited` (see `Retry-After`), `bad_request`, `unavailable`, `timeout`, `address_refused`, `invalid_response`, or `invalid_output`", body = ProblemDetails, content_type = "application/problem+json")))]
pub async fn generate(
    State(state): State<AppState>,
    Scope(scope): Scope,
    ApiJson(body): ApiJson<GenerateRequestDto>,
) -> Result<Json<GenerationDto>, ApiError> {
    charge(&state, &scope)?;
    let generation = state.drafter.generate(&scope, body.into()).await?;
    Ok(Json(generation.into()))
}

/// The owner's generations, newest first, for the spend view: who asked, the mode, the
/// provider and model, the outcome, and the tokens. The prompt only when the owner keeps
/// prompts. Owner-only.
///
/// # Errors
///
/// 422 for invalid paging.
#[utoipa::path(get, path = "/api/v1/ai/generations", operation_id = "list_ai_generations", tag = "ai", security(("oidc" = [])),
    params(PageQuery),
    responses((status = 200, body = Page<AiGenerationDto>), (status = 401, description = "No valid bearer token (`unauthenticated`)", body = ProblemDetails, content_type = "application/problem+json"), (status = 403, description = "`account_disabled`, `account_pending`, `delegation_invalid` (`X-Postit-Act-As` names no workspace the caller holds a live grant to) or `owner_only` (sent through `X-Postit-Act-As`: only the owner sees the history)", body = ProblemDetails, content_type = "application/problem+json"), (status = 422, description = "`validation_failed`", body = ProblemDetails, content_type = "application/problem+json"), (status = 429, description = "`rate_limited`; see `Retry-After`", body = ProblemDetails, content_type = "application/problem+json"), (status = 500, description = "`internal`", body = ProblemDetails, content_type = "application/problem+json")))]
pub async fn generations(
    State(state): State<AppState>,
    Scope(scope): Scope,
    ApiQuery(q): ApiQuery<PageQuery>,
) -> Result<Json<Page<AiGenerationDto>>, ApiError> {
    let pagination = q.to_pagination()?;
    let set = state.drafter.generations(&scope, pagination).await?;
    Ok(Json(Page::new(
        set.data.into_iter().map(AiGenerationDto::from).collect(),
        set.total,
        &pagination,
    )))
}

/// The AI routes that run under the default body limit and timeout.
pub(super) fn router() -> axum::Router<AppState> {
    use axum::routing::{get, patch};

    axum::Router::new()
        .route("/ai/presets", get(presets))
        .route("/ai/providers", get(list_providers).post(create_provider))
        .route(
            "/ai/providers/{id}",
            patch(update_provider).delete(delete_provider),
        )
        .route("/ai/generations", get(generations))
}

/// The AI routes that call the provider: a timeout of `ai.request_timeout` plus 5 seconds, so
/// the drafter's own deadline answers first with a clear reason.
pub(super) fn calls_router(settings: &crate::settings::ApiSettings) -> axum::Router<AppState> {
    use axum::routing::{get, post};

    crate::router::with_limits(
        axum::Router::new()
            .route("/ai/providers/{id}/test", post(test_provider))
            .route("/ai/providers/{id}/models", get(models))
            .route("/ai/generate", post(generate)),
        settings.body_limit,
        settings
            .ai_request_timeout
            .saturating_add(std::time::Duration::from_secs(5)),
    )
}
