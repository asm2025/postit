//! Wire types of the `/ai` routes. API keys and header values are write-only: the create and
//! update bodies accept them, and no response carries them. Neither the user's prompt nor a
//! drafted text appears in a `Debug` output: the request bodies have hand-written `Debug`
//! impls, and the response types have none.

use std::collections::BTreeMap;
use std::fmt;

use chrono::{DateTime, Utc};
use postit_core::{Message, Post, PostOverride, Violation};
use postit_data::ai_generations::{AiGeneration, GenerationOutcome};
use postit_drafter::config::{Modalities, ModelLimit};
use postit_drafter::{
    FailureReason, GenerateRequest, Generation, Mode, ModelConfig, ModelList, ModelSource,
    ModelView, PresetView, Protocol, ProviderView, TestResult, Usage,
};
use secrecy::SecretString;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use url::Url;
use utoipa::ToSchema;
use uuid::Uuid;

/// The wire protocol a provider speaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
pub enum AiProtocolDto {
    /// `POST {base}/chat/completions`.
    #[serde(rename = "openai-compatible")]
    OpenAiCompatible,
    /// Anthropic's native `POST {base}/v1/messages`.
    #[serde(rename = "anthropic")]
    Anthropic,
}

impl From<Protocol> for AiProtocolDto {
    fn from(protocol: Protocol) -> Self {
        match protocol {
            Protocol::OpenAiCompatible => Self::OpenAiCompatible,
            Protocol::Anthropic => Self::Anthropic,
        }
    }
}

/// Why a provider call failed. The provider's own message is never passed on, since it may
/// echo the request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum AiFailureReasonDto {
    /// The provider refused the key.
    KeyRejected,
    /// The model does not exist for this provider.
    ModelNotFound,
    /// The provider is rate limiting the key.
    RateLimited,
    /// The provider rejected the request.
    BadRequest,
    /// The provider is down or unreachable.
    Unavailable,
    /// The provider took too long.
    Timeout,
    /// The provider's address is not allowed from this server.
    AddressRefused,
    /// The provider's answer was not in its protocol's shape.
    InvalidResponse,
    /// The model's answer was not usable, even after one repair.
    InvalidOutput,
}

impl From<FailureReason> for AiFailureReasonDto {
    fn from(reason: FailureReason) -> Self {
        match reason {
            FailureReason::KeyRejected => Self::KeyRejected,
            FailureReason::ModelNotFound => Self::ModelNotFound,
            FailureReason::RateLimited => Self::RateLimited,
            FailureReason::BadRequest => Self::BadRequest,
            FailureReason::Unavailable => Self::Unavailable,
            FailureReason::Timeout => Self::Timeout,
            FailureReason::AddressRefused => Self::AddressRefused,
            FailureReason::InvalidResponse => Self::InvalidResponse,
            FailureReason::InvalidOutput => Self::InvalidOutput,
        }
    }
}

/// One entry of the preset catalog.
#[derive(Debug, Serialize, ToSchema)]
pub struct AiPresetDto {
    /// The preset key, such as `gemini`.
    pub key: String,
    /// Display name.
    pub name: String,
    /// Wire protocol.
    pub protocol: AiProtocolDto,
    /// The prefilled base URL.
    pub base_url: String,
    /// Where the user gets a key.
    pub guide_url: String,
    /// A server on the user's own machine.
    pub local: bool,
    /// Whether it can be added on this server: false for local presets while
    /// `ai.allow_private_endpoints` is off.
    pub available: bool,
    /// A consumer chat subscription from the same company cannot be used here; the
    /// guide says so.
    pub subscription_note: bool,
}

impl From<PresetView> for AiPresetDto {
    fn from(p: PresetView) -> Self {
        Self {
            key: p.key.to_owned(),
            name: p.name.to_owned(),
            protocol: p.protocol.into(),
            base_url: p.base_url.to_owned(),
            guide_url: p.guide_url.to_owned(),
            local: p.local,
            available: p.available,
            subscription_note: p.subscription_note,
        }
    }
}

/// Where a listed model comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum AiModelSourceDto {
    /// The provider's configuration only.
    Config,
    /// The provider's live list only.
    Remote,
    /// Both.
    Both,
}

impl From<ModelSource> for AiModelSourceDto {
    fn from(source: ModelSource) -> Self {
        match source {
            ModelSource::Config => Self::Config,
            ModelSource::Remote => Self::Remote,
            ModelSource::Both => Self::Both,
        }
    }
}

/// One model of a provider.
#[derive(Debug, Serialize, ToSchema)]
pub struct AiModelDto {
    /// The model ID.
    pub id: String,
    /// Display name, when known.
    pub name: Option<String>,
    /// Whether the model is shown images and video frames.
    pub images: bool,
    /// Where it comes from.
    pub source: AiModelSourceDto,
}

impl From<ModelView> for AiModelDto {
    fn from(m: ModelView) -> Self {
        Self {
            id: m.id,
            name: m.name,
            images: m.images,
            source: m.source.into(),
        }
    }
}

/// `view` of the owner's provider view.
#[derive(Debug, Clone, Copy, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum AiOwnerViewTag {
    /// The owner's view.
    Owner,
}

/// `view` of a delegate's provider view.
#[derive(Debug, Clone, Copy, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum AiDelegateViewTag {
    /// A delegate's view.
    Delegate,
}

/// A provider as its owner sees it: the configuration without any secret.
#[derive(Debug, Serialize, ToSchema)]
pub struct AiOwnerProviderDto {
    /// Always `owner`.
    #[schema(inline)]
    pub view: AiOwnerViewTag,
    /// Provider ID.
    pub id: Uuid,
    /// The preset key, or `custom` for a pasted entry.
    pub preset: String,
    /// Display name.
    pub name: String,
    /// Wire protocol.
    pub protocol: AiProtocolDto,
    /// Where requests go, when the provider sets its own address.
    pub base_url: Option<Url>,
    /// Names of the headers sent with every request; their values are never shown.
    pub header_names: Vec<String>,
    /// Whether a key or header values are stored.
    pub has_key: bool,
    /// The configured models.
    pub models: Vec<AiModelDto>,
    /// The model used when a request names none.
    pub default_model: Option<String>,
    /// Whether it is the owner's default provider.
    pub is_default: bool,
}

/// A provider as a delegate sees it: names and models only.
#[derive(Debug, Serialize, ToSchema)]
pub struct AiDelegateProviderDto {
    /// Always `delegate`.
    #[schema(inline)]
    pub view: AiDelegateViewTag,
    /// Provider ID.
    pub id: Uuid,
    /// The preset key, or `custom` for a pasted entry.
    pub preset: String,
    /// Display name.
    pub name: String,
    /// The configured models.
    pub models: Vec<AiModelDto>,
    /// The model used when a request names none.
    pub default_model: Option<String>,
    /// Whether it is the owner's default provider.
    pub is_default: bool,
}

/// A provider in the view the caller may see: the owner's, or a delegate's. `view` tells them
/// apart.
#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum AiProviderDto {
    /// The owner's view.
    Owner(AiOwnerProviderDto),
    /// A delegate's view.
    Delegate(AiDelegateProviderDto),
}

impl From<ProviderView> for AiProviderDto {
    fn from(view: ProviderView) -> Self {
        let models = |m: Vec<ModelView>| m.into_iter().map(AiModelDto::from).collect();
        match view {
            ProviderView::Owner {
                id,
                preset,
                name,
                protocol,
                base_url,
                header_names,
                has_key,
                models: list,
                default_model,
                is_default,
            } => Self::Owner(AiOwnerProviderDto {
                view: AiOwnerViewTag::Owner,
                id: id.into(),
                preset,
                name,
                protocol: protocol.into(),
                base_url,
                header_names,
                has_key,
                models: models(list),
                default_model,
                is_default,
            }),
            ProviderView::Delegate {
                id,
                preset,
                name,
                models: list,
                default_model,
                is_default,
            } => Self::Delegate(AiDelegateProviderDto {
                view: AiDelegateViewTag::Delegate,
                id: id.into(),
                preset,
                name,
                models: models(list),
                default_model,
                is_default,
            }),
        }
    }
}

impl AiProviderDto {
    /// The provider's ID.
    #[must_use]
    pub fn id(&self) -> Uuid {
        match self {
            Self::Owner(p) => p.id,
            Self::Delegate(p) => p.id,
        }
    }
}

/// The schema: a `oneOf` over the two views with `view` as its discriminator.
mod provider_schema {
    use utoipa::openapi::RefOr;
    use utoipa::openapi::schema::{Discriminator, OneOfBuilder, Ref, Schema};
    use utoipa::{PartialSchema, ToSchema};

    use super::{AiDelegateProviderDto, AiOwnerProviderDto, AiProviderDto};

    impl PartialSchema for AiProviderDto {
        fn schema() -> RefOr<Schema> {
            let mut discriminator = Discriminator::new("view");
            let mut one_of = OneOfBuilder::new().description(Some(
                "A provider in the view the caller may see: `owner` or `delegate`.",
            ));
            for (tag, name) in [
                ("owner", AiOwnerProviderDto::name()),
                ("delegate", AiDelegateProviderDto::name()),
            ] {
                discriminator
                    .mapping
                    .insert(tag.to_owned(), format!("#/components/schemas/{name}"));
                one_of = one_of.item(Ref::from_schema_name(name));
            }
            Schema::OneOf(one_of.discriminator(Some(discriminator)).build()).into()
        }
    }

    impl ToSchema for AiProviderDto {
        fn schemas(schemas: &mut Vec<(String, RefOr<Schema>)>) {
            schemas.push((
                AiOwnerProviderDto::name().into(),
                AiOwnerProviderDto::schema(),
            ));
            <AiOwnerProviderDto as ToSchema>::schemas(schemas);
            schemas.push((
                AiDelegateProviderDto::name().into(),
                AiDelegateProviderDto::schema(),
            ));
            <AiDelegateProviderDto as ToSchema>::schemas(schemas);
        }
    }
}

/// A model's token limits.
#[derive(Debug, Clone, Default, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct AiModelLimitDto {
    /// Context window, in tokens.
    pub context: Option<u32>,
    /// Output limit, in tokens; lowers the drafter's 8,192.
    pub output: Option<u32>,
}

/// What a model accepts.
#[derive(Debug, Clone, Default, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct AiModalitiesDto {
    /// Input kinds, such as `text` and `image`.
    pub input: Option<Vec<String>>,
}

/// One configured model.
#[derive(Debug, Clone, Default, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct AiModelConfigDto {
    /// Display name.
    pub name: Option<String>,
    /// Token limits.
    pub limit: Option<AiModelLimitDto>,
    /// What the model accepts; `image` in `input` sends it images.
    pub modalities: Option<AiModalitiesDto>,
}

impl From<AiModelConfigDto> for ModelConfig {
    fn from(m: AiModelConfigDto) -> Self {
        Self {
            name: m.name,
            limit: m.limit.map(|l| ModelLimit {
                context: l.context,
                output: l.output,
            }),
            modalities: m.modalities.map(|x| Modalities { input: x.input }),
        }
    }
}

/// Adds a provider: from a preset (`preset`, plus `api_key` unless the preset is local, and an
/// optional `default_model`), or from a pasted opencode configuration (`opencode`). Exactly one
/// of `preset` and `opencode`.
#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateAiProviderDto {
    /// The preset key, from `GET /api/v1/ai/presets`.
    pub preset: Option<String>,
    /// The API key. Write-only: never returned.
    #[schema(value_type = Option<String>, write_only)]
    pub api_key: Option<SecretString>,
    /// The model used when a request names none.
    pub default_model: Option<String>,
    /// A pasted opencode `provider` entry, optionally wrapped as `{ "provider": { "<id>":
    /// {...} } }`. Its `options.apiKey` and `options.headers` values are stored sealed and never
    /// returned; `{env:...}` and `{file:...}` values are refused.
    #[schema(value_type = Option<Object>, write_only)]
    pub opencode: Option<Value>,
}

impl fmt::Debug for CreateAiProviderDto {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CreateAiProviderDto")
            .field("preset", &self.preset)
            .field("has_api_key", &self.api_key.is_some())
            .field("default_model", &self.default_model)
            .field("has_opencode", &self.opencode.is_some())
            .finish()
    }
}

/// Changes a provider. Absent members keep their value.
#[derive(Default, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateAiProviderDto {
    /// New display name.
    pub name: Option<String>,
    /// New base URL.
    pub base_url: Option<Url>,
    /// New default model; added to the models when missing.
    pub default_model: Option<String>,
    /// Replaces every configured model, by model ID.
    pub models: Option<BTreeMap<String, AiModelConfigDto>>,
    /// Replaces the API key. Write-only: never returned.
    #[schema(value_type = Option<String>, write_only)]
    pub api_key: Option<SecretString>,
    /// Replaces every header and its value, by header name. Write-only: only the names are
    /// returned.
    #[schema(value_type = Option<BTreeMap<String, String>>, write_only)]
    pub headers: Option<BTreeMap<String, SecretString>>,
    /// Makes it the owner's default provider. `false` changes nothing.
    pub make_default: Option<bool>,
}

impl fmt::Debug for UpdateAiProviderDto {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("UpdateAiProviderDto")
            .field("name", &self.name)
            .field("base_url", &self.base_url)
            .field("default_model", &self.default_model)
            .field("models", &self.models.as_ref().map(BTreeMap::len))
            .field("has_api_key", &self.api_key.is_some())
            .field(
                "header_names",
                &self.headers.as_ref().map(|h| h.keys().collect::<Vec<_>>()),
            )
            .field("make_default", &self.make_default)
            .finish()
    }
}

impl From<UpdateAiProviderDto> for postit_drafter::UpdateProvider {
    fn from(body: UpdateAiProviderDto) -> Self {
        Self {
            name: body.name,
            base_url: body.base_url,
            default_model: body.default_model,
            models: body.models.map(|models| {
                models
                    .into_iter()
                    .map(|(id, model)| (id, model.into()))
                    .collect()
            }),
            api_key: body.api_key,
            headers: body.headers,
            make_default: body.make_default.unwrap_or(false),
        }
    }
}

/// A provider's models: the configured ones, then the ones its live list adds.
#[derive(Debug, Serialize, ToSchema)]
pub struct AiModelListDto {
    /// Configured models first, then listed ones the configuration does not name.
    pub models: Vec<AiModelDto>,
    /// Why the live list could not be read; the configured models are still returned.
    pub remote_error: Option<AiFailureReasonDto>,
}

impl From<ModelList> for AiModelListDto {
    fn from(list: ModelList) -> Self {
        Self {
            models: list.models.into_iter().map(AiModelDto::from).collect(),
            remote_error: list.remote_error.map(Into::into),
        }
    }
}

/// What a connection test found.
#[derive(Debug, Serialize, ToSchema)]
pub struct AiTestResultDto {
    /// Whether the provider answered.
    pub ok: bool,
    /// Why not.
    pub reason: Option<AiFailureReasonDto>,
    /// How many models the provider's live list holds, when it was read.
    pub models_found: Option<u64>,
}

impl From<TestResult> for AiTestResultDto {
    fn from(r: TestResult) -> Self {
        Self {
            ok: r.ok,
            reason: r.reason.map(Into::into),
            models_found: r.models_found.map(|n| u64::try_from(n).unwrap_or(u64::MAX)),
        }
    }
}

/// What the drafter writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum AiModeDto {
    /// A new post for each target.
    Post,
    /// An answer to a comment (`comment_id`), for its one account.
    Reply,
    /// Fresh wording of `prompt` for each target.
    Rewrite,
}

impl From<AiModeDto> for Mode {
    fn from(mode: AiModeDto) -> Self {
        match mode {
            AiModeDto::Post => Self::Post,
            AiModeDto::Reply => Self::Reply,
            AiModeDto::Rewrite => Self::Rewrite,
        }
    }
}

/// A generation request. Nothing is published and no post is created.
#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct GenerateRequestDto {
    /// What to write.
    pub mode: AiModeDto,
    /// What to write about (at most 8,000 characters), or the text to rewrite. May be empty in
    /// reply mode.
    #[serde(default)]
    pub prompt: String,
    /// The accounts to write for, 1 to 20; exactly one, the comment's account, in reply mode.
    pub targets: Vec<Uuid>,
    /// The language to write in, as a BCP 47 tag; else the account's writing style, the brand
    /// profile, the comment's language, then the caller's interface language.
    pub language: Option<String>,
    /// Media the post carries, at most 10, in order; the model sees up to 4 images and 4
    /// frames of each video when it reads images.
    #[serde(default)]
    pub media_ids: Vec<Uuid>,
    /// The inbox item to answer, in reply mode.
    pub comment_id: Option<Uuid>,
    /// The provider; the owner's default when absent.
    pub provider_id: Option<Uuid>,
    /// The model; the provider's default when absent.
    pub model: Option<String>,
    /// The link the post carries. The drafter never writes a link of its own.
    pub link: Option<Url>,
}

impl fmt::Debug for GenerateRequestDto {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GenerateRequestDto")
            .field("mode", &self.mode)
            .field("prompt_chars", &self.prompt.chars().count())
            .field("targets", &self.targets)
            .field("has_language", &self.language.is_some())
            .field("media_ids", &self.media_ids.len())
            .field("comment_id", &self.comment_id)
            .field("provider_id", &self.provider_id)
            .field("model", &self.model)
            .field("has_link", &self.link.is_some())
            .finish()
    }
}

impl From<GenerateRequestDto> for GenerateRequest {
    fn from(body: GenerateRequestDto) -> Self {
        Self {
            mode: body.mode.into(),
            prompt: body.prompt,
            targets: body.targets.into_iter().map(Into::into).collect(),
            language: body.language,
            media_ids: body.media_ids.into_iter().map(Into::into).collect(),
            comment_id: body.comment_id.map(Into::into),
            provider_id: body.provider_id.map(Into::into),
            model: body.model,
            link: body.link,
        }
    }
}

/// Tokens a generation spent, summed over the repair call.
#[derive(Debug, Serialize, ToSchema)]
pub struct AiUsageDto {
    /// Tokens sent.
    pub input_tokens: u32,
    /// Tokens received.
    pub output_tokens: u32,
}

impl From<Usage> for AiUsageDto {
    fn from(u: Usage) -> Self {
        Self {
            input_tokens: u.input_tokens,
            output_tokens: u.output_tokens,
        }
    }
}

/// The draft, in the shape of a post plus per-account overrides, ready for the post editor.
#[derive(Serialize, ToSchema)]
pub struct AiDraftDto {
    /// The shared post: the first target's text and tags, the media, the link, and
    /// `disclose_ai_generated: true`.
    pub post: Post,
    /// One override per requested target, by account ID.
    pub overrides: BTreeMap<Uuid, PostOverride>,
}

/// A generation's result. The draft is not saved: the user reviews it and saves it as a post.
#[derive(Serialize, ToSchema)]
pub struct GenerationDto {
    /// The generation record, for `GET /api/v1/ai/generations`.
    pub generation_id: Uuid,
    /// The provider used.
    pub provider_id: Uuid,
    /// The model used.
    pub model: String,
    /// The draft.
    pub draft: AiDraftDto,
    /// What is still wrong, by account ID; accounts with nothing wrong are left out.
    pub violations: BTreeMap<Uuid, Vec<Violation>>,
    /// Notes for the user, such as `ai.media_ignored`.
    pub notes: Vec<Message>,
    /// Tokens spent, when the provider reported them.
    pub usage: Option<AiUsageDto>,
}

impl From<Generation> for GenerationDto {
    fn from(g: Generation) -> Self {
        Self {
            generation_id: g.id.into(),
            provider_id: g.provider_id.into(),
            model: g.model,
            draft: AiDraftDto {
                post: g.draft.post,
                overrides: g
                    .draft
                    .overrides
                    .into_iter()
                    .map(|(account, o)| (account.into(), o))
                    .collect(),
            },
            violations: g
                .violations
                .into_iter()
                .map(|(account, v)| (account.into(), v))
                .collect(),
            notes: g.notes,
            usage: g.usage.map(Into::into),
        }
    }
}

/// How a generation ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum AiGenerationOutcomeDto {
    /// The answer was usable.
    Ok,
    /// The provider answered, but its output was not usable, even after one repair.
    InvalidOutput,
    /// The provider failed after taking the request.
    Failed,
}

impl From<GenerationOutcome> for AiGenerationOutcomeDto {
    fn from(outcome: GenerationOutcome) -> Self {
        match outcome {
            GenerationOutcome::Ok => Self::Ok,
            GenerationOutcome::InvalidOutput => Self::InvalidOutput,
            GenerationOutcome::Failed => Self::Failed,
        }
    }
}

/// One generation in the owner's spend history.
#[derive(Serialize, ToSchema)]
pub struct AiGenerationDto {
    /// Generation ID.
    pub id: Uuid,
    /// The provider, or null once it was deleted.
    pub provider_id: Option<Uuid>,
    /// The provider's name, or null once it was deleted (clients show "deleted provider").
    pub provider_name: Option<String>,
    /// The model.
    pub model: String,
    /// What was written.
    pub mode: AiModeDto,
    /// The prompt, only when the owner keeps prompts (`store_ai_prompts`) and retention has
    /// not cleared it.
    pub prompt: Option<String>,
    /// Tokens sent.
    pub input_tokens: Option<i32>,
    /// Tokens received.
    pub output_tokens: Option<i32>,
    /// How it ended.
    pub outcome: AiGenerationOutcomeDto,
    /// The post saved from it, when there is one.
    pub post_id: Option<Uuid>,
    /// Who asked: the owner, or a delegate.
    pub actor: Option<Uuid>,
    /// When it was made.
    pub created_at: DateTime<Utc>,
}

impl TryFrom<AiGeneration> for AiGenerationDto {
    type Error = crate::error::ApiError;

    /// # Errors
    ///
    /// `internal` (logged with the stored value only) when the row's mode is not one this build
    /// knows: a mislabeled row would misstate the owner's spend.
    fn try_from(g: AiGeneration) -> Result<Self, Self::Error> {
        // `AiModeDto`'s own wire names decide, so a new mode cannot be mapped to another.
        let Ok(mode) = serde_json::from_value::<AiModeDto>(Value::String(g.mode.clone())) else {
            tracing::error!(mode = %g.mode, "unknown AI generation mode");
            return Err(crate::error::ApiError::new(
                crate::error::ErrorCode::Internal,
            ));
        };
        Ok(Self {
            id: g.id.into(),
            provider_id: g.provider_id.map(Into::into),
            provider_name: g.provider_name,
            model: g.model,
            mode,
            prompt: g.prompt,
            input_tokens: g.input_tokens,
            output_tokens: g.output_tokens,
            outcome: g.outcome.into(),
            post_id: g.post_id.map(Into::into),
            actor: g.actor.map(Into::into),
            created_at: g.created_at,
        })
    }
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use postit_core::AiGenerationId;
    use postit_data::ai_generations::{AiGeneration, GenerationOutcome};
    use uuid::Uuid;

    use super::{AiGenerationDto, AiModeDto};

    fn row(mode: &str) -> AiGeneration {
        AiGeneration {
            id: AiGenerationId::from(Uuid::now_v7()),
            provider_id: None,
            provider_name: None,
            model: "m".to_owned(),
            mode: mode.to_owned(),
            prompt: None,
            input_tokens: None,
            output_tokens: None,
            outcome: GenerationOutcome::Ok,
            post_id: None,
            actor: None,
            created_at: Utc::now(),
        }
    }

    #[test]
    fn generation_modes_map_by_their_wire_names_and_unknown_ones_fail() {
        for (wire, mode) in [
            ("post", AiModeDto::Post),
            ("reply", AiModeDto::Reply),
            ("rewrite", AiModeDto::Rewrite),
        ] {
            let dto = AiGenerationDto::try_from(row(wire)).ok();
            assert_eq!(dto.map(|d| d.mode), Some(mode), "{wire}");
        }
        let err = AiGenerationDto::try_from(row("shout")).err();
        assert_eq!(
            err.map(|e| e.0.code),
            Some(crate::error::ErrorCode::Internal)
        );
    }
}
