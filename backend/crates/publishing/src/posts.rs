//! Post editing: what the API's post routes call. A post is created, edited, and deleted under
//! the caller's scope; the repositories apply the capability, the post lock, and the
//! whole-post rule (`ensure_whole_post`), so this service adds no limit check of its own beyond
//! naming an account outside the limit up front.

use std::collections::BTreeMap;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use emixdb::dto::{Pagination, ResultSet};
use postit_core::{
    AccountId, AiGenerationId, CampaignId, FormatId, MediaAssetId, MediaItem, PostId, PostKind,
    PostOverride, PostVersion, ReplyTo, Violation,
};
use postit_data::ai_generations::AiGenerationRepo;
use postit_data::posts::{
    LastPublished, NewPost, Post, PostContent, PostFilter, PostMediaItem, PostOverrideRow, PostRepo,
};
use postit_data::{Capability, OwnerScope};
use serde_json::Value;

use crate::error::PublishingError;
use crate::service::{TargetCheck, due_time, tagged, target_list_violations};
use crate::{PublishingDeps, PublishingService};

/// One media attachment of a draft, in display order.
#[derive(Debug, Clone)]
pub struct PostMediaInput {
    /// The asset; it must be the owner's.
    pub media_id: MediaAssetId,
    /// Alternative text.
    pub alt_text: Option<String>,
}

/// A post to create.
#[derive(Debug, Clone)]
pub struct PostDraft {
    /// Whether it starts a conversation or answers one.
    pub kind: PostKind,
    /// What a comment answers.
    pub reply_to: Option<Value>,
    /// The campaign it belongs to.
    pub campaign_id: Option<CampaignId>,
    /// The text.
    pub text: String,
    /// The link to attach.
    pub link: Option<String>,
    /// The tags.
    pub tags: Vec<String>,
    /// Whether to disclose that AI wrote it.
    pub disclose_ai_generated: bool,
    /// The attachments, in order.
    pub media: Vec<PostMediaInput>,
    /// The accounts it goes to.
    pub targets: Vec<AccountId>,
}

/// Changes to a post. An absent field stays as it is; a nullable field clears with
/// `Some(None)`. The kind never changes.
#[allow(clippy::option_option)] // absent and null are different requests
#[derive(Debug, Clone, Default)]
pub struct PostPatch {
    /// What a comment answers.
    pub reply_to: Option<Option<Value>>,
    /// The campaign it belongs to.
    pub campaign_id: Option<Option<CampaignId>>,
    /// The text.
    pub text: Option<String>,
    /// The link to attach.
    pub link: Option<Option<String>>,
    /// The tags.
    pub tags: Option<Vec<String>>,
    /// Whether to disclose that AI wrote it.
    pub disclose_ai_generated: Option<bool>,
    /// The attachments, replacing the earlier ones.
    pub media: Option<Vec<PostMediaInput>>,
    /// The targets, replacing the ones inside the caller's limit; the others stay.
    pub targets: Option<Vec<AccountId>>,
}

/// A post with everything the caller can see of it.
#[derive(Debug, Clone)]
pub struct PostView {
    /// The post.
    pub post: Post,
    /// Its attachments, in order.
    pub media: Vec<PostMediaItem>,
    /// Its targets inside the caller's limit.
    pub targets: Vec<AccountId>,
    /// Its overrides inside the caller's limit.
    pub overrides: Vec<PostOverrideRow>,
    /// The newest live delivery per visible account.
    pub last_published: Vec<LastPublished>,
}

/// Creates, reads, edits, and deletes posts and their overrides. Cheap to clone.
#[derive(Clone)]
pub struct PostService {
    deps: Arc<PublishingDeps>,
    service: PublishingService,
}

/// What validating one target found.
#[derive(Debug, Clone, PartialEq)]
pub struct TargetReport {
    /// The account.
    pub account_id: AccountId,
    /// The format the content would be published as, when one fits.
    pub format: Option<FormatId>,
    /// The rules the content breaks for this account.
    pub violations: Vec<Violation>,
}

/// The verdict of a validate call: one report per target, and the violations that belong to no
/// target (the schedule's).
#[derive(Debug, Clone, PartialEq)]
pub struct ValidationReport {
    /// One entry per requested target, in order.
    pub targets: Vec<TargetReport>,
    /// Violations not bound to a target.
    pub violations: Vec<Violation>,
}

impl ValidationReport {
    /// Whether a publish of the same content, targets, and time would be accepted.
    #[must_use]
    pub fn valid(&self) -> bool {
        self.violations.is_empty() && self.targets.iter().all(|t| t.violations.is_empty())
    }

    /// Every violation in one list, as a refused publish reports them: the report's own first,
    /// then each target's naming its account in an `account_id` parameter.
    #[must_use]
    pub fn into_violations(self) -> Vec<Violation> {
        let mut all = self.violations;
        for target in self.targets {
            all.extend(tagged(target.violations, target.account_id));
        }
        all
    }
}

impl PublishingService {
    /// The post editing half of the engine.
    #[must_use]
    pub fn posts(&self) -> PostService {
        PostService {
            deps: Arc::clone(&self.deps),
            service: self.clone(),
        }
    }
}

/// The core post a draft describes, for validation.
fn core_post(draft: &PostDraft) -> Result<postit_core::Post, PublishingError> {
    let link = draft
        .link
        .as_deref()
        .map(url::Url::parse)
        .transpose()
        .map_err(|_| PublishingError::Corrupt("post_link_invalid".to_owned()))?;
    let reply_to = draft
        .reply_to
        .clone()
        .map(serde_json::from_value::<ReplyTo>)
        .transpose()
        .map_err(|_| PublishingError::Corrupt("post_reply_to_invalid".to_owned()))?;
    Ok(postit_core::Post {
        kind: draft.kind,
        text: draft.text.clone(),
        media: draft
            .media
            .iter()
            .map(|item| MediaItem {
                asset: item.media_id,
                alt_text: item.alt_text.clone(),
            })
            .collect(),
        link,
        tags: draft.tags.clone(),
        disclose_ai_generated: draft.disclose_ai_generated,
        reply_to,
    })
}

fn items(media: &[PostMediaInput]) -> Result<Vec<PostMediaItem>, PublishingError> {
    media
        .iter()
        .enumerate()
        .map(|(position, item)| {
            Ok(PostMediaItem {
                media_id: item.media_id,
                position: i32::try_from(position)
                    .map_err(|_| PublishingError::Corrupt("too many media items".to_owned()))?,
                alt_text: item.alt_text.clone(),
            })
        })
        .collect()
}

impl PostService {
    /// Creates a draft with its media and targets in one transaction, so a refused media item
    /// or target leaves no post behind. With `generation`, links that AI generation to the
    /// post in the same transaction.
    ///
    /// # Errors
    ///
    /// [`PublishingError::Scope`] below `edit`, [`PublishingError::AccountNotInScope`] for a
    /// target outside the caller's limit, and [`PublishingError::NotFound`] for a campaign,
    /// media asset, or target account that is not the owner's, or a generation that is not the
    /// owner's or already linked to a post.
    pub async fn create(
        &self,
        scope: &OwnerScope,
        draft: PostDraft,
        generation: Option<AiGenerationId>,
    ) -> Result<PostView, PublishingError> {
        scope.require(Capability::Edit)?;
        if draft
            .targets
            .iter()
            .any(|account| !scope.allows_account(*account))
        {
            return Err(PublishingError::AccountNotInScope);
        }
        let id = PostId::from(self.deps.ids.generate());
        let media = items(&draft.media)?;
        let mut tx = self.deps.pool.begin().await?;
        PostRepo::create(
            &mut tx,
            scope,
            NewPost {
                id,
                kind: draft.kind,
                reply_to: draft.reply_to,
                campaign_id: draft.campaign_id,
                text: draft.text,
                link: draft.link,
                tags: draft.tags,
                disclose_ai_generated: draft.disclose_ai_generated,
            },
        )
        .await?;
        let locked = PostRepo::lock(&mut tx, scope, id, None).await?;
        if !media.is_empty() {
            PostRepo::set_media(&mut tx, &locked, &media).await?;
        }
        if !draft.targets.is_empty() {
            PostRepo::set_targets(&mut tx, &locked, &draft.targets).await?;
        }
        if let Some(generation) = generation
            && !AiGenerationRepo::link_unlinked(&mut tx, scope, generation, id).await?
        {
            return Err(PublishingError::NotFound);
        }
        tx.commit().await?;
        self.get(scope, id).await
    }

    /// One post with its media, targets, overrides, and last published versions, filtered by
    /// the caller's account limit.
    ///
    /// # Errors
    ///
    /// [`PublishingError::NotFound`] when the owner has no such post.
    pub async fn get(&self, scope: &OwnerScope, id: PostId) -> Result<PostView, PublishingError> {
        let mut conn = self.deps.pool.acquire().await?;
        let post = PostRepo::get(&mut conn, scope, id).await?;
        let media = PostRepo::media(&mut conn, scope, id).await?;
        let targets = PostRepo::targets(&mut conn, scope, id).await?;
        let overrides = PostRepo::overrides(&mut conn, scope, id).await?;
        let last_published = PostRepo::last_published(&mut conn, scope, id).await?;
        Ok(PostView {
            post,
            media,
            targets,
            overrides,
            last_published,
        })
    }

    /// The owner's posts, newest first.
    ///
    /// # Errors
    ///
    /// [`PublishingError::Scope`] below `view`.
    pub async fn list(
        &self,
        scope: &OwnerScope,
        pagination: Pagination,
        filter: PostFilter,
    ) -> Result<ResultSet<Post>, PublishingError> {
        let mut conn = self.deps.pool.acquire().await?;
        Ok(PostRepo::list(&mut conn, scope, pagination, filter).await?)
    }

    /// Changes a post at `if_match`. Targets outside the caller's limit stay.
    ///
    /// # Errors
    ///
    /// [`PublishingError::VersionConflict`], [`PublishingError::PostLocked`] while a job holds
    /// the post, [`PublishingError::AccountNotInScope`] for a target outside the limit or when
    /// an override or unfinished delivery belongs to an account outside it, and
    /// [`PublishingError::NotFound`].
    pub async fn update(
        &self,
        scope: &OwnerScope,
        id: PostId,
        if_match: PostVersion,
        patch: PostPatch,
    ) -> Result<PostView, PublishingError> {
        let media = patch.media.as_deref().map(items).transpose()?;
        let mut tx = self.deps.pool.begin().await?;
        let locked = PostRepo::lock(&mut tx, scope, id, Some(if_match.0)).await?;
        let current = locked.post().clone();
        let edits_content = patch.reply_to.is_some()
            || patch.campaign_id.is_some()
            || patch.text.is_some()
            || patch.link.is_some()
            || patch.tags.is_some()
            || patch.disclose_ai_generated.is_some();
        if edits_content {
            PostRepo::update_content(
                &mut tx,
                &locked,
                PostContent {
                    reply_to: patch.reply_to.unwrap_or(current.reply_to),
                    campaign_id: patch.campaign_id.unwrap_or(current.campaign_id),
                    text: patch.text.unwrap_or(current.text),
                    link: patch.link.unwrap_or(current.link),
                    tags: patch.tags.unwrap_or(current.tags),
                    disclose_ai_generated: patch
                        .disclose_ai_generated
                        .unwrap_or(current.disclose_ai_generated),
                },
            )
            .await?;
        }
        if let Some(media) = media {
            PostRepo::set_media(&mut tx, &locked, &media).await?;
        }
        if let Some(targets) = &patch.targets {
            PostRepo::set_targets(&mut tx, &locked, targets).await?;
        }
        tx.commit().await?;
        self.get(scope, id).await
    }

    /// Deletes a post at `if_match`, with its history. Needs `publish` when the post has live
    /// history and `edit` otherwise; the repository decides.
    ///
    /// # Errors
    ///
    /// As [`Self::update`], and [`PublishingError::Scope`] below the level the history needs.
    pub async fn delete(
        &self,
        scope: &OwnerScope,
        id: PostId,
        if_match: PostVersion,
    ) -> Result<(), PublishingError> {
        let mut tx = self.deps.pool.begin().await?;
        let locked = PostRepo::lock(&mut tx, scope, id, Some(if_match.0)).await?;
        PostRepo::delete(&mut tx, locked).await?;
        tx.commit().await?;
        Ok(())
    }

    /// Sets the override for `account`, replacing any earlier one. `patch` holds the fields
    /// that differ from the post and `options` the platform options for this account.
    ///
    /// # Errors
    ///
    /// As [`Self::update`]; an account outside the caller's limit, not the owner's, or
    /// disconnected is [`PublishingError::NotFound`].
    pub async fn put_override(
        &self,
        scope: &OwnerScope,
        id: PostId,
        account: AccountId,
        if_match: PostVersion,
        patch: Value,
        options: Value,
    ) -> Result<PostView, PublishingError> {
        let mut tx = self.deps.pool.begin().await?;
        let locked = PostRepo::lock(&mut tx, scope, id, Some(if_match.0)).await?;
        PostRepo::upsert_override(&mut tx, &locked, account, patch, options).await?;
        tx.commit().await?;
        self.get(scope, id).await
    }

    /// Validates an unsaved post for its targets with the same check a publish runs, and
    /// reports each target's violations. A media asset that is not the owner's is a
    /// `media_missing` violation for every target that uses it.
    ///
    /// # Errors
    ///
    /// [`PublishingError::Scope`] below `edit`, [`PublishingError::AccountNotInScope`] for a
    /// target outside the caller's limit, and [`PublishingError::NotFound`] for a target that
    /// is not the owner's. Violations are in the report, not errors.
    pub async fn validate_draft(
        &self,
        scope: &OwnerScope,
        draft: PostDraft,
        overrides: BTreeMap<AccountId, PostOverride>,
        at: Option<DateTime<Utc>>,
        native_schedule: bool,
    ) -> Result<ValidationReport, PublishingError> {
        scope.require(Capability::Edit)?;
        let targets = draft.targets.clone();
        let base = core_post(&draft)?;
        self.report(scope, &base, &overrides, &targets, at, native_schedule)
            .await
    }

    /// Validates the stored post for `targets` (its own targets by default) as a publish of
    /// its current version would be checked.
    ///
    /// # Errors
    ///
    /// As [`Self::validate_draft`], and [`PublishingError::NotFound`] for a missing post.
    pub async fn validate_post(
        &self,
        scope: &OwnerScope,
        id: PostId,
        targets: Option<Vec<AccountId>>,
        at: Option<DateTime<Utc>>,
        native_schedule: bool,
    ) -> Result<ValidationReport, PublishingError> {
        scope.require(Capability::Edit)?;
        let current = {
            let mut conn = self.deps.pool.acquire().await?;
            PostRepo::get(&mut conn, scope, id).await?.version
        };
        let (_, base, overrides) = self
            .service
            .read_post(scope, id, PostVersion(current))
            .await?;
        let targets = if let Some(targets) = targets {
            targets
        } else {
            let mut conn = self.deps.pool.acquire().await?;
            PostRepo::targets(&mut conn, scope, id).await?
        };
        self.report(scope, &base, &overrides, &targets, at, native_schedule)
            .await
    }

    async fn report(
        &self,
        scope: &OwnerScope,
        base: &postit_core::Post,
        overrides: &BTreeMap<AccountId, PostOverride>,
        targets: &[AccountId],
        at: Option<DateTime<Utc>>,
        native_schedule: bool,
    ) -> Result<ValidationReport, PublishingError> {
        if targets
            .iter()
            .any(|account| !scope.allows_account(*account))
        {
            return Err(PublishingError::AccountNotInScope);
        }
        let now = self.deps.clock.now();
        let mut violations = target_list_violations(targets);
        let at = match due_time(at, now) {
            Ok(at) => at,
            Err(PublishingError::ValidationFailed(found)) => {
                violations.extend(found);
                None
            }
            Err(other) => return Err(other),
        };
        let checks = self
            .service
            .check_targets_for(scope, base, overrides, targets, at, native_schedule, now)
            .await?;
        let reports = checks
            .into_iter()
            .map(|check: TargetCheck| TargetReport {
                account_id: check.account,
                format: check.format,
                violations: check.violations,
            })
            .collect();
        Ok(ValidationReport {
            targets: reports,
            violations,
        })
    }

    /// Removes the override for `account`.
    ///
    /// # Errors
    ///
    /// As [`Self::put_override`]; an account with no override is
    /// [`PublishingError::NotFound`].
    pub async fn delete_override(
        &self,
        scope: &OwnerScope,
        id: PostId,
        account: AccountId,
        if_match: PostVersion,
    ) -> Result<PostView, PublishingError> {
        let mut tx = self.deps.pool.begin().await?;
        let locked = PostRepo::lock(&mut tx, scope, id, Some(if_match.0)).await?;
        PostRepo::delete_override(&mut tx, &locked, account).await?;
        tx.commit().await?;
        self.get(scope, id).await
    }
}
