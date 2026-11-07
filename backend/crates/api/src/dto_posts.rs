//! Wire types of the post routes.

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use postit_core::{
    AccountId, CampaignId, FormatId, MediaAssetId, PostKind, PostOverride, ReplyTo, ScheduleInput,
    Violation,
};
use postit_data::posts::{LastPublished, Post, PostMediaItem, PostOverrideRow, PostStatus};
use postit_publishing::{
    PostDraft, PostMediaInput, PostPatch, PostView, TargetReport, ValidationReport,
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::dto_contacts::present;
use crate::error::{ApiError, ErrorCode};

/// Where a post is in its publishing life.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum PostStatusDto {
    /// Being written.
    Draft,
    /// A publish job waits for its time.
    Scheduled,
    /// A publish job is running.
    Publishing,
    /// Every delivery went out.
    Published,
    /// Some deliveries went out.
    Partial,
    /// No delivery went out.
    Failed,
    /// The job was cancelled.
    Cancelled,
}

impl From<PostStatus> for PostStatusDto {
    fn from(status: PostStatus) -> Self {
        match status {
            PostStatus::Draft => Self::Draft,
            PostStatus::Scheduled => Self::Scheduled,
            PostStatus::Publishing => Self::Publishing,
            PostStatus::Published => Self::Published,
            PostStatus::Partial => Self::Partial,
            PostStatus::Failed => Self::Failed,
            PostStatus::Cancelled => Self::Cancelled,
        }
    }
}

impl From<PostStatusDto> for PostStatus {
    fn from(status: PostStatusDto) -> Self {
        match status {
            PostStatusDto::Draft => Self::Draft,
            PostStatusDto::Scheduled => Self::Scheduled,
            PostStatusDto::Publishing => Self::Publishing,
            PostStatusDto::Published => Self::Published,
            PostStatusDto::Partial => Self::Partial,
            PostStatusDto::Failed => Self::Failed,
            PostStatusDto::Cancelled => Self::Cancelled,
        }
    }
}

/// A post without its media, targets, or overrides: an item of the post list.
#[derive(Debug, Serialize, ToSchema)]
pub struct PostSummaryDto {
    /// The post.
    pub id: Uuid,
    /// Whether it starts a conversation or answers one.
    pub kind: PostKind,
    /// What a comment answers.
    #[schema(value_type = Option<ReplyTo>)]
    pub reply_to: Option<Value>,
    /// The campaign it belongs to.
    pub campaign_id: Option<Uuid>,
    /// The text.
    pub text: String,
    /// The link to attach.
    pub link: Option<String>,
    /// The tags, without the leading `#`.
    pub tags: Vec<String>,
    /// Whether the post says it was made with AI.
    pub disclose_ai_generated: bool,
    /// Where it is in its publishing life.
    pub status: PostStatusDto,
    /// Counts content changes, starting at 1; the `ETag` is `"v{version}"`.
    pub version: i64,
    /// When it was created.
    pub created_at: DateTime<Utc>,
    /// When it last changed.
    pub updated_at: DateTime<Utc>,
}

impl From<Post> for PostSummaryDto {
    fn from(post: Post) -> Self {
        Self {
            id: post.id.as_uuid(),
            kind: post.kind,
            reply_to: post.reply_to,
            campaign_id: post.campaign_id.map(|id| id.as_uuid()),
            text: post.text,
            link: post.link,
            tags: post.tags,
            disclose_ai_generated: post.disclose_ai_generated,
            status: post.status.into(),
            version: post.version,
            created_at: post.created_at,
            updated_at: post.updated_at,
        }
    }
}

/// One attachment of a post.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct PostMediaDto {
    /// The media asset.
    pub media_id: Uuid,
    /// Its place among the attachments, counting from 0.
    #[serde(default)]
    pub position: i32,
    /// Alternative text.
    #[serde(default)]
    pub alt_text: Option<String>,
}

impl From<PostMediaItem> for PostMediaDto {
    fn from(item: PostMediaItem) -> Self {
        Self {
            media_id: item.media_id.as_uuid(),
            position: item.position,
            alt_text: item.alt_text,
        }
    }
}

/// A per-account override of a post.
#[derive(Debug, Serialize, ToSchema)]
pub struct PostOverrideDto {
    /// The account it applies to.
    pub account_id: Uuid,
    /// The fields that differ from the post.
    pub patch: PostOverride,
    /// Platform options for this account.
    #[schema(value_type = Object)]
    pub options: Map<String, Value>,
    /// When it last changed.
    pub updated_at: DateTime<Utc>,
}

impl From<PostOverrideRow> for PostOverrideDto {
    fn from(row: PostOverrideRow) -> Self {
        let options = match row.options {
            Value::Object(map) => map,
            _ => Map::new(),
        };
        Self {
            account_id: row.account_id.as_uuid(),
            patch: serde_json::from_value(row.patch).unwrap_or_default(),
            options,
            updated_at: row.updated_at,
        }
    }
}

/// The version and time of the newest live delivery to an account.
#[derive(Debug, Serialize, ToSchema)]
pub struct LastPublishedDto {
    /// The account.
    pub account_id: Uuid,
    /// The post version that went out.
    pub post_version: i64,
    /// When it went out.
    pub published_at: DateTime<Utc>,
}

impl From<LastPublished> for LastPublishedDto {
    fn from(item: LastPublished) -> Self {
        Self {
            account_id: item.account_id.as_uuid(),
            post_version: item.post_version,
            published_at: item.published_at,
        }
    }
}

/// A post with everything the caller can see of it. A delegate with a limited account list
/// sees only the targets, overrides, and last published entries inside the limit.
#[derive(Debug, Serialize, ToSchema)]
pub struct PostDto {
    /// The post itself.
    #[serde(flatten)]
    pub post: PostSummaryDto,
    /// Its attachments, in order.
    pub media: Vec<PostMediaDto>,
    /// The accounts it goes to.
    pub targets: Vec<Uuid>,
    /// Its per-account overrides.
    pub overrides: Vec<PostOverrideDto>,
    /// The newest live delivery per account, so a client can warn that a version was already
    /// published there.
    pub last_published: Vec<LastPublishedDto>,
}

impl From<PostView> for PostDto {
    fn from(view: PostView) -> Self {
        Self {
            post: view.post.into(),
            media: view.media.into_iter().map(Into::into).collect(),
            targets: view.targets.into_iter().map(|a| a.as_uuid()).collect(),
            overrides: view.overrides.into_iter().map(Into::into).collect(),
            last_published: view.last_published.into_iter().map(Into::into).collect(),
        }
    }
}

fn default_kind() -> PostKind {
    PostKind::Post
}

/// A post to create (or to validate without saving).
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreatePostRequest {
    /// Whether it starts a conversation or answers one (default `post`).
    #[serde(default = "default_kind")]
    pub kind: PostKind,
    /// What a comment answers; required for a comment.
    #[serde(default)]
    pub reply_to: Option<ReplyTo>,
    /// The campaign it belongs to.
    #[serde(default)]
    pub campaign_id: Option<Uuid>,
    /// The text.
    #[serde(default)]
    pub text: String,
    /// The link to attach, an absolute URL.
    #[serde(default)]
    pub link: Option<String>,
    /// The tags, without the leading `#`.
    #[serde(default)]
    pub tags: Vec<String>,
    /// Whether the post says it was made with AI.
    #[serde(default)]
    pub disclose_ai_generated: bool,
    /// The attachments, in order (the position field is ignored).
    #[serde(default)]
    pub media: Vec<PostMediaDto>,
    /// The accounts it goes to.
    #[serde(default)]
    pub targets: Vec<Uuid>,
    /// The AI generation this post was saved from; it links to the post.
    #[serde(default)]
    pub generation_id: Option<Uuid>,
}

/// A link that is not an absolute URL is `validation_failed` at `link`.
fn checked_link(link: Option<String>) -> Result<Option<String>, ApiError> {
    if let Some(text) = &link
        && url::Url::parse(text).is_err()
    {
        return Err(ApiError::new(ErrorCode::ValidationFailed).with_extension("field", "link"));
    }
    Ok(link)
}

fn reply_value(reply: Option<ReplyTo>) -> Option<Value> {
    reply.and_then(|r| serde_json::to_value(r).ok())
}

fn media_inputs(media: Vec<PostMediaDto>) -> Vec<PostMediaInput> {
    media
        .into_iter()
        .map(|m| PostMediaInput {
            media_id: MediaAssetId::from(m.media_id),
            alt_text: m.alt_text,
        })
        .collect()
}

impl CreatePostRequest {
    /// The draft, and the generation to link.
    ///
    /// # Errors
    ///
    /// `validation_failed` at `link` for a link that is not an absolute URL.
    pub fn into_draft(self) -> Result<PostDraft, ApiError> {
        Ok(PostDraft {
            kind: self.kind,
            reply_to: reply_value(self.reply_to),
            campaign_id: self.campaign_id.map(CampaignId::from),
            text: self.text,
            link: checked_link(self.link)?,
            tags: self.tags,
            disclose_ai_generated: self.disclose_ai_generated,
            media: media_inputs(self.media),
            targets: self.targets.into_iter().map(AccountId::from).collect(),
        })
    }
}

/// Changes to a post. Absent fields stay as they are; `null` clears the reply target, the
/// campaign, or the link. The kind never changes.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PatchPostRequest {
    /// What a comment answers, or `null` to clear it.
    #[serde(default, deserialize_with = "present")]
    #[schema(value_type = Option<ReplyTo>)]
    pub reply_to: Option<Option<ReplyTo>>,
    /// The campaign, or `null` to remove the post from it.
    #[serde(default, deserialize_with = "present")]
    #[schema(value_type = Option<Uuid>)]
    pub campaign_id: Option<Option<Uuid>>,
    /// The text.
    pub text: Option<String>,
    /// The link, or `null` to remove it.
    #[serde(default, deserialize_with = "present")]
    #[schema(value_type = Option<String>)]
    pub link: Option<Option<String>>,
    /// The tags.
    pub tags: Option<Vec<String>>,
    /// Whether the post says it was made with AI.
    pub disclose_ai_generated: Option<bool>,
    /// The attachments, replacing the earlier ones.
    pub media: Option<Vec<PostMediaDto>>,
    /// The targets, replacing the ones the caller can see; targets outside a limited
    /// delegate's account limit stay.
    pub targets: Option<Vec<Uuid>>,
}

impl PatchPostRequest {
    /// The patch.
    ///
    /// # Errors
    ///
    /// `validation_failed` at `link` for a link that is not an absolute URL.
    pub fn into_patch(self) -> Result<PostPatch, ApiError> {
        let link = match self.link {
            Some(Some(link)) => Some(checked_link(Some(link))?),
            Some(None) => Some(None),
            None => None,
        };
        Ok(PostPatch {
            reply_to: self.reply_to.map(reply_value),
            campaign_id: self.campaign_id.map(|id| id.map(CampaignId::from)),
            text: self.text,
            link,
            tags: self.tags,
            disclose_ai_generated: self.disclose_ai_generated,
            media: self.media.map(media_inputs),
            targets: self
                .targets
                .map(|ids| ids.into_iter().map(AccountId::from).collect()),
        })
    }
}

/// An override to store.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PutOverrideRequest {
    /// The fields that differ from the post: text, link, tags, a subset of its media, the
    /// email document for an email target.
    #[serde(default)]
    pub patch: PostOverride,
    /// Platform options for this account, checked against the agent's schema when publishing
    /// or validating.
    #[serde(default)]
    #[schema(value_type = Object)]
    pub options: Map<String, Value>,
}

/// Flat on purpose (see `UserListQuery`).
#[derive(Debug, Deserialize, utoipa::IntoParams)]
#[serde(deny_unknown_fields)]
#[into_params(parameter_in = Query)]
pub struct PostListQuery {
    /// Only posts in this status.
    pub status: Option<PostStatusDto>,
    /// Only posts in this campaign.
    pub campaign_id: Option<Uuid>,
    /// Only posts of this kind.
    pub kind: Option<PostKind>,
    /// 1-based page number (default 1).
    pub page: Option<u64>,
    /// Items per page, 1-100 (default 20).
    pub page_size: Option<u64>,
}

/// An unsaved post to validate: the `POST /posts` body, per-account overrides, and when it
/// would go out.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ValidateDraftRequest {
    /// Whether it starts a conversation or answers one (default `post`).
    #[serde(default = "default_kind")]
    pub kind: PostKind,
    /// What a comment answers; required for a comment.
    #[serde(default)]
    pub reply_to: Option<ReplyTo>,
    /// The campaign it belongs to.
    #[serde(default)]
    pub campaign_id: Option<Uuid>,
    /// The text.
    #[serde(default)]
    pub text: String,
    /// The link to attach, an absolute URL.
    #[serde(default)]
    pub link: Option<String>,
    /// The tags, without the leading `#`.
    #[serde(default)]
    pub tags: Vec<String>,
    /// Whether the post says it was made with AI.
    #[serde(default)]
    pub disclose_ai_generated: bool,
    /// The attachments, in order. An asset that is not the owner's is a `media_missing`
    /// violation, not a 404.
    #[serde(default)]
    pub media: Vec<PostMediaDto>,
    /// The accounts it would go to.
    #[serde(default)]
    pub targets: Vec<Uuid>,
    /// Per-account overrides, keyed by account ID.
    #[serde(default)]
    pub overrides: BTreeMap<Uuid, PutOverrideRequest>,
    /// When it would go out; now when absent.
    #[serde(default)]
    pub at: Option<ScheduleInput>,
    /// Whether platforms that can hold the post until `at` would do so.
    #[serde(default)]
    pub native_schedule: bool,
}

impl ValidateDraftRequest {
    /// The draft, the overrides, and the schedule input to resolve.
    ///
    /// # Errors
    ///
    /// `validation_failed` at `link` for a link that is not an absolute URL.
    pub fn into_parts(self) -> Result<DraftToValidate, ApiError> {
        let overrides = self
            .overrides
            .into_iter()
            .map(|(account, over)| {
                let mut patch = over.patch;
                patch.options = over.options;
                (AccountId::from(account), patch)
            })
            .collect();
        let draft = PostDraft {
            kind: self.kind,
            reply_to: reply_value(self.reply_to),
            campaign_id: self.campaign_id.map(CampaignId::from),
            text: self.text,
            link: checked_link(self.link)?,
            tags: self.tags,
            disclose_ai_generated: self.disclose_ai_generated,
            media: media_inputs(self.media),
            targets: self.targets.into_iter().map(AccountId::from).collect(),
        };
        Ok(DraftToValidate {
            draft,
            overrides,
            at: self.at,
            native_schedule: self.native_schedule,
        })
    }
}

/// A validate request, parsed.
#[derive(Debug)]
pub struct DraftToValidate {
    /// The post.
    pub draft: PostDraft,
    /// Its overrides.
    pub overrides: BTreeMap<AccountId, PostOverride>,
    /// When it would go out, as the client wrote it.
    pub at: Option<ScheduleInput>,
    /// Whether platforms that can hold the post until `at` would do so.
    pub native_schedule: bool,
}

/// A saved post to validate.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ValidatePostRequest {
    /// The accounts to check; the post's own targets when absent.
    #[serde(default)]
    pub targets: Option<Vec<Uuid>>,
    /// When it would go out; now when absent.
    #[serde(default)]
    pub at: Option<ScheduleInput>,
    /// Whether platforms that can hold the post until `at` would do so.
    #[serde(default)]
    pub native_schedule: bool,
}

/// What validating one target found.
#[derive(Debug, Serialize, ToSchema)]
pub struct TargetReportDto {
    /// The account.
    pub account_id: Uuid,
    /// The format the content would go out as, when one fits.
    pub format: Option<FormatId>,
    /// The rules the content breaks for this account.
    pub violations: Vec<Violation>,
}

impl From<TargetReport> for TargetReportDto {
    fn from(report: TargetReport) -> Self {
        Self {
            account_id: report.account_id.as_uuid(),
            format: report.format,
            violations: report.violations,
        }
    }
}

/// The verdict of a validate call. The call itself succeeded either way, so it answers 200.
#[derive(Debug, Serialize, ToSchema)]
pub struct ValidationDto {
    /// Whether a publish of the same content, targets, and time would be accepted.
    pub valid: bool,
    /// One entry per target.
    pub targets: Vec<TargetReportDto>,
    /// Violations that belong to no target: the schedule's.
    pub violations: Vec<Violation>,
}

impl ValidationDto {
    /// The report with `extra` top-level violations (a schedule that did not resolve).
    #[must_use]
    pub fn new(report: ValidationReport, extra: Option<Violation>) -> Self {
        let mut violations = report.violations;
        violations.extend(extra);
        let valid = violations.is_empty() && report.targets.iter().all(|t| t.violations.is_empty());
        Self {
            valid,
            targets: report.targets.into_iter().map(Into::into).collect(),
            violations,
        }
    }
}
