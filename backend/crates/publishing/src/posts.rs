//! Post editing: what the API's post routes call. A post is created, edited, and deleted under
//! the caller's scope; the repositories apply the capability, the post lock, and the
//! whole-post rule (`ensure_whole_post`), so this service adds no limit check of its own beyond
//! naming an account outside the limit up front.

use std::sync::Arc;

use emixdb::dto::{Pagination, ResultSet};
use postit_core::{
    AccountId, AiGenerationId, CampaignId, MediaAssetId, PostId, PostKind, PostVersion,
};
use postit_data::ai_generations::AiGenerationRepo;
use postit_data::posts::{
    LastPublished, NewPost, Post, PostContent, PostFilter, PostMediaItem, PostOverrideRow, PostRepo,
};
use postit_data::{Capability, OwnerScope};
use serde_json::Value;

use crate::error::PublishingError;
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
}

impl PublishingService {
    /// The post editing half of the engine.
    #[must_use]
    pub fn posts(&self) -> PostService {
        PostService {
            deps: Arc::clone(&self.deps),
        }
    }
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
